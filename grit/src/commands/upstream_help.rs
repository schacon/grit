//! Vendored `git/Documentation/*.adoc` synopsis strings for `git <cmd> -h` / `--help` (t0450).

use std::io::{self, Write};

mod synopsis_data {
    include!(concat!(env!("OUT_DIR"), "/upstream_help_synopsis.rs"));
}

/// Returns the raw synopsis block extracted from the vendored adoc for `cmd`, if present.
#[must_use]
pub(crate) fn synopsis_for_builtin(cmd: &str) -> Option<&'static str> {
    synopsis_data::synopsis_for_builtin(cmd)
}

/// Split adoc synopsis into usage variants: each variant starts with a `git …` line; following
/// lines are continuations (AsciiDoc tabs) until the next `git …` line.
pub(crate) fn synopsis_variants_from_adoc(syn: &str) -> Vec<Vec<String>> {
    let mut variants: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in syn.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("git ") && !current.is_empty() {
            variants.push(core::mem::take(&mut current));
        }
        current.push(trimmed.to_owned());
    }
    if !current.is_empty() {
        variants.push(current);
    }
    variants
}

/// Writes the same synopsis block as `-h` / `--help` to `w` (no trailing process exit).
pub(crate) fn write_upstream_synopsis(
    subcmd: &str,
    syn: &str,
    mut w: impl Write,
) -> io::Result<()> {
    let pad = " ".repeat(format!("git {subcmd} ").len());
    let variants = synopsis_variants_from_adoc(syn);
    for (i, var) in variants.iter().enumerate() {
        let Some(first) = var.first() else {
            continue;
        };
        if i == 0 {
            writeln!(w, "usage: {first}")?;
        } else {
            writeln!(w, "   or: {first}")?;
        }
        for cont in var.iter().skip(1) {
            writeln!(w, "{pad}{cont}")?;
        }
    }
    writeln!(w)?;
    Ok(())
}

/// Print `git <cmd> -h` synopsis (from vendored Documentation/*.adoc), then exit.
///
/// Continuation lines are padded with spaces to width `git <cmd> ` (same as t0450 `align_after_nl`).
///
/// Git's `-h` uses exit **129** (t0450). Long `--help` uses exit **0** so POSIX `sh` scripts can
/// chain `git <cmd> --help && grep …`.
pub(crate) fn print_upstream_synopsis_and_exit(subcmd: &str, syn: &str, exit_code: u8) -> ! {
    let _ = write_upstream_synopsis(subcmd, syn, io::stdout());
    std::process::exit(exit_code.into());
}

/// Same as [`print_upstream_synopsis_and_exit`] but writes to stderr.
pub(crate) fn eprint_upstream_synopsis_and_exit(subcmd: &str, syn: &str, exit_code: u8) -> ! {
    let _ = write_upstream_synopsis(subcmd, syn, io::stderr());
    std::process::exit(exit_code.into());
}
