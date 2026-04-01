//! `gust ls-files` — list information about files in the index and working tree.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Write};
use std::path::Component;
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
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot ls-files in bare repository"))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let cwd_prefix = cwd_prefix_bytes(work_tree, &cwd)?;
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

    let mut pathspec_filter: Vec<Vec<u8>> = args
        .pathspecs
        .iter()
        .map(|p| resolve_pathspec(work_tree, &cwd, p))
        .collect::<Result<Vec<_>>>()?;
    if pathspec_filter.is_empty() && !cwd_prefix.is_empty() {
        pathspec_filter.push(cwd_prefix.clone());
    }

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
            let display = display_path_from_cwd(&entry.path, &cwd_prefix);
            let name = String::from_utf8_lossy(display);
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
            let display = display_path_from_cwd(&entry.path, &cwd_prefix);
            let name = String::from_utf8_lossy(display);
            write!(out, "{name}")?;
            out.write_all(&[term])?;
        }
    }

    Ok(())
}

fn resolve_pathspec(
    work_tree: &std::path::Path,
    cwd: &std::path::Path,
    pathspec: &std::path::Path,
) -> Result<Vec<u8>> {
    if pathspec.as_os_str().is_empty() || pathspec == std::path::Path::new(".") {
        return cwd_prefix_bytes(work_tree, cwd);
    }
    let combined = if pathspec.is_absolute() {
        pathspec.to_path_buf()
    } else {
        cwd.join(pathspec)
    };
    let normalized = normalize_path(&combined);
    let rel = normalized.strip_prefix(work_tree).with_context(|| {
        format!(
            "pathspec '{}' is outside repository work tree",
            pathspec.display()
        )
    })?;
    Ok(path_to_bytes(rel))
}

fn cwd_prefix_bytes(work_tree: &std::path::Path, cwd: &std::path::Path) -> Result<Vec<u8>> {
    let rel = cwd.strip_prefix(work_tree).with_context(|| {
        format!(
            "current directory '{}' is outside repository work tree '{}'",
            cwd.display(),
            work_tree.display()
        )
    })?;
    if rel.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let mut bytes = path_to_bytes(rel);
    bytes.push(b'/');
    Ok(bytes)
}

fn display_path_from_cwd<'a>(path: &'a [u8], cwd_prefix: &[u8]) -> &'a [u8] {
    if cwd_prefix.is_empty() {
        return path;
    }
    path.strip_prefix(cwd_prefix).unwrap_or(path)
}

fn normalize_path(path: &std::path::Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_to_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}
