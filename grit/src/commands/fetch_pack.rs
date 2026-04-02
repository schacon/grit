//! `grit fetch-pack` — download objects and refs from a remote (plumbing).
//!
//! Low-level plumbing command that fetches pack data from a remote repository.
//! Only **local** transports are supported.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit fetch-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download objects from a remote repository (plumbing)")]
pub struct Args {
    /// Path to the remote repository (bare or non-bare).
    #[arg(value_name = "REMOTE")]
    pub remote: String,

    /// Ref(s) to fetch (e.g. "refs/heads/main"). If empty, fetches all.
    #[arg(value_name = "REF")]
    pub refs: Vec<String>,

    /// Show what would be done, without making changes.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Be quiet — suppress informational output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,
}

pub fn run(args: Args) -> Result<()> {
    let git_dir = resolve_git_dir()?;

    let remote_path = PathBuf::from(&args.remote);
    let remote_repo = open_repo(&remote_path)
        .with_context(|| format!("could not open remote repository at '{}'", remote_path.display()))?;

    // Enumerate remote refs
    let remote_heads = refs::list_refs(&remote_repo.git_dir, "refs/heads/")?;
    let remote_tags = refs::list_refs(&remote_repo.git_dir, "refs/tags/")?;

    // Filter refs if specific ones were requested
    let requested_refs: Vec<(String, ObjectId)> = if args.refs.is_empty() {
        remote_heads.into_iter().chain(remote_tags.into_iter()).collect()
    } else {
        let all_refs: Vec<(String, ObjectId)> = remote_heads
            .into_iter()
            .chain(remote_tags.into_iter())
            .collect();
        all_refs
            .into_iter()
            .filter(|(name, _)| {
                args.refs.iter().any(|r| {
                    name == r || name.ends_with(&format!("/{r}"))
                })
            })
            .collect()
    };

    if !args.dry_run {
        // Copy objects from remote → local
        copy_objects(&remote_repo.git_dir, &git_dir)
            .context("copying objects from remote")?;
    }

    // Print fetched refs (plumbing output: <oid> <refname>)
    for (refname, oid) in &requested_refs {
        println!("{}\t{}", oid.to_hex(), refname);
    }

    Ok(())
}

/// Copy all objects (loose + packs) from src to dst, skipping existing.
fn copy_objects(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    let src_objects = src_git_dir.join("objects");
    let dst_objects = dst_git_dir.join("objects");

    if src_objects.is_dir() {
        for entry in fs::read_dir(&src_objects)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str == "info" || name_str == "pack" {
                continue;
            }
            if !entry.file_type()?.is_dir() || name_str.len() != 2 {
                continue;
            }

            let dst_dir = dst_objects.join(&*name);
            for inner in fs::read_dir(entry.path())? {
                let inner = inner?;
                if inner.file_type()?.is_file() {
                    let dst_file = dst_dir.join(inner.file_name());
                    if !dst_file.exists() {
                        fs::create_dir_all(&dst_dir)?;
                        if fs::hard_link(inner.path(), &dst_file).is_err() {
                            fs::copy(inner.path(), &dst_file)?;
                        }
                    }
                }
            }
        }
    }

    let src_pack = src_objects.join("pack");
    let dst_pack = dst_objects.join("pack");
    if src_pack.is_dir() {
        fs::create_dir_all(&dst_pack)?;
        for entry in fs::read_dir(&src_pack)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let dst_file = dst_pack.join(entry.file_name());
                if !dst_file.exists() {
                    if fs::hard_link(entry.path(), &dst_file).is_err() {
                        fs::copy(entry.path(), &dst_file)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

/// Resolve the git directory from CWD.
fn resolve_git_dir() -> Result<PathBuf> {
    use anyhow::bail;
    if let Ok(dir) = std::env::var("GIT_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let mut cur = cwd.as_path();
    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() {
            return Ok(dot_git);
        }
        if dot_git.is_file() {
            if let Ok(content) = fs::read_to_string(&dot_git) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("gitdir:") {
                        let path = rest.trim();
                        let resolved = if Path::new(path).is_absolute() {
                            PathBuf::from(path)
                        } else {
                            cur.join(path)
                        };
                        return Ok(resolved);
                    }
                }
            }
        }
        if cur.join("objects").is_dir() && cur.join("HEAD").is_file() {
            return Ok(cur.to_path_buf());
        }
        cur = match cur.parent() {
            Some(p) => p,
            None => bail!("not a git repository (or any of the parent directories): .git"),
        };
    }
}
