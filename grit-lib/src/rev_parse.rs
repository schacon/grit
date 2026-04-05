//! Revision parsing and repository discovery helpers for `rev-parse`.
//!
//! This module implements a focused subset of Git's revision parser used by
//! `grit rev-parse` in v2 scope: repository/work-tree discovery flags, basic
//! object-name resolution, and lightweight peeling (`^{}`, `^{object}`,
//! `^{commit}`).

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path};

use crate::error::{Error, Result};
use crate::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use crate::reflog::read_reflog;
use crate::refs;
use crate::repo::Repository;

/// Return `Some(repo)` when a repository can be discovered at `start`.
///
/// # Parameters
///
/// - `start` - starting path for discovery; when `None`, uses current directory.
///
/// # Errors
///
/// Returns errors other than "not a repository" (for example I/O and path
/// canonicalization failures).
pub fn discover_optional(start: Option<&Path>) -> Result<Option<Repository>> {
    match Repository::discover(start) {
        Ok(repo) => Ok(Some(repo)),
        Err(Error::NotARepository(msg))
            if msg.contains("not a regular file") || msg.contains("invalid gitfile format") =>
        {
            Err(Error::NotARepository(msg))
        }
        Err(Error::NotARepository(_)) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Compute whether `cwd` is inside the repository's work tree.
#[must_use]
pub fn is_inside_work_tree(repo: &Repository, cwd: &Path) -> bool {
    let Some(work_tree) = &repo.work_tree else {
        return false;
    };
    path_is_within(cwd, work_tree)
}

/// Compute whether `cwd` is inside the repository's git-dir.
#[must_use]
pub fn is_inside_git_dir(repo: &Repository, cwd: &Path) -> bool {
    path_is_within(cwd, &repo.git_dir)
}

/// Compute the `--show-prefix` output.
///
/// Returns an empty string when `cwd` is at repository root or outside the work
/// tree. Returned prefixes always use `/` separators and end with `/`.
#[must_use]
pub fn show_prefix(repo: &Repository, cwd: &Path) -> String {
    let Some(work_tree) = &repo.work_tree else {
        return String::new();
    };
    if !path_is_within(cwd, work_tree) {
        return String::new();
    }
    if cwd == work_tree {
        return String::new();
    }
    let Ok(rel) = cwd.strip_prefix(work_tree) else {
        return String::new();
    };
    let mut out = rel
        .components()
        .filter_map(component_to_text)
        .collect::<Vec<_>>()
        .join("/");
    if !out.is_empty() {
        out.push('/');
    }
    out
}

/// Resolve a symbolic ref name to its full form.
///
/// For `HEAD`, returns the symbolic target (e.g., `refs/heads/main`).
/// For branch names, returns `refs/heads/<name>`.
/// For tag names, returns `refs/tags/<name>`.
/// Returns `None` when the name cannot be resolved symbolically.
#[must_use]
pub fn symbolic_full_name(repo: &Repository, spec: &str) -> Option<String> {
    // Handle @{upstream} and @{push} suffixes
    if let Some(base) = spec
        .strip_suffix("@{upstream}")
        .or_else(|| spec.strip_suffix("@{u}"))
        .or_else(|| spec.strip_suffix("@{UPSTREAM}"))
        .or_else(|| spec.strip_suffix("@{U}"))
        .or_else(|| spec.strip_suffix("@{UpSTReam}"))
    {
        return resolve_upstream_ref(repo, base);
    }
    if let Some(base) = spec.strip_suffix("@{push}") {
        return resolve_push_ref(repo, base);
    }

    // Handle @{-N} syntax: return symbolic ref of Nth previously checked out branch
    if spec.starts_with("@{-") && spec.ends_with('}') {
        let inner = &spec[3..spec.len() - 1];
        if let Ok(n) = inner.parse::<usize>() {
            if n >= 1 {
                let entries = read_reflog(&repo.git_dir, "HEAD").ok()?;
                let mut count = 0usize;
                for entry in entries.iter().rev() {
                    let msg = &entry.message;
                    if let Some(rest) = msg.strip_prefix("checkout: moving from ") {
                        count += 1;
                        if count == n {
                            if let Some(to_pos) = rest.find(" to ") {
                                let from_branch = &rest[..to_pos];
                                let ref_name = format!("refs/heads/{from_branch}");
                                if refs::resolve_ref(&repo.git_dir, &ref_name).is_ok() {
                                    return Some(ref_name);
                                }
                            }
                        }
                    }
                }
            }
        }
        return None;
    }

    if spec == "HEAD" {
        if let Ok(Some(target)) = refs::read_symbolic_ref(&repo.git_dir, "HEAD") {
            return Some(target);
        }
        return None;
    }
    // If it's already a full ref path
    if spec.starts_with("refs/") {
        if refs::resolve_ref(&repo.git_dir, spec).is_ok() {
            return Some(spec.to_owned());
        }
        return None;
    }
    // DWIM: try refs/heads, refs/tags, refs/remotes
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        let candidate = format!("{prefix}{spec}");
        if refs::resolve_ref(&repo.git_dir, &candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

/// Abbreviate a full ref name to its shortest unambiguous form.
///
/// For example, `refs/heads/main` becomes `main`.
#[must_use]
pub fn abbreviate_ref_name(full_name: &str) -> String {
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Some(short) = full_name.strip_prefix(prefix) {
            return short.to_owned();
        }
    }
    if let Some(short) = full_name.strip_prefix("refs/") {
        return short.to_owned();
    }
    full_name.to_owned()
}

/// Resolve `@{upstream}` for a given branch.
fn resolve_upstream_ref(repo: &Repository, branch: &str) -> Option<String> {
    // If branch is empty, use current branch from HEAD
    let branch_name = if branch.is_empty() {
        match refs::read_head(&repo.git_dir) {
            Ok(Some(target)) => target.strip_prefix("refs/heads/")?.to_owned(),
            _ => return None,
        }
    } else {
        // Handle @ prefix (e.g., @funny) and branch names with @
        branch.to_owned()
    };

    // Read branch.<name>.remote and branch.<name>.merge from config
    let config_path = repo.git_dir.join("config");
    let config_content = fs::read_to_string(&config_path).ok()?;
    let (remote, merge) = parse_branch_tracking(&config_content, &branch_name)?;

    // For local tracking (remote = "."), use the merge ref directly.
    // For remote tracking, convert to refs/remotes/<remote>/<branch>.
    if remote == "." {
        Some(merge.clone())
    } else {
        let merge_branch = merge.strip_prefix("refs/heads/")?;
        Some(format!("refs/remotes/{remote}/{merge_branch}"))
    }
}

/// Resolve `@{push}` for a given branch.
fn resolve_push_ref(repo: &Repository, branch: &str) -> Option<String> {
    // @{push} is typically the same as @{upstream} unless push remote differs
    // For simplicity, treat it the same way
    resolve_upstream_ref(repo, branch)
}

/// Parse branch tracking configuration from git config content.
fn parse_branch_tracking(config: &str, branch: &str) -> Option<(String, String)> {
    let mut remote = None;
    let mut merge = None;
    let mut in_section = false;
    let target_section = format!("[branch \"{}\"]", branch);

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == target_section
                || trimmed.starts_with(&format!("[branch \"{}\"", branch));
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("remote = ") {
            remote = Some(value.trim().to_owned());
        } else if let Some(value) = trimmed.strip_prefix("merge = ") {
            merge = Some(value.trim().to_owned());
        }
        // Also handle with tabs
        if let Some(value) = trimmed.strip_prefix("remote=") {
            remote = Some(value.trim().to_owned());
        } else if let Some(value) = trimmed.strip_prefix("merge=") {
            merge = Some(value.trim().to_owned());
        }
    }

    match (remote, merge) {
        (Some(r), Some(m)) => Some((r, m)),
        _ => None,
    }
}

/// Resolve a revision string to an object ID.
///
/// Supports:
/// - full 40-hex object IDs (must exist in loose store),
/// - abbreviated object IDs (length 4-39, must resolve uniquely),
/// - direct refs (`HEAD`, `refs/...`),
/// - DWIM branch/tag/remote names (`name` -> `refs/heads/name`, etc.),
/// - peeling suffixes: `^{}`, `^{object}`, `^{commit}`.
///
/// # Errors
///
/// Returns [`Error::ObjectNotFound`] or [`Error::InvalidRef`] when resolution
/// fails.
pub fn resolve_revision(repo: &Repository, spec: &str) -> Result<ObjectId> {
    // Handle `:/message` early — it can contain any characters so must
    // not be confused with peel/nav syntax.
    if let Some(pattern) = spec.strip_prefix(":/") {
        if !pattern.is_empty() {
            return resolve_commit_message_search(repo, pattern);
        }
    }

    // Handle A...B (symmetric difference / merge-base)
    // Also handles A... (implies A...HEAD)
    if let Some(idx) = spec.find("...") {
        let left_raw = &spec[..idx];
        let right_raw = &spec[idx + 3..];
        if !left_raw.is_empty() || !right_raw.is_empty() {
            let left_oid = if left_raw.is_empty() {
                resolve_revision(repo, "HEAD")?
            } else {
                resolve_revision(repo, left_raw)?
            };
            let right_oid = if right_raw.is_empty() {
                resolve_revision(repo, "HEAD")?
            } else {
                resolve_revision(repo, right_raw)?
            };
            let bases = crate::merge_base::merge_bases_first_vs_rest(repo, left_oid, &[right_oid])?;
            return bases
                .into_iter()
                .next()
                .ok_or_else(|| Error::ObjectNotFound(format!("no merge base for '{spec}'")));
        }
    }

    // Handle <rev>:<path> — resolve a tree entry.
    // Must come after :/ handling. Look for a single colon that isn't doubled,
    // and isn't part of ^{ or :/. The colon separates the revision from the path.
    if let Some(colon_idx) = spec.find(':') {
        // Exclude :/ (commit search) and :N:path (stage number)
        let before = &spec[..colon_idx];
        let after = &spec[colon_idx + 1..];
        if !before.is_empty() && !spec.starts_with(":/") {
            // <rev>:<path> — resolve rev to tree, then navigate path
            let rev_oid = resolve_revision(repo, before)?;
            let tree_oid = peel_to_tree(repo, rev_oid)?;
            if after.is_empty() {
                // <rev>: means the tree itself
                return Ok(tree_oid);
            }
            // Navigate into the tree by path
            return resolve_tree_path(repo, &tree_oid, after);
        }
    }

    let (base_with_nav, peel) = parse_peel_suffix(spec);
    let (base, nav_steps) = parse_nav_steps(base_with_nav);
    let mut oid = resolve_base(repo, base)?;
    for step in nav_steps {
        oid = apply_nav_step(repo, oid, step)?;
    }
    apply_peel(repo, oid, peel)
}

/// Peel an object to a tree (commit → tree, tree → tree).
fn peel_to_tree(repo: &Repository, oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&oid)?;
    match obj.kind {
        crate::objects::ObjectKind::Tree => Ok(oid),
        crate::objects::ObjectKind::Commit => {
            let commit = crate::objects::parse_commit(&obj.data)?;
            Ok(commit.tree)
        }
        crate::objects::ObjectKind::Tag => {
            let tag = crate::objects::parse_tag(&obj.data)?;
            peel_to_tree(repo, tag.object)
        }
        _ => Err(Error::ObjectNotFound(format!(
            "cannot peel {} to tree",
            oid
        ))),
    }
}

/// Navigate a tree to find an object at a given path.
fn resolve_tree_path(repo: &Repository, tree_oid: &ObjectId, path: &str) -> Result<ObjectId> {
    let obj = repo.odb.read(tree_oid)?;
    let entries = crate::objects::parse_tree(&obj.data)?;
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.is_empty() {
        return Ok(*tree_oid);
    }
    let first = components[0];
    let rest: Vec<&str> = components[1..].to_vec();
    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        if name == first {
            if rest.is_empty() {
                return Ok(entry.oid);
            } else {
                return resolve_tree_path(repo, &entry.oid, &rest.join("/"));
            }
        }
    }
    Err(Error::ObjectNotFound(format!(
        "path '{}' not found in tree {}",
        path, tree_oid
    )))
}

