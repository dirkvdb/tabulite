use anyhow::Result;
use gpui::SharedString;
use std::path::{Path};

use geo::vector::dataframe::{DataFrameOptions, HeaderRow, create_dataframe_reader};

pub fn layers_for_path(path: &Path) -> Result<Vec<SharedString>> {
    let reader = create_dataframe_reader(path)?;
    Ok(reader.layer_names()?.iter().map(Into::into).collect())
}

pub fn layer_data(path: &Path, layer: &str) -> Result<polars::frame::DataFrame> {
    let df = geo::vector::dataframe::polars::read_dataframe(
        path,
        &DataFrameOptions {
            layer: Some(layer.to_string()),
            header_row: HeaderRow::Auto,
            ..Default::default()
        },
    )?;

    Ok(df)
}
