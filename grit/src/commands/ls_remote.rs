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

    /// Path to git-upload-pack on the remote host.
    #[arg(long = "upload-pack", alias = "exec")]
    pub upload_pack: Option<String>,

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
    // Accepted for compatibility; local-path implementation does not use it.
    let _ = &args.upload_pack;

    // If the repository argument is a configured remote name, resolve its URL
    let effective_path = resolve_remote_or_path(&args.repository);

    // Check if the path is a bundle file
    if is_bundle_file(&effective_path) {
        return run_bundle_ls_remote(&effective_path, &args);
    }

    let repo = open_local_repo(&effective_path)?;

    let opts = Options {
        heads: args.heads,
        tags: args.tags,
        refs_only: args.refs_only,
        symref: args.symref,
        patterns: args.patterns,
    };

    let refs_git_dir = common_git_dir_or_self(&repo.git_dir);
    let entries = ls_remote(&refs_git_dir, &repo.odb, &opts)?;

    if entries.is_empty() {
        // git ls-remote exits 0 even when no refs match patterns
        return Ok(());
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
    // Strip file:// URL scheme if present
    let effective_path = {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            path.to_path_buf()
        }
    };
    let path = &effective_path;

    // Bare repository or explicit git-dir directory.
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }

    // Explicit gitfile path (e.g. ".../foo/.git" where ".git" is a file).
    if path.is_file() {
        if let Ok(git_dir) = resolve_gitdir_from_gitfile_path(path) {
            return Ok(Repository::open(&git_dir, path.parent())?);
        }
    }

    // Standard working-tree repository path.
    let dot_git = path.join(".git");
    if dot_git.is_file() {
        if let Ok(git_dir) = resolve_gitdir_from_gitfile_path(&dot_git) {
            return Ok(Repository::open(&git_dir, Some(path))?);
        }
    }
    if dot_git.is_dir() {
        return Ok(Repository::open(&dot_git, Some(path))?);
    }

    Repository::open(&dot_git, Some(path)).with_context(|| {
        format!(
            "'{}' does not appear to be a git repository: {}",
            path.display(),
            "not a git repository (or any of the parent directories)"
        )
    })
}

fn resolve_gitdir_from_gitfile_path(gitfile_path: &Path) -> Result<PathBuf> {
    let content = std::fs::read_to_string(gitfile_path).with_context(|| {
        format!(
            "'{}' does not appear to be a git repository: {}",
            gitfile_path.display(),
            "not a git repository (or any of the parent directories)"
        )
    })?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("gitdir:") {
            let rel = rest.trim();
            if rel.is_empty() {
                break;
            }
            let candidate = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                gitfile_path.parent().unwrap_or(Path::new(".")).join(rel)
            };
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "'{}' does not appear to be a git repository: not a git repository (or any of the parent directories)",
        gitfile_path.display()
    )
}

/// If the repository argument matches a configured remote name, resolve to its URL.
/// Otherwise return the original path.
fn resolve_remote_or_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();

    // Remote config takes precedence over filesystem paths, even when the
    // remote name itself contains slashes.
    if let Ok(repo) = Repository::discover(None) {
        let config_path = repo.git_dir.join("config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Some(url) = parse_remote_url(&content, &path_str) {
                return PathBuf::from(url);
            }
        }
    }

    path.to_path_buf()
}

/// Check if a path looks like a git bundle file (starts with v2 bundle header).
fn is_bundle_file(path: &Path) -> bool {
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = [0u8; 20];
        if let Ok(n) = std::io::Read::read(&mut f, &mut buf) {
            return buf[..n].starts_with(b"# v2 git bundle");
        }
    }
    false
}

/// Run ls-remote against a bundle file.
fn run_bundle_ls_remote(path: &Path, args: &Args) -> Result<()> {
    let data = std::fs::read(path)
        .with_context(|| format!("could not read bundle '{}'.", path.display()))?;
    let refs = parse_bundle_refs(&data)?;

    if refs.is_empty() {
        return Ok(());
    }

    if args.quiet {
        return Ok(());
    }

    for (refname, oid) in &refs {
        if args.heads && !refname.starts_with("refs/heads/") {
            continue;
        }
        if args.tags && !refname.starts_with("refs/tags/") {
            continue;
        }
        if !args.patterns.is_empty() {
            let matched = args
                .patterns
                .iter()
                .any(|p| refname.contains(p) || refname.ends_with(p));
            if !matched {
                continue;
            }
        }
        println!("{oid}\t{refname}");
    }
    Ok(())
}

/// Parse refs from a v2 bundle header.
fn parse_bundle_refs(data: &[u8]) -> Result<Vec<(String, grit_lib::objects::ObjectId)>> {
    let header_line = b"# v2 git bundle\n";
    if !data.starts_with(header_line) {
        anyhow::bail!("not a v2 git bundle");
    }
    let mut pos = header_line.len();
    let mut refs = Vec::new();
    loop {
        let eol = data[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| pos + i)
            .ok_or_else(|| anyhow::anyhow!("truncated bundle header"))?;
        let line = &data[pos..eol];
        if line.is_empty() {
            break;
        }
        let line_str = std::str::from_utf8(line)?;
        if line_str.starts_with('-') {
            pos = eol + 1;
            continue;
        }
        if let Some((hex, refname)) = line_str.split_once(' ') {
            let oid = grit_lib::objects::ObjectId::from_hex(hex)
                .map_err(|e| anyhow::anyhow!("bad oid in bundle: {e}"))?;
            refs.push((refname.to_string(), oid));
        }
        pos = eol + 1;
    }
    Ok(refs)
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

fn common_git_dir_or_self(git_dir: &Path) -> PathBuf {
    let commondir_path = git_dir.join("commondir");
    let Ok(raw) = std::fs::read_to_string(commondir_path) else {
        return git_dir.to_path_buf();
    };
    let rel = raw.trim();
    if rel.is_empty() {
        return git_dir.to_path_buf();
    }
    let candidate = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    candidate.canonicalize().unwrap_or(candidate)
}
