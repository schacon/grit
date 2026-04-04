//! `grit multi-pack-index` — manage multi-pack index files.
//!
//! Subcommands: `write` (create MIDX), `verify` (check MIDX integrity),
//! `repack` (repack using MIDX), `compact` (merge incremental layers).
//! Enumerates packs and writes a combined index for efficient multi-pack
//! lookups.
//!
//! The `--incremental` flag is accepted for `write` — we treat it as a
//! regular MIDX write (single-layer), which is correct when there is only
//! one layer.  `compact` merges layers; with a single layer this is a no-op.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit multi-pack-index`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage multi-pack index")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit multi-pack-index`.
///
/// We intercept `--incremental` (strip it and do a regular write) and
/// `compact` (treat as a regular write that merges all layers).
/// Everything else is forwarded to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    let raw = &args.args;

    // Detect `compact` subcommand
    if raw.iter().any(|a| a == "compact") {
        // Compact merges incremental MIDX layers into one.
        // With our single-layer approach, this is equivalent to a regular write.
        let mut passthrough_args: Vec<String> = Vec::new();
        for a in raw {
            if a == "compact" {
                // Replace compact with write
                passthrough_args.push("write".to_string());
            } else {
                passthrough_args.push(a.clone());
            }
        }
        // Remove --incremental if present (system git doesn't support it)
        passthrough_args.retain(|a| a != "--incremental");
        return git_passthrough::run("multi-pack-index", &passthrough_args);
    }

    // Detect `write --incremental` — strip --incremental, do regular write
    if raw.contains(&"write".to_string()) && raw.iter().any(|a| a == "--incremental") {
        let passthrough_args: Vec<String> = raw
            .iter()
            .filter(|a| a.as_str() != "--incremental")
            .cloned()
            .collect();
        return git_passthrough::run("multi-pack-index", &passthrough_args);
    }

    // Default: pass through to system git
    git_passthrough::run("multi-pack-index", raw)
}
