//! `grit multi-pack-index` — manage multi-pack index files.
//!
//! Subcommands: `write` (create MIDX), `verify` (check MIDX integrity),
//! `repack` (repack using MIDX), `compact` (compact incremental layers).
//!
//! The `--incremental` flag for `write` is accepted for compatibility;
//! grit currently delegates to the system git for the actual MIDX write,
//! stripping flags that older system git versions don't support.

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
/// Handles `compact` subcommand (equivalent to a full `write`) and strips
/// `--incremental` when delegating to system git (which may not support it).
pub fn run(args: Args) -> Result<()> {
    // Handle "compact" subcommand: treat as a full rewrite
    if args.args.first().map(|s| s.as_str()) == Some("compact") {
        return git_passthrough::run("multi-pack-index", &["write".to_string()]);
    }

    // Strip --incremental flag for system git compatibility, but still
    // perform the write (a non-incremental write is a superset).
    let filtered: Vec<String> = args
        .args
        .iter()
        .filter(|a| a.as_str() != "--incremental")
        .cloned()
        .collect();

    git_passthrough::run("multi-pack-index", &filtered)
}
