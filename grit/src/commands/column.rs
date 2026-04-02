//! `grit column` — display data in columns.
//!
//! Reads lines from stdin and formats them into columns, similar to
//! `git column`. Useful for displaying lists (branches, tags, etc.)
//! in a compact columnar layout.
//!
//! Usage:
//!   echo -e "a\nb\nc\nd\ne" | grit column --mode=column --width=40

use anyhow::Result;
use clap::{Args as ClapArgs, ValueEnum};
use std::io::{self, BufRead, IsTerminal, Write};

/// Arguments for `grit column`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Display data in columns",
    override_usage = "grit column [--mode=<mode>] [--width=<n>] [--padding=<n>]"
)]
pub struct Args {
    /// Column layout mode.
    #[arg(long = "mode", default_value = "column")]
    pub mode: ColumnMode,

    /// Total display width (defaults to 80).
    #[arg(long = "width", default_value_t = 80)]
    pub width: usize,

    /// Padding between columns (defaults to 1).
    #[arg(long = "padding", default_value_t = 1)]
    pub padding: usize,

    /// Indentation prefix for each output line.
    #[arg(long = "indent", default_value = "")]
    pub indent: String,
}

/// Column display modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColumnMode {
    /// Always lay out in columns (fill columns top-to-bottom, then left-to-right).
    Always,
    /// Alias for always.
    Column,
    /// Fill rows left-to-right, then top-to-bottom.
    Row,
    /// One item per line (no columnar layout).
    Plain,
    /// One item per line.
    Never,
    /// Columnar if stdout is a terminal, plain otherwise.
    Auto,
}

/// Run the `column` command.
pub fn run(args: Args) -> Result<()> {
    let stdin = io::stdin();
    let items: Vec<String> = stdin
        .lock()
        .lines()
        .collect::<io::Result<Vec<_>>>()?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mode = match args.mode {
        // Auto → plain when piped (which it typically is when used in a pipeline).
        ColumnMode::Auto => {
            if io::stdout().is_terminal() {
                ColumnMode::Column
            } else {
                ColumnMode::Plain
            }
        }
        other => other,
    };

    match mode {
        ColumnMode::Plain | ColumnMode::Never => {
            for item in &items {
                writeln!(out, "{}{item}", args.indent)?;
            }
        }
        ColumnMode::Row => {
            format_rows(&items, args.width, args.padding, &args.indent, &mut out)?;
        }
        ColumnMode::Column | ColumnMode::Always | ColumnMode::Auto => {
            format_columns(&items, args.width, args.padding, &args.indent, &mut out)?;
        }
    }

    Ok(())
}

/// Format items filling rows left-to-right, wrapping when width exceeded.
fn format_rows(
    items: &[String],
    width: usize,
    padding: usize,
    indent: &str,
    out: &mut impl Write,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }

    let max_item = items.iter().map(|s| s.len()).max().unwrap_or(0);
    let col_width = max_item + padding;
    let usable = width.saturating_sub(indent.len());
    let num_cols = if col_width == 0 { 1 } else { (usable / col_width).max(1) };

    let mut col = 0;
    for item in items {
        if col == 0 {
            write!(out, "{indent}")?;
        }
        col += 1;
        if col < num_cols {
            write!(out, "{item:<width$}", width = col_width)?;
        } else {
            writeln!(out, "{item}")?;
            col = 0;
        }
    }
    if col != 0 {
        writeln!(out)?;
    }

    Ok(())
}

/// Format items filling columns top-to-bottom, then left-to-right.
fn format_columns(
    items: &[String],
    width: usize,
    padding: usize,
    indent: &str,
    out: &mut impl Write,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }

    let max_item = items.iter().map(|s| s.len()).max().unwrap_or(0);
    let col_width = max_item + padding;
    let usable = width.saturating_sub(indent.len());
    let num_cols = if col_width == 0 { 1 } else { (usable / col_width).max(1) };
    let num_rows = (items.len() + num_cols - 1) / num_cols;

    for row in 0..num_rows {
        write!(out, "{indent}")?;
        for col in 0..num_cols {
            let idx = col * num_rows + row;
            if idx >= items.len() {
                break;
            }
            let item = &items[idx];
            if col + 1 < num_cols && (col + 1) * num_rows + row < items.len() {
                write!(out, "{item:<width$}", width = col_width)?;
            } else {
                write!(out, "{item}")?;
            }
        }
        writeln!(out)?;
    }

    Ok(())
}
