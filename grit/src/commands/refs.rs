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
    /// Optimize the ref database (pack loose refs).
    Optimize,
    /// List all refs.
    List,
}

/// Run `grit refs`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.action {
        RefsAction::Verify => verify_refs(&repo),
        RefsAction::Migrate { ref_format } => migrate_refs(&repo, &ref_format),
        RefsAction::Optimize => optimize_refs(&repo),
        RefsAction::List => list_refs(&repo),
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
    let refs = grit_lib::refs::list_refs(&repo.git_dir, "refs/")
        .context("failed to list refs")?;
    for (name, oid) in refs {
        println!("{oid} {name}");
    }
    Ok(())
}

fn optimize_refs(_repo: &Repository) -> Result<()> {
    // Delegate to pack-refs --all
    crate::commands::pack_refs::run(crate::commands::pack_refs::Args { all: true, no_prune: false })
}

fn migrate_refs(repo: &Repository, ref_format: &str) -> Result<()> {
    let git_dir = &repo.git_dir;
    let is_reftable = grit_lib::reftable::is_reftable_repo(git_dir);

    match ref_format {
        "reftable" => {
            if is_reftable {
                eprintln!("ref storage is already in 'reftable' format");
                return Ok(());
            }
            migrate_files_to_reftable(git_dir)
        }
        "files" => {
            if !is_reftable {
                eprintln!("ref storage is already in 'files' format");
                return Ok(());
            }
            migrate_reftable_to_files(git_dir)
        }
        other => {
            eprintln!("unknown ref format: {other}");
            std::process::exit(1);
        }
    }
}

/// Migrate from files backend to reftable backend.
fn migrate_files_to_reftable(git_dir: &Path) -> Result<()> {
    // 1. Collect all refs from files backend
    let refs = grit_lib::refs::list_refs(git_dir, "refs/")
        .context("listing refs for migration")?;

    // 2. Read HEAD symref target
    let head_path = git_dir.join("HEAD");
    let head_content = fs::read_to_string(&head_path).context("reading HEAD")?;
    let head_target = head_content.trim().strip_prefix("ref: ").map(String::from);

    // 3. Create reftable directory
    let reftable_dir = git_dir.join("reftable");
    fs::create_dir_all(&reftable_dir)?;
    let tables_list = reftable_dir.join("tables.list");
    if !tables_list.exists() {
        fs::write(&tables_list, "")?;
    }

    // 4. Update config to add extensions.refStorage = reftable
    update_config_ref_format(git_dir, "reftable")?;

    // 5. Write all refs to reftable
    for (refname, oid) in &refs {
        grit_lib::reftable::reftable_write_ref(
            git_dir, refname, oid, None, Some("refs migrate"),
        ).with_context(|| format!("writing ref {refname} to reftable"))?;
    }

    // 6. Write HEAD symref to reftable if it's a symbolic ref
    if let Some(target) = &head_target {
        grit_lib::reftable::reftable_write_symref(
            git_dir, "HEAD", target, None, Some("refs migrate"),
        ).context("writing HEAD symref to reftable")?;
    }

    // 7. Remove old files backend data
    let refs_dir = git_dir.join("refs");
    if refs_dir.is_dir() {
        // Remove all ref files but keep the refs directory structure
        // (some tools expect refs/ to exist)
        remove_ref_files(&refs_dir)?;
    }
    let packed_refs = git_dir.join("packed-refs");
    if packed_refs.exists() {
        let _ = fs::remove_file(&packed_refs);
    }

    Ok(())
}

/// Migrate from reftable backend to files backend.
fn migrate_reftable_to_files(git_dir: &Path) -> Result<()> {
    // 1. Collect all refs from reftable
    let refs = grit_lib::reftable::reftable_list_refs(git_dir, "refs/")
        .context("listing refs from reftable")?;

    // 2. Read HEAD symref from reftable
    let head_target = grit_lib::reftable::reftable_read_symbolic_ref(git_dir, "HEAD")
        .unwrap_or(None);

    // 3. Update config to remove extensions.refStorage
    update_config_ref_format(git_dir, "files")?;

    // 4. Write HEAD
    let head_path = git_dir.join("HEAD");
    if let Some(target) = &head_target {
        fs::write(&head_path, format!("ref: {target}\n"))?;
    }

    // 5. Write all refs as loose refs
    for (refname, oid) in &refs {
        let ref_path = git_dir.join(refname);
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ref_path, format!("{oid}\n"))?;
    }

    // 6. Remove reftable directory
    let reftable_dir = git_dir.join("reftable");
    if reftable_dir.is_dir() {
        let _ = fs::remove_dir_all(&reftable_dir);
    }

    Ok(())
}

/// Update the config file to set or remove extensions.refStorage.
fn update_config_ref_format(git_dir: &Path, format: &str) -> Result<()> {
    let config_path = git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    // Remove existing extensions.refStorage and repositoryformatversion lines
    let mut in_extensions = false;
    let mut ext_section_start = None;
    let mut ext_section_end = None;
    let mut refstorage_line = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_extensions {
                ext_section_end = Some(i);
            }
            in_extensions = trimmed.eq_ignore_ascii_case("[extensions]");
            if in_extensions {
                ext_section_start = Some(i);
            }
        } else if in_extensions {
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim().eq_ignore_ascii_case("refstorage") {
                    refstorage_line = Some(i);
                }
            }
        }
    }
    if in_extensions && ext_section_end.is_none() {
        ext_section_end = Some(lines.len());
    }

    if format == "reftable" {
        // Ensure repositoryformatversion = 1
        let mut found_version = false;
        for line in lines.iter_mut() {
            let trimmed = line.trim();
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim().eq_ignore_ascii_case("repositoryformatversion") {
                    *line = "\trepositoryformatversion = 1".to_string();
                    found_version = true;
                    break;
                }
            }
        }
        if !found_version {
            // Add it under [core] if it exists
            for (i, line) in lines.iter().enumerate() {
                if line.trim().eq_ignore_ascii_case("[core]") {
                    lines.insert(i + 1, "\trepositoryformatversion = 1".to_string());
                    break;
                }
            }
        }

        // Add [extensions] refStorage = reftable
        if let Some(line_idx) = refstorage_line {
            lines[line_idx] = "\trefStorage = reftable".to_string();
        } else if ext_section_start.is_some() {
            // Section exists but no refStorage line — add it
            let end = ext_section_end.unwrap();
            lines.insert(end, "\trefStorage = reftable".to_string());
        } else {
            // No [extensions] section — add it
            lines.push("[extensions]".to_string());
            lines.push("\trefStorage = reftable".to_string());
        }
    } else {
        // format == "files" — remove extensions.refStorage
        if let Some(line_idx) = refstorage_line {
            lines.remove(line_idx);
            // If extensions section is now empty, remove it too
            if let Some(start) = ext_section_start {
                let end = ext_section_end.unwrap_or(lines.len());
                let adjusted_end = if line_idx < end { end - 1 } else { end };
                let section_empty = (start + 1..adjusted_end).all(|i| {
                    i >= lines.len() || lines[i].trim().is_empty()
                });
                if section_empty {
                    // Remove the section header too
                    if start < lines.len() {
                        lines.remove(start);
                    }
                }
            }
        }
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    fs::write(&config_path, output)?;
    Ok(())
}

/// Remove ref files from a directory tree (but keep directory structure).
fn remove_ref_files(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            remove_ref_files(&path)?;
            // Remove empty directories
            let _ = fs::remove_dir(&path);
        } else if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}
