//! `gust read-tree` — read tree information into the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;

use gust_lib::index::{Index, IndexEntry};
use gust_lib::objects::{parse_tree, ObjectId, ObjectKind};
use gust_lib::refs::resolve_ref;
use gust_lib::repo::Repository;

/// Arguments for `gust read-tree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Perform a merge (2-tree or 3-tree).
    #[arg(short = 'm')]
    pub merge: bool,

    /// Update working tree after reading.
    #[arg(short = 'u')]
    pub update: bool,

    /// Reset the index (discard conflicting entries).
    #[arg(long)]
    pub reset: bool,

    /// Stage a tree into the index under the given prefix (must end with /).
    #[arg(long)]
    pub prefix: Option<String>,

    /// Do not print error messages for missing paths.
    #[arg(long = "aggressive")]
    pub aggressive: bool,

    /// Tree-ish arguments (1 for reset, 2 for 2-way merge, 3 for 3-way merge).
    pub trees: Vec<String>,
}

/// Run `gust read-tree`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();

    let tree_oids: Vec<ObjectId> = args
        .trees
        .iter()
        .map(|t| resolve_tree_ish(&repo, t))
        .collect::<Result<Vec<_>>>()?;

    match tree_oids.len() {
        0 => bail!("at least one tree required"),
        1 => {
            // Single-tree read: load tree into index (replacing it)
            let mut index = if args.merge || args.reset {
                Index::load(&index_path).context("loading index")?
            } else {
                Index::new()
            };

            if let Some(prefix) = &args.prefix {
                read_tree_into_index_prefixed(&repo, &tree_oids[0], prefix, &mut index)?;
            } else {
                // Overlay or replace
                let new_entries = tree_to_index_entries(&repo, &tree_oids[0], "")?;
                if args.merge || args.reset {
                    // Overlay: just add/replace
                    for e in new_entries {
                        index.add_or_replace(e);
                    }
                } else {
                    index.entries = new_entries;
                    index.sort();
                }
            }

            if args.update || args.reset {
                checkout_index_entries(&repo, &index)?;
            }

            index.write(&index_path).context("writing index")?;
        }
        2 => {
            // 2-way merge
            let mut index = Index::load(&index_path).context("loading index")?;
            let entries_a = tree_to_index_entries(&repo, &tree_oids[0], "")?;
            let entries_b = tree_to_index_entries(&repo, &tree_oids[1], "")?;
            two_way_merge(&mut index, entries_a, entries_b);
            if args.update {
                checkout_index_entries(&repo, &index)?;
            }
            index.write(&index_path).context("writing index")?;
        }
        3 => {
            // 3-way merge
            let mut index = Index::load(&index_path).context("loading index")?;
            let base = tree_to_index_entries(&repo, &tree_oids[0], "")?;
            let ours = tree_to_index_entries(&repo, &tree_oids[1], "")?;
            let theirs = tree_to_index_entries(&repo, &tree_oids[2], "")?;
            three_way_merge(&mut index, base, ours, theirs);
            if args.update {
                checkout_index_entries(&repo, &index)?;
            }
            index.write(&index_path).context("writing index")?;
        }
        _ => bail!("too many trees (max 3)"),
    }

    Ok(())
}

/// Recursively read a tree object into index entries.
fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            // Sub-tree: recurse
            let sub = tree_to_index_entries(repo, &te.oid, &path)?;
            result.extend(sub);
        } else {
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: te.mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
            });
        }
    }
    Ok(result)
}

/// Read a tree into the index under a prefix.
fn read_tree_into_index_prefixed(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
    index: &mut Index,
) -> Result<()> {
    // Strip trailing slash from prefix for storage
    let prefix = prefix.trim_end_matches('/');
    let entries = tree_to_index_entries(repo, oid, prefix)?;
    for e in entries {
        index.add_or_replace(e);
    }
    Ok(())
}

/// Trivial 2-way merge: entries only in B replace entries only in A.
fn two_way_merge(index: &mut Index, entries_a: Vec<IndexEntry>, entries_b: Vec<IndexEntry>) {
    // Simple: replace index with entries_b (overlay on top of index contents)
    for e in entries_b {
        index.add_or_replace(e);
    }
    // Remove entries that were in A but not B (by checking against original)
    let _ = entries_a; // placeholder for more complex logic
}

/// Trivial 3-way merge: produce stage 1/2/3 for conflicts.
fn three_way_merge(
    index: &mut Index,
    base: Vec<IndexEntry>,
    ours: Vec<IndexEntry>,
    theirs: Vec<IndexEntry>,
) {
    use std::collections::HashMap;

    let base_map: HashMap<Vec<u8>, &IndexEntry> =
        base.iter().map(|e| (e.path.clone(), e)).collect();
    let our_map: HashMap<Vec<u8>, &IndexEntry> = ours.iter().map(|e| (e.path.clone(), e)).collect();
    let their_map: HashMap<Vec<u8>, &IndexEntry> =
        theirs.iter().map(|e| (e.path.clone(), e)).collect();

    let mut all_paths: Vec<Vec<u8>> = {
        let mut s: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
        for e in &base {
            s.insert(e.path.clone());
        }
        for e in &ours {
            s.insert(e.path.clone());
        }
        for e in &theirs {
            s.insert(e.path.clone());
        }
        s.into_iter().collect()
    };
    all_paths.sort();

    index.entries.clear();

    for path in all_paths {
        let b = base_map.get(&path);
        let o = our_map.get(&path);
        let t = their_map.get(&path);

        match (b, o, t) {
            (_, Some(oe), Some(te)) if oe.oid == te.oid => {
                // Both same: take ours
                index.entries.push((*oe).clone());
            }
            (Some(be), Some(oe), Some(te)) if be.oid == oe.oid => {
                // Only theirs changed: take theirs
                index.entries.push((*te).clone());
            }
            (Some(be), Some(oe), Some(te)) if be.oid == te.oid => {
                // Only ours changed: take ours
                index.entries.push((*oe).clone());
            }
            (None, Some(oe), None) => {
                // Added by us only
                index.entries.push((*oe).clone());
            }
            (None, None, Some(te)) => {
                // Added by them only
                index.entries.push((*te).clone());
            }
            (Some(_), None, None) => {
                // Deleted by both: skip
            }
            (Some(be), None, Some(te)) => {
                // Deleted by us, modified by them: conflict
                stage_entry(index, be, 1);
                stage_entry(index, te, 3);
            }
            (Some(be), Some(oe), None) => {
                // Modified by us, deleted by them: conflict
                stage_entry(index, be, 1);
                stage_entry(index, oe, 2);
            }
            _ => {
                // True conflict: add all three stages
                if let Some(be) = b {
                    stage_entry(index, be, 1);
                }
                if let Some(oe) = o {
                    stage_entry(index, oe, 2);
                }
                if let Some(te) = t {
                    stage_entry(index, te, 3);
                }
            }
        }
    }

    index.sort();
}

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    // Clear and set stage bits in flags
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

/// Check out index entries to working tree (minimal: just write blobs).
fn checkout_index_entries(repo: &Repository, index: &Index) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let obj = repo
            .odb
            .read(&entry.oid)
            .context("reading blob for checkout")?;
        std::fs::write(&abs_path, &obj.data)?;
    }
    Ok(())
}

fn resolve_tree_ish(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return Ok(oid);
    }
    let as_branch = format!("refs/heads/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_branch) {
        return Ok(oid);
    }
    bail!("not a valid tree-ish: '{s}'")
}
