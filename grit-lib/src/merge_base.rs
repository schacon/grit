//! Merge-base and reachability primitives.
//!
//! This module implements the subset needed by `grit merge-base`:
//! default merge-base selection, `--all`, `--octopus`, `--independent`,
//! and `--is-ancestor`.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::config::ConfigSet;
use crate::error::{Error, Result};
use crate::objects::{parse_commit, ObjectId, ObjectKind};
use crate::promisor::{promisor_pack_object_ids, repo_treats_promisor_packs};
use crate::reflog::read_reflog;
use crate::repo::Repository;
use crate::rev_parse::{peel_to_commit_for_merge_base, resolve_revision};

/// Resolve commit-ish command arguments to commit object IDs.
///
/// # Parameters
///
/// - `repo` - repository used for revision lookup and object reads.
/// - `specs` - revision arguments such as `HEAD`, ref names, or object IDs.
///
/// # Errors
///
/// Returns [`Error::ObjectNotFound`] when a revision does not resolve and
/// [`Error::CorruptObject`] when the resolved object is not a commit.
pub fn resolve_commit_specs(repo: &Repository, specs: &[String]) -> Result<Vec<ObjectId>> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let oid = resolve_revision(repo, spec)?;
        ensure_is_commit(repo, oid)?;
        out.push(oid);
    }
    Ok(out)
}

/// Compute merge bases for one commit vs one or more others.
///
/// Semantics match Git's default mode: for `<a> <b>...`, this computes merge
/// bases between `a` and a hypothetical merge of all remaining commits.
///
/// # Parameters
///
/// - `repo` - repository used to walk commit parents.
/// - `first` - first commit argument.
/// - `others` - remaining commit arguments.
///
/// # Errors
///
/// Returns parse and object read errors from commit traversal.
pub fn merge_bases_first_vs_rest(
    repo: &Repository,
    first: ObjectId,
    others: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let mut cache = CommitGraphCache::new(repo);
    let first_anc = cache.ancestor_closure(first)?;
    let mut others_union = HashSet::new();
    for &other in others {
        others_union.extend(cache.ancestor_closure(other)?);
    }
    let candidates: HashSet<ObjectId> = first_anc.intersection(&others_union).copied().collect();
    reduce_to_best(candidates, &mut cache)
}

/// Compute merge bases common to all supplied commits (`--octopus` mode).
///
/// # Parameters
///
/// - `repo` - repository used to walk commit parents.
/// - `commits` - commits to intersect.
///
/// # Errors
///
/// Returns parse and object read errors from commit traversal.
pub fn merge_bases_octopus(repo: &Repository, commits: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let mut cache = CommitGraphCache::new(repo);
    let mut iter = commits.iter();
    let Some(&first) = iter.next() else {
        return Ok(Vec::new());
    };
    let mut common = cache.ancestor_closure(first)?;
    for &oid in iter {
        let set = cache.ancestor_closure(oid)?;
        common.retain(|item| set.contains(item));
    }
    reduce_to_best(common, &mut cache)
}

/// Check whether `ancestor` is reachable from `descendant`.
///
/// # Errors
///
/// Returns parse and object read errors from commit traversal.
pub fn is_ancestor(repo: &Repository, ancestor: ObjectId, descendant: ObjectId) -> Result<bool> {
    let mut cache = CommitGraphCache::new(repo);
    if ancestor == descendant {
        return Ok(true);
    }
    Ok(cache.ancestor_closure(descendant)?.contains(&ancestor))
}

/// Reflog file path to scan for `git merge-base --fork-point`-style selection.
///
/// Mirrors the resolution used by Git's merge-base and rebase commands.
pub fn fork_point_reflog_ref(git_dir: &Path, spec: &str) -> String {
    if spec == "HEAD" || spec.starts_with("refs/") {
        return spec.to_string();
    }

    let logs_dir = git_dir.join("logs");
    let candidates = [
        spec.to_string(),
        format!("refs/heads/{spec}"),
        format!("refs/remotes/{spec}"),
    ];

    for candidate in candidates {
        if logs_dir.join(&candidate).is_file() {
            return candidate;
        }
    }

    format!("refs/heads/{spec}")
}

fn select_fork_point_from_reflog_candidates(
    repo: &Repository,
    candidates: &[ObjectId],
) -> Result<Option<ObjectId>> {
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut best = HashSet::new();
    for &candidate in candidates {
        let mut dominated = false;
        for &other in candidates {
            if candidate == other {
                continue;
            }
            if is_ancestor(repo, candidate, other)? {
                dominated = true;
                break;
            }
        }
        if !dominated {
            best.insert(candidate);
        }
    }

    Ok(candidates.iter().copied().find(|oid| best.contains(oid)))
}

/// Compute the fork-point commit between `upstream` and `head`, or `None` when the
/// reflog heuristic finds nothing (caller should fall back to the ordinary merge base).
///
/// This matches `git merge-base --fork-point <upstream-spec> <head>`: walk the upstream
/// ref's reflog (newest first), collect OIDs that are ancestors of `head`, then pick the
/// best candidate; if that set is empty, return `None`.
pub fn merge_base_fork_point(
    repo: &Repository,
    git_dir: &Path,
    upstream_spec: &str,
    head_oid: ObjectId,
) -> Result<Option<ObjectId>> {
    let reflog_ref = fork_point_reflog_ref(git_dir, upstream_spec);
    let entries = read_reflog(git_dir, &reflog_ref)?;
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for entry in entries.iter().rev() {
        let oid = if entry.message.starts_with("checkout:") {
            entry.old_oid
        } else {
            entry.new_oid
        };
        if !seen.insert(oid) {
            continue;
        }
        if is_ancestor(repo, oid, head_oid)? {
            candidates.push(oid);
        }
    }

    select_fork_point_from_reflog_candidates(repo, &candidates)
}

