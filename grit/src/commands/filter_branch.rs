//! `grit filter-branch` — rewrite branches (deprecated).
//!
//! This command is a stub that prints a deprecation warning directing
//! users to `git filter-repo` instead.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Write};

/// Arguments for `grit filter-branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Rewrite branches (deprecated — use git-filter-repo)")]
pub struct Args {
    /// Environment filter.
    #[arg(long = "env-filter")]
    pub env_filter: Option<String>,

    /// Tree filter.
    #[arg(long = "tree-filter")]
    pub tree_filter: Option<String>,

    /// Index filter.
    #[arg(long = "index-filter")]
    pub index_filter: Option<String>,

    /// Parent filter.
    #[arg(long = "parent-filter")]
    pub parent_filter: Option<String>,

    /// Message filter.
    #[arg(long = "msg-filter")]
    pub msg_filter: Option<String>,

    /// Commit filter.
    #[arg(long = "commit-filter")]
    pub commit_filter: Option<String>,

    /// Tag name filter.
    #[arg(long = "tag-name-filter")]
    pub tag_name_filter: Option<String>,

    /// Subdirectory filter.
    #[arg(long = "subdirectory-filter")]
    pub subdirectory_filter: Option<String>,

    /// Prune empty commits.
    #[arg(long = "prune-empty")]
    pub prune_empty: bool,

    /// Force operation (override safety checks).
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Revision range.
    #[arg(trailing_var_arg = true)]
    pub revisions: Vec<String>,
}

pub fn run(_args: Args) -> Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(
        stderr,
        "WARNING: git filter-branch is deprecated and has many pitfalls."
    )?;
    writeln!(stderr)?;
    writeln!(
        stderr,
        "It is recommended to use `git filter-repo` instead."
    )?;
    writeln!(
        stderr,
        "See: https://github.com/newren/git-filter-repo"
    )?;
    writeln!(stderr)?;
    writeln!(
        stderr,
        "To install: pip install git-filter-repo"
    )?;
    writeln!(
        stderr,
        "Or see https://github.com/newren/git-filter-repo/blob/main/INSTALL.md"
    )?;

    // Exit with error to indicate the command is not implemented
    std::process::exit(1);
}
