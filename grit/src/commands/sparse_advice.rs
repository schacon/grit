//! Sparse-checkout advice messages (`advice.updateSparsePath`), shared by commands
//! that refuse to update skip-worktree or out-of-cone index entries.

use anyhow::Result;
use grit_lib::config::ConfigSet;
use std::io::Write;

/// Emit the standard "paths outside sparse-checkout" message.
///
/// Git always prints the header and pathspec lines; the hint block is gated by
/// `advice.updateSparsePath` (`advise_on_updating_sparse_paths` in Git's `advice.c`).
pub fn emit_sparse_path_advice(
    w: &mut impl Write,
    config: &ConfigSet,
    paths: &[String],
) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    writeln!(
        w,
        "The following paths and/or pathspecs matched paths that exist\n\
outside of your sparse-checkout definition, so will not be\n\
updated in the index:"
    )?;
    for p in paths {
        writeln!(w, "{p}")?;
    }
    if advice_update_sparse_path_enabled(config) {
        writeln!(
            w,
            "hint: If you intend to update such entries, try one of the following:\n\
hint: * Use the --sparse option.\n\
hint: * Disable or modify the sparsity rules.\n\
hint: Disable this message with \"git config set advice.updateSparsePath false\""
        )?;
    }
    Ok(())
}

/// Emit advice when paths were moved outside the cone but remain non-sparse due to local changes.
pub fn emit_dirty_sparse_advice(
    w: &mut impl Write,
    config: &ConfigSet,
    paths: &[String],
) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    writeln!(
        w,
        "The following paths have been moved outside the\n\
sparse-checkout definition but are not sparse due to local\n\
modifications."
    )?;
    for p in paths {
        writeln!(w, "{p}")?;
    }
    if advice_update_sparse_path_enabled(config) {
        writeln!(
            w,
            "hint: To correct the sparsity of these paths, do the following:\n\
hint: * Use \"git add --sparse <paths>\" to update the index\n\
hint: * Use \"git sparse-checkout reapply\" to apply the sparsity rules\n\
hint: Disable this message with \"git config set advice.updateSparsePath false\""
        )?;
    }
    Ok(())
}

/// Whether `advice.updateSparsePath` is enabled (honours `GIT_ADVICE`).
#[must_use]
pub fn advice_update_sparse_path_enabled(config: &ConfigSet) -> bool {
    if let Ok(v) = std::env::var("GIT_ADVICE") {
        if v == "0" || v.eq_ignore_ascii_case("false") {
            return false;
        }
        if v == "1" || v.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    config
        .get_bool("advice.updateSparsePath")
        .and_then(|r| r.ok())
        .unwrap_or(true)
}
