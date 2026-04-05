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

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let bad_ref_name_level = config
        .get("fsck.badRefName")
        .unwrap_or_default()
        .to_lowercase();

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
        errors += verify_refs_dir(repo, &refs_dir, &bad_ref_name_level)?;
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
        eprintln!("{errors} ref(s) with issues");
        std::process::exit(1);
    }

    Ok(())
}

fn verify_refs_dir(repo: &Repository, dir: &Path, bad_ref_name_level: &str) -> Result<usize> {
    let mut errors = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            errors += verify_refs_dir(repo, &path, bad_ref_name_level)?;
        } else if path.is_file() {
            // Check ref name validity
            let ref_name = path
                .strip_prefix(&repo.git_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            if grit_lib::check_ref_format::check_refname_format(
                &ref_name,
                &grit_lib::check_ref_format::RefNameOptions {
                    allow_onelevel: false,
                    refspec_pattern: false,
                    normalize: false,
                },
            )
            .is_err()
            {
                if bad_ref_name_level == "warn" {
                    eprintln!("warning: {ref_name}: badRefName: invalid refname format");
                } else if bad_ref_name_level == "ignore" {
                    // skip
                } else {
                    eprintln!("error: {ref_name}: badRefName: invalid refname format");
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
                )
                .is_err()
                {
                    let name = path.strip_prefix(&repo.git_dir).unwrap_or(&path);
                    eprintln!(
                        "error: {} points to invalid ref target '{}'",
                        name.display(),
                        target
                    );
                    errors += 1;
                }
            } else if trimmed.len() >= 40 {
                if let Ok(oid) = grit_lib::objects::ObjectId::from_hex(trimmed) {
                    if !repo.odb.exists(&oid) {
                        let name = path.strip_prefix(&repo.git_dir).unwrap_or(&path);
                        eprintln!(
                            "error: {} points to missing object {trimmed}",
                            name.display()
                        );
                        errors += 1;
                    }
                }
            }
        }
    }
    Ok(errors)
}

fn list_refs(repo: &Repository) -> Result<()> {
    let refs = grit_lib::refs::list_refs(&repo.git_dir, "refs/").context("failed to list refs")?;
    for (name, oid) in refs {
        println!("{oid} {name}");
    }
    Ok(())
}

fn optimize_refs(_repo: &Repository) -> Result<()> {
    // Delegate to pack-refs --all
    crate::commands::pack_refs::run(crate::commands::pack_refs::Args {
        all: true,
        no_prune: false,
    })
}

/// Detect the current ref storage format of a repository.
fn current_ref_format(repo: &Repository) -> &'static str {
    if grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
        "reftable"
    } else {
        "files"
    }
}

/// Migrate ref storage between backends.
///
/// Supported migrations:
/// - `files` → `reftable`: reads all loose/packed refs, writes them into a
///   reftable stack, updates the config, removes old files.
/// - `reftable` → `files`: reads all reftable refs, writes them as loose
///   refs + packed-refs, updates the config, removes the reftable directory.
fn migrate_refs(repo: &Repository, target_format: &str) -> Result<()> {
    let current = current_ref_format(repo);
    if current == target_format {
        eprintln!("ref storage is already in '{target_format}' format");
        return Ok(());
    }

    match (current, target_format) {
        ("files", "reftable") => migrate_files_to_reftable(repo),
        ("reftable", "files") => migrate_reftable_to_files(repo),
        (_, other) => {
            eprintln!("unknown ref format: {other}");
            std::process::exit(1);
        }
    }
}

/// Collect all refs from the files backend (loose + packed).
fn collect_files_refs(git_dir: &Path) -> Result<Vec<(String, String)>> {
    // `String` values: either an OID hex or "symref:<target>" for symbolic refs.
    let mut result: Vec<(String, String)> = Vec::new();

    // Read HEAD
    let head = fs::read_to_string(git_dir.join("HEAD")).context("reading HEAD")?;
    let head = head.trim();
    if let Some(target) = head.strip_prefix("ref: ") {
        result.push(("HEAD".to_owned(), format!("symref:{target}")));
    } else {
        result.push(("HEAD".to_owned(), head.to_owned()));
    }

    // Collect loose refs
    fn walk_loose(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) -> Result<()> {
        let rd = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in rd {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let refname = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type()?.is_dir() {
                walk_loose(&entry.path(), &refname, out)?;
            } else {
                let content = fs::read_to_string(entry.path())?.trim().to_owned();
                if let Some(target) = content.strip_prefix("ref: ") {
                    out.push((refname, format!("symref:{target}")));
                } else {
                    out.push((refname, content));
                }
            }
        }
        Ok(())
    }

    walk_loose(&git_dir.join("refs"), "refs", &mut result)?;

    // Read packed-refs (lower priority — only add if not already present)
    let packed_path = git_dir.join("packed-refs");
    if let Ok(content) = fs::read_to_string(&packed_path) {
        let existing: std::collections::HashSet<String> =
            result.iter().map(|(n, _)| n.clone()).collect();
        for line in content.lines() {
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            if let Some((hex, name)) = line.split_once(' ') {
                if hex.len() == 40 && !existing.contains(name) {
                    result.push((name.to_owned(), hex.to_owned()));
                }
            }
        }
    }

    Ok(result)
}