/// A single parent/ancestor navigation step.
#[derive(Debug, Clone, Copy)]
enum NavStep {
    /// `^N` — navigate to the Nth parent (1-indexed; 0 is a no-op).
    ParentN(usize),
    /// `~N` — follow the first parent N times.
    AncestorN(usize),
}

/// Parse and strip any trailing `^N` / `~N` navigation steps from `spec`.
///
/// Returns `(base, steps)` where `steps` are in left-to-right application order.
fn parse_nav_steps(spec: &str) -> (&str, Vec<NavStep>) {
    let mut steps = Vec::new();
    let mut remaining = spec;

    loop {
        // Try `~<digits>` or bare `~` at the end.
        if let Some(tilde_pos) = remaining.rfind('~') {
            let after = &remaining[tilde_pos + 1..];
            if after.is_empty() {
                // bare `~` = `~1`
                steps.push(NavStep::AncestorN(1));
                remaining = &remaining[..tilde_pos];
                continue;
            }
            if after.bytes().all(|b| b.is_ascii_digit()) {
                let n: usize = after.parse().unwrap_or(1);
                steps.push(NavStep::AncestorN(n));
                remaining = &remaining[..tilde_pos];
                continue;
            }
        }

        // Try `^<single-digit>` or bare `^` at the end (but not `^{...}`).
        if let Some(caret_pos) = remaining.rfind('^') {
            let after = &remaining[caret_pos + 1..];
            if after.is_empty() {
                // bare `^` = `^1`
                steps.push(NavStep::ParentN(1));
                remaining = &remaining[..caret_pos];
                continue;
            }
            if after.len() == 1 && after.as_bytes()[0].is_ascii_digit() {
                let n = (after.as_bytes()[0] - b'0') as usize;
                steps.push(NavStep::ParentN(n));
                remaining = &remaining[..caret_pos];
                continue;
            }
        }

        break;
    }

    steps.reverse();
    (remaining, steps)
}

