//! `gust read-tree` — read tree information into the index.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use gust_lib::error::Error as GustError;
use gust_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use gust_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
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
    let index_path = effective_index_path(&repo)?;

    let tree_oids: Vec<ObjectId> = args
        .trees
        .iter()
        .map(|t| resolve_tree_ish(&repo, t))
        .collect::<Result<Vec<_>>>()?;

    if tree_oids.is_empty() {
        bail!("at least one tree required");
    }
    if tree_oids.len() > 3 {
        bail!("too many trees (max 3)");
    }

    if let Some(prefix) = &args.prefix {
        if prefix.starts_with('/') {
            bail!("--prefix must be relative to repository root");
        }
        if !prefix.ends_with('/') {
            bail!("--prefix requires a trailing '/'");
        }
        if args.merge || args.update || args.reset || tree_oids.len() != 1 {
            bail!("--prefix only supports a single non-merge tree read");
        }
    }

    if args.reset {
        // Reset mode is a hard replacement by the final tree argument.
        let old_index = load_index_for_read_tree(&index_path).context("loading index")?;
        let mut new_index = Index::new();
        new_index.entries = tree_to_index_entries(&repo, &tree_oids[tree_oids.len() - 1], "")?;
        new_index.sort();
        if args.update {
            checkout_index_entries(&repo, &old_index, &new_index)?;
        }
        new_index.write(&index_path).context("writing index")?;
        return Ok(());
    }

    let old_index = load_index_for_read_tree(&index_path).context("loading index")?;
    let mut new_index = old_index.clone();

    if let Some(prefix) = &args.prefix {
        read_tree_into_index_prefixed(&repo, &tree_oids[0], prefix, &mut new_index)?;
    } else if !args.merge {
        if tree_oids.len() == 1 {
            // Replace index with one tree.
            new_index = Index::new();
            new_index.entries = tree_to_index_entries(&repo, &tree_oids[0], "")?;
            new_index.sort();
        } else {
            // Multi-tree overlay: later trees override earlier trees by path.
            new_index = Index::new();
            for oid in &tree_oids {
                for e in tree_to_index_entries(&repo, oid, "")? {
                    add_or_replace_with_df_cleanup(&mut new_index, e);
                }
            }
        }
    } else {
        match tree_oids.len() {
            1 => {
                // `-m` with one tree acts like a carry-forward overlay.
                for e in tree_to_index_entries(&repo, &tree_oids[0], "")? {
                    add_or_replace_with_df_cleanup(&mut new_index, e);
                }
            }
            2 => {
                let old_tree = tree_to_map(tree_to_index_entries(&repo, &tree_oids[0], "")?);
                let new_tree = tree_to_map(tree_to_index_entries(&repo, &tree_oids[1], "")?);
                new_index = two_way_merge(&old_index, &old_tree, &new_tree)?;
            }
            3 => {
                let base = tree_to_map(tree_to_index_entries(&repo, &tree_oids[0], "")?);
                let ours = tree_to_map(tree_to_index_entries(&repo, &tree_oids[1], "")?);
                let theirs = tree_to_map(tree_to_index_entries(&repo, &tree_oids[2], "")?);
                new_index = three_way_merge(&old_index, &base, &ours, &theirs);
            }
            _ => unreachable!("tree count validated above"),
        }
    }

    if args.update {
        checkout_index_entries(&repo, &old_index, &new_index)?;
    }
    new_index.write(&index_path).context("writing index")?;

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
        add_or_replace_with_df_cleanup(index, e);
    }
    Ok(())
}

fn tree_to_map(entries: Vec<IndexEntry>) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in entries {
        out.insert(e.path.clone(), e);
    }
    out
}

fn add_or_replace_with_df_cleanup(index: &mut Index, entry: IndexEntry) {
    let new_path = entry.path.clone();
    index
        .entries
        .retain(|e| e.stage() != 0 || !paths_conflict_for_df(&e.path, &new_path));
    index.add_or_replace(entry);
}

