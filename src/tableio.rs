use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, bail};

use duckdb::{Connection, arrow::datatypes::DataType, params_from_iter, types::ValueRef};
use gpui_kit::SharedString;

use crate::excel;

#[derive(Clone)]
pub struct TableData {
    connection: Arc<Mutex<Connection>>,
    pub columns: Vec<TableColumn>,
}

#[derive(Clone)]
pub struct TableColumn {
    pub name: String,
    pub kind: ColumnKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    Boolean,
    Number,
    Float,
    Temporal,
    Text,
    Blob,
    Other,
}

pub struct TableRows {
    pub rows: Vec<Vec<Option<String>>>,
}

#[derive(Debug, PartialEq, Eq)]
enum FileType {
    Csv,
    Tsv,
    Parquet,
    Json,
    Xlsx,
    Sqlite,
}

fn file_type(path: &Path) -> Result<FileType> {
    let extension = path
        .extension()
        .context("Missing file extension")?
        .to_string_lossy()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => Ok(FileType::Csv),
        "tsv" => Ok(FileType::Tsv),
        "parquet" => Ok(FileType::Parquet),
        "json" | "jsonl" | "ndjson" => Ok(FileType::Json),
        "xlsx" => Ok(FileType::Xlsx),
        "db" | "sqlite" | "sqlite3" | "gpkg" => Ok(FileType::Sqlite),
        ext => bail!("Unsupported table format: {ext}"),
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn layers_for_path(path: &Path) -> Result<Vec<SharedString>> {
    match file_type(path)? {
        FileType::Xlsx => Ok(excel::sheet_names(path)?
            .into_iter()
            .map(Into::into)
            .collect()),
        FileType::Sqlite => {
            let conn = Connection::open_in_memory()?;
            attach_sqlite(&conn, path)?;
            let mut statement = conn.prepare(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_catalog = 'source' AND table_schema = 'main' \
                 AND table_type = 'BASE TABLE' ORDER BY table_name",
            )?;
            let names = statement.query_map([], |row| row.get::<_, String>(0))?;
            Ok(names
                .collect::<duckdb::Result<Vec<_>>>()?
                .into_iter()
                .map(Into::into)
                .collect())
        }
        FileType::Csv | FileType::Tsv | FileType::Parquet | FileType::Json => Ok(vec![
            path.file_stem()
                .context("Missing file name")?
                .to_string_lossy()
                .into_owned()
                .into(),
        ]),
    }
}

fn attach_sqlite(conn: &Connection, path: &Path) -> Result<()> {
    conn.execute_batch("LOAD sqlite_scanner")
        .context("Cannot load SQLite support from DuckDB")?;
    // ATTACH cannot bind its filename; escape the local path as a SQL string.
    let path = path.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!(
        "ATTACH '{path}' AS source (TYPE sqlite, READ_ONLY)"
    ))?;
    Ok(())
}

