//! `gust write-tree` — create a tree object from the current index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::BTreeMap;

use gust_lib::index::{Index, MODE_TREE};
use gust_lib::objects::{serialize_tree, tree_entry_cmp, ObjectId, ObjectKind, TreeEntry};
use gust_lib::repo::Repository;

/// Arguments for `gust write-tree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Allow writing a tree with missing objects.
    #[arg(long = "missing-ok")]
    pub missing_ok: bool,

    /// Write the tree of the named directory (prefix must end with '/').
    #[arg(long)]
    pub prefix: Option<String>,
}

/// Run `gust write-tree`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index = Index::load(&repo.index_path()).context("loading index")?;

    let prefix = args.prefix.as_deref().unwrap_or("");
    let oid = write_tree_from_index(&repo.odb, &index, prefix, args.missing_ok)
        .context("building tree from index")?;

    println!("{oid}");
    Ok(())
}

/// Build and write tree objects from the index, return the root tree OID.
///
/// Supports a `prefix` to restrict to a subtree.
pub fn write_tree_from_index(
    odb: &gust_lib::odb::Odb,
    index: &Index,
    prefix: &str,
    missing_ok: bool,
) -> Result<ObjectId> {
    // Filter entries by prefix
    let prefix_bytes = prefix.as_bytes();
    let entries: Vec<_> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.path.starts_with(prefix_bytes))
        .collect();

    if !missing_ok {
        for entry in &entries {
            if !odb.exists(&entry.oid) {
                bail!(
                    "missing object {} for path '{}'",
                    entry.oid,
                    String::from_utf8_lossy(&entry.path)
                );
            }
        }
    }

    build_tree(odb, &entries, prefix_bytes)
}

/// Recursively build tree objects for a directory level.
///
/// `entries` must be sorted (index guarantees this).
/// `prefix` is the path prefix of the current directory (may be empty for root).
fn build_tree(
    odb: &gust_lib::odb::Odb,
    entries: &[&gust_lib::index::IndexEntry],
    dir_prefix: &[u8],
) -> Result<ObjectId> {
    // Map from immediate child name → entries in that subtree (or the entry itself)
    // We build a BTreeMap keyed by (child_name, is_tree) using the Git sort order.
    let mut children: BTreeMap<Vec<u8>, ChildKind> = BTreeMap::new();

    for entry in entries {
        let path = &entry.path;
        // Strip dir prefix
        let rel = if dir_prefix.is_empty() {
            path.as_slice()
        } else {
            path.strip_prefix(dir_prefix)
                .and_then(|s| s.strip_prefix(b"/"))
                .unwrap_or(path.as_slice())
        };

        if let Some(slash) = rel.iter().position(|&b| b == b'/') {
            // Sub-directory entry
            let child_name = rel[..slash].to_vec();
            let sub_prefix: Vec<u8> = if dir_prefix.is_empty() {
                child_name.clone()
            } else {
                let mut p = dir_prefix.to_vec();
                p.push(b'/');
                p.extend_from_slice(&child_name);
                p
            };
            children
                .entry(child_name)
                .or_insert_with(|| ChildKind::Tree(sub_prefix, Vec::new()))
                .push_entry(entry);
        } else {
            // Direct file entry
            children
                .entry(rel.to_vec())
                .or_insert(ChildKind::Blob(entry));
        }
    }

    // Build sorted tree entries using Git's tree sort order
    let mut tree_entries: Vec<TreeEntry> = Vec::new();

    for (name, child) in children {
        match child {
            ChildKind::Blob(e) => {
                tree_entries.push(TreeEntry {
                    mode: e.mode,
                    name: name.clone(),
                    oid: e.oid,
                });
            }
            ChildKind::Tree(sub_prefix, sub_entries) => {
                let refs: Vec<&gust_lib::index::IndexEntry> = sub_entries.to_vec();
                let sub_oid = build_tree(odb, &refs, &sub_prefix)?;
                tree_entries.push(TreeEntry {
                    mode: MODE_TREE,
                    name: name.clone(),
                    oid: sub_oid,
                });
            }
        }
    }

    // Sort using Git's tree entry comparator
    tree_entries.sort_by(|a, b| {
        let a_tree = a.mode == MODE_TREE;
        let b_tree = b.mode == MODE_TREE;
        tree_entry_cmp(&a.name, a_tree, &b.name, b_tree)
    });

    let data = serialize_tree(&tree_entries);
    let oid = odb.write(ObjectKind::Tree, &data).context("writing tree")?;
    Ok(oid)
}

enum ChildKind<'a> {
    Blob(&'a gust_lib::index::IndexEntry),
    Tree(Vec<u8>, Vec<&'a gust_lib::index::IndexEntry>),
}

impl<'a> ChildKind<'a> {
    fn push_entry(&mut self, entry: &'a gust_lib::index::IndexEntry) {
        if let ChildKind::Tree(_, ref mut v) = self {
            v.push(entry);
        }
    }
}
