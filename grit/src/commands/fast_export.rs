//! `grit fast-export` — export repository as a fast-import stream.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::fast_export::{export_stream, FastExportOptions};
use grit_lib::repo::Repository;
use std::io;

/// Arguments for `grit fast-export`.
#[derive(Debug, ClapArgs)]
#[command(about = "Export repository as fast-import stream")]
pub struct Args {
    /// Export all local branches (`refs/heads/`) and reachable objects.
    #[arg(long)]
    pub all: bool,

    /// Anonymize paths, identities, messages, and opaque OIDs.
    #[arg(long)]
    pub anonymize: bool,

    /// Map a token in anonymized output (`from` or `from:to`).
    #[arg(long = "anonymize-map", value_name = "MAP")]
    pub anonymize_map: Vec<String>,

    /// Raw arguments for compatibility (`--all`, `--anonymize`, etc. may appear here).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-export`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let mut all = args.all;
    let mut anonymize = args.anonymize;
    let mut maps = args.anonymize_map;
    let mut no_data = false;
    let mut use_done_feature = false;

    for a in &args.args {
        match a.as_str() {
            "--all" => all = true,
            "--anonymize" => anonymize = true,
            "--no-data" => no_data = true,
            "--use-done-feature" => use_done_feature = true,
            _ if a.starts_with("--anonymize-map=") => {
                maps.push(a.trim_start_matches("--anonymize-map=").to_string());
            }
            _ => {}
        }
    }

    if !all {
        anyhow::bail!("fast-export: --all is required");
    }

    let opts = FastExportOptions {
        all: true,
        anonymize,
        anonymize_maps: maps,
        no_data,
        use_done_feature,
    };

    let stdout = io::stdout().lock();
    export_stream(&repo, stdout, &opts).map_err(|e| anyhow::anyhow!("{e}"))
}
