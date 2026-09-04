use polars::{
    frame::DataFrame,
    prelude::{AnyValue, IntoLazy, PlSmallStr, col, lit},
};

use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable,
    input::{Escape, Input, InputEvent, InputState},
    table::{Column, ColumnGroup, ColumnSort, TableDelegate, TableState},
    tag::Tag,
};
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

    pub fn set_table_focus_handle(&mut self, focus_handle: FocusHandle) {
        self.table_focus_handle = Some(focus_handle);
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

    fn ensure_filter_inputs(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>) {
        if !self.filter_inputs.is_empty() {
            return;
        }

        for _ in 0..self.columns.len() {
            let input = cx.new(|cx| InputState::new(window, cx));
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
        let schema = self.data.schema();
        self.columns = schema
            .iter()
            .map(|(name, _dtype)| {
                let name = SharedString::new(name.as_str());
                Column {
                    key: name.clone(),
                    name,
                    ..Default::default()
                }
                .sortable()
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
            div()
                .size_full()
                .child(
                    Input::new(&self.filter_inputs.get(col_ix).expect("BUG: column index"))
                        .prefix(Icon::new(IconName::Search))
                        .text_xs()
                        .xsmall(),
                )
                .on_action(move |_: &Escape, window, cx| {
                    if let Some(focus_handle) = &table_focus_handle {
                        focus_handle.focus(window, cx);
                    } else {
                        window.blur(cx);
                    }
                })
        } else {
            div()
                .size_full()
                .child(self.column(col_ix, cx).name.clone())
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        match self.data[col_ix].get(row_ix) {
            Ok(AnyValue::String(str)) => div().child(SharedString::new(str)),
            Ok(AnyValue::StringOwned(str)) => div().child(SharedString::new(str.as_str())),
            Ok(AnyValue::Null) => div()
                .flex()
                .justify_center()
                .child(Tag::secondary().outline().xsmall().child(NULL))
                .text_color(cx.theme().accent),
            Ok(val) => div().child(SharedString::new(val.to_string())),
            Err(_) => div().child(SharedString::new("ERR")),
        }
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
