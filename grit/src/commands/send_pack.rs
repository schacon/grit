//! `grit send-pack` — push objects to a remote repository (plumbing).
//!
//! Low-level plumbing command that sends pack data to a remote repository
//! and updates remote refs.  Only **local** transports are supported.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit send-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Push objects to a remote repository (plumbing)")]
pub struct Args {
    /// Path to the remote repository (bare or non-bare).
    #[arg(value_name = "REMOTE")]
    pub remote: String,

    /// Refspec(s) to push (e.g. "main:main", "refs/heads/main:refs/heads/main").
    /// Format: <src>:<dst> or just <ref> (implies same name on both sides).
    #[arg(value_name = "REF")]
    pub refs: Vec<String>,

    /// Allow non-fast-forward updates.
    #[arg(long = "force")]
    pub force: bool,

    /// Show what would be done, without making changes.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,
}

/// A single ref update to perform on the remote.
struct RefUpdate {
    remote_ref: String,
    old_oid: Option<ObjectId>,
    new_oid: ObjectId,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let remote_path = PathBuf::from(&args.remote);
    let remote_repo = open_repo(&remote_path).with_context(|| {
        format!(
            "could not open remote repository at '{}'",
            remote_path.display()
        )
    })?;

    // Build list of ref updates from refspecs
    let mut updates = Vec::new();

    if args.refs.is_empty() {
        bail!("no refs specified; nothing to push");
    }

    for spec in &args.refs {
        let (src, dst) = parse_refspec(spec);
        let local_ref = normalize_ref(&src);
        let remote_ref = normalize_ref(&dst);

        let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
            .with_context(|| format!("src ref '{}' does not match any", src))?;
        let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

        updates.push(RefUpdate {
            remote_ref,
            old_oid,
            new_oid: local_oid,
        });
    }

    // Validate fast-forward unless --force
    for update in &updates {
        if let Some(old) = &update.old_oid {
            if *old != update.new_oid && !args.force
                && !is_ancestor(&repo, *old, update.new_oid)? {
                    bail!(
                        "non-fast-forward update to '{}' rejected (use --force to override)",
                        update.remote_ref
                    );
                }
        }
    }

    if !args.dry_run {
        // Copy objects from local → remote
        copy_objects(&repo.git_dir, &remote_repo.git_dir).context("copying objects to remote")?;
    }

    // Apply ref updates
    for update in &updates {
        let old_hex = update
            .old_oid
            .as_ref()
            .map(|o| o.to_hex())
            .unwrap_or_else(|| "0".repeat(40));
        let new_hex = update.new_oid.to_hex();
        let status = if args.dry_run { " (dry run)" } else { "" };

        if !args.dry_run {
            refs::write_ref(&remote_repo.git_dir, &update.remote_ref, &update.new_oid)
                .with_context(|| format!("updating remote ref {}", update.remote_ref))?;
        }

        println!(
            "{}..{}\t{}{status}",
            &old_hex[..7],
            &new_hex[..7],
            update.remote_ref,
        );
    }

    Ok(())
}

/// Parse a refspec "src:dst" or just "ref" (implies same name both sides).
fn parse_refspec(spec: &str) -> (String, String) {
    // Strip leading '+' (force marker)
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    if let Some((src, dst)) = spec.split_once(':') {
        (src.to_owned(), dst.to_owned())
    } else {
        (spec.to_owned(), spec.to_owned())
    }
}

/// Normalize a short ref name into a full ref path.
fn normalize_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

/// Copy all objects (loose + packs) from src to dst, skipping existing.
fn copy_objects(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    let src_objects = src_git_dir.join("objects");
    let dst_objects = dst_git_dir.join("objects");

    // Copy loose objects
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

    // Copy pack files
    let src_pack = src_objects.join("pack");
    let dst_pack = dst_objects.join("pack");
    if src_pack.is_dir() {
        fs::create_dir_all(&dst_pack)?;
        for entry in fs::read_dir(&src_pack)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let dst_file = dst_pack.join(entry.file_name());
                if !dst_file.exists()
                    && fs::hard_link(entry.path(), &dst_file).is_err() {
                        fs::copy(entry.path(), &dst_file)?;
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