pub fn layer_data(path: &Path, layer: &str) -> Result<TableData> {
    log::debug!("Read table: {}", path.display());
    let conn = Connection::open_in_memory()?;
    match file_type(path)? {
        FileType::Csv | FileType::Tsv => {
            conn.execute(
                "CREATE TABLE data AS SELECT * FROM read_csv(?)",
                [path.to_string_lossy().as_ref()],
            )?;
        }
        FileType::Parquet => {
            conn.execute(
                "CREATE TABLE data AS SELECT * FROM read_parquet(?)",
                [path.to_string_lossy().as_ref()],
            )?;
        }
        FileType::Json => {
            conn.execute(
                "CREATE TABLE data AS SELECT * FROM read_json_auto(?)",
                [path.to_string_lossy().as_ref()],
            )?;
        }
        FileType::Sqlite => {
            attach_sqlite(&conn, path)?;
            conn.execute(
                &format!(
                    "CREATE TABLE data AS SELECT * FROM source.main.{}",
                    quoted(layer)
                ),
                [],
            )?;
        }
        FileType::Xlsx => {
            excel::load(&conn)?;
            conn.execute(
                "CREATE TABLE data AS SELECT * FROM read_xlsx(?, sheet = ?, header = true)",
                [path.to_string_lossy().as_ref(), layer],
            )?;
        }
    }

    let columns = {
        let mut statement = conn.prepare("SELECT * FROM data LIMIT 0")?;
        let _ = statement.query([])?;
        statement
            .column_names()
            .into_iter()
            .enumerate()
            .map(|(ix, name)| {
                let kind = match statement.column_type(ix) {
                    DataType::Boolean => ColumnKind::Boolean,
                    DataType::Int8
                    | DataType::Int16
                    | DataType::Int32
                    | DataType::Int64
                    | DataType::UInt8
                    | DataType::UInt16
                    | DataType::UInt32
                    | DataType::UInt64
                    | DataType::Decimal128(_, _)
                    | DataType::Decimal256(_, _) => ColumnKind::Number,
                    DataType::Float16 | DataType::Float32 | DataType::Float64 => ColumnKind::Float,
                    DataType::Date32
                    | DataType::Date64
                    | DataType::Time32(_)
                    | DataType::Time64(_)
                    | DataType::Timestamp(_, _) => ColumnKind::Temporal,
                    DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => ColumnKind::Text,
                    DataType::Binary
                    | DataType::LargeBinary
                    | DataType::BinaryView
                    | DataType::FixedSizeBinary(_) => ColumnKind::Blob,
                    _ => ColumnKind::Other,
                };
                TableColumn { name, kind }
            })
            .collect()
    };
    log::debug!("Read table done");
    Ok(TableData {
        connection: Arc::new(Mutex::new(conn)),
        columns,
    })
}