/// Apply a single navigation step to an OID, resolving parent/ancestor links.
fn apply_nav_step(repo: &Repository, oid: ObjectId, step: NavStep) -> Result<ObjectId> {
    match step {
        NavStep::ParentN(0) => Ok(oid),
        NavStep::ParentN(n) => {
            let obj = repo.odb.read(&oid)?;
            if obj.kind != ObjectKind::Commit {
                return Err(Error::InvalidRef(format!("{oid} is not a commit")));
            }
            let commit = parse_commit(&obj.data)?;
            commit
                .parents
                .get(n - 1)
                .copied()
                .ok_or_else(|| Error::ObjectNotFound(format!("{oid}^{n}")))
        }
        NavStep::AncestorN(n) => {
            let mut current = oid;
            for _ in 0..n {
                current = apply_nav_step(repo, current, NavStep::ParentN(1))?;
            }
            Ok(current)
        }
    }
}

/// Abbreviate an object ID to a unique prefix.
///
/// The returned prefix is at least `min_len` and at most 40 hex characters.
///
/// # Errors
///
/// Returns [`Error::ObjectNotFound`] when the target OID does not exist in the
/// object database.
pub fn abbreviate_object_id(repo: &Repository, oid: ObjectId, min_len: usize) -> Result<String> {
    let min_len = min_len.clamp(4, 40);
    let target = oid.to_hex();

    // If object doesn't exist, just return the minimum abbreviation
    if !repo.odb.exists(&oid) {
        return Ok(target[..min_len].to_owned());
    }

    let all = collect_loose_object_ids(repo)?;

    for len in min_len..=40 {
        let prefix = &target[..len];
        let matches = all
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .count();
        if matches <= 1 {
            return Ok(prefix.to_owned());
        }
    }

    Ok(target)
}

