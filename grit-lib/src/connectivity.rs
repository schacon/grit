//! Reachability checks for push / receive-pack connectivity verification.
//!
//! Approximates Git's `check_connected` / `rev-list --objects` walk for the cases
//! exercised by the harness: full object closure from a tip, and "new objects only"
//! when existing ref tips are treated as roots.

use std::collections::{HashSet, VecDeque};

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::objects::{parse_commit, parse_tag, parse_tree, Object, ObjectId, ObjectKind};
use crate::refs;
use crate::repo::Repository;

/// Returns every commit reachable from all refs under `refs/` (not `HEAD` alone).
///
/// Git's connectivity check uses `--all` (refs), not `HEAD` resolution: a symbolic `HEAD`
/// that points at `refs/heads/main` must not be treated as an extra root, or pushes that
/// only advance `main` would be misclassified as already connected.
fn commit_closure_from_all_refs(repo: &Repository) -> Result<HashSet<ObjectId>> {
    let mut seeds = Vec::new();

    for prefix in &[
        "refs/heads/",
        "refs/tags/",
        "refs/remotes/",
        "refs/bundles/",
    ] {
        if let Ok(entries) = refs::list_refs(&repo.git_dir, prefix) {
            for (_, oid) in entries {
                seeds.push(oid);
            }
        }
    }

    let mut seen = HashSet::new();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();

    for oid in seeds {
        let obj = match repo.odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        match obj.kind {
            ObjectKind::Commit => {
                if seen.insert(oid) {
                    queue.push_back(oid);
                }
            }
            ObjectKind::Tag => {
                if let Some(target) = peel_tag_to_object(repo, &obj.data)? {
                    if let Ok(tobj) = repo.odb.read(&target) {
                        if tobj.kind == ObjectKind::Commit && seen.insert(target) {
                            queue.push_back(target);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    while let Some(c) = queue.pop_front() {
        let obj = repo.odb.read(&c)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = parse_commit(&obj.data)?;
        for p in commit.parents {
            if seen.insert(p) {
                queue.push_back(p);
            }
        }
    }

    Ok(seen)
}

/// Returns whether every bundle prerequisite commit is already reachable from existing refs.
///
/// Matches Git's `verify_bundle` connectivity check: prerequisite OIDs may exist loose in the ODB
/// but must still lie in the commit closure of `refs/heads/`, `refs/tags/`, `refs/remotes/`, and
/// `refs/bundles/` (so dangling objects do not satisfy prerequisites).
pub fn bundle_prerequisites_connected_to_refs(
    repo: &Repository,
    prerequisites: &[ObjectId],
) -> Result<bool> {
    if prerequisites.is_empty() {
        return Ok(true);
    }
    let closure = commit_closure_from_all_refs(repo)?;
    Ok(prerequisites.iter().all(|p| closure.contains(p)))
}

fn peel_tag_to_object(repo: &Repository, tag_data: &[u8]) -> Result<Option<ObjectId>> {
    let tag = parse_tag(tag_data)?;
    let mut oid = tag.object;
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Tag => {
                let inner = parse_tag(&obj.data)?;
                oid = inner.object;
            }
            _ => return Ok(Some(oid)),
        }
    }
}

fn tree_entry_is_tree(mode: u32) -> bool {
    mode == 0o040000
}

fn tree_entry_is_blob(mode: u32) -> bool {
    matches!(mode, 0o100644 | 0o100755 | 0o120000)
}

/// Walk commits starting from `roots`, optionally stopping parent traversal when a commit is in
/// `parent_stop` (still enqueueing the edge from child: parent is treated as pre-existing).
///
/// Returns `Ok(false)` if any required object is missing from the ODB.
fn walk_commit_graph_for_push(
    repo: &Repository,
    roots: &[ObjectId],
    parent_stop: Option<&HashSet<ObjectId>>,
) -> Result<bool> {
    let mut seen_commits = HashSet::new();
    let mut seen_trees = HashSet::new();
    let mut seen_blobs = HashSet::new();
    let mut commit_q: VecDeque<ObjectId> = VecDeque::new();
    let mut tree_q: VecDeque<ObjectId> = VecDeque::new();

    for &root in roots {
        let tip_obj = match repo.odb.read(&root) {
            Err(_) => return Ok(false),
            Ok(o) => o,
        };
        if tip_obj.kind != ObjectKind::Commit {
            return Err(Error::CorruptObject(format!(
                "object {root} is not a commit"
            )));
        }
        if seen_commits.insert(root) {
            commit_q.push_back(root);
        }
    }

    while let Some(c) = commit_q.pop_front() {
        let obj = match repo.odb.read(&c) {
            Err(_) => return Ok(false),
            Ok(o) => o,
        };
        if obj.kind != ObjectKind::Commit {
            return Err(Error::CorruptObject(format!("object {c} is not a commit")));
        }
        let commit = parse_commit(&obj.data)?;
        for p in &commit.parents {
            if parent_stop.is_some_and(|s| s.contains(p)) {
                continue;
            }
            if seen_commits.insert(*p) {
                commit_q.push_back(*p);
            }
        }
        if seen_trees.insert(commit.tree) {
            tree_q.push_back(commit.tree);
        }
    }

    while let Some(t) = tree_q.pop_front() {
        let obj = match repo.odb.read(&t) {
            Err(_) => return Ok(false),
            Ok(o) => o,
        };
        if obj.kind != ObjectKind::Tree {
            return Err(Error::CorruptObject(format!("object {t} is not a tree")));
        }
        for entry in parse_tree(&obj.data)? {
            if entry.mode == 0o160000 {
                // Git submodule link: the pointed-to commit lives in another object store.
            } else if tree_entry_is_tree(entry.mode) {
                if seen_trees.insert(entry.oid) {
                    tree_q.push_back(entry.oid);
                }
            } else if tree_entry_is_blob(entry.mode) {
                if !seen_blobs.insert(entry.oid) {
                    continue;
                }
                if repo.odb.read(&entry.oid).is_err() {
                    return Ok(false);
                }
            }
        }
    }

    Ok(true)
}

/// Verifies that every object reachable from `tip` exists in `repo.odb`.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] when `tip` is not a commit or tree data is invalid.
pub fn push_tip_objects_exist(repo: &Repository, tip: ObjectId) -> Result<bool> {
    walk_commit_graph_for_push(repo, &[tip], None)
}

/// Like [`push_tip_objects_exist`], but does not require parent commits listed in
/// `parent_exceptions` to exist in the ODB (alternate `.have` semantics).
pub fn push_tip_objects_exist_with_parent_exceptions(
    repo: &Repository,
    tip: ObjectId,
    parent_exceptions: &HashSet<ObjectId>,
) -> Result<bool> {
    walk_commit_graph_for_push(repo, &[tip], Some(parent_exceptions))
}

fn read_object_for_push(
    repo: &Repository,
    pack_objects: Option<&HashMap<ObjectId, Object>>,
    oid: &ObjectId,
) -> Result<Object> {
    if let Some(m) = pack_objects {
        if let Some(o) = m.get(oid) {
            return Ok(o.clone());
        }
    }
    repo.odb.read(oid)
}

/// Objects reachable from `tip` that are not already reachable from existing refs (plus
/// `extra_root_commits`) must exist in the ODB (or in `pack_objects` when verifying before
/// unpacking a push pack).
pub fn push_tip_connected_to_refs(
    repo: &Repository,
    tip: ObjectId,
    extra_root_commits: &HashSet<ObjectId>,
    pack_objects: Option<&HashMap<ObjectId, Object>>,
) -> Result<bool> {
    let mut existing = commit_closure_from_all_refs(repo)?;
    existing.extend(extra_root_commits.iter().copied());

    if existing.contains(&tip) {
        return Ok(true);
    }

    let mut seen_commits = HashSet::new();
    let mut seen_trees = HashSet::new();
    let mut seen_blobs = HashSet::new();
    let mut commit_q: VecDeque<ObjectId> = VecDeque::new();
    let mut tree_q: VecDeque<ObjectId> = VecDeque::new();

    seen_commits.insert(tip);
    commit_q.push_back(tip);

    while let Some(c) = commit_q.pop_front() {
        let obj = match read_object_for_push(repo, pack_objects, &c) {
            Err(_) => return Ok(false),
            Ok(o) => o,
        };
        if obj.kind != ObjectKind::Commit {
            return Err(Error::CorruptObject(format!("object {c} is not a commit")));
        }
        let commit = parse_commit(&obj.data)?;
        for p in &commit.parents {
            if existing.contains(p) {
                continue;
            }
            if seen_commits.insert(*p) {
                commit_q.push_back(*p);
            }
        }
        if seen_trees.insert(commit.tree) {
            tree_q.push_back(commit.tree);
        }
    }

    while let Some(t) = tree_q.pop_front() {
        let obj = match read_object_for_push(repo, pack_objects, &t) {
            Err(_) => return Ok(false),
            Ok(o) => o,
        };
        if obj.kind != ObjectKind::Tree {
            return Err(Error::CorruptObject(format!("object {t} is not a tree")));
        }
        for entry in parse_tree(&obj.data)? {
            if entry.mode == 0o160000 {
                // Git submodule link: the pointed-to commit lives in another object store.
            } else if tree_entry_is_tree(entry.mode) {
                if seen_trees.insert(entry.oid) {
                    tree_q.push_back(entry.oid);
                }
            } else if tree_entry_is_blob(entry.mode) {
                if !seen_blobs.insert(entry.oid) {
                    continue;
                }
                if read_object_for_push(repo, pack_objects, &entry.oid).is_err() {
                    return Ok(false);
                }
            }
        }
    }

    Ok(true)
}

/// If [`push_tip_connected_to_refs`] would fail, returns a missing object OID and a "context"
/// commit OID for Git-compatible `Could not read` / `Failed to traverse parents` stderr.
///
/// `Ok(None)` means connected (same predicate as [`push_tip_connected_to_refs`] returning `Ok(true)`).
pub fn diagnose_push_connectivity_failure(
    repo: &Repository,
    tip: ObjectId,
    extra_root_commits: &HashSet<ObjectId>,
    pack_objects: Option<&HashMap<ObjectId, Object>>,
) -> Result<Option<(ObjectId, ObjectId)>> {
    let mut existing = commit_closure_from_all_refs(repo)?;
    existing.extend(extra_root_commits.iter().copied());

    if existing.contains(&tip) {
        return Ok(None);
    }

    let mut seen_commits = HashSet::new();
    let mut seen_trees = HashSet::new();
    let mut seen_blobs = HashSet::new();
    let mut commit_q: VecDeque<ObjectId> = VecDeque::new();
    let mut tree_q: VecDeque<ObjectId> = VecDeque::new();

    seen_commits.insert(tip);
    commit_q.push_back(tip);

    while let Some(c) = commit_q.pop_front() {
        let obj = match read_object_for_push(repo, pack_objects, &c) {
            Err(_) => return Ok(Some((c, tip))),
            Ok(o) => o,
        };
        if obj.kind != ObjectKind::Commit {
            return Err(Error::CorruptObject(format!("object {c} is not a commit")));
        }
        let commit = parse_commit(&obj.data)?;
        for p in &commit.parents {
            if existing.contains(p) {
                continue;
            }
            if read_object_for_push(repo, pack_objects, p).is_err() {
                return Ok(Some((*p, c)));
            }
            if seen_commits.insert(*p) {
                commit_q.push_back(*p);
            }
        }
        if seen_trees.insert(commit.tree) {
            tree_q.push_back(commit.tree);
        }
    }

    while let Some(t) = tree_q.pop_front() {
        let obj = match read_object_for_push(repo, pack_objects, &t) {
            Err(_) => return Ok(Some((t, tip))),
            Ok(o) => o,
        };
        if obj.kind != ObjectKind::Tree {
            return Err(Error::CorruptObject(format!("object {t} is not a tree")));
        }
        for entry in parse_tree(&obj.data)? {
            if entry.mode == 0o160000 {
                continue;
            } else if tree_entry_is_tree(entry.mode) {
                if seen_trees.insert(entry.oid) {
                    tree_q.push_back(entry.oid);
                }
            } else if tree_entry_is_blob(entry.mode) {
                if !seen_blobs.insert(entry.oid) {
                    continue;
                }
                if read_object_for_push(repo, pack_objects, &entry.oid).is_err() {
                    return Ok(Some((entry.oid, tip)));
                }
            }
        }
    }

    Ok(None)
}
