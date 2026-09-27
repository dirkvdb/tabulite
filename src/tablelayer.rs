use crate::tableio::{ColumnKind, TableData, TableRows};
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
    data: Vec<Vec<Option<String>>>,
    source: Option<TableData>,
    sort: Option<(usize, bool)>,
    query_generation: u64,
    filter_enabled: bool,
    filter_inputs: Vec<Entity<InputState>>,
    input_subscriptions: Vec<Subscription>,
    columns: Vec<Column>,
    table_focus_handle: Option<FocusHandle>,
    selected_col: Option<usize>,
}

const NULL: &'static str = "null";

impl TableLayer {
    pub fn update_data(&mut self, data: TableData, rows: TableRows) {
        self.source = Some(data);
        self.data = rows.rows;
        self.sort = None;
        self.query_generation += 1;
        self.create_column_info();
    }

    pub fn columns_count(&self) -> usize {
        self.columns.len()
    }

    pub fn rows_count(&self) -> usize {
        self.data.len()
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
        let filters = self
            .filter_inputs
            .iter()
            .enumerate()
            .filter_map(|(ix, input)| {
                let text = input.read(cx).value().to_string();
                (!text.is_empty()).then_some((ix, text))
            })
            .collect();
        self.refresh_data(filters, cx);
    }

    fn refresh_data(&mut self, filters: Vec<(usize, String)>, cx: &mut Context<TableState<Self>>) {
        let Some(source) = self.source.clone() else {
            return;
        };
        self.query_generation += 1;
        let generation = self.query_generation;
        let sort = self.sort;
        cx.spawn(async move |table_state, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { source.query(&filters, sort) })
                .await;
            match result {
                Ok(rows) => {
                    let _ = table_state.update(cx, |table_state, cx| {
                        let delegate = table_state.delegate_mut();
                        if delegate.query_generation == generation {
                            delegate.data = rows.rows;
                            cx.notify();
                        }
                    });
                }
                Err(err) => log::error!("Failed to query table: {err}"),
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
        let Some(source) = &self.source else {
            return;
        };
        self.columns = source
            .columns
            .iter()
            .enumerate()
            .map(|(col_ix, column_info)| {
                let name = SharedString::new(column_info.name.as_str());
                let dtype = column_info.kind;
                let (min_width, preferred_width, max_width) = column_widths(
                    name.as_ref(),
                    dtype,
                    self.data
                        .iter()
                        .take(100)
                        .filter_map(|row| row[col_ix].clone()),
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

                if matches!(dtype, ColumnKind::Number | ColumnKind::Float) {
                    column = column.text_right();
                } else if dtype == ColumnKind::Boolean {
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
        debug_assert!(self.data.iter().all(|row| row.len() == self.columns.len()));
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.data.len()
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
        match self.data[row_ix][col_ix].as_ref() {
            None => div()
                .flex()
                .justify_center()
                .child(Tag::secondary().outline().xsmall().child(NULL))
                .text_color(cx.theme().accent),
            Some(value) => {
                let align = self.columns[col_ix].align;
                div()
                    .flex()
                    .items_center()
                    .size_full()
                    .truncate()
                    .when(align == TextAlign::Center, |this| this.justify_center())
                    .when(align == TextAlign::Right, |this| this.justify_end())
                    .child(SharedString::new(value.clone()))
            }
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        self.data[row_ix][col_ix].clone().unwrap_or_default()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.sort = match sort {
            ColumnSort::Ascending => Some((col_ix, false)),
            ColumnSort::Descending => Some((col_ix, true)),
            ColumnSort::Default => None,
        };
        self.filter_data(cx);
    }
}

fn column_widths(
    name: &str,
    dtype: ColumnKind,
    values: impl Iterator<Item = String>,
) -> (f32, f32, f32) {
    let (min_width, max_width) = if dtype == ColumnKind::Boolean {
        (80.0, 140.0)
    } else if matches!(dtype, ColumnKind::Number | ColumnKind::Float) {
        (90.0, 240.0)
    } else if dtype == ColumnKind::Temporal {
        (130.0, 280.0)
    } else if dtype == ColumnKind::Text {
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
    fn constrains_inferred_column_widths() {
        assert_eq!(
            column_widths("enabled", ColumnKind::Boolean, [].into_iter()),
            (80.0, 92.5, 140.0)
        );
        assert_eq!(
            column_widths("name", ColumnKind::Text, ["x".repeat(200)].into_iter()),
            (120.0, 480.0, 480.0)
        );
    }
}