/// Render `path` relative to `cwd` with `/` separators.
#[must_use]
pub fn to_relative_path(path: &Path, cwd: &Path) -> String {
    let path_components = normalize_components(path);
    let cwd_components = normalize_components(cwd);

    let mut common = 0usize;
    let max_common = path_components.len().min(cwd_components.len());
    while common < max_common && path_components[common] == cwd_components[common] {
        common += 1;
    }

    let mut parts = Vec::new();
    let up_count = cwd_components.len().saturating_sub(common);
    for _ in 0..up_count {
        parts.push("..".to_owned());
    }
    for item in path_components.iter().skip(common) {
        parts.push(item.clone());
    }

    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

fn resolve_base(repo: &Repository, spec: &str) -> Result<ObjectId> {
    // Handle @{upstream} / @{u} / @{push} suffixes
    if let Some(full_ref) = try_resolve_at_suffix(repo, spec) {
        return refs::resolve_ref(&repo.git_dir, &full_ref)
            .map_err(|_| Error::ObjectNotFound(spec.to_owned()));
    }

    // Handle @{-N} syntax: Nth previously checked out branch
    // Also handle @{-N}@{M} compound form: resolve branch, then reflog
    if spec.starts_with("@{-") {
        // Find the closing } for the @{-N} part
        if let Some(close) = spec[3..].find('}') {
            let n_str = &spec[3..3 + close];
            if let Ok(n) = n_str.parse::<usize>() {
                if n >= 1 {
                    let suffix = &spec[3 + close + 1..]; // after the first }
                    if suffix.is_empty() {
                        // Plain @{-N}
                        if let Some(oid) = try_resolve_at_minus(repo, spec)? {
                            return Ok(oid);
                        }
                    } else {
                        // @{-N}@{M} or @{-N}@{...} compound form
                        // Resolve @{-N} to branch name, then re-resolve as branch+suffix
                        let branch = resolve_at_minus_to_branch(repo, n)?;
                        let new_spec = format!("{branch}{suffix}");
                        return resolve_base(repo, &new_spec);
                    }
                }
            }
        }
    }

    // Handle @{N} reflog syntax: ref@{N} or @{N} (meaning HEAD@{N})
    if let Some(oid) = try_resolve_reflog_index(repo, spec)? {
        return Ok(oid);
    }

    // Handle `:/pattern` — search commit messages from HEAD
    if let Some(pattern) = spec.strip_prefix(":/") {
        if !pattern.is_empty() {
            return resolve_commit_message_search(repo, pattern);
        }
    }

    // Handle `:N:path` — look up path in the index at stage N
    // Also handle `:path` — look up path in the index (stage 0)
    if let Some(rest) = spec.strip_prefix(':') {
        if !rest.is_empty() && !rest.starts_with('/') {
            // Check for :N:path pattern (N is a single digit 0-3)
            if rest.len() >= 3 && rest.as_bytes()[1] == b':' {
                if let Some(stage_char) = rest.chars().next() {
                    if let Some(stage) = stage_char.to_digit(10) {
                        if stage <= 3 {
                            let path = &rest[2..];
                            return resolve_index_path_at_stage(repo, path, stage as u8);
                        }
                    }
                }
            }
            return resolve_index_path(repo, rest);
        }
    }

    if let Some((treeish, path)) = split_treeish_spec(spec) {
        let root_oid = resolve_revision(repo, treeish)?;
        return resolve_treeish_path(repo, root_oid, path);
    }

    if let Ok(oid) = spec.parse::<ObjectId>() {
        // A full 40-hex OID is always accepted, even if the object
        // doesn't exist in the ODB (matches git behavior).
        return Ok(oid);
    }

    if is_hex_prefix(spec) {
        let matches = find_abbrev_matches(repo, spec)?;
        if matches.len() == 1 {
            return Ok(matches[0]);
        }
        if matches.len() > 1 {
            return Err(Error::InvalidRef(format!(
                "short object ID {} is ambiguous",
                spec
            )));
        }
    }

    if let Ok(oid) = refs::resolve_ref(&repo.git_dir, spec) {
        return Ok(oid);
    }
    for candidate in &[
        format!("refs/heads/{spec}"),
        format!("refs/tags/{spec}"),
        format!("refs/remotes/{spec}"),
    ] {
        if let Ok(oid) = refs::resolve_ref(&repo.git_dir, candidate) {
            return Ok(oid);
        }
    }

    // As a last resort, try resolving as HEAD:<spec> (index path lookup)
    // This allows `git rev-parse b` to resolve to the blob for file `b`
    // in the current HEAD tree, matching Git's behavior for path arguments.
    if !spec.contains(':') && !spec.starts_with('-') {
        if let Ok(oid) = resolve_index_path(repo, spec) {
            return Ok(oid);
        }
    }

    Err(Error::ObjectNotFound(spec.to_owned()))
}

/// Resolve `@{-N}` to the branch name (e.g. "side"), not to an OID.
fn resolve_at_minus_to_branch(repo: &Repository, n: usize) -> Result<String> {
    let entries = read_reflog(&repo.git_dir, "HEAD")?;
    let mut count = 0usize;
    for entry in entries.iter().rev() {
        let msg = &entry.message;
        if let Some(rest) = msg.strip_prefix("checkout: moving from ") {
            count += 1;
            if count == n {
                if let Some(to_pos) = rest.find(" to ") {
                    return Ok(rest[..to_pos].to_string());
                }
            }
        }
    }
    Err(Error::InvalidRef(format!(
        "@{{-{n}}}: only {count} checkout(s) in reflog"
    )))
}

/// Try to resolve `@{-N}` syntax — the Nth previously checked out branch.
/// Returns the resolved OID if matching, or None if not matching.
fn try_resolve_at_minus(repo: &Repository, spec: &str) -> Result<Option<ObjectId>> {
    // Match @{-N} only (no ref prefix)
    if !spec.starts_with("@{-") || !spec.ends_with('}') {
        return Ok(None);
    }
    let inner = &spec[3..spec.len() - 1];
    let n: usize = match inner.parse() {
        Ok(n) if n >= 1 => n,
        _ => return Ok(None),
    };
    // Read HEAD reflog and find the Nth "checkout: moving from X to Y" entry
    let entries = read_reflog(&repo.git_dir, "HEAD")?;
    let mut count = 0usize;
    // Iterate newest-first
    for entry in entries.iter().rev() {
        let msg = &entry.message;
        if let Some(rest) = msg.strip_prefix("checkout: moving from ") {
            count += 1;
            if count == n {
                // Extract the "from" branch name
                if let Some(to_pos) = rest.find(" to ") {
                    let from_branch = &rest[..to_pos];
                    // Try to resolve the branch name
                    let ref_name = format!("refs/heads/{from_branch}");
                    if let Ok(oid) = refs::resolve_ref(&repo.git_dir, &ref_name) {
                        return Ok(Some(oid));
                    }
                    // Try as-is (might be a detached HEAD SHA)
                    if let Ok(oid) = from_branch.parse::<ObjectId>() {
                        if repo.odb.exists(&oid) {
                            return Ok(Some(oid));
                        }
                    }
                    return Err(Error::InvalidRef(format!(
                        "cannot resolve @{{-{n}}}: branch '{}' not found",
                        from_branch
                    )));
                }
            }
        }
    }
    Err(Error::InvalidRef(format!(
        "@{{-{n}}}: only {count} checkout(s) in reflog"
    )))
}

/// Try to resolve `ref@{N}` reflog index syntax.
/// Returns the OID at that reflog position, or None if not matching.
fn try_resolve_reflog_index(repo: &Repository, spec: &str) -> Result<Option<ObjectId>> {
    // Match patterns like HEAD@{0}, main@{1}, @{0}, refs/heads/main@{2}
    let at_pos = match spec.find("@{") {
        Some(p) => p,
        None => return Ok(None),
    };
    if !spec.ends_with('}') {
        return Ok(None);
    }
    let inner = &spec[at_pos + 2..spec.len() - 1];
    // Handle @{now} — equivalent to @{0} (most recent reflog entry)
    let index_or_date: ReflogSelector = if inner.eq_ignore_ascii_case("now") {
        ReflogSelector::Index(0)
    } else if let Ok(n) = inner.parse::<usize>() {
        ReflogSelector::Index(n)
    } else if let Some(ts) = approxidate(inner) {
        ReflogSelector::Date(ts)
    } else {
        return Ok(None);
    };
    let refname_raw = &spec[..at_pos];
    let refname = if refname_raw.is_empty() {
        "HEAD".to_string()
    } else if refname_raw == "HEAD" || refname_raw.starts_with("refs/") {
        refname_raw.to_string()
    } else {
        // DWIM: try refs/heads/<name>
        let candidate = format!("refs/heads/{refname_raw}");
        if refs::resolve_ref(&repo.git_dir, &candidate).is_ok() {
            candidate
        } else {
            refname_raw.to_string()
        }
    };
    let entries = read_reflog(&repo.git_dir, &refname)?;
    if entries.is_empty() {
        return Err(Error::InvalidRef(format!(
            "log for '{}' is empty",
            refname_raw
        )));
    }
    match index_or_date {
        ReflogSelector::Index(index) => {
            // Reflog entries are oldest-first in file; @{0} is the newest (last)
            let reversed_idx = entries.len().checked_sub(1 + index).ok_or_else(|| {
                Error::InvalidRef(format!(
                    "log for '{}' only has {} entries",
                    refname_raw,
                    entries.len()
                ))
            })?;
            Ok(Some(entries[reversed_idx].new_oid))
        }
        ReflogSelector::Date(target_ts) => {
            // Find the reflog entry whose timestamp is closest to but >= target_ts.
            // Entries are oldest-first; scan newest-first to find the first
            // entry at or before the target date.
            for entry in entries.iter().rev() {
                let ts = parse_reflog_entry_timestamp(entry);
                if let Some(t) = ts {
                    if t <= target_ts {
                        return Ok(Some(entry.new_oid));
                    }
                }
            }
            // If all entries are after target date, return the oldest entry
            Ok(Some(entries[0].new_oid))
        }
    }
}

enum ReflogSelector {
    Index(usize),
    Date(i64),
}

/// Parse a timestamp from a reflog entry's identity string.
fn parse_reflog_entry_timestamp(entry: &crate::reflog::ReflogEntry) -> Option<i64> {
    // Identity looks like: "Name <email> 1234567890 +0000"
    let parts: Vec<&str> = entry.identity.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse::<i64>().ok()
    } else {
        None
    }
}

