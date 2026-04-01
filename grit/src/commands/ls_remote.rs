//! `grit ls-remote` — list references from a local repository path.
//!
//! Only the **local path** transport is supported.  Network URLs are not
//! handled in v1.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::ls_remote::{ls_remote, Options};
use grit_lib::repo::Repository;
use std::path::Path;
use std::path::PathBuf;

/// Arguments for `grit ls-remote`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show only branches (`refs/heads/`).
    #[arg(long = "heads")]
    pub heads: bool,

    /// Show only tags (`refs/tags/`).
    #[arg(long = "tags")]
    pub tags: bool,

    /// Exclude pseudo-refs (HEAD) and peeled tag `^{}` entries.
    #[arg(long = "refs")]
    pub refs_only: bool,

    /// Show the symbolic ref that HEAD points to.
    #[arg(long = "symref")]
    pub symref: bool,

    /// Quiet: suppress output, only set the exit status.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Path to the local repository (bare or non-bare).
    #[arg(value_name = "REPOSITORY")]
    pub repository: PathBuf,

    /// Optional ref patterns; only matching refs are printed.
    #[arg(value_name = "PATTERN", num_args = 0..)]
    pub patterns: Vec<String>,
}

/// Run `grit ls-remote`.
///
/// Opens the repository at `args.repository`, enumerates its references
/// according to the supplied flags, and prints them to stdout as
/// `<oid>\t<refname>` lines, with HEAD first.
///
/// Exits with status 1 when no refs match (same behaviour as `git ls-remote`).
pub fn run(args: Args) -> Result<()> {
    let repo = open_local_repo(&args.repository)?;

    let opts = Options {
        heads: args.heads,
        tags: args.tags,
        refs_only: args.refs_only,
        symref: args.symref,
        patterns: args.patterns,
    };

    let entries = ls_remote(&repo.git_dir, &repo.odb, &opts)?;

    if entries.is_empty() {
        std::process::exit(1);
    }

    if args.quiet {
        return Ok(());
    }

    for entry in &entries {
        if let Some(target) = &entry.symref_target {
            println!("ref: {target}\t{}", entry.name);
        }
        println!("{}\t{}", entry.oid, entry.name);
    }

    Ok(())
}

/// Open a local repository given a user-supplied path.
///
/// Tries `path` directly (bare repository or an explicit `.git` directory),
/// and falls back to `path/.git` for a standard non-bare working directory.
///
/// # Errors
///
/// Returns an error when neither location looks like a valid git repository.
fn open_local_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).with_context(|| {
        format!(
            "'{}' does not appear to be a git repository",
            path.display()
        )
    })
}
