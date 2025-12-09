use gpui::{App, IntoElement, Window};
use polars::{
    frame::DataFrame,
    prelude::{AnyValue, IntoLazy, PlSmallStr, col, lit},
};

use gpui::*;
use gpui_component::{
    Icon, IconName, Sizable, StyledExt, ActiveTheme,
    tag::Tag,
    input::{Input, InputEvent, InputState},
    table::{Column, ColumnSort, TableDelegate, TableState},
};

#[derive(Default)]
pub struct TableLayer {
    data: polars::frame::DataFrame,
    original_data: polars::frame::DataFrame,
    filter_enabled: bool,
    filter_inputs: Vec<Entity<InputState>>,
    input_subscriptions: Vec<Subscription>,
    columns: Vec<Column>,
}

const NULL: &'static str = "null";

impl TableLayer {
    pub fn update_data(&mut self, data: polars::frame::DataFrame) {
        self.original_data = data.clone();
        self.data = data;
        self.create_column_info();
    }

    pub fn toggle_filter(&mut self) {
        self.filter_enabled = !self.filter_enabled;
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
                            .contains(lit(filter_text.to_lowercase()), true /* literal, use false for regex support*/);

                        lazy_df = lazy_df.filter(filter_expr);
                    }

                    lazy_df.collect().ok()
                })
                .await;

            // Update the data on the UI thread
            if let Some(filtered) = filtered_data {
                let _ = table_state
                    .update(cx, |table_state, cx| {
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
            },
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

    fn column(&self, col_ix: usize, _: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn render_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let mut div = div().id("header");
        if self.filter_enabled {
            if self.filter_inputs.is_empty() {
                for _ in 0..self.columns.len() {
                    let input = cx.new(|cx| InputState::new(window, cx).clean_on_escape());
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

            div = div.h_12()
        }

        div
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let mut div = div()
            .v_flex()
            .size_full()
            .child(self.column(col_ix, cx).name.clone());

        if self.filter_enabled {
            div = div.child(
                Input::new(&self.filter_inputs.get(col_ix).expect("BUG: column index"))
                    .prefix(Icon::new(IconName::Search))
                    .text_xs()
                    .xsmall(),
            );
        }

        div
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut gpui::Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        match self.data[col_ix].get(row_ix) {
            Ok(AnyValue::String(str)) => div().child(SharedString::new(str)),
            Ok(AnyValue::StringOwned(str)) => div().child(SharedString::new(str.as_str())),
            Ok(AnyValue::Null) => div()
                .flex()
                .justify_center()
                .child(Tag::secondary().outline().xsmall().child(NULL))
                .text_color(cx.theme().accent),
            Ok(val) =>
                div().child(SharedString::new(val.to_string()))
            ,
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
