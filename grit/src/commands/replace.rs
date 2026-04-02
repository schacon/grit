//! `grit replace` — create, list, delete replacement references.
//!
//! Replace refs let you substitute one object for another transparently.
//! They are stored as `refs/replace/<original-sha>` pointing to the
//! replacement object's SHA.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, ValueEnum};

use grit_lib::objects::ObjectId;
use grit_lib::refs::{delete_ref, list_refs, resolve_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};

/// Arguments for `grit replace`.
#[derive(Debug, ClapArgs)]
#[command(about = "Create, list, delete refs to replace objects")]
pub struct Args {
    /// The object to be replaced.
    #[arg()]
    pub object: Option<String>,

    /// The replacement object.
    #[arg()]
    pub replacement: Option<String>,

    /// Delete existing replace refs for the given objects.
    #[arg(short = 'd', long = "delete")]
    pub delete: bool,

    /// List replace refs (default when no arguments given).
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// Force overwrite of existing replace ref.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Format for listing: short, medium, long.
    #[arg(long = "format", default_value = "short")]
    pub format: ListFormat,
}

/// Format used when listing replace refs.
#[derive(Debug, Clone, ValueEnum)]
pub enum ListFormat {
    Short,
    Medium,
    Long,
}

/// Run the `replace` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Delete mode: -d <object>...
    if args.delete {
        return delete_replace_refs(&repo, &args);
    }

    // List mode: no positional args, or -l [pattern]
    if args.list || (args.object.is_none() && args.replacement.is_none()) {
        let pattern = args.object.as_deref();
        return list_replace_refs(&repo, pattern, &args.format);
    }

    // Create mode: <object> <replacement>
    let object_str = args
        .object
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("object argument required"))?;
    let replacement_str = args
        .replacement
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("replacement argument required"))?;

    create_replace_ref(&repo, object_str, replacement_str, args.force)
}

/// Create a replace ref: `refs/replace/<original-sha>` → `<replacement-sha>`.
fn create_replace_ref(
    repo: &Repository,
    object_str: &str,
    replacement_str: &str,
    force: bool,
) -> Result<()> {
    let object_oid = resolve_revision(repo, object_str)
        .with_context(|| format!("Failed to resolve '{object_str}'"))?;
    let replacement_oid = resolve_revision(repo, replacement_str)
        .with_context(|| format!("Failed to resolve '{replacement_str}'"))?;

    // Verify both objects exist in the ODB
    repo.odb
        .read(&object_oid)
        .with_context(|| format!("object {} not found", object_oid.to_hex()))?;
    repo.odb
        .read(&replacement_oid)
        .with_context(|| format!("object {} not found", replacement_oid.to_hex()))?;

    let refname = format!("refs/replace/{}", object_oid.to_hex());

    // Check if replace ref already exists
    if !force && resolve_ref(&repo.git_dir, &refname).is_ok() {
        bail!(
            "replace ref '{}' already exists; use -f to force",
            object_oid.to_hex()
        );
    }

    write_ref(&repo.git_dir, &refname, &replacement_oid).context("writing replace ref")?;

    Ok(())
}

/// List replace refs, optionally filtered by a glob pattern.
fn list_replace_refs(repo: &Repository, pattern: Option<&str>, format: &ListFormat) -> Result<()> {
    let refs = list_refs(&repo.git_dir, "refs/replace/")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (refname, replacement_oid) in &refs {
        // Extract the original SHA from the ref name
        let original_hex = refname
            .strip_prefix("refs/replace/")
            .unwrap_or(refname);

        // Apply glob pattern filter if given
        if let Some(pat) = pattern {
            if !glob_matches(pat, original_hex) {
                continue;
            }
        }

        match format {
            ListFormat::Short => {
                writeln!(out, "{original_hex}")?;
            }
            ListFormat::Medium => {
                writeln!(out, "{original_hex} -> {}", replacement_oid.to_hex())?;
            }
            ListFormat::Long => {
                // Long format shows: <replaced-sha> (<type>) -> <replacement-sha> (<type>)
                let orig_type = if let Ok(oid) = ObjectId::from_hex(original_hex) {
                    repo.odb
                        .read(&oid)
                        .map(|o| o.kind.as_str().to_owned())
                        .unwrap_or_else(|_| "unknown".to_owned())
                } else {
                    "unknown".to_owned()
                };
                let repl_type = repo
                    .odb
                    .read(replacement_oid)
                    .map(|o| o.kind.as_str().to_owned())
                    .unwrap_or_else(|_| "unknown".to_owned());
                writeln!(
                    out,
                    "{original_hex} ({orig_type}) -> {} ({repl_type})",
                    replacement_oid.to_hex()
                )?;
            }
        }
    }

    Ok(())
}

/// Delete one or more replace refs.
fn delete_replace_refs(repo: &Repository, args: &Args) -> Result<()> {
    // The object(s) to delete come from the positional args.
    // With clap we get at most object + replacement; for -d we treat both as objects to delete.
    let mut objects = Vec::new();
    if let Some(ref o) = args.object {
        objects.push(o.as_str());
    }
    if let Some(ref r) = args.replacement {
        objects.push(r.as_str());
    }

    if objects.is_empty() {
        bail!("object argument required for -d");
    }

    for obj_str in objects {
        let oid = resolve_revision(repo, obj_str)
            .with_context(|| format!("Failed to resolve '{obj_str}'"))?;
        let refname = format!("refs/replace/{}", oid.to_hex());

        if resolve_ref(&repo.git_dir, &refname).is_err() {
            bail!("replace ref for '{}' not found", oid.to_hex());
        }

        delete_ref(&repo.git_dir, &refname).context("deleting replace ref")?;
        eprintln!("Deleted replace ref for {}", oid.to_hex());
    }

    Ok(())
}

/// Simple glob pattern matching (supports `*` and `?`).
fn glob_matches(pattern: &str, name: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_bytes(pat: &[u8], text: &[u8]) -> bool {
    match (pat.first(), text.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            let pat_rest = pat
                .iter()
                .position(|&b| b != b'*')
                .map_or(&pat[pat.len()..], |i| &pat[i..]);
            if pat_rest.is_empty() {
                return true;
            }
            for i in 0..=text.len() {
                if glob_match_bytes(pat_rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        (Some(&b'?'), Some(_)) => glob_match_bytes(&pat[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob_match_bytes(&pat[1..], &text[1..]),
        _ => false,
    }
}
