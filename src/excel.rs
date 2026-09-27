use std::path::Path;

use anyhow::{Context as _, Result, bail};
use calamine::{Reader, Xlsx, open_workbook};
use duckdb::Connection;

pub fn sheet_names(path: &Path) -> Result<Vec<String>> {
    let workbook: Xlsx<_> = open_workbook(path)
        .with_context(|| format!("Cannot open XLSX workbook {}", path.display()))?;
    let names = workbook.sheet_names();
    if names.is_empty() {
        bail!("Workbook has no worksheets");
    }
    Ok(names)
}

pub fn load(conn: &Connection) -> Result<()> {
    // The Nix DuckDB library statically links Excel; never INSTALL or download extensions.
    conn.execute_batch("LOAD excel").context(
        "Cannot load the Excel extension from DuckDB; use the DuckDB build with Excel support",
    )?;
    Ok(())
}
