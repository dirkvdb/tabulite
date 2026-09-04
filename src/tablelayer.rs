use polars::{
    frame::DataFrame,
    prelude::{AnyValue, DataType, IntoLazy, PlSmallStr, col, lit},
};
use std::collections::HashMap;

use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, Size, StyleSized,
    input::{Enter, Escape, Input, InputEvent, InputState},
    table::{Column, ColumnGroup, ColumnSort, TableDelegate, TableState},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

#[derive(Default)]
pub struct TableLayer {
    data: polars::frame::DataFrame,
    original_data: polars::frame::DataFrame,
    filter_enabled: bool,
    filter_inputs: Vec<Entity<InputState>>,
    input_subscriptions: Vec<Subscription>,
    columns: Vec<Column>,
    table_focus_handle: Option<FocusHandle>,
    selected_col: Option<usize>,
}

const NULL: &'static str = "null";

impl TableLayer {
    pub fn update_data(&mut self, data: polars::frame::DataFrame) {
        self.original_data = data.clone();
        self.data = data;
        self.create_column_info();
    }

    pub fn columns_count(&self) -> usize {
        self.columns.len()
    }

    pub fn rows_count(&self) -> usize {
        self.data.height()
    }

    pub fn set_table_focus_handle(&mut self, focus_handle: FocusHandle) {
        self.table_focus_handle = Some(focus_handle);
    }

    pub fn set_selected_col(&mut self, col_ix: usize) {
        self.selected_col = Some(col_ix);
    }

    pub fn set_column_widths(&mut self, widths: &[Pixels]) {
        for (column, width) in self.columns.iter_mut().zip(widths) {
            column.width = *width;
        }
    }

