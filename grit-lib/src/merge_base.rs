//! Merge-base and reachability primitives.
//!
//! This module implements the subset needed by `grit merge-base`:
//! default merge-base selection, `--all`, `--octopus`, `--independent`,
//! and `--is-ancestor`.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::error::{Error, Result};
use crate::objects::{parse_commit, ObjectId, ObjectKind};
use crate::reflog::read_reflog;
use crate::repo::Repository;
use crate::rev_parse::{
    peel_to_commit_for_merge_base, resolve_revision, resolve_upstream_symbolic_name,
    upstream_suffix_info,
};

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

/// Returns the ref path under `logs/` used for fork-point reflog scanning for `merge-base --fork-point`
/// and `rebase --fork-point`, matching Git's resolution order.
///
/// # Parameters
///
/// - `spec` - upstream argument as given on the command line (`main`, `refs/heads/main`, `HEAD`, …).
pub fn resolve_fork_point_reflog_ref(repo: &Repository, spec: &str) -> String {
    if spec == "HEAD" || spec.starts_with("refs/") {
        return spec.to_string();
    }

    let logs_dir = repo.git_dir.join("logs");
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

/// Picks the fork-point candidate that is not strictly dominated by another candidate in the list.
fn select_best_fork_point(repo: &Repository, candidates: &[ObjectId]) -> Result<Option<ObjectId>> {
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

/// Computes the fork-point commit between `upstream_tip` and `head`, using the upstream ref's reflog.
///
/// This matches `git merge-base --fork-point` / the merge base `git rebase --fork-point` uses for
/// selecting commits to replay.
///
/// # Parameters
///
/// - `upstream_spec` - upstream revision string (used to locate the reflog; e.g. `main`,
///   `refs/heads/main`, or `topic@{{upstream}}`).
/// - `upstream_tip` - resolved commit of the upstream branch tip.
/// - `head` - commit to rebase (usually `HEAD`).
///
/// # Errors
///
/// Propagates object read, reflog, and graph walk errors.
pub fn fork_point(
    repo: &Repository,
    upstream_spec: &str,
    upstream_tip: ObjectId,
    head: ObjectId,
) -> Result<ObjectId> {
    let reflog_ref = if upstream_suffix_info(upstream_spec).is_some() {
        resolve_upstream_symbolic_name(repo, upstream_spec)?
    } else {
        resolve_fork_point_reflog_ref(repo, upstream_spec)
    };

    let entries = read_reflog(&repo.git_dir, &reflog_ref)
        .map_err(|e| Error::Message(format!("failed to read reflog for '{reflog_ref}': {e}")))?;

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
        if is_ancestor(repo, oid, head)? {
            candidates.push(oid);
        }
    }

    if let Some(fp) = select_best_fork_point(repo, &candidates)? {
        return Ok(fp);
    }

    let mut bases = merge_bases_first_vs_rest(repo, upstream_tip, &[head])?;
    if bases.is_empty() {
        return Err(Error::Message(
            "no merge base found between upstream and HEAD".to_owned(),
        ));
    }
    bases.sort();
    Ok(bases[0])
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
}

impl<'r> CommitGraphCache<'r> {
    fn new(repo: &'r Repository) -> Self {
        Self {
            repo,
            parents: HashMap::new(),
            closures: HashMap::new(),
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
        let object = self.repo.odb.read(&commit_oid)?;
        if object.kind != ObjectKind::Commit {
            return Err(Error::CorruptObject(format!(
                "object {commit_oid} is not a commit"
            )));
        }
        let commit = parse_commit(&object.data)?;
        self.parents.insert(oid, commit.parents.clone());
        Ok(commit.parents)
    }
}
