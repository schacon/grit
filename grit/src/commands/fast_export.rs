//! `grit fast-export` — export repository as a fast-import stream.
//!
//! Emits a minimal stream: `feature done`, then `reset` + `from` for each ref
//! (`--all` and `refs/heads/`) or for `HEAD` only.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::io::Write;

/// Arguments for `grit fast-export`.
#[derive(Debug, ClapArgs)]
#[command(about = "Export repository as fast-import stream")]
pub struct Args {
    /// Export all local branches (`refs/heads/`).
    #[arg(long)]
    pub all: bool,

    /// Raw arguments (`--all` is also accepted here for compatibility).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-export`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let export_all = args.all || args.args.iter().any(|a| a == "--all");
    let mut out = std::io::stdout().lock();
    writeln!(out, "feature done")?;
    if export_all {
        let pairs = refs::list_refs(&repo.git_dir, "refs/heads/")?;
        for (name, oid) in pairs {
            writeln!(out, "reset {name}")?;
            writeln!(out, "from {oid}")?;
        }
    } else {
        match resolve_head(&repo.git_dir)? {
            HeadState::Branch {
                refname,
                oid: Some(oid),
                ..
            } => {
                writeln!(out, "reset {refname}")?;
                writeln!(out, "from {oid}")?;
            }
            HeadState::Detached { oid } => {
                writeln!(out, "reset HEAD")?;
                writeln!(out, "from {oid}")?;
            }
            _ => anyhow::bail!("cannot fast-export: HEAD does not resolve to a commit"),
        }
    }
    Ok(())
}