/// Migrate from files backend to reftable.
fn migrate_files_to_reftable(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;
    let refs = collect_files_refs(git_dir)?;

    // Create reftable directory
    let reftable_dir = git_dir.join("reftable");
    fs::create_dir_all(&reftable_dir)?;
    let tables_list = reftable_dir.join("tables.list");
    if !tables_list.exists() {
        fs::write(&tables_list, "")?;
    }

    // Update config to enable reftable BEFORE writing refs
    update_config_ref_format(git_dir, "reftable")?;

    // Write all refs into reftable
    for (refname, value) in &refs {
        if refname == "HEAD" {
            // HEAD is kept as a file, not in reftable
            continue;
        }
        if let Some(target) = value.strip_prefix("symref:") {
            grit_lib::reftable::reftable_write_symref(git_dir, refname, target, None, None)
                .with_context(|| format!("writing symref {refname}"))?;
        } else {
            let oid: grit_lib::objects::ObjectId = value
                .parse()
                .with_context(|| format!("parsing oid for {refname}"))?;
            grit_lib::reftable::reftable_write_ref(git_dir, refname, &oid, None, None)
                .with_context(|| format!("writing ref {refname}"))?;
        }
    }

    // Remove old files backend artifacts
    let _ = fs::remove_file(git_dir.join("packed-refs"));
    let _ = remove_dir_contents(&git_dir.join("refs").join("heads"));
    let _ = remove_dir_contents(&git_dir.join("refs").join("tags"));

    Ok(())
}

/// Migrate from reftable backend to files.
fn migrate_reftable_to_files(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;

    // Read all refs from reftable
    let refs = grit_lib::reftable::reftable_list_refs(git_dir, "refs/")
        .context("reading reftable refs")?;

    // Also read HEAD
    let head_content = fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();

    // Update config to files format BEFORE writing
    update_config_ref_format(git_dir, "files")?;

    // Write refs as loose files
    for (refname, oid) in &refs {
        let ref_path = git_dir.join(refname);
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ref_path, format!("{oid}\n"))?;
    }

    // Ensure refs/heads and refs/tags directories exist
    fs::create_dir_all(git_dir.join("refs").join("heads"))?;
    fs::create_dir_all(git_dir.join("refs").join("tags"))?;

    // Remove reftable directory
    let reftable_dir = git_dir.join("reftable");
    if reftable_dir.exists() {
        fs::remove_dir_all(&reftable_dir)?;
    }

    // Ensure HEAD is preserved
    if !head_content.is_empty() {
        let head_path = git_dir.join("HEAD");
        if !head_path.exists() {
            fs::write(head_path, head_content)?;
        }
    }

    Ok(())
}

/// Update the repository config to reflect the new ref storage format.
fn update_config_ref_format(git_dir: &Path, format: &str) -> Result<()> {
    let config_path = git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();

    let mut new_content = String::new();
    let mut in_extensions = false;
    let mut wrote_ref_storage = false;
    let mut has_extensions = false;
    let mut _wrote_version = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track section headers
        if trimmed.starts_with('[') {
            // If we were in [extensions] and didn't write refStorage, do it now
            if in_extensions && !wrote_ref_storage && format == "reftable" {
                new_content.push_str(&format!("\trefStorage = {format}\n"));
                wrote_ref_storage = true;
            }
            in_extensions = trimmed.starts_with("[extensions]");
            if in_extensions {
                has_extensions = true;
            }
        }

        // Handle repositoryformatversion
        if trimmed.starts_with("repositoryformatversion") {
            let version = if format == "reftable" { 1 } else { 0 };
            new_content.push_str(&format!("\trepositoryformatversion = {version}\n"));
            _wrote_version = true;
            continue;
        }

        // Handle refStorage line
        if in_extensions && trimmed.to_lowercase().starts_with("refstorage") {
            if format == "reftable" {
                new_content.push_str(&format!("\trefStorage = {format}\n"));
            }
            // For "files", just skip (remove) the refStorage line
            wrote_ref_storage = true;
            continue;
        }

        new_content.push_str(line);
        new_content.push('\n');
    }

    // If [extensions] section existed and we still need to write refStorage
    if has_extensions && !wrote_ref_storage && format == "reftable" {
        new_content.push_str(&format!("\trefStorage = {format}\n"));
    }

    // If no [extensions] section and we need one
    if !has_extensions && format == "reftable" {
        new_content.push_str("[extensions]\n");
        new_content.push_str(&format!("\trefStorage = {format}\n"));
    }

    fs::write(&config_path, &new_content)?;
    Ok(())
}

/// Remove all files and subdirectories inside a directory (but keep the dir).
fn remove_dir_contents(dir: &Path) -> Result<()> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}
