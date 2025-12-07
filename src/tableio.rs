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

// pub fn load_file(path: PathBuf) -> Result<Vec<TableLayer>> {
//     let mut layers = Vec::new();

//     let mut reader = create_dataframe_reader(&path)?;
//     for layer in reader.layer_names()? {
//         let opts = DataFrameOptions {
//             layer: Some(layer.clone()),
//             header_row: HeaderRow::Auto,
//             ..Default::default()
//         };

//         let columns = reader
//             .schema(&opts)?
//             .fields
//             .iter()
//             .map(|f| {
//                 let name = SharedString::new(f.name());
//                 Column {
//                     key: name.clone(),
//                     name,
//                     ..Default::default()
//                 }
//             })
//             .collect();

//         let mut column_data = Vec::new();
//         reader.iter_rows(&opts)?.for_each(|row| {
//             column_data.push(
//                 row.fields
//                     .into_iter()
//                     .map(|field| match field {
//                         Ok(field) => field,
//                         Err(_) => None,
//                     })
//                     .collect(),
//             );
//         });

//         layers.push(TableLayer {
//             name: layer.into(),
//             columns,
//             data: column_data,
//         });
//     }

//     Ok(layers)
// }
