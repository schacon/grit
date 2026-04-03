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
    // If the repository argument is a configured remote name, resolve its URL
    let effective_path = resolve_remote_or_path(&args.repository);
    let repo = open_local_repo(&effective_path)?;

    let opts = Options {
        heads: args.heads,
        tags: args.tags,
        refs_only: args.refs_only,
        symref: args.symref,
        patterns: args.patterns,
    };

    let entries = ls_remote(&repo.git_dir, &repo.odb, &opts)?;

    if entries.is_empty() && !opts.patterns.is_empty() {
        std::process::exit(2);
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

/// If the repository argument matches a configured remote name, resolve to its URL.
/// Otherwise return the original path.
fn resolve_remote_or_path(path: &Path) -> PathBuf {
    // Only try remote resolution if path doesn't contain path separators
    // and doesn't already exist as a filesystem path
    let path_str = path.to_string_lossy();
    if path.exists() || path_str.contains('/') || path_str.contains('\\') {
        return path.to_path_buf();
    }

    // Try to discover a repo and check config
    if let Ok(repo) = Repository::discover(None) {
        let config_path = repo.git_dir.join("config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            let _key = format!("remote.{}.url", path_str);
            // Simple config parsing: look for [remote "name"] section and url key
            if let Some(url) = parse_remote_url(&content, &path_str) {
                return PathBuf::from(url);
            }
        }
    }

    path.to_path_buf()
}

fn parse_remote_url(config: &str, remote_name: &str) -> Option<String> {
    let section_header = format!("[remote \"{remote_name}\"]");
    let mut in_section = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section_header;
            continue;
        }
        if in_section {
            if let Some(value) = trimmed.strip_prefix("url") {
                let value = value.trim_start();
                if let Some(value) = value.strip_prefix('=') {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}