/// Simple approximate date parser for reflog date lookups.
/// Handles formats like "2001-09-17", "3.hot.dogs.on.2001-09-17", etc.
fn approxidate(s: &str) -> Option<i64> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();

    let parse_relative_ago = |input: &str| -> Option<i64> {
        let normalized = input.trim().to_ascii_lowercase().replace(['.', '_'], " ");
        let parts: Vec<&str> = normalized.split_whitespace().collect();
        if parts.len() != 3 || parts[2] != "ago" {
            return None;
        }

        let amount: i64 = parts[0].parse().ok()?;
        if amount < 0 {
            return None;
        }

        let unit_seconds: i64 = match parts[1] {
            "second" | "seconds" | "sec" | "secs" => 1,
            "minute" | "minutes" | "min" | "mins" => 60,
            "hour" | "hours" | "hr" | "hrs" => 60 * 60,
            "day" | "days" => 60 * 60 * 24,
            "week" | "weeks" => 60 * 60 * 24 * 7,
            // Git's approxidate handles calendar-aware months/years; for this
            // focused parser we use fixed-length approximations.
            "month" | "months" => 60 * 60 * 24 * 30,
            "year" | "years" => 60 * 60 * 24 * 365,
            _ => return None,
        };

        let delta = amount.checked_mul(unit_seconds)?;
        now.checked_sub(delta)
    };

    if s.trim().eq_ignore_ascii_case("now") {
        return Some(now);
    }

    if let Some(ts) = parse_relative_ago(s) {
        return Some(ts);
    }

    // Try to extract a YYYY-MM-DD pattern from the string
    let re_like = |input: &str| -> Option<i64> {
        // Scan for 4-digit year followed by -MM-DD
        for (i, _) in input.char_indices() {
            let rest = &input[i..];
            if rest.len() >= 10 {
                let bytes = rest.as_bytes();
                if bytes[4] == b'-'
                    && bytes[7] == b'-'
                    && bytes[0..4].iter().all(|b| b.is_ascii_digit())
                    && bytes[5..7].iter().all(|b| b.is_ascii_digit())
                    && bytes[8..10].iter().all(|b| b.is_ascii_digit())
                {
                    let year: i32 = rest[0..4].parse().ok()?;
                    let month: u8 = rest[5..7].parse().ok()?;
                    let day: u8 = rest[8..10].parse().ok()?;
                    let date = time::Date::from_calendar_date(
                        year,
                        time::Month::try_from(month).ok()?,
                        day,
                    )
                    .ok()?;
                    let dt = date.with_hms(0, 0, 0).ok()?;
                    let odt = dt.assume_utc();
                    return Some(odt.unix_timestamp());
                }
            }
        }
        None
    };
    re_like(s)
}

