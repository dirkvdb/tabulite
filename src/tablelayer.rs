use gpui::{App, IntoElement, Window};
use polars::prelude::AnyValue;

use gpui::*;
use gpui_component::table::{Column, TableDelegate, TableState};

#[derive(Clone, Default)]
pub struct TableLayer {
    data: polars::frame::DataFrame,
    columns: Vec<Column>,
}

impl TableLayer {
    pub fn update_data(&mut self, data: polars::frame::DataFrame) {
        self.data = data;
        self.create_column_info();
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
            })
            .collect();
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
}
