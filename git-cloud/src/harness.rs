//! Load rows from `data/test-files.csv` (tab-separated).

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use csv::ReaderBuilder;

/// One row from the grit harness CSV (subset of columns).
#[derive(Debug)]
pub struct CsvRow {
    pub file: String,
    pub group: String,
    pub in_scope: String,
    pub tests_total: i64,
    pub passed_last: i64,
    pub fully_passing: bool,
}

pub fn read_test_files_csv(repo: &Path) -> Result<Vec<CsvRow>> {
    let path = repo.join("data/test-files.csv");
    let f = File::open(&path).with_context(|| format!("open {}", path.display()))?;
    let mut rdr = ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .from_reader(f);
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.context("csv record")?;
        let file = rec
            .get(0)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let Some(file) = file else {
            continue;
        };
        let group = rec.get(1).unwrap_or("").trim().to_string();
        let in_scope = rec.get(2).unwrap_or("yes").trim().to_lowercase();
        let tests_total = rec.get(3).unwrap_or("0").trim().parse::<i64>().unwrap_or(0);
        let passed_last = rec.get(4).unwrap_or("0").trim().parse::<i64>().unwrap_or(0);
        let fully_passing = rec
            .get(6)
            .unwrap_or("false")
            .trim()
            .eq_ignore_ascii_case("true");
        out.push(CsvRow {
            file,
            group,
            in_scope,
            tests_total,
            passed_last,
            fully_passing,
        });
    }
    Ok(out)
}

/// Returns the CSV row for a test file stem (e.g. `t0000-basic`), if present.
pub fn row_for_file<'a>(rows: &'a [CsvRow], stem: &str) -> Option<&'a CsvRow> {
    rows.iter().find(|r| r.file == stem)
}

/// Rows that should be tracked: in harness scope and not yet fully passing.
pub fn incomplete_in_scope_rows<'a>(rows: &'a [CsvRow]) -> impl Iterator<Item = &'a CsvRow> + 'a {
    rows.iter()
        .filter(|r| r.in_scope != "skip" && !r.fully_passing)
}

/// Re-read passed and total counts for a test stem after a harness run updates the CSV.
pub fn stats_for_file(repo: &Path, stem: &str) -> Result<(i64, i64)> {
    let rows = read_test_files_csv(repo)?;
    for r in rows {
        if r.file == stem {
            return Ok((r.passed_last, r.tests_total));
        }
    }
    Ok((0, 0))
}