/// Returns every commit reachable from `tip` by walking parent links (including `tip`).
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] if an encountered object is not a commit.
pub fn ancestor_closure(repo: &Repository, tip: ObjectId) -> Result<HashSet<ObjectId>> {
    let mut cache = CommitGraphCache::new(repo);
    cache.ancestor_closure(tip)
}

/// Count symmetric-diff commits between two tips, matching `git rev-list --left-right A...B`.
///
/// Returns `(ahead, behind)` where `ahead` counts commits reachable from `local` but not from
/// `other`, and `behind` the converse. Shared history is excluded from both counts.
///
/// # Errors
///
/// Propagates errors from commit graph walks.
pub fn count_symmetric_ahead_behind(
    repo: &Repository,
    local: ObjectId,
    other: ObjectId,
) -> Result<(usize, usize)> {
    let left = ancestor_closure(repo, local)?;
    let right = ancestor_closure(repo, other)?;
    let ahead = left.difference(&right).count();
    let behind = right.difference(&left).count();
    Ok((ahead, behind))
}

/// Return commits that are not reachable from any other input commit.
///
/// The output order follows input order, dropping any commit reachable from
/// another supplied commit.
///
/// # Errors
///
/// Returns parse and object read errors from commit traversal.
pub fn independent_commits(repo: &Repository, commits: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let mut cache = CommitGraphCache::new(repo);
    let mut out = Vec::new();
    for (i, &candidate) in commits.iter().enumerate() {
        let mut reachable = false;
        for (j, &other) in commits.iter().enumerate() {
            if i == j {
                continue;
            }
            if cache.ancestor_closure(other)?.contains(&candidate) {
                reachable = true;
                break;
            }
        }
        if !reachable {
            out.push(candidate);
        }
    }
    Ok(out)
}

fn ensure_is_commit(repo: &Repository, oid: ObjectId) -> Result<()> {
    let object = repo.odb.read(&oid)?;
    if object.kind != ObjectKind::Commit {
        return Err(Error::CorruptObject(format!(
            "object {oid} is not a commit"
        )));
    }
    Ok(())
}

fn reduce_to_best(
    candidates: HashSet<ObjectId>,
    cache: &mut CommitGraphCache<'_>,
) -> Result<Vec<ObjectId>> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut best = BTreeSet::new();
    for &candidate in &candidates {
        let mut better_found = false;
        for &other in &candidates {
            if candidate == other {
                continue;
            }
            if cache.ancestor_closure(other)?.contains(&candidate) {
                better_found = true;
                break;
            }
        }
        if !better_found {
            best.insert(candidate);
        }
    }
    Ok(best.into_iter().collect())
}

struct CommitGraphCache<'r> {
    repo: &'r Repository,
    parents: HashMap<ObjectId, Vec<ObjectId>>,
    closures: HashMap<ObjectId, HashSet<ObjectId>>,
    promisor_stop: std::collections::HashSet<ObjectId>,
}

impl<'r> CommitGraphCache<'r> {
    fn new(repo: &'r Repository) -> Self {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let promisor_stop = if repo_treats_promisor_packs(&repo.git_dir, &cfg) {
            promisor_pack_object_ids(&repo.git_dir.join("objects"))
        } else {
            HashSet::new()
        };
        Self {
            repo,
            parents: HashMap::new(),
            closures: HashMap::new(),
            promisor_stop,
        }
    }

    fn ancestor_closure(&mut self, start: ObjectId) -> Result<HashSet<ObjectId>> {
        if let Some(existing) = self.closures.get(&start) {
            return Ok(existing.clone());
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        while let Some(oid) = queue.pop_front() {
            if !visited.insert(oid) {
                continue;
            }
            for parent in self.parents_of(oid)? {
                queue.push_back(parent);
            }
        }
        self.closures.insert(start, visited.clone());
        Ok(visited)
    }

    fn parents_of(&mut self, oid: ObjectId) -> Result<Vec<ObjectId>> {
        if let Some(parents) = self.parents.get(&oid) {
            return Ok(parents.clone());
        }
        let commit_oid = peel_to_commit_for_merge_base(self.repo, oid).map_err(|e| match e {
            Error::InvalidRef(msg) => Error::CorruptObject(msg),
            other => other,
        })?;
        let object = match self.repo.odb.read(&commit_oid) {
            Ok(o) => o,
            Err(Error::ObjectNotFound(_)) => {
                self.parents.insert(oid, Vec::new());
                return Ok(Vec::new());
            }
            Err(e) => return Err(e),
        };
        if object.kind != ObjectKind::Commit {
            return Err(Error::CorruptObject(format!(
                "object {commit_oid} is not a commit"
            )));
        }
        let commit = parse_commit(&object.data)?;
        let parents: Vec<ObjectId> = commit
            .parents
            .iter()
            .copied()
            .filter(|p| !self.promisor_stop.contains(p))
            .collect();
        self.parents.insert(oid, parents.clone());
        Ok(parents)
    }
}