/// Try to resolve `@{upstream}`, `@{u}`, `@{push}` style suffixes.
/// Returns the full ref name if recognized, None otherwise.
fn try_resolve_at_suffix(repo: &Repository, spec: &str) -> Option<String> {
    // Check for @{upstream}, @{u}, @{UPSTREAM}, @{U}, @{push} (case-insensitive for upstream)
    let lower = spec.to_lowercase();
    if lower.ends_with("@{upstream}") || lower.ends_with("@{u}") {
        let suffix_len = if lower.ends_with("@{upstream}") {
            11
        } else {
            4
        };
        let base = &spec[..spec.len() - suffix_len];
        return resolve_upstream_ref(repo, base);
    }
    if lower.ends_with("@{push}") {
        let base = &spec[..spec.len() - 7];
        return resolve_push_ref(repo, base);
    }
    None
}

/// Look up a path in the index (stage 0) and return its OID.

fn resolve_index_path(repo: &Repository, path: &str) -> Result<ObjectId> {
    resolve_index_path_at_stage(repo, path, 0)
}

/// Look up a path in the index at a given stage and return its OID.
fn resolve_index_path_at_stage(repo: &Repository, path: &str, stage: u8) -> Result<ObjectId> {
    use crate::index::Index;
    let index_path = repo.index_path();
    let index =
        Index::load(&index_path).map_err(|_| Error::ObjectNotFound(format!(":{stage}:{path}")))?;
    match index.get(path.as_bytes(), stage) {
        Some(entry) => Ok(entry.oid),
        None => Err(Error::ObjectNotFound(format!(":{stage}:{path}"))),
    }
}

