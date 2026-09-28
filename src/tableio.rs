use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, bail};

use duckdb::{Connection, arrow::datatypes::DataType, params_from_iter, types::ValueRef};
use gpui_kit::SharedString;

use crate::excel;

pub const ANY_COLUMN: usize = usize::MAX;

#[derive(Clone)]
pub struct TableData {
    connection: Arc<Mutex<Connection>>,
    pub columns: Vec<TableColumn>,
    pub loading_mode: LoadingMode,
    // SQLite BLOBs are represented by their byte lengths in the data view.
    blob_sizes: bool,
    count_source: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadingMode {
    Paged,
    Eager,
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

impl FileType {
    fn loading_mode(self) -> LoadingMode {
        match self {
            Self::Xlsx => LoadingMode::Eager,
            Self::Csv | Self::Tsv | Self::Parquet | Self::Json | Self::Sqlite => LoadingMode::Paged,
        }
    }
}

fn file_type(path: &Path) -> Result<FileType> {
    let extension = path
        .extension()
        .context("Missing file extension")?
        .to_string_lossy()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => Ok(FileType::Csv),
        "tsv" | "tab" => Ok(FileType::Tsv),
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

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(0);

fn data_connection() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    let temp = std::env::temp_dir().join(format!(
        "tabulite-duckdb-{}-{}",
        std::process::id(),
        NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)
    ));
    // Large sorted pages should use DuckDB's external sort rather than a TopN
    // heap whose size grows with OFFSET. Spill files must not use the app's cwd.
    conn.execute_batch(&format!(
        "SET memory_limit = '512MiB'; SET temp_directory = {}; SET disabled_optimizers = 'top_n'",
        sql_literal(&temp.to_string_lossy())
    ))?;
    Ok(conn)
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
    let path = sql_literal(&path.to_string_lossy());
    conn.execute_batch(&format!("ATTACH {path} AS source (TYPE sqlite, READ_ONLY)"))?;
    Ok(())
}

pub fn layer_data(path: &Path, layer: &str) -> Result<TableData> {
    log::debug!("Read table: {}", path.display());
    let conn = data_connection()?;
    let source_path = sql_literal(&path.to_string_lossy());
    let file_type = file_type(path)?;
    let source = match file_type {
        FileType::Csv | FileType::Tsv => format!("read_csv({source_path})"),
        FileType::Parquet => format!("read_parquet({source_path})"),
        FileType::Json => format!("read_json_auto({source_path})"),
        FileType::Sqlite => {
            attach_sqlite(&conn, path)?;
            format!("source.main.{}", quoted(layer))
        }
        FileType::Xlsx => {
            excel::load(&conn)?;
            format!(
                "read_xlsx({source_path}, sheet = {}, header = true)",
                sql_literal(layer)
            )
        }
    };
    conn.execute_batch(&format!("CREATE VIEW data AS SELECT * FROM {source}"))?;

    let mut sqlite_types = Vec::new();
    let columns: Vec<TableColumn> = {
        let mut statement = conn.prepare("SELECT * FROM data LIMIT 0")?;
        let _ = statement.query([])?;
        statement
            .column_names()
            .into_iter()
            .enumerate()
            .map(|(ix, name)| {
                let data_type = statement.column_type(ix);
                if file_type == FileType::Sqlite {
                    sqlite_types.push(match data_type {
                        DataType::Int8
                        | DataType::Int16
                        | DataType::Int32
                        | DataType::Int64
                        | DataType::UInt8
                        | DataType::UInt16
                        | DataType::UInt32
                        | DataType::UInt64 => "BIGINT",
                        DataType::Float16 | DataType::Float32 | DataType::Float64 => "DOUBLE",
                        DataType::Date32 | DataType::Date64 => "DATE",
                        DataType::Timestamp(_, _) => "TIMESTAMP",
                        DataType::Binary | DataType::LargeBinary | DataType::BinaryView => "BIGINT",
                        _ => "VARCHAR",
                    });
                }
                let kind = match data_type {
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
    let blob_sizes = file_type == FileType::Sqlite
        && columns.iter().any(|column| column.kind == ColumnKind::Blob);
    if blob_sizes {
        // The SQLite scanner copies a full vector (2048 rows) of BLOBs before
        // DuckDB applies LIMIT. Compute sizes inside SQLite to avoid loading the
        // payloads into DuckDB, including during sorting and filtering.
        let sqlite_columns = columns
            .iter()
            .map(|column| {
                let name = quoted(&column.name);
                if column.kind == ColumnKind::Blob {
                    format!("length({name}) AS {name}")
                } else {
                    name
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sqlite_sql = format!("SELECT {sqlite_columns} FROM {}", quoted(layer));
        let typed_columns = columns
            .iter()
            .zip(sqlite_types)
            .map(|(column, ty)| {
                let name = quoted(&column.name);
                format!("CAST({name} AS {ty}) AS {name}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        conn.execute_batch("DROP VIEW data")?;
        conn.execute_batch(&format!(
            "CREATE VIEW data AS SELECT {typed_columns} FROM sqlite_query('source', {})",
            sql_literal(&sqlite_sql)
        ))?;
    }
    log::debug!("Read table done");
    Ok(TableData {
        connection: Arc::new(Mutex::new(conn)),
        columns,
        loading_mode: file_type.loading_mode(),
        blob_sizes,
        count_source: blob_sizes.then(|| format!("source.main.{}", quoted(layer))),
    })
}

impl TableData {
    pub fn count(&self, filters: &[(usize, String)]) -> Result<usize> {
        let (where_clause, params) = self.where_clause(filters);
        // An unfiltered count can use the original table without evaluating
        // SQLite's BLOB-length projection for every row.
        let source = if filters.is_empty() {
            self.count_source.as_deref().unwrap_or("data")
        } else {
            "data"
        };
        let sql = format!("SELECT count(*) FROM {source} {where_clause}");
        let conn = self.connection.lock().expect("DuckDB connection poisoned");
        let count: i64 = conn.query_row(&sql, params_from_iter(&params), |row| row.get(0))?;
        Ok(usize::try_from(count)?)
    }

    fn where_clause(&self, filters: &[(usize, String)]) -> (String, Vec<String>) {
        let mut params = Vec::new();
        let conditions = filters
            .iter()
            .map(|(ix, value)| {
                if *ix == ANY_COLUMN {
                    let columns = self
                        .columns
                        .iter()
                        .map(|column| {
                            params.push(value.clone());
                            format!(
                                "contains(lower(CAST({} AS VARCHAR)), lower(?))",
                                quoted(&column.name)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" OR ");
                    return if columns.is_empty() {
                        "FALSE".into()
                    } else {
                        format!("({columns})")
                    };
                }
                let column = &self.columns[*ix];
                let name = quoted(&column.name);
                for op in [">=", "<=", "<>", "=", ">", "<"] {
                    if let Some(operand) = value.strip_prefix(op) {
                        let typed = matches!(
                            column.kind,
                            ColumnKind::Number | ColumnKind::Float | ColumnKind::Temporal
                        );
                        params.push(if typed || !matches!(op, "=" | "<>") {
                            operand.trim_start().to_owned()
                        } else {
                            operand.to_owned()
                        });
                        return if typed {
                            format!("{name} {op} TRY(cast_to_type(?, {name}))")
                        } else {
                            format!("CAST({name} AS VARCHAR) {op} ?")
                        };
                    }
                }
                if matches!(column.kind, ColumnKind::Number | ColumnKind::Float) {
                    if let Some((start, end)) = value.split_once('~') {
                        params.extend([start.trim().to_owned(), end.trim().to_owned()]);
                        return format!(
                            "{name} BETWEEN TRY(cast_to_type(?, {name})) AND TRY(cast_to_type(?, {name}))"
                        );
                    }
                }
                // LIKE treats '_' as a wildcard; only '%' is special in user input.
                let pattern = value.replace('\\', "\\\\").replace('_', "\\_");
                params.push(if value.contains('%') {
                    pattern
                } else {
                    format!("%{pattern}%")
                });
                format!("lower(CAST({name} AS VARCHAR)) LIKE lower(?) ESCAPE '\\'")
            })
            .collect::<Vec<_>>();
        if conditions.is_empty() {
            (String::new(), params)
        } else {
            (format!("WHERE {}", conditions.join(" AND ")), params)
        }
    }

    pub fn query_page(
        &self,
        filters: &[(usize, String)],
        sort: Option<(usize, bool)>,
        offset: usize,
        limit: usize,
    ) -> Result<TableRows> {
        self.query_rows(filters, sort, Some((offset, limit)))
    }

    pub fn query_all(
        &self,
        filters: &[(usize, String)],
        sort: Option<(usize, bool)>,
    ) -> Result<TableRows> {
        self.query_rows(filters, sort, None)
    }

    fn query_rows(
        &self,
        filters: &[(usize, String)],
        sort: Option<(usize, bool)>,
        page: Option<(usize, usize)>,
    ) -> Result<TableRows> {
        let select = self
            .columns
            .iter()
            .map(|column| {
                let name = quoted(&column.name);
                if column.kind == ColumnKind::Blob && !self.blob_sizes {
                    format!("CAST(octet_length({name}) AS VARCHAR)")
                } else {
                    format!("CAST({name} AS VARCHAR)")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let (where_clause, filter_params) = self.where_clause(filters);
        let order = match sort {
            Some((ix, descending)) => {
                // Page boundaries must not depend on the execution order of equal sort keys.
                // Rows with identical displayed values are interchangeable to the viewer.
                let tie_breaker = self
                    .columns
                    .iter()
                    .map(|column| {
                        let name = quoted(&column.name);
                        if column.kind == ColumnKind::Blob && !self.blob_sizes {
                            format!("octet_length({name})")
                        } else {
                            name
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "ORDER BY {} {} NULLS LAST, hash(row({tie_breaker}))",
                    quoted(&self.columns[ix].name),
                    if descending { "DESC" } else { "ASC" }
                )
            }
            None => String::new(),
        };
        let paging = if page.is_some() {
            "LIMIT ? OFFSET ?"
        } else {
            ""
        };
        let sql = format!("SELECT {select} FROM data {where_clause} {order} {paging}");
        let limit = page.map(|(_, limit)| i64::try_from(limit)).transpose()?;
        let offset = page.map(|(offset, _)| i64::try_from(offset)).transpose()?;
        let mut params: Vec<&dyn duckdb::ToSql> = filter_params
            .iter()
            .map(|value| value as &dyn duckdb::ToSql)
            .collect();
        if let (Some(limit), Some(offset)) = (&limit, &offset) {
            params.extend([limit as &dyn duckdb::ToSql, offset as &dyn duckdb::ToSql]);
        }
        let conn = self.connection.lock().expect("DuckDB connection poisoned");
        let mut statement = conn.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(params))?;
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
                        } else if column.kind == ColumnKind::Blob {
                            Some(format_bytes(text.parse()?))
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

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut unit = 0;
    let mut divisor = 1;
    while unit < UNITS.len() - 1 && bytes / divisor >= 1024 {
        divisor *= 1024;
        unit += 1;
    }
    let whole = bytes / divisor;
    if unit == 0 {
        return format!("{whole} B");
    }
    let tenths = (bytes % divisor) * 10 / divisor;
    if tenths == 0 {
        format!("{whole} {}", UNITS[unit])
    } else {
        format!("{whole}.{tenths} {}", UNITS[unit])
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
    use std::{io::Write, path::PathBuf};

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name)
    }

    #[test]
    fn formats_blob_sizes_compactly() {
        for (bytes, expected) in [
            (0, "0 B"),
            (3, "3 B"),
            (1023, "1023 B"),
            (1024, "1 KB"),
            (1536, "1.5 KB"),
            (1_048_575, "1023.9 KB"),
            (1_048_576, "1 MB"),
            (1_572_864, "1.5 MB"),
            (1_073_741_824, "1 GB"),
        ] {
            assert_eq!(format_bytes(bytes), expected);
        }
    }

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

        let path = fixture_path("multiple-sheets.xlsx");
        let layers = layers_for_path(&path).unwrap();
        let sales = layer_data(&path, layers[0].as_ref()).unwrap();
        let other = layer_data(&path, layers[1].as_ref()).unwrap();
        assert_eq!(sales.loading_mode, LoadingMode::Eager);
        assert_eq!(sales.query_all(&[], None).unwrap().rows.len(), 2);
        assert_eq!(
            sales
                .query_all(&[(0, "ali".into())], None)
                .unwrap()
                .rows
                .len(),
            1
        );
        assert_eq!(
            sales.query_all(&[], Some((1, false))).unwrap().rows[0][0].as_deref(),
            Some("Bob")
        );
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
            sales.query_page(&[], None, 0, 10).unwrap().rows,
            vec![
                vec![Some("Alice".into()), Some("10".into())],
                vec![Some("Bob".into()), Some("2".into())],
            ]
        );
        assert_eq!(
            sales
                .query_page(&[(0, "ali".into())], None, 0, 10)
                .unwrap()
                .rows,
            vec![vec![Some("Alice".into()), Some("10".into())]]
        );
        assert_eq!(
            sales.query_page(&[], Some((1, false)), 0, 1).unwrap().rows[0][0].as_deref(),
            Some("Bob")
        );
        assert_eq!(other.columns[0].name, "label");
        assert_eq!(
            other.query_page(&[], None, 0, 1).unwrap().rows,
            vec![vec![Some("second".into())]]
        );
        let all_other = other.query_all(&[], None).unwrap();
        assert_eq!(all_other.rows.len(), 150);
        assert_eq!(all_other.rows[149][0].as_deref(), Some("row_150"));
        assert_eq!(sales.count(&[]).unwrap(), 2);
        assert_eq!(
            sales.query_page(&[], None, 1, 1).unwrap().rows[0][0].as_deref(),
            Some("Bob")
        );
    }

    #[test]
    fn reads_sqlite_tables_read_only_with_duckdb() {
        let path = fixture_path("sqlite-tables.db");
        let layers = layers_for_path(&path).unwrap();
        let metadata = layer_data(&path, layers[0].as_ref()).unwrap();
        let tiles = layer_data(&path, layers[1].as_ref()).unwrap();
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
            metadata.query_page(&[], None, 0, 10).unwrap().rows,
            vec![
                vec![Some("Alice".into()), Some("10".into())],
                vec![Some("Bob".into()), Some("2".into())],
            ]
        );
        assert_eq!(
            metadata
                .query_page(&[(0, "ali".into())], None, 0, 10)
                .unwrap()
                .rows,
            vec![vec![Some("Alice".into()), Some("10".into())]]
        );
        assert_eq!(
            metadata
                .query_page(&[], Some((1, false)), 0, 1)
                .unwrap()
                .rows[0][0]
                .as_deref(),
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
            tiles.query_page(&[], Some((0, false)), 0, 10).unwrap().rows,
            vec![
                vec![Some("1".into()), Some("3 B".into())],
                vec![Some("2".into()), Some("1.5 KB".into())],
                vec![Some("3".into()), Some("1.5 MB".into())],
                vec![Some("4".into()), None],
            ]
        );
        assert_eq!(metadata.count(&[(0, "ali".into())]).unwrap(), 1);
        assert_eq!(
            metadata.query_page(&[], None, 1, 1).unwrap().rows[0][0].as_deref(),
            Some("Bob")
        );
    }

    #[test]
    fn pages_sqlite_blobs_without_materializing_payloads() {
        let path = fixture_path("large-files/paged-blobs.db");
        let result = (|| -> Result<()> {
            let table = layer_data(&path, "payload")?;
            assert_eq!(table.loading_mode, LoadingMode::Paged);
            // The scanner's vector would exceed this limit if it read the BLOBs.
            table
                .connection
                .lock()
                .unwrap()
                .execute_batch("SET memory_limit = '64MiB'")?;
            assert_eq!(table.count(&[])?, 80);
            let rows = table.query_page(&[], None, 0, 81)?.rows;
            assert_eq!(rows.len(), 80);
            assert_eq!(rows[0][1].as_deref(), Some("1 MB"));

            assert_eq!(
                table.query_page(&[], Some((0, true)), 0, 1)?.rows[0][0].as_deref(),
                Some("79")
            );
            assert_eq!(table.count(&[(1, ">1000000".into())])?, 80);
            Ok(())
        })();
        result.unwrap();
    }

    #[test]
    fn reads_csv_file_with_inferred_types() {
        let path = fixture_path("inferred-types.csv");
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
                table.query_page(&[], None, 0, 10)?.rows,
                table.query_page(&[(0, "ali".into())], None, 0, 10)?.rows,
                table.query_page(&[], Some((1, false)), 0, 10)?.rows,
            ))
        })();
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
    fn csv_view_reads_appended_rows_and_pages() {
        let path = std::env::temp_dir().join(format!(
            "tabulite-'lazy'-{}-{}.csv",
            std::process::id(),
            NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::copy(fixture_path("lazy.csv"), &path).unwrap();
        let result = (|| -> Result<()> {
            let table = layer_data(&path, "unused")?;
            assert_eq!(table.loading_mode, LoadingMode::Paged);
            assert_eq!(table.count(&[])?, 2);
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)?
                .write_all(&std::fs::read(fixture_path("lazy-appended.csv"))?)?;
            assert_eq!(table.count(&[])?, 3);
            assert_eq!(table.count(&[(0, "ar".into())])?, 1);
            assert_eq!(
                table.query_page(&[], Some((1, false)), 1, 1)?.rows,
                vec![vec![Some("Carol".into()), Some("3".into())]]
            );
            assert!(table.query_page(&[], None, 0, 0)?.rows.is_empty());
            assert!(table.query_page(&[], None, 10, 2)?.rows.is_empty());
            Ok(())
        })();
        std::fs::remove_file(&path).unwrap();
        result.unwrap();
    }

    #[test]
    fn sorted_page_boundaries_are_stable_for_equal_keys() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&std::fs::read_to_string(fixture_path("sorted-pages.sql")).unwrap())
            .unwrap();
        let table = TableData {
            connection: Arc::new(Mutex::new(conn)),
            loading_mode: LoadingMode::Paged,
            blob_sizes: false,
            count_source: None,
            columns: vec![
                TableColumn {
                    name: "name".into(),
                    kind: ColumnKind::Text,
                },
                TableColumn {
                    name: "amount".into(),
                    kind: ColumnKind::Number,
                },
            ],
        };
        let full = table.query_page(&[], Some((1, false)), 0, 4).unwrap().rows;
        assert_eq!(full[0][0].as_deref(), Some("Carol"));
        for (ix, row) in full.into_iter().enumerate() {
            assert_eq!(
                table.query_page(&[], Some((1, false)), ix, 1).unwrap().rows,
                vec![row]
            );
        }
    }

    fn filter_table() -> TableData {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&std::fs::read_to_string(fixture_path("filter-table.sql")).unwrap())
            .unwrap();
        TableData {
            connection: Arc::new(Mutex::new(conn)),
            loading_mode: LoadingMode::Paged,
            blob_sizes: false,
            count_source: None,
            columns: [
                ("a\"b", ColumnKind::Text),
                ("note", ColumnKind::Text),
                ("amount", ColumnKind::Number),
                ("price", ColumnKind::Float),
                ("day", ColumnKind::Temporal),
            ]
            .into_iter()
            .map(|(name, kind)| TableColumn {
                name: name.into(),
                kind,
            })
            .collect(),
        }
    }

    fn assert_filtered(table: &TableData, filters: &[(usize, String)], expected: &[&str]) {
        let rows = table.query_all(filters, Some((0, false))).unwrap().rows;
        let names = rows
            .iter()
            .map(|row| row[0].as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, expected, "filters: {filters:?}");
        assert_eq!(table.count(filters).unwrap(), expected.len());
        let pages = (0..=expected.len())
            .flat_map(|offset| {
                table
                    .query_page(filters, Some((0, false)), offset, 1)
                    .unwrap()
                    .rows
            })
            .collect::<Vec<_>>();
        assert_eq!(pages, rows, "filters: {filters:?}");
    }

    #[test]
    fn wildcard_filters_escape_underscores_and_backslashes() {
        let table = filter_table();
        for (filter, expected) in [
            ("alp", vec!["Alpha"]),
            ("A%", vec!["Alpha", "a_b"]),
            ("%b", vec!["Bob", "a_b"]),
            ("%b%", vec!["Bob", "a_b", "b%", "x\\back"]),
            ("a_%", vec!["a_b"]),
            ("%\\back", vec!["x\\back"]),
            ("b%", vec!["Bob", "b%"]),
            ("%", vec!["Alpha", "Bob", "a_b", "b%", "x\\back"]),
        ] {
            assert_filtered(&table, &[(0, filter.into())], &expected);
        }
    }

    #[test]
    fn exact_filters_are_case_sensitive_and_keep_wildcards_literal() {
        let table = filter_table();
        for (filter, expected) in [
            ("=Alpha", vec!["Alpha"]),
            ("=alpha", vec![]),
            ("=b%", vec!["b%"]),
            ("<>Alpha", vec!["Bob", "a_b", "b%", "x\\back"]),
            ("<>b%", vec!["Alpha", "Bob", "a_b", "x\\back"]),
            ("=' OR 1=1 --", vec![]),
        ] {
            assert_filtered(&table, &[(0, filter.into())], &expected);
        }
    }

    #[test]
    fn numeric_and_temporal_filters_use_native_order_and_tolerate_partial_input() {
        let table = filter_table();
        for (column, filter, expected) in [
            (2, ">5", vec!["a_b", "x\\back"]),
            (2, "=5", vec!["Bob"]),
            (2, "<>5", vec!["Alpha", "a_b", "b%", "x\\back"]),
            (2, "<=5", vec!["Alpha", "Bob", "b%"]),
            (2, "<2", vec!["b%"]),
            (2, ">=9007199254740993", vec!["x\\back"]),
            (2, ">9007199254740992", vec!["x\\back"]),
            (2, "9007199254740993~9007199254740993", vec!["x\\back"]),
            (2, "1~5", vec!["Alpha", "Bob", "b%"]),
            (2, "2~", vec![]),
            (2, ">-", vec![]),
            (3, ">2", vec!["Bob", "a_b", "b%", "x\\back"]),
            (3, "=2.50", vec!["a_b"]),
            (3, "1.5~2.5", vec!["Alpha", "a_b"]),
            (4, ">=2024-02-01", vec!["Bob", "a_b", "x\\back"]),
            (4, "<2024-01-01", vec!["b%"]),
            (4, "=2024-02-01", vec!["Bob"]),
            (4, ">2024-", vec![]),
            (0, ">Alpha", vec!["Bob", "a_b", "b%", "x\\back"]),
        ] {
            assert_filtered(&table, &[(column, filter.into())], &expected);
        }
    }

    #[test]
    fn multiple_columns_and_any_column_words_compose_with_and() {
        let table = filter_table();
        assert_filtered(&table, &[(0, "b%".into()), (2, ">=5".into())], &["Bob"]);
        assert_filtered(&table, &[(ANY_COLUMN, "ALP".into())], &["Alpha", "Bob"]);
        assert_filtered(
            &table,
            &[(ANY_COLUMN, "alp".into()), (ANY_COLUMN, "first".into())],
            &["Alpha"],
        );
        assert_filtered(
            &table,
            &[(ANY_COLUMN, "%".into()), (1, "=percent".into())],
            &["b%"],
        );
        assert_filtered(&table, &[(ANY_COLUMN, "a_b".into())], &["a_b"]);
        assert_filtered(&table, &[(ANY_COLUMN, "' OR 1=1 --".into())], &[]);
        assert_filtered(&table, &[(0, "%' OR 1=1 --".into())], &[]);
        assert_filtered(&table, &[(0, "=NULL".into())], &[]);
        assert_filtered(&table, &[(2, "1~5' OR 1=1 --".into())], &[]);
        assert_eq!(table.count(&[]).unwrap(), 6);
    }

    #[test]
    fn filters_are_literal_case_insensitive_and_sort_is_numeric() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&std::fs::read_to_string(fixture_path("literal-filters.sql")).unwrap())
            .unwrap();
        let table = TableData {
            connection: Arc::new(Mutex::new(conn)),
            loading_mode: LoadingMode::Paged,
            blob_sizes: false,
            count_source: None,
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
        assert_eq!(table.count(&[(0, "a_%".into())]).unwrap(), 1);
        assert_eq!(
            table
                .query_page(&[(0, "a_%".into())], None, 0, 10)
                .unwrap()
                .rows,
            vec![vec![Some("A_%".into()), Some("10".into())]]
        );
        assert_eq!(
            table.query_page(&[], Some((1, false)), 0, 1).unwrap().rows[0][1].as_deref(),
            Some("2")
        );
    }
}