    pub fn filter_input(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Entity<InputState> {
        self.filter_enabled = true;
        self.ensure_filter_inputs(window, cx);
        self.filter_inputs[col_ix].clone()
    }

    pub fn clear_filter(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let Some(input) = self.filter_inputs.get(col_ix).cloned() else {
            return;
        };

        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.filter_data(cx);
    }

    pub fn hide_filters_if_empty(&mut self, cx: &App) -> bool {
        if !self.filter_enabled
            || self
                .filter_inputs
                .iter()
                .any(|input| !input.read(cx).value().is_empty())
        {
            return false;
        }

        self.filter_enabled = false;
        true
    }

    fn ensure_filter_inputs(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>) {
        if !self.filter_inputs.is_empty() {
            return;
        }

        for _ in &self.columns {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter..."));
            self.input_subscriptions.push(cx.subscribe(
                &input,
                |this, entity, event: &InputEvent, cx| {
                    this.delegate_mut()
                        .on_filter_input_event(&entity, event, cx);
                },
            ));
            self.filter_inputs.push(input);
        }
    }

    fn filter_data(&mut self, cx: &mut Context<TableState<Self>>) {
        // Collect filter texts with column names
        let filters: Vec<(String, String)> = self
            .filter_inputs
            .iter()
            .enumerate()
            .filter_map(|(col_ix, input)| {
                let filter_text = input.read(cx).value().to_string();
                if !filter_text.is_empty() {
                    let col_name = self
                        .columns
                        .get(col_ix)
                        .map(|col| col.key.to_string())
                        .unwrap_or_default();
                    Some((col_name, filter_text))
                } else {
                    None
                }
            })
            .collect();

        if filters.is_empty() {
            self.data = self.original_data.clone();
            return;
        }

        // Clone the data to move into background task
        let data = self.original_data.clone();

        // Spawn background task to perform filtering
        cx.spawn(async move |table_state, cx| {
            let filtered_data = cx
                .background_executor()
                .spawn(async move {
                    let mut lazy_df = data.lazy();

                    // Apply each filter using polars lazy API
                    for (col_name, filter_text) in filters {
                        // Create filter expression: cast to string, convert to lowercase, check if contains filter text
                        let filter_expr = col(&col_name)
                            .cast(polars::prelude::DataType::String)
                            .str()
                            .to_lowercase()
                            .str()
                            .contains(
                                lit(filter_text.to_lowercase()),
                                true, /* literal, use false for regex support*/
                            );

                        lazy_df = lazy_df.filter(filter_expr);
                    }

                    lazy_df.collect().ok()
                })
                .await;

            // Update the data on the UI thread
            if let Some(filtered) = filtered_data {
                let _ = table_state.update(cx, |table_state, cx| {
                    table_state.delegate_mut().data = filtered;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn on_filter_input_event(
        &mut self,
        _state: &Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<TableState<Self>>,
    ) {
        match event {
            InputEvent::Change => {
                self.filter_data(cx);
            }
            _ => {}
        };
    }

    fn create_column_info(&mut self) {
        let previous_widths: HashMap<_, _> = self
            .columns
            .iter()
            .map(|column| (column.key.clone(), column.width))
            .collect();
        let schema = self.data.schema();
        self.columns = schema
            .iter()
            .enumerate()
            .map(|(col_ix, (name, dtype))| {
                let name = SharedString::new(name.as_str());
                let (min_width, preferred_width, max_width) = column_widths(
                    name.as_ref(),
                    dtype,
                    (0..self.data.height().min(100)).filter_map(|row_ix| {
                        self.data[col_ix]
                            .get(row_ix)
                            .ok()
                            .map(|value| format_cell_value(&value))
                    }),
                );
                let width = previous_widths
                    .get(&name)
                    .copied()
                    .unwrap_or_else(|| px(preferred_width));
                let mut column = Column::new(name.clone(), name)
                    .sortable()
                    .width(width)
                    .min_width(px(min_width))
                    .max_width(px(max_width));

                if dtype.is_numeric() {
                    column = column.text_right();
                } else if dtype == &DataType::Boolean {
                    column = column.text_center();
                }

                column
            })
            .collect();

        self.input_subscriptions.clear();
        self.filter_inputs.clear();
    }
}

impl TableDelegate for TableLayer {
    fn loading(&self, _cx: &App) -> bool {
        self.columns.is_empty()
    }

    fn columns_count(&self, _: &App) -> usize {
        debug_assert_eq!(self.data.shape().1, self.columns.len());
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.data.shape().0
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let div = div().id("header");
        if self.filter_enabled {
            self.ensure_filter_inputs(window, cx);
        }

        div
    }

    fn group_headers(&self, _: &App) -> Option<Vec<Vec<ColumnGroup>>> {
        self.filter_enabled.then(|| {
            vec![
                self.columns
                    .iter()
                    .map(|column| ColumnGroup::new(column.name.clone(), 1))
                    .collect(),
            ]
        })
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if self.filter_enabled {
            let table_focus_handle = self.table_focus_handle.clone();
            let enter_focus_handle = table_focus_handle.clone();
            let table = cx.entity();
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(
                    Input::new(&self.filter_inputs.get(col_ix).expect("BUG: column index"))
                        .w_full()
                        .prefix(Icon::new(IconName::Search))
                        .text_xs()
                        .xsmall(),
                )
                .on_action(move |_: &Enter, window, cx| {
                    if let Some(focus_handle) = &enter_focus_handle {
                        focus_handle.focus(window, cx);
                    } else {
                        window.blur(cx);
                    }
                })
                .on_action(move |_: &Escape, window, cx| {
                    table.update(cx, |table, cx| {
                        if table.delegate_mut().hide_filters_if_empty(cx) {
                            table.refresh_header_layout(cx);
                        }
                    });
                    if let Some(focus_handle) = &table_focus_handle {
                        focus_handle.focus(window, cx);
                    } else {
                        window.blur(cx);
                    }
                })
        } else {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .truncate()
                .child(self.column(col_ix, cx).name.clone())
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("row", row_ix)).when(row_ix % 2 != 0, |this| {
            this.bg(cx.theme().tokens.table_even)
        })
    }

    fn render_group_th(
        &mut self,
        label: &SharedString,
        _col_span: usize,
        width: Pixels,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let is_selected = self
            .selected_col
            .and_then(|col_ix| self.columns.get(col_ix))
            .is_some_and(|column| &column.name == label);

        div()
            .w(width)
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .table_cell_size(Size::XSmall)
            .border_r_1()
            .border_color(cx.theme().border)
            .when(is_selected, |this| {
                this.bg(cx.theme().tokens.table_active)
                    .text_color(cx.theme().foreground)
            })
            .child(div().w_full().truncate().child(label.clone()))
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        match self.data[col_ix].get(row_ix) {
            Ok(AnyValue::Null) => div()
                .flex()
                .justify_center()
                .child(Tag::secondary().outline().xsmall().child(NULL))
                .text_color(cx.theme().accent),
            Ok(value) => {
                let align = self.columns[col_ix].align;
                div()
                    .flex()
                    .items_center()
                    .size_full()
                    .truncate()
                    .when(align == TextAlign::Center, |this| this.justify_center())
                    .when(align == TextAlign::Right, |this| this.justify_end())
                    .child(SharedString::new(format_cell_value(&value)))
            }
            Err(_) => div().child(SharedString::new("ERR")),
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        self.data[col_ix]
            .get(row_ix)
            .map(|value| format_cell_value(&value))
            .unwrap_or_default()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let col = &self.columns[col_ix];

        let mut temp_df = DataFrame::default();
        std::mem::swap(&mut self.data, &mut temp_df);

        let df = temp_df.lazy();
        let sort_options = match sort {
            ColumnSort::Ascending => polars::prelude::SortMultipleOptions::default(),
            ColumnSort::Descending => {
                polars::prelude::SortMultipleOptions::default().with_order_descending(true)
            }
            ColumnSort::Default => {
                // No sorting, return original DataFrame
                self.data = df.collect().unwrap();
                return;
            }
        }
        .with_multithreaded(true)
        .with_nulls_last(true);

        self.data = df
            .sort(vec![PlSmallStr::from(col.key.as_ref())], sort_options)
            .collect()
            .unwrap();
    }
}

fn format_cell_value(value: &AnyValue<'_>) -> String {
    match value {
        AnyValue::Null => String::new(),
        AnyValue::Boolean(true) => "true".to_string(),
        AnyValue::Boolean(false) => "false".to_string(),
        AnyValue::Float16(value) => format_float(f32::from(*value) as f64),
        AnyValue::Float32(value) => format_float(*value as f64),
        AnyValue::Float64(value) => format_float(*value),
        AnyValue::String(value) => (*value).to_string(),
        AnyValue::StringOwned(value) => value.to_string(),
        AnyValue::Binary(value) => format!("{} bytes", value.len()),
        AnyValue::BinaryOwned(value) => format!("{} bytes", value.len()),
        value => value.str_value().into_owned(),
    }
}

fn format_float(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }

    if value != 0.0 && !(0.0001..1_000_000_000.0).contains(&value.abs()) {
        return format!("{value:.6e}");
    }

    let mut value = format!("{value:.6}");
    if value.contains('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    value
}

fn column_widths(
    name: &str,
    dtype: &DataType,
    values: impl Iterator<Item = String>,
) -> (f32, f32, f32) {
    let (min_width, max_width) = if dtype == &DataType::Boolean {
        (80.0, 140.0)
    } else if dtype.is_numeric() {
        (90.0, 240.0)
    } else if dtype.is_temporal() {
        (130.0, 280.0)
    } else if dtype.is_string() {
        (120.0, 480.0)
    } else {
        (110.0, 360.0)
    };
    let max_chars = values
        .map(|value| value.chars().count())
        .fold(name.chars().count(), usize::max)
        .min(64);
    let preferred_width = (max_chars as f32 * 7.5 + 40.0).clamp(min_width, max_width);

    (min_width, preferred_width, max_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn formats_cells_for_display() {
        assert_eq!(format_cell_value(&AnyValue::Float64(12.340000)), "12.34");
        assert_eq!(
            format_cell_value(&AnyValue::Float64(0.000012)),
            "1.200000e-5"
        );
        assert_eq!(format_cell_value(&AnyValue::String("value")), "value");
        assert_eq!(format_cell_value(&AnyValue::Null), "");
    }

    #[test]
    fn constrains_inferred_column_widths() {
        assert_eq!(
            column_widths("enabled", &DataType::Boolean, [].into_iter()),
            (80.0, 92.5, 140.0)
        );
        assert_eq!(
            column_widths("name", &DataType::String, ["x".repeat(200)].into_iter()),
            (120.0, 480.0, 480.0)
        );
    }
}