fn split_treeish_spec(spec: &str) -> Option<(&str, &str)> {
    let (treeish, path) = spec.split_once(':')?;
    if treeish.is_empty() || path.is_empty() {
        return None;
    }
    Some((treeish, path))
}

fn resolve_treeish_path(repo: &Repository, treeish: ObjectId, path: &str) -> Result<ObjectId> {
    let object = repo.odb.read(&treeish)?;
    let mut current_tree = match object.kind {
        ObjectKind::Commit => parse_commit(&object.data)?.tree,
        ObjectKind::Tree => treeish,
        _ => {
            return Err(Error::InvalidRef(format!(
                "object {treeish} does not name a tree"
            )))
        }
    };

    let mut parts = path.split('/').filter(|part| !part.is_empty()).peekable();
    if parts.peek().is_none() {
        return Ok(current_tree);
    }
    while let Some(part) = parts.next() {
        let tree_object = repo.odb.read(&current_tree)?;
        if tree_object.kind != ObjectKind::Tree {
            return Err(Error::CorruptObject(format!(
                "object {current_tree} is not a tree"
            )));
        }
        let entries = parse_tree(&tree_object.data)?;
        let Some(entry) = entries.iter().find(|entry| entry.name == part.as_bytes()) else {
            return Err(Error::ObjectNotFound(path.to_owned()));
        };
        if parts.peek().is_none() {
            return Ok(entry.oid);
        }
        current_tree = entry.oid;
    }

    Err(Error::ObjectNotFound(path.to_owned()))
}

fn apply_peel(repo: &Repository, mut oid: ObjectId, peel: Option<&str>) -> Result<ObjectId> {
    match peel {
        None | Some("object") => Ok(oid),
        Some("") => {
            while let Ok(obj) = repo.odb.read(&oid) {
                if obj.kind != ObjectKind::Tag {
                    break;
                }
                oid = parse_tag_target(&obj.data)?;
            }
            Ok(oid)
        }
        Some("commit") => {
            oid = apply_peel(repo, oid, Some(""))?;
            let obj = repo.odb.read(&oid)?;
            if obj.kind == ObjectKind::Commit {
                Ok(oid)
            } else {
                Err(Error::InvalidRef("expected commit".to_owned()))
            }
        }
        Some("tree") => {
            // Peel tags, then dereference a commit to its tree.
            oid = apply_peel(repo, oid, Some(""))?;
            let obj = repo.odb.read(&oid)?;
            match obj.kind {
                ObjectKind::Tree => Ok(oid),
                ObjectKind::Commit => Ok(parse_commit(&obj.data)?.tree),
                _ => Err(Error::InvalidRef("expected tree or commit".to_owned())),
            }
        }
        Some(other) => Err(Error::InvalidRef(format!(
            "unsupported peel operator '{{{other}}}'"
        ))),
    }
}

