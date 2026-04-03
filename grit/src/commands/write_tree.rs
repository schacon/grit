//! `grit write-tree` — create a tree object from the current index.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use grit_lib::index::{Index, MODE_TREE};
use grit_lib::objects::{serialize_tree, ObjectId, ObjectKind, TreeEntry};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;

/// Arguments for `grit write-tree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Allow writing a tree with missing objects.
    #[arg(long = "missing-ok")]
    pub missing_ok: bool,

    /// Write the tree of the named directory (prefix must end with '/').
    #[arg(long)]
    pub prefix: Option<String>,
}

/// Run `grit write-tree`.
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
    odb: &Odb,
    index: &Index,
    prefix: &str,
    missing_ok: bool,
) -> Result<ObjectId> {
    let prefix_bytes = prefix.as_bytes();

    // Collect stage-0 entries matching the prefix.
    // The index is already sorted by path — we exploit this for a single-pass
    // tree build (similar to git's cache_tree_update).
    let entries: Vec<_> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.path.starts_with(prefix_bytes))
        .collect();

    // Verify all referenced objects exist (unless --missing-ok).
    // Skip gitlink entries (mode 160000) — their OIDs reference commits
    // in submodule repositories, not the parent ODB.
    if !missing_ok {
        for entry in &entries {
            if entry.mode == 0o160000 {
                continue; // gitlink: submodule commit, not in our ODB
            }
            if odb.read(&entry.oid).is_err() {
                let path = String::from_utf8_lossy(&entry.path);
                anyhow::bail!("invalid object {} '{}'", entry.oid.to_hex(), path);
            }
        }
    }

    let dir_prefix = if prefix_bytes.ends_with(b"/") {
        &prefix_bytes[..prefix_bytes.len() - 1]
    } else {
        prefix_bytes
    };

    let (oid, _) = build_tree_flat(odb, &entries, 0, dir_prefix)?;
    Ok(oid)
}

/// Single-pass tree builder.
///
/// Processes `entries[start..]` that belong under `dir_prefix`.
/// Returns `(tree_oid, next_index)` where `next_index` is the first entry
/// index NOT consumed by this directory level.
fn build_tree_flat(
    odb: &Odb,
    entries: &[&grit_lib::index::IndexEntry],
    start: usize,
    dir_prefix: &[u8],
) -> Result<(ObjectId, usize)> {
    let mut tree_entries: Vec<TreeEntry> = Vec::new();
    let mut i = start;

    while i < entries.len() {
        let entry = entries[i];
        let path = &entry.path;

        // Check if this entry still belongs under dir_prefix
        let rel = if dir_prefix.is_empty() {
            path.as_slice()
        } else {
            match path.strip_prefix(dir_prefix) {
                Some(rest) => match rest.strip_prefix(b"/") {
                    Some(r) => r,
                    None => break, // doesn't belong here
                },
                None => break, // doesn't belong here
            }
        };

        if let Some(slash) = rel.iter().position(|&b| b == b'/') {
            // Subdirectory — recurse. All entries sharing this directory
            // component will be consumed by the recursive call.
            let child_name = &rel[..slash];
            let sub_prefix: Vec<u8> = if dir_prefix.is_empty() {
                child_name.to_vec()
            } else {
                let mut p = Vec::with_capacity(dir_prefix.len() + 1 + child_name.len());
                p.extend_from_slice(dir_prefix);
                p.push(b'/');
                p.extend_from_slice(child_name);
                p
            };

            let (sub_oid, next) = build_tree_flat(odb, entries, i, &sub_prefix)?;
            tree_entries.push(TreeEntry {
                mode: MODE_TREE,
                name: child_name.to_vec(),
                oid: sub_oid,
            });
            i = next;
        } else {
            // Leaf blob/symlink/gitlink entry
            tree_entries.push(TreeEntry {
                mode: entry.mode,
                name: rel.to_vec(),
                oid: entry.oid,
            });
            i += 1;
        }
    }

    // Index entries are sorted, so tree_entries is already in Git's canonical
    // tree order — no need to sort.  (Git's tree sort appends '/' to directory
    // names, which is consistent with lexicographic byte order of the paths
    // stored in the index.)

    let data = serialize_tree(&tree_entries);
    let oid = odb.write(ObjectKind::Tree, &data).context("writing tree")?;
    Ok((oid, i))
}
