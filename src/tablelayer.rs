use gpui::{App, IntoElement, Window};
use polars::{
    frame::DataFrame,
    prelude::{AnyValue, IntoLazy, PlSmallStr},
};

use gpui::*;
use gpui_component::table::{Column, ColumnSort, TableDelegate, TableState};

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
                .sortable()
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
