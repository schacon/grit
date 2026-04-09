//! `grit pack-refs` command.
//!
//! Packs loose refs into `.git/packed-refs` for faster ref lookups.
//! By default, all refs are packed and loose ref files are pruned.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectKind;
use grit_lib::odb::Odb;
use grit_lib::refs::{list_refs, read_ref_file, Ref};
use grit_lib::repo::Repository;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Arguments for `grit pack-refs`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Pack all refs (default).
    #[arg(long)]
    pub all: bool,

    /// Prune loose refs after packing (default).
    #[arg(long = "prune")]
    pub prune: bool,

    /// Don't remove loose refs after packing.
    #[arg(long = "no-prune")]
    pub no_prune: bool,

    /// Accepted for Git compatibility; `maintenance`/hooks may pass this. Ignored for now.
    #[arg(long)]
    pub auto: bool,
}

/// Run `grit pack-refs`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;
    let git_dir = &repo.git_dir;

    // Collect all loose refs under refs/
    let refs = list_refs(git_dir, "refs/").context("failed to list refs")?;

    // Read existing packed-refs to merge with
    let mut packed = read_existing_packed_refs(git_dir)?;

    if refs.is_empty() && packed.is_empty() {
        return Ok(());
    }

    // Add/update with loose refs
    for (refname, oid) in &refs {
        let peeled = peel_to_non_tag(&repo.odb, oid);
        packed.insert(
            refname.clone(),
            PackedRef {
                oid: oid.to_string(),
                peeled,
            },
        );
    }

    // Write packed-refs (always rewrite to normalize header)
    write_packed_refs(git_dir, &packed).context("failed to write packed-refs")?;

    // Prune loose refs unless --no-prune
    if !args.no_prune {
        for (refname, _) in &refs {
            prune_loose_ref(git_dir, refname);
        }
    }

    Ok(())
}

struct PackedRef {
    oid: String,
    /// If this is an annotated tag, the peeled (non-tag) OID.
    peeled: Option<String>,
}

/// Read existing packed-refs file into a map.
fn read_existing_packed_refs(git_dir: &Path) -> Result<BTreeMap<String, PackedRef>> {
    let path = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e.into()),
    };

    let mut map: BTreeMap<String, PackedRef> = BTreeMap::new();
    let mut last_ref: Option<String> = None;

    for line in content.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(hex) = line.strip_prefix('^') {
            // Peeled line for the previous ref
            if let Some(ref name) = last_ref {
                if let Some(entry) = map.get_mut(name) {
                    entry.peeled = Some(hex.trim().to_owned());
                }
            }
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let hash = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        if hash.len() == 40 && !name.is_empty() {
            last_ref = Some(name.to_owned());
            map.insert(
                name.to_owned(),
                PackedRef {
                    oid: hash.to_owned(),
                    peeled: None,
                },
            );
        }
    }

    Ok(map)
}

/// Write packed-refs file atomically via a lock file.
fn write_packed_refs(git_dir: &Path, packed: &BTreeMap<String, PackedRef>) -> Result<()> {
    let mut out = String::new();
    out.push_str("# pack-refs with: peeled fully-peeled sorted\n");

    for (name, entry) in packed {
        out.push_str(&entry.oid);
        out.push(' ');
        out.push_str(name);
        out.push('\n');
        if let Some(ref peeled) = entry.peeled {
            out.push('^');
            out.push_str(peeled);
            out.push('\n');
        }
    }

    let path = git_dir.join("packed-refs");
    let lock = git_dir.join("packed-refs.lock");
    fs::write(&lock, &out)?;
    fs::rename(&lock, &path)?;
    Ok(())
}

/// Peel an annotated tag to its ultimate non-tag target.
/// Returns None if the object is not a tag.
fn peel_to_non_tag(odb: &Odb, oid: &grit_lib::objects::ObjectId) -> Option<String> {
    let obj = odb.read(oid).ok()?;
    if obj.kind != ObjectKind::Tag {
        return None;
    }

    // Walk the tag chain
    let mut current_oid = parse_tag_target(&obj.data)?;
    loop {
        let inner = odb.read(&current_oid).ok()?;
        if inner.kind != ObjectKind::Tag {
            return Some(current_oid.to_string());
        }
        current_oid = parse_tag_target(&inner.data)?;
    }
}

/// Parse the `object <hex>` line from raw tag data.
fn parse_tag_target(data: &[u8]) -> Option<grit_lib::objects::ObjectId> {
    let text = std::str::from_utf8(data).ok()?;
    for line in text.lines() {
        if let Some(target) = line.strip_prefix("object ") {
            return target.trim().parse().ok();
        }
    }
    None
}

/// Remove a loose ref file and clean up empty parent directories.
fn prune_loose_ref(git_dir: &Path, refname: &str) {
    let path = git_dir.join(refname);

    // Don't remove symbolic refs
    if let Ok(Ref::Symbolic(_)) = read_ref_file(&path) {
        return;
    }

    let _ = fs::remove_file(&path);

    // Clean up empty parent dirs up to refs/
    let refs_dir = git_dir.join("refs");
    let mut dir = path.parent().map(|p| p.to_path_buf());
    while let Some(d) = dir {
        if d == refs_dir || !d.starts_with(&refs_dir) {
            break;
        }
        if fs::remove_dir(&d).is_err() {
            break; // not empty or other error
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
}
