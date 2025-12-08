use gpui::{App, IntoElement, Window};
use polars::{
    frame::DataFrame,
    prelude::{AnyValue, IntoLazy, PlSmallStr},
};

use gpui::*;
use gpui_component::{
    Icon, IconName, Sizable, StyledExt,
    input::{Input, InputEvent, InputState},
    table::{Column, ColumnSort, TableDelegate, TableState},
};

#[derive(Default)]
pub struct TableLayer {
    data: polars::frame::DataFrame,
    filter_enabled: bool,
    filter_inputs: Vec<Entity<InputState>>,
    input_subscriptions: Vec<Subscription>,
    columns: Vec<Column>,
}

impl TableLayer {
    pub fn update_data(&mut self, data: polars::frame::DataFrame) {
        self.data = data;
        self.create_column_info();
    }

    pub fn toggle_filter(&mut self) {
        self.filter_enabled = !self.filter_enabled;
    }

    fn on_filter_input_event(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<TableState<Self>>,
    ) {
        match event {
            InputEvent::Change => {
                let text = state.read(cx).value();
                println!("Change: {}", text)
                // if state == &self.input2 {
                //     println!("Set disabled value: {}", text);
                //     self.disabled_input.update(cx, |this, cx| {
                //         this.set_value(text, window, cx);
                //     })
                // } else {
                //     println!("Change: {}", text)
                // }
            }
            InputEvent::PressEnter { secondary } => println!("PressEnter secondary: {}", secondary),
            InputEvent::Focus => println!("Focus"),
            InputEvent::Blur => println!("Blur"),
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
        _: &mut gpui::Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        match self.data[col_ix].get(row_ix) {
            Ok(AnyValue::String(str)) => str.into(),
            Ok(val) => val.to_string(),
            Err(_) => "ERR".into(),
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
