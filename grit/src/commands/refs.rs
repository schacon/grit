//! `grit refs` — low-level ref management.
//!
//! Provides subcommands for ref database operations:
//! - `verify`  — verify the ref database integrity
//! - `migrate` — migrate ref storage format (stub)

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::fs;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;

/// Arguments for `grit refs`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub action: RefsAction,
}

#[derive(Debug, Subcommand)]
pub enum RefsAction {
    /// Verify the ref database.
    Verify,
    /// Migrate ref storage format.
    Migrate {
        /// Target ref format (e.g. "files", "reftable").
        #[arg(long = "ref-format")]
        ref_format: String,
    },
    /// List all refs.
    List,
}

/// Run `grit refs`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.action {
        RefsAction::Verify => verify_refs(&repo),
        RefsAction::Migrate { ref_format } => migrate_refs(&repo, &ref_format),
        RefsAction::List => list_refs(&repo),
    }
}

/// Determine the fsck.badRefName severity from config.
/// Returns `true` if badRefName is downgraded to a warning (not an error).
fn bad_ref_name_is_warn(git_dir: &Path) -> bool {
    ConfigSet::load(Some(git_dir), true)
        .ok()
        .and_then(|cfg| cfg.get("fsck.badRefName"))
        .map(|v| v.eq_ignore_ascii_case("warn"))
        .unwrap_or(false)
}

/// Verify that all refs in the repository point to valid objects.
fn verify_refs(repo: &Repository) -> Result<()> {
    let refs_dir = repo.git_dir.join("refs");
    let mut errors = 0;
    let warn_only = bad_ref_name_is_warn(&repo.git_dir);

    // Check HEAD
    let head_path = repo.git_dir.join("HEAD");
    if head_path.exists() {
        let content = fs::read_to_string(&head_path).context("reading HEAD")?;
        let trimmed = content.trim();
        if let Some(target) = trimmed.strip_prefix("ref: ") {
            let ref_path = repo.git_dir.join(target);
            if !ref_path.exists() {
                // Symbolic ref to nonexistent ref is OK (empty repo, detached, etc.)
            }
        } else if trimmed.len() >= 40 {
            // Direct OID — verify it exists
            if let Ok(oid) = grit_lib::objects::ObjectId::from_hex(trimmed) {
                if !repo.odb.exists(&oid) {
                    eprintln!("error: HEAD points to missing object {trimmed}");
                    errors += 1;
                }
            }
        }
    }

    // Walk refs directory
    if refs_dir.is_dir() {
        errors += verify_refs_dir(repo, &refs_dir, warn_only)?;
    }

    // Check packed-refs
    let packed_refs = repo.git_dir.join("packed-refs");
    if packed_refs.exists() {
        let content = fs::read_to_string(&packed_refs).context("reading packed-refs")?;
        for line in content.lines() {
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            if let Some((hex, name)) = line.split_once(' ') {
                if let Ok(oid) = grit_lib::objects::ObjectId::from_hex(hex) {
                    if !repo.odb.exists(&oid) {
                        eprintln!("error: {name} points to missing object {hex}");
                        errors += 1;
                    }
                }
            }
        }
    }

    if errors > 0 {
        eprintln!("{errors} ref(s) point to missing objects");
        std::process::exit(1);
    }

    Ok(())
}

fn verify_refs_dir(repo: &Repository, dir: &Path, warn_only: bool) -> Result<usize> {
    let mut errors = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            errors += verify_refs_dir(repo, &path, warn_only)?;
        } else if path.is_file() {
            // Derive the ref name relative to .git/ (e.g. "refs/heads/main")
            let refname = path
                .strip_prefix(&repo.git_dir)
                .unwrap_or(&path)
                .to_string_lossy();

            // Validate the ref name
            if grit_lib::check_ref_format::check_refname_format(
                &refname,
                &grit_lib::check_ref_format::RefNameOptions {
                    allow_onelevel: false,
                    refspec_pattern: false,
                    normalize: false,
                },
            )
            .is_err()
            {
                if warn_only {
                    eprintln!("warning: badRefName: {refname}");
                } else {
                    eprintln!("error: badRefName: {refname}");
                    errors += 1;
                }
            }

            let content = fs::read_to_string(&path)
                .with_context(|| format!("reading ref {}", path.display()))?;
            let trimmed = content.trim();
            if let Some(target) = trimmed.strip_prefix("ref: ") {
                // Validate symbolic ref target
                if grit_lib::check_ref_format::check_refname_format(
                    target,
                    &grit_lib::check_ref_format::RefNameOptions {
                        allow_onelevel: false,
                        refspec_pattern: false,
                        normalize: false,
                    },
                ).is_err() {
                    let name = path.strip_prefix(&repo.git_dir).unwrap_or(&path);
                    eprintln!("error: {} points to invalid ref target '{}'", name.display(), target);
                    errors += 1;
                }
            } else if trimmed.len() >= 40 {
                if let Ok(oid) = grit_lib::objects::ObjectId::from_hex(trimmed) {
                    if !repo.odb.exists(&oid) {
                        let name = path.strip_prefix(&repo.git_dir).unwrap_or(&path);
                        eprintln!("error: {} points to missing object {trimmed}", name.display());
                        errors += 1;
                    }
                }
            }
        }
    }
    Ok(errors)
}

fn list_refs(repo: &Repository) -> Result<()> {
    // Resolve HEAD
    if let Ok(oid) = grit_lib::refs::resolve_ref(&repo.git_dir, "HEAD") {
        println!("{} HEAD", oid);
    }

    // List all refs under refs/
    let refs = grit_lib::refs::list_refs(&repo.git_dir, "refs/")?;
    for (refname, oid) in refs {
        println!("{} {}", oid, refname);
    }

    Ok(())
}

fn migrate_refs(_repo: &Repository, ref_format: &str) -> Result<()> {
    match ref_format {
        "files" => {
            eprintln!("ref storage is already in 'files' format");
        }
        "reftable" => {
            eprintln!("reftable migration is not yet implemented");
            std::process::exit(1);
        }
        other => {
            eprintln!("unknown ref format: {other}");
            std::process::exit(1);
        }
    }
    Ok(())
}