impl TableData {
    pub fn query(
        &self,
        filters: &[(usize, String)],
        sort: Option<(usize, bool)>,
    ) -> Result<TableRows> {
        let select = self
            .columns
            .iter()
            .map(|column| {
                let name = quoted(&column.name);
                if column.kind == ColumnKind::Blob {
                    format!("CASE WHEN {name} IS NULL THEN NULL ELSE concat(octet_length({name}), ' bytes') END")
                } else {
                    format!("CAST({name} AS VARCHAR)")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let where_clause = filters
            .iter()
            .map(|(ix, _)| {
                format!(
                    "contains(lower(CAST({} AS VARCHAR)), lower(?))",
                    quoted(&self.columns[*ix].name)
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        let order = match sort {
            Some((ix, descending)) => format!(
                "{} {} NULLS LAST, rowid",
                quoted(&self.columns[ix].name),
                if descending { "DESC" } else { "ASC" }
            ),
            None => "rowid".to_string(),
        };
        let sql = format!(
            "SELECT {select} FROM data {} ORDER BY {order}",
            if where_clause.is_empty() {
                String::new()
            } else {
                format!("WHERE {where_clause}")
            }
        );
        let conn = self.connection.lock().expect("DuckDB connection poisoned");
        let mut statement = conn.prepare(&sql)?;
        let mut result =
            statement.query(params_from_iter(filters.iter().map(|(_, value)| value)))?;
        let mut rows = Vec::new();
        while let Some(row) = result.next()? {
            let mut cells = Vec::with_capacity(self.columns.len());
            for (ix, column) in self.columns.iter().enumerate() {
                let value = match row.get_ref(ix)? {
                    ValueRef::Null => None,
                    ValueRef::Text(bytes) => {
                        let text = std::str::from_utf8(bytes)?.to_owned();
                        if column.kind == ColumnKind::Float {
                            Some(format_float(text.parse()?))
                        } else {
                            Some(text)
                        }
                    }
                    _ => unreachable!("selected values are cast to VARCHAR"),
                };
                cells.push(value);
            }
            rows.push(cells);
        }
        Ok(TableRows { rows })
    }
}

fn format_float(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value != 0.0 && !(0.0001..1_000_000_000.0).contains(&value.abs()) {
        return format!("{value:.6e}");
    }
    let mut text = format!("{value:.6}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn recognizes_file_types_and_aliases() {
        for (extension, expected) in [
            ("csv", FileType::Csv),
            ("TSV", FileType::Tsv),
            ("parquet", FileType::Parquet),
            ("json", FileType::Json),
            ("jsonl", FileType::Json),
            ("ndjson", FileType::Json),
            ("XLSX", FileType::Xlsx),
            ("db", FileType::Sqlite),
            ("sqlite", FileType::Sqlite),
            ("sqlite3", FileType::Sqlite),
            ("gpkg", FileType::Sqlite),
        ] {
            assert_eq!(
                file_type(Path::new(&format!("table.{extension}"))).unwrap(),
                expected
            );
        }
        assert!(
            file_type(Path::new("table.xls"))
                .unwrap_err()
                .to_string()
                .contains("xls")
        );
        assert_eq!(
            file_type(Path::new("table")).unwrap_err().to_string(),
            "Missing file extension"
        );
    }

    #[test]
    fn reads_multiple_xlsx_sheets_without_installing_extensions() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false",
        )
        .unwrap();
        assert_eq!(
            conn.query_row("SELECT version()", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "v1.5.5"
        );
        crate::excel::load(&conn).unwrap();

        let path = std::env::temp_dir().join(format!("tabulite-{}.xlsx", std::process::id()));
        let result = (|| -> Result<_> {
            let file = std::fs::File::create(&path)?;
            let mut zip = ZipWriter::new(file);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("[Content_Types].xml", options)?;
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#)?;
            zip.start_file("_rels/.rels", options)?;
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)?;
            zip.start_file("xl/workbook.xml", options)?;
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sales &amp; Costs" sheetId="1" r:id="rId1"/><sheet name="Other" sheetId="2" r:id="rId2"/></sheets></workbook>"#)?;
            zip.start_file("xl/_rels/workbook.xml.rels", options)?;
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#)?;
            zip.start_file("xl/worksheets/sheet1.xml", options)?;
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:B3"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>name</t></is></c><c r="B1" t="inlineStr"><is><t>amount</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Alice</t></is></c><c r="B2"><v>10</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Bob</t></is></c><c r="B3"><v>2</v></c></row></sheetData></worksheet>"#)?;
            zip.start_file("xl/worksheets/sheet2.xml", options)?;
            zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:A2"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>label</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>second</t></is></c></row></sheetData></worksheet>"#)?;
            zip.finish()?;

            let layers = layers_for_path(&path)?;
            let sales = layer_data(&path, layers[0].as_ref())?;
            let other = layer_data(&path, layers[1].as_ref())?;
            Ok((layers, sales, other))
        })();
        std::fs::remove_file(&path).unwrap();
        let (layers, sales, other) = result.unwrap();
        assert_eq!(
            layers
                .iter()
                .map(|name| name.as_ref())
                .collect::<Vec<&str>>(),
            vec!["Sales & Costs", "Other"]
        );
        assert_eq!(
            sales
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "amount"]
        );
        assert_eq!(
            sales.query(&[], None).unwrap().rows,
            vec![
                vec![Some("Alice".into()), Some("10".into())],
                vec![Some("Bob".into()), Some("2".into())],
            ]
        );
        assert_eq!(
            sales.query(&[(0, "ali".into())], None).unwrap().rows,
            vec![vec![Some("Alice".into()), Some("10".into())]]
        );
        assert_eq!(
            sales.query(&[], Some((1, false))).unwrap().rows[0][0].as_deref(),
            Some("Bob")
        );
        assert_eq!(other.columns[0].name, "label");
        assert_eq!(
            other.query(&[], None).unwrap().rows,
            vec![vec![Some("second".into())]]
        );
    }

    #[test]
    fn reads_sqlite_tables_read_only_with_duckdb() {
        let path = std::env::temp_dir().join(format!("tabulite-sqlite-{}.db", std::process::id()));
        let result = (|| -> Result<_> {
            let conn = Connection::open_in_memory()?;
            conn.execute_batch(
                "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false; \
                 LOAD sqlite_scanner",
            )?;
            conn.execute_batch(&format!(
                "ATTACH '{}' AS fixture (TYPE sqlite); \
                 CREATE TABLE fixture.metadata (name TEXT, amount INTEGER); \
                 INSERT INTO fixture.metadata VALUES ('Alice', 10), ('Bob', 2); \
                 CREATE TABLE fixture.\"tile\"\"data\" (zoom INTEGER, tile_data BLOB); \
                 INSERT INTO fixture.\"tile\"\"data\" VALUES (1, from_hex('010203')); \
                 DETACH fixture",
                path.display()
            ))?;
            drop(conn);

            let layers = layers_for_path(&path)?;
            let metadata = layer_data(&path, layers[0].as_ref())?;
            let tiles = layer_data(&path, layers[1].as_ref())?;
            Ok((layers, metadata, tiles))
        })();
        let _ = std::fs::remove_file(&path);
        let (layers, metadata, tiles) = result.unwrap();
        assert_eq!(
            layers
                .iter()
                .map(|layer| layer.as_ref())
                .collect::<Vec<&str>>(),
            vec!["metadata", "tile\"data"]
        );
        assert_eq!(
            metadata
                .columns
                .iter()
                .map(|column| (&*column.name, column.kind))
                .collect::<Vec<_>>(),
            vec![("name", ColumnKind::Text), ("amount", ColumnKind::Number)]
        );
        assert_eq!(
            metadata.query(&[], None).unwrap().rows,
            vec![
                vec![Some("Alice".into()), Some("10".into())],
                vec![Some("Bob".into()), Some("2".into())],
            ]
        );
        assert_eq!(
            metadata.query(&[(0, "ali".into())], None).unwrap().rows,
            vec![vec![Some("Alice".into()), Some("10".into())]]
        );
        assert_eq!(
            metadata.query(&[], Some((1, false))).unwrap().rows[0][0].as_deref(),
            Some("Bob")
        );
        assert_eq!(
            tiles
                .columns
                .iter()
                .map(|column| column.kind)
                .collect::<Vec<_>>(),
            vec![ColumnKind::Number, ColumnKind::Blob]
        );
        assert_eq!(
            tiles.query(&[], None).unwrap().rows,
            vec![vec![Some("1".into()), Some("3 bytes".into())]]
        );
    }

    #[test]
    fn reads_csv_file_with_inferred_types() {
        let path = std::env::temp_dir().join(format!("tabulite-{}.csv", std::process::id()));
        std::fs::write(&path, "name,amount,enabled\nAlice,10,true\nBob,2,false\n").unwrap();
        let result = (|| -> Result<_> {
            let layers = layers_for_path(&path)?;
            let table = layer_data(&path, layers[0].as_ref())?;
            Ok((
                layers,
                table
                    .columns
                    .iter()
                    .map(|column| (column.name.clone(), column.kind))
                    .collect::<Vec<_>>(),
                table.query(&[], None)?.rows,
                table.query(&[(0, "ali".into())], None)?.rows,
                table.query(&[], Some((1, false)))?.rows,
            ))
        })();
        std::fs::remove_file(&path).unwrap();
        let (layers, columns, rows, filtered, sorted) = result.unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(
            layers[0].as_ref(),
            path.file_stem().unwrap().to_str().unwrap()
        );
        assert_eq!(
            columns,
            vec![
                ("name".into(), ColumnKind::Text),
                ("amount".into(), ColumnKind::Number),
                ("enabled".into(), ColumnKind::Boolean),
            ]
        );
        assert_eq!(
            rows,
            vec![
                vec![Some("Alice".into()), Some("10".into()), Some("true".into())],
                vec![Some("Bob".into()), Some("2".into()), Some("false".into())],
            ]
        );
        assert_eq!(filtered, vec![rows[0].clone()]);
        assert_eq!(sorted[0][0].as_deref(), Some("Bob"));
    }

    #[test]
    fn filters_are_literal_case_insensitive_and_sort_is_numeric() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE data (\"a\"\"b\" VARCHAR, amount INTEGER); INSERT INTO data VALUES ('A_%', 10), ('aX%', 2), (NULL, NULL)").unwrap();
        let table = TableData {
            connection: Arc::new(Mutex::new(conn)),
            columns: vec![
                TableColumn {
                    name: "a\"b".into(),
                    kind: ColumnKind::Text,
                },
                TableColumn {
                    name: "amount".into(),
                    kind: ColumnKind::Number,
                },
            ],
        };
        assert_eq!(
            table.query(&[(0, "a_%".into())], None).unwrap().rows,
            vec![vec![Some("A_%".into()), Some("10".into())]]
        );
        assert_eq!(
            table.query(&[], Some((1, false))).unwrap().rows[0][1].as_deref(),
            Some("2")
        );
    }
}
