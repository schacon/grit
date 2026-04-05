//! `grit merge-tree` — three-way merge without touching index or working tree.
//!
//! `git merge-tree <base-tree> <branch1-tree> <branch2-tree>`
//!
//! Performs a three-way merge of two trees against their common base and
//! prints the result to stdout.  No index or working-tree changes are made.
//! For each file that differs, the merged content (or conflict markers) is
//! shown.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::merge_file::{merge, ConflictStyle, MergeFavor, MergeInput};
use grit_lib::objects::{parse_tree, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::collections::BTreeMap;
use std::io::{self, Write};

/// Arguments for `grit merge-tree`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Show three-way merge without touching index/worktree",
    override_usage = "grit merge-tree <base-tree> <branch1> <branch2>"
)]
pub struct Args {
    /// Base tree-ish (common ancestor).
    pub base: String,
    /// First branch tree-ish.
    pub branch1: String,
    /// Second branch tree-ish.
    pub branch2: String,
}

/// Run the `merge-tree` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)?;

    let base_tree = resolve_to_tree(&repo, &args.base)?;
    let branch1_tree = resolve_to_tree(&repo, &args.branch1)?;
    let branch2_tree = resolve_to_tree(&repo, &args.branch2)?;

    let base_entries = read_tree_flat(&repo, &base_tree, b"")?;
    let b1_entries = read_tree_flat(&repo, &branch1_tree, b"")?;
    let b2_entries = read_tree_flat(&repo, &branch2_tree, b"")?;

    // Collect all paths.
    let mut all_paths: Vec<Vec<u8>> = Vec::new();
    for k in base_entries
        .keys()
        .chain(b1_entries.keys())
        .chain(b2_entries.keys())
    {
        if !all_paths.contains(k) {
            all_paths.push(k.clone());
        }
    }
    all_paths.sort();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &all_paths {
        let base_entry = base_entries.get(path);
        let b1_entry = b1_entries.get(path);
        let b2_entry = b2_entries.get(path);

        // Skip if all three are the same.
        if b1_entry == base_entry && b2_entry == base_entry {
            continue;
        }

        // If only one side changed from base, the result is that side (no conflict).
        // If both changed identically, also no conflict.
        // Only show output when there's something interesting.
        let b1_changed = b1_entry != base_entry;
        let b2_changed = b2_entry != base_entry;

        if !b1_changed && !b2_changed {
            continue;
        }

        // Both changed identically — clean merge, skip output.
        if b1_changed && b2_changed && b1_entry == b2_entry {
            continue;
        }

        let path_str = String::from_utf8_lossy(path);

        // Both sides changed differently — potential conflict or content merge.
        if b1_changed && b2_changed {
            // Need content-level merge for blobs.
            let base_content = read_blob_content(&repo, base_entry)?;
            let b1_content = read_blob_content(&repo, b1_entry)?;
            let b2_content = read_blob_content(&repo, b2_entry)?;

            let input = MergeInput {
                base: &base_content,
                ours: &b1_content,
                theirs: &b2_content,
                label_ours: "branch1",
                label_base: "base",
                label_theirs: "branch2",
                favor: MergeFavor::None,
                style: ConflictStyle::Merge,
                marker_size: 0,
            };

            let result = merge(&input).context("merge failed")?;

            // Print header and content.
            if result.conflicts > 0 {
                writeln!(out, "changed in both")?;
                writeln!(out, "  base   {}", format_entry_info(base_entry))?;
                writeln!(out, "  our    {}", format_entry_info(b1_entry))?;
                writeln!(out, "  their  {}", format_entry_info(b2_entry))?;
            }
            // Show the merged/conflicted content.
            write!(out, "{}", String::from_utf8_lossy(&result.content))?;
        } else if b1_changed {
            // Only branch1 changed — show addition/removal info.
            if base_entry.is_none() {
                writeln!(out, "added in local: {path_str}")?;
            } else if b1_entry.is_none() {
                writeln!(out, "removed in local: {path_str}")?;
            }
        } else {
            // Only branch2 changed.
            if base_entry.is_none() {
                writeln!(out, "added in remote: {path_str}")?;
            } else if b2_entry.is_none() {
                writeln!(out, "removed in remote: {path_str}")?;
            }
        }
    }

    Ok(())
}

fn format_entry_info(entry: Option<&(u32, ObjectId)>) -> String {
    match entry {
        Some((mode, oid)) => format!("{:o} {}", mode, oid),
        None => "(empty)".to_string(),
    }
}

/// Resolve a revision spec to a tree OID.
fn resolve_to_tree(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let oid = resolve_revision(repo, spec)?;
    let obj = repo.odb.read(&oid)?;
    match obj.kind {
        ObjectKind::Tree => Ok(oid),
        ObjectKind::Commit => {
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            Ok(commit.tree)
        }
        ObjectKind::Tag => {
            // Peel tag to commit, then to tree.
            let tag_text = String::from_utf8_lossy(&obj.data);
            for line in tag_text.lines() {
                if let Some(rest) = line.strip_prefix("object ") {
                    let target_oid: ObjectId = rest.trim().parse()?;
                    return resolve_to_tree(repo, &target_oid.to_hex());
                }
            }
            anyhow::bail!("cannot peel tag to tree: {spec}");
        }
        _ => anyhow::bail!("{spec} is not a tree-ish"),
    }
}

/// Read a tree recursively into a flat map of path → (mode, oid) for blobs.
fn read_tree_flat(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &[u8],
) -> Result<BTreeMap<Vec<u8>, (u32, ObjectId)>> {
    let obj = repo.odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;
    let mut map = BTreeMap::new();

    for entry in entries {
        let mut full_path = prefix.to_vec();
        if !full_path.is_empty() {
            full_path.push(b'/');
        }
        full_path.extend_from_slice(&entry.name);

        if entry.mode == 0o040000 {
            // Recurse into subtrees.
            let sub = read_tree_flat(repo, &entry.oid, &full_path)?;
            map.extend(sub);
        } else {
            map.insert(full_path, (entry.mode, entry.oid));
        }
    }

    Ok(map)
}

/// Read blob content from an entry, returning empty bytes if entry is None.
fn read_blob_content(repo: &Repository, entry: Option<&(u32, ObjectId)>) -> Result<Vec<u8>> {
    match entry {
        Some((_, oid)) => {
            let obj = repo.odb.read(oid)?;
            Ok(obj.data)
        }
        None => Ok(Vec::new()),
    }
}