fn parse_peel_suffix(spec: &str) -> (&str, Option<&str>) {
    if let Some(base) = spec.strip_suffix("^{}") {
        return (base, Some(""));
    }
    if let Some(start) = spec.rfind("^{") {
        if spec.ends_with('}') {
            let base = &spec[..start];
            let op = &spec[start + 2..spec.len() - 1];
            return (base, Some(op));
        }
    }
    // `^0` is shorthand for `^{commit}` — peel tags and verify commit.
    if let Some(base) = spec.strip_suffix("^0") {
        // Only match if the character before `^0` is not also a `^` (avoid
        // matching `^^0` as a peel instead of nav+nav).
        if !base.ends_with('^') {
            return (base, Some("commit"));
        }
    }
    (spec, None)
}

fn parse_tag_target(data: &[u8]) -> Result<ObjectId> {
    let text = std::str::from_utf8(data)
        .map_err(|_| Error::CorruptObject("invalid tag object".to_owned()))?;
    let Some(line) = text.lines().find(|line| line.starts_with("object ")) else {
        return Err(Error::CorruptObject("tag missing object header".to_owned()));
    };
    let oid_text = line.trim_start_matches("object ").trim();
    oid_text.parse::<ObjectId>()
}

fn find_abbrev_matches(repo: &Repository, prefix: &str) -> Result<Vec<ObjectId>> {
    if !is_hex_prefix(prefix) || !(4..=40).contains(&prefix.len()) {
        return Ok(Vec::new());
    }
    let all = collect_loose_object_ids(repo)?;
    let mut matches = Vec::new();
    for candidate in all {
        if candidate.starts_with(prefix) {
            matches.push(candidate.parse::<ObjectId>()?);
        }
    }
    Ok(matches)
}

/// Enumerate loose object IDs that match a hexadecimal prefix.
///
/// The returned list is sorted and deduplicated. This scans only loose
/// objects (not pack indexes), matching the scope currently used by
/// abbreviated-hex lookup in this module.
///
/// # Errors
///
/// Returns an I/O error when the loose object directory cannot be read.
pub fn list_loose_abbrev_matches(repo: &Repository, prefix: &str) -> Result<Vec<ObjectId>> {
    if !is_hex_prefix(prefix) || !(4..=40).contains(&prefix.len()) {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    for candidate in collect_loose_object_ids(repo)? {
        if candidate.starts_with(prefix) {
            matches.push(candidate.parse::<ObjectId>()?);
        }
    }
    matches.sort();
    matches.dedup();
    Ok(matches)
}

fn collect_loose_object_ids(repo: &Repository) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let objects_dir = repo.odb.objects_dir();
    let read = match fs::read_dir(objects_dir) {
        Ok(read) => read,
        Err(err) => return Err(Error::Io(err)),
    };

    for dir_entry in read {
        let dir_entry = dir_entry?;
        let name = dir_entry.file_name();
        let Some(prefix) = name.to_str() else {
            continue;
        };
        if !is_two_hex(prefix) {
            continue;
        }
        if !dir_entry.file_type()?.is_dir() {
            continue;
        }

        let files = fs::read_dir(dir_entry.path())?;
        for file_entry in files {
            let file_entry = file_entry?;
            if !file_entry.file_type()?.is_file() {
                continue;
            }
            let file_name = file_entry.file_name();
            let Some(suffix) = file_name.to_str() else {
                continue;
            };
            if suffix.len() == 38 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
                ids.push(format!("{prefix}{suffix}"));
            }
        }
    }

    Ok(ids)
}

fn is_two_hex(text: &str) -> bool {
    text.len() == 2 && text.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn is_hex_prefix(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn path_is_within(path: &Path, container: &Path) -> bool {
    if path == container {
        return true;
    }
    path.starts_with(container)
}

fn normalize_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::RootDir => Some(String::from("/")),
            Component::Normal(item) => Some(item.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn component_to_text(component: Component<'_>) -> Option<String> {
    match component {
        Component::Normal(item) => Some(os_to_string(item)),
        _ => None,
    }
}

fn os_to_string(text: &OsStr) -> String {
    text.to_string_lossy().into_owned()
}

/// Search commit messages from HEAD backwards for a commit whose message
/// contains `pattern`.  Returns the first matching commit OID.
fn resolve_commit_message_search(
    repo: &crate::repo::Repository,
    pattern: &str,
) -> Result<ObjectId> {
    use crate::state::resolve_head;
    let head =
        resolve_head(&repo.git_dir).map_err(|_| Error::ObjectNotFound(format!(":/{pattern}")))?;
    let start_oid = match head.oid() {
        Some(oid) => *oid,
        None => return Err(Error::ObjectNotFound(format!(":/{pattern}"))),
    };

    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start_oid);
    visited.insert(start_oid);

    while let Some(oid) = queue.pop_front() {
        let obj = match repo.odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        // Skip non-commit objects
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = match parse_commit(&obj.data) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check if message contains pattern
        if commit.message.contains(pattern) {
            return Ok(oid);
        }

        // Enqueue parents
        for parent in &commit.parents {
            if visited.insert(*parent) {
                queue.push_back(*parent);
            }
        }
    }

    Err(Error::ObjectNotFound(format!(":/{pattern}")))
}
