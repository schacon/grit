//! `grit merge-one-file` — standard helper for merge-index.
//!
//! Performs a three-way file merge for a single path. Intended to be invoked
//! by `merge-index` as the merge program.
//!
//! Arguments (passed by merge-index):
//!   <base-oid> <ours-oid> <theirs-oid> <path> <base-mode> <ours-mode> <theirs-mode>

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::path::{Path, PathBuf};

use grit_lib::index::{Index, IndexEntry, MODE_REGULAR};
use grit_lib::merge_file::{merge, ConflictStyle, MergeFavor, MergeInput};
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::repo::Repository;

/// Arguments for `grit merge-one-file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Base blob OID (all zeros if none).
    pub base_oid: String,
    /// Ours blob OID (all zeros if none).
    pub ours_oid: String,
    /// Theirs blob OID (all zeros if none).
    pub theirs_oid: String,
    /// Path of the file being merged.
    pub path: String,
    /// Base file mode (octal).
    pub base_mode: String,
    /// Ours file mode (octal).
    pub ours_mode: String,
    /// Theirs file mode (octal).
    pub theirs_mode: String,
}

const EMPTY_OID: &str = "0000000000000000000000000000000000000000";

fn parse_oid(oid_hex: &str) -> Result<Option<ObjectId>> {
    if oid_hex.is_empty() || oid_hex == EMPTY_OID {
        return Ok(None);
    }
    Ok(Some(
        ObjectId::from_hex(oid_hex).with_context(|| format!("invalid OID: {oid_hex}"))?,
    ))
}

fn read_blob(repo: &Repository, oid: Option<ObjectId>) -> Result<Vec<u8>> {
    let Some(oid) = oid else {
        return Ok(Vec::new());
    };
    let obj = repo.odb.read(&oid)?;
    if obj.kind != ObjectKind::Blob {
        bail!("{} is not a blob", oid.to_hex());
    }
    Ok(obj.data)
}

fn parse_mode(mode: &str) -> Option<u32> {
    if mode.is_empty() {
        return None;
    }
    u32::from_str_radix(mode, 8).ok()
}

fn make_stage0_entry(path: &[u8], oid: ObjectId, mode: u32, size: u32) -> IndexEntry {
    IndexEntry {
        ctime_sec: 0,
        ctime_nsec: 0,
        mtime_sec: 0,
        mtime_nsec: 0,
        dev: 0,
        ino: 0,
        mode,
        uid: 0,
        gid: 0,
        size,
        oid,
        flags: path.len().min(0x0FFF) as u16,
        flags_extended: None,
        path: path.to_vec(),
    }
}

fn update_index_with_merged_blob(
    repo: &Repository,
    path: &[u8],
    merged_oid: ObjectId,
    merged_size: usize,
    preferred_mode: u32,
) -> Result<()> {
    let index_path = effective_index_path(repo)?;
    let mut index = Index::load(&index_path).context("loading index")?;

    let template = index
        .entries
        .iter()
        .find(|e| e.path == path && e.stage() == 2)
        .or_else(|| {
            index
                .entries
                .iter()
                .find(|e| e.path == path && e.stage() == 3)
        })
        .or_else(|| {
            index
                .entries
                .iter()
                .find(|e| e.path == path && e.stage() == 1)
        })
        .cloned();

    index.entries.retain(|e| e.path != path);

    let mut merged_entry = if let Some(mut t) = template {
        t.oid = merged_oid;
        t.mode = preferred_mode;
        t.size = merged_size as u32;
        t.flags &= 0x0FFF; // clear conflict stage bits
        t.path = path.to_vec();
        t
    } else {
        make_stage0_entry(path, merged_oid, preferred_mode, merged_size as u32)
    };

    merged_entry.flags &= 0x0FFF;
    index.entries.push(merged_entry);
    index.sort();
    index.write(&index_path)?;
    Ok(())
}

fn effective_index_path(repo: &Repository) -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Ok(path);
        }
        let cwd = std::env::current_dir().context("resolving GIT_INDEX_FILE")?;
        return Ok(cwd.join(path));
    }
    Ok(repo.index_path())
}

fn write_worktree_file(work_tree: &Path, path: &str, content: &[u8]) -> Result<()> {
    let abs: PathBuf = work_tree.join(path);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(abs, content)?;
    Ok(())
}

/// Run `grit merge-one-file`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let base_oid = parse_oid(&args.base_oid)?;
    let ours_oid = parse_oid(&args.ours_oid)?;
    let theirs_oid = parse_oid(&args.theirs_oid)?;

    // We currently only support regular file content merges for this helper.
    if ours_oid.is_none() || theirs_oid.is_none() {
        eprintln!("ERROR: {}: Not handling case with missing sides", args.path);
        std::process::exit(1);
    }

    let base = read_blob(&repo, base_oid)?;
    let ours = read_blob(&repo, ours_oid)?;
    let theirs = read_blob(&repo, theirs_oid)?;

    let merge_out = merge(&MergeInput {
        base: &base,
        ours: &ours,
        theirs: &theirs,
        label_ours: "ours",
        label_base: "base",
        label_theirs: "theirs",
        favor: MergeFavor::None,
        style: ConflictStyle::Merge,
        marker_size: 7,
        diff_algorithm: None,
        ignore_all_space: false,
        ignore_space_change: false,
        ignore_space_at_eol: false,
        ignore_cr_at_eol: false,
    })?;

    let path_bytes = args.path.as_bytes();
    let preferred_mode = parse_mode(&args.ours_mode)
        .or_else(|| parse_mode(&args.theirs_mode))
        .or_else(|| parse_mode(&args.base_mode))
        .unwrap_or(MODE_REGULAR);

    if merge_out.conflicts > 0 {
        eprintln!("CONFLICT (content): Merge conflict in {}", args.path);
        write_worktree_file(work_tree, &args.path, &merge_out.content)?;
        std::process::exit(1);
    }

    let merged_oid = repo.odb.write(ObjectKind::Blob, &merge_out.content)?;
    update_index_with_merged_blob(
        &repo,
        path_bytes,
        merged_oid,
        merge_out.content.len(),
        preferred_mode,
    )?;
    write_worktree_file(work_tree, &args.path, &merge_out.content)?;

    Ok(())
}
