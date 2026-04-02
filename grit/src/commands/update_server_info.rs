//! `grit update-server-info` — update auxiliary info for dumb HTTP transport.
//!
//! Writes `info/refs` and `objects/info/packs` so that dumb HTTP/FTP
//! clients can discover refs and pack files without smart protocol.
//!
//! Usage:
//!   grit update-server-info

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

/// Arguments for `grit update-server-info`.
#[derive(Debug, ClapArgs)]
#[command(about = "Update auxiliary info file to help dumb servers")]
pub struct Args {
    /// Force overwriting of existing info files.
    #[arg(short = 'f', long = "force")]
    pub force: bool,
}

/// Run the `update-server-info` command.
pub fn run(args: Args) -> Result<()> {
    let _ = args;
    let repo = Repository::discover(None)?;

    update_info_refs(&repo)?;
    update_info_packs(&repo)?;

    Ok(())
}

// ── info/refs ────────────────────────────────────────────────────────

/// Write `info/refs` — one line per ref: `<hex-oid>\t<refname>\n`.
fn update_info_refs(repo: &Repository) -> Result<()> {
    let info_dir = repo.git_dir.join("info");
    fs::create_dir_all(&info_dir)
        .with_context(|| format!("creating {}", info_dir.display()))?;

    let refs = collect_all_refs(&repo.git_dir)?;
    let mut out = String::new();
    for (name, oid) in &refs {
        out.push_str(&format!("{oid}\t{name}\n"));
    }

    fs::write(info_dir.join("refs"), out)
        .context("writing info/refs")?;

    Ok(())
}

/// Collect all refs (loose + packed), sorted by name.
fn collect_all_refs(git_dir: &Path) -> Result<BTreeMap<String, ObjectId>> {
    let mut refs = BTreeMap::new();

    // Loose refs under refs/
    collect_loose_refs(git_dir, &git_dir.join("refs"), "refs", &mut refs)?;

    // Packed refs
    let packed_path = git_dir.join("packed-refs");
    if let Ok(text) = fs::read_to_string(&packed_path) {
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(oid_str) = parts.next() else { continue };
            let Some(name) = parts.next() else { continue };
            if let Ok(oid) = oid_str.parse::<ObjectId>() {
                // Loose refs take priority (already inserted).
                refs.entry(name.to_owned()).or_insert(oid);
            }
        }
    }

    Ok(refs)
}

fn collect_loose_refs(
    git_dir: &Path,
    path: &Path,
    relative: &str,
    out: &mut BTreeMap<String, ObjectId>,
) -> Result<()> {
    let read_dir = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    for entry in read_dir {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let next_relative = format!("{relative}/{file_name}");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_loose_refs(git_dir, &entry.path(), &next_relative, out)?;
        } else if file_type.is_file() {
            if let Ok(oid) = grit_lib::refs::resolve_ref(git_dir, &next_relative) {
                out.insert(next_relative, oid);
            }
        }
    }
    Ok(())
}

// ── objects/info/packs ───────────────────────────────────────────────

/// Write `objects/info/packs` — one `P <pack-name>.pack\n` per pack file.
fn update_info_packs(repo: &Repository) -> Result<()> {
    let objects_dir = repo.odb.objects_dir();
    let info_dir = objects_dir.join("info");
    fs::create_dir_all(&info_dir)
        .with_context(|| format!("creating {}", info_dir.display()))?;

    let pack_dir = objects_dir.join("pack");
    let mut packs: Vec<String> = Vec::new();

    if let Ok(rd) = fs::read_dir(&pack_dir) {
        for entry in rd {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".pack") {
                packs.push(name);
            }
        }
    }

    packs.sort();

    let mut out = String::new();
    for name in &packs {
        out.push_str(&format!("P {name}\n"));
    }
    // Git always writes a trailing blank line.
    if !packs.is_empty() {
        out.push('\n');
    }

    fs::write(info_dir.join("packs"), out)
        .context("writing objects/info/packs")?;

    Ok(())
}
