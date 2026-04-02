//! `grit refs` — low-level ref management.
//!
//! Provides subcommands for ref database operations:
//! - `verify`  — verify the ref database integrity
//! - `migrate` — migrate ref storage format (stub)

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::fs;
use std::path::Path;

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
}

/// Run `grit refs`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.action {
        RefsAction::Verify => verify_refs(&repo),
        RefsAction::Migrate { ref_format } => migrate_refs(&repo, &ref_format),
    }
}

/// Verify that all refs in the repository point to valid objects.
fn verify_refs(repo: &Repository) -> Result<()> {
    let refs_dir = repo.git_dir.join("refs");
    let mut errors = 0;

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
        errors += verify_refs_dir(repo, &refs_dir)?;
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

    eprintln!("ref database verified");
    Ok(())
}

fn verify_refs_dir(repo: &Repository, dir: &Path) -> Result<usize> {
    let mut errors = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            errors += verify_refs_dir(repo, &path)?;
        } else if path.is_file() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("reading ref {}", path.display()))?;
            let hex = content.trim();
            if hex.len() >= 40 {
                if let Ok(oid) = grit_lib::objects::ObjectId::from_hex(hex) {
                    if !repo.odb.exists(&oid) {
                        let name = path.strip_prefix(&repo.git_dir).unwrap_or(&path);
                        eprintln!("error: {} points to missing object {hex}", name.display());
                        errors += 1;
                    }
                }
            }
        }
    }
    Ok(errors)
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