fn paths_conflict_for_df(a: &[u8], b: &[u8]) -> bool {
    a == b || path_is_parent_of(a, b) || path_is_parent_of(b, a)
}

fn path_is_parent_of(parent: &[u8], child: &[u8]) -> bool {
    if parent.len() >= child.len() {
        return false;
    }
    child.starts_with(parent) && child[parent.len()] == b'/'
}

fn stage0_index_map(index: &Index) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in &index.entries {
        if e.stage() == 0 {
            out.insert(e.path.clone(), e.clone());
        }
    }
    out
}

fn same_blob(a: &IndexEntry, b: &IndexEntry) -> bool {
    a.oid == b.oid && a.mode == b.mode
}

fn two_way_merge(
    current_index: &Index,
    old_tree: &HashMap<Vec<u8>, IndexEntry>,
    new_tree: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<Index> {
    let mut result = stage0_index_map(current_index);
    let current = stage0_index_map(current_index);
    let mut conflicts = Vec::new();

    let mut all_paths = BTreeSet::new();
    all_paths.extend(old_tree.keys().cloned());
    all_paths.extend(new_tree.keys().cloned());

    for path in all_paths {
        let old = old_tree.get(&path);
        let new = new_tree.get(&path);
        let cur = current.get(&path);

        match (old, new) {
            (Some(o), Some(n)) if same_blob(o, n) => {
                // unchanged between trees: carry current index forward, or
                // populate from the trees when starting from an empty index.
                if cur.is_none() {
                    result.insert(path.clone(), n.clone());
                }
            }
            (None, Some(n)) => match cur {
                None => {
                    result.insert(path.clone(), n.clone());
                }
                Some(c) if same_blob(c, n) => {}
                Some(_) => conflicts.push(String::from_utf8_lossy(&path).into_owned()),
            },
            (Some(o), None) => match cur {
                None => {
                    result.remove(&path);
                }
                Some(c) if same_blob(c, o) => {
                    result.remove(&path);
                }
                Some(_) => conflicts.push(String::from_utf8_lossy(&path).into_owned()),
            },
            (Some(o), Some(n)) => match cur {
                None => {
                    // Empty/new index case: just move to the merged head.
                    result.insert(path.clone(), n.clone());
                }
                Some(c) if same_blob(c, o) => {
                    result.insert(path.clone(), n.clone());
                }
                Some(c) if same_blob(c, n) => {
                    // already at target
                }
                Some(_) => conflicts.push(String::from_utf8_lossy(&path).into_owned()),
            },
            (None, None) => {}
        }
    }

    if !conflicts.is_empty() {
        bail!(
            "read-tree: merge conflict in {} path(s): {}",
            conflicts.len(),
            conflicts.join(", ")
        );
    }

    let mut out = Index::new();
    out.entries = result.into_values().collect();
    out.sort();
    Ok(out)
}

fn three_way_merge(
    current_index: &Index,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Index {
    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut out = Index::new();
    // Preserve unrelated current stage-0 paths that are outside merge inputs.
    let merge_paths: HashSet<Vec<u8>> = all_paths.iter().cloned().collect();
    for e in &current_index.entries {
        if e.stage() == 0 && !merge_paths.contains(&e.path) {
            out.entries.push(e.clone());
        }
    }

    for path in all_paths {
        let b = base.get(&path);
        let o = ours.get(&path);
        let t = theirs.get(&path);

        match (b, o, t) {
            (_, Some(oe), Some(te)) if oe.oid == te.oid => {
                // Both same: take ours
                out.entries.push((*oe).clone());
            }
            (Some(be), Some(oe), Some(te)) if be.oid == oe.oid => {
                // Only theirs changed: take theirs
                out.entries.push((*te).clone());
            }
            (Some(be), Some(oe), Some(te)) if be.oid == te.oid => {
                // Only ours changed: take ours
                out.entries.push((*oe).clone());
            }
            (None, Some(oe), None) => {
                // Added by us only
                out.entries.push((*oe).clone());
            }
            (None, None, Some(te)) => {
                // Added by them only
                out.entries.push((*te).clone());
            }
            (Some(_), None, None) => {
                // Deleted by both: skip
            }
            (Some(be), None, Some(te)) => {
                // Deleted by us, modified by them: conflict
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, te, 3);
            }
            (Some(be), Some(oe), None) => {
                // Modified by us, deleted by them: conflict
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, oe, 2);
            }
            _ => {
                // True conflict: add all three stages
                if let Some(be) = b {
                    stage_entry(&mut out, be, 1);
                }
                if let Some(oe) = o {
                    stage_entry(&mut out, oe, 2);
                }
                if let Some(te) = t {
                    stage_entry(&mut out, te, 3);
                }
            }
        }
    }

    out.sort();
    out
}

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    // Clear and set stage bits in flags
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

