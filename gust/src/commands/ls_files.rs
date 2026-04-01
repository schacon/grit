//! `gust ls-files` — list information about files in the index and working tree.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Write};
use std::path::PathBuf;

use gust_lib::index::Index;
use gust_lib::repo::Repository;

/// Arguments for `gust ls-files`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show cached (staged) files (default).
    #[arg(short = 'c', long)]
    pub cached: bool,

    /// Show deleted files.
    #[arg(short = 'd', long)]
    pub deleted: bool,

    /// Show modified files.
    #[arg(short = 'm', long)]
    pub modified: bool,

    /// Show other (untracked) files.
    #[arg(short = 'o', long)]
    pub others: bool,

    /// Show ignored files.
    #[arg(short = 'i', long)]
    pub ignored: bool,

    /// Show unmerged files.
    #[arg(short = 'u', long)]
    pub unmerged: bool,

    /// Show killed files.
    #[arg(short = 'k', long)]
    pub killed: bool,

    /// Show object name in each line.
    #[arg(short = 's', long)]
    pub stage: bool,

    /// \0 line termination on output.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Show only unmerged files and their stage numbers.
    #[arg(long = "error-unmatch")]
    pub error_unmatch: bool,

    /// Deduplicate entries (for untracked files).
    #[arg(long)]
    pub deduplicate: bool,

    /// Suppress any error message (for -t).
    #[arg(short = 't')]
    pub show_tag: bool,

    /// Show verbose long format.
    #[arg(long)]
    pub long: bool,

    /// Pathspecs to restrict output.
    pub pathspecs: Vec<PathBuf>,
}

/// Run `gust ls-files`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();
    let index = Index::load(&index_path).context("loading index")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let term = if args.null_terminated { b'\0' } else { b'\n' };

    // Determine which mode to use
    let show_cached = args.cached
        || args.stage
        || (!args.deleted && !args.modified && !args.others && !args.unmerged && !args.killed);
    let show_stage = args.stage || args.unmerged;

    let pathspec_filter: Vec<Vec<u8>> = args
        .pathspecs
        .iter()
        .map(|p| {
            use std::os::unix::ffi::OsStrExt;
            p.as_os_str().as_bytes().to_vec()
        })
        .collect();

    for entry in &index.entries {
        // Filter by pathspec
        if !pathspec_filter.is_empty() {
            let matches = pathspec_filter
                .iter()
                .any(|spec| entry.path == spec.as_slice() || entry.path.starts_with(spec));
            if !matches {
                continue;
            }
        }

        // Unmerged: stage != 0
        if args.unmerged && entry.stage() == 0 {
            continue;
        }
        if show_cached && !args.unmerged && entry.stage() != 0 {
            continue;
        }

        if show_stage {
            let name = String::from_utf8_lossy(&entry.path);
            write!(
                out,
                "{:06o} {} {}\t{}",
                entry.mode,
                entry.oid,
                entry.stage(),
                name
            )?;
            out.write_all(&[term])?;
        } else {
            let name = String::from_utf8_lossy(&entry.path);
            write!(out, "{name}")?;
            out.write_all(&[term])?;
        }
    }

    Ok(())
}