/// Update working tree to match stage-0 entries in `new_index`.
fn checkout_index_entries(repo: &Repository, old_index: &Index, new_index: &Index) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let old_stage0: HashSet<Vec<u8>> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    let new_stage0: HashSet<Vec<u8>> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();

    for old_path in old_stage0.difference(&new_stage0) {
        let rel = String::from_utf8_lossy(old_path).into_owned();
        let abs = work_tree.join(&rel);
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        } else if abs.is_dir() {
            let _ = std::fs::remove_dir_all(&abs);
        }
        remove_empty_parent_dirs(&work_tree, &abs);
    }

    for entry in &new_index.entries {
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
            .context("reading object for checkout")?;
        if obj.kind != ObjectKind::Blob {
            bail!("cannot checkout non-blob at '{}'", path_str);
        }
        if abs_path.is_dir() {
            std::fs::remove_dir_all(&abs_path)?;
        }
        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if abs_path.exists() {
                std::fs::remove_file(&abs_path)?;
            }
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            std::fs::write(&abs_path, &obj.data)?;
            if entry.mode == MODE_EXECUTABLE {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&abs_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&abs_path, perms)?;
            }
        }
    }
    Ok(())
}

fn remove_empty_parent_dirs(work_tree: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match std::fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

fn effective_index_path(repo: &Repository) -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            return Ok(p);
        }
        let cwd = std::env::current_dir().context("resolving GIT_INDEX_FILE")?;
        return Ok(cwd.join(p));
    }
    Ok(repo.index_path())
}

fn load_index_for_read_tree(path: &Path) -> Result<Index> {
    match Index::load(path) {
        Ok(index) => Ok(index),
        Err(GustError::IndexError(msg)) if msg == "file too short" => Ok(Index::new()),
        Err(err) => Err(err.into()),
    }
}

fn resolve_tree_ish(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return peel_to_tree(repo, oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return peel_to_tree(repo, oid);
    }
    let as_branch = format!("refs/heads/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_branch) {
        return peel_to_tree(repo, oid);
    }
    bail!("not a valid tree-ish: '{s}'")
}

fn peel_to_tree(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let c = parse_commit(&obj.data)?;
                oid = c.tree;
            }
            ObjectKind::Tag => {
                let (target, target_kind) = parse_tag_target(&obj.data)?;
                if target_kind == "tree" {
                    return Ok(target);
                }
                oid = target;
            }
            _ => bail!("object '{}' does not name a tree", oid),
        }
    }
}

fn parse_tag_target(data: &[u8]) -> Result<(ObjectId, String)> {
    let text = std::str::from_utf8(data).context("tag object is not UTF-8")?;
    let mut object = None;
    let mut kind = None;
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("object ") {
            object = Some(rest.trim().parse::<ObjectId>()?);
        } else if let Some(rest) = line.strip_prefix("type ") {
            kind = Some(rest.trim().to_owned());
        }
    }
    Ok((
        object.context("tag missing object header")?,
        kind.context("tag missing type header")?,
    ))
}
