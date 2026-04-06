//! Reference storage — files backend + reftable backend.
//!
//! Git stores references as text files under `<git-dir>/refs/` (and
//! `<git-dir>/packed-refs` for the packed backend).  Each loose ref file
//! contains either:
//!
//! - A 40-character hex SHA-1 followed by a newline, **or**
//! - The string `"ref: <target>\n"` for symbolic refs.
//!
//! `HEAD` is a special case: it is normally a symbolic ref but may also be
//! detached (pointing directly at a commit hash).
//!
//! When `extensions.refStorage = reftable`, the reftable backend is used
//! instead.  The public API is the same; dispatch is handled internally.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::objects::ObjectId;

/// A symbolic or direct reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// Direct reference: stores an [`ObjectId`].
    Direct(ObjectId),
    /// Symbolic reference: stores the name of the target ref.
    Symbolic(String),
}

/// Read a single reference file from `path`.
///
/// # Errors
///
/// - [`Error::InvalidRef`] if the file content is not a valid ref.
/// - [`Error::Io`] on filesystem errors.
pub fn read_ref_file(path: &Path) -> Result<Ref> {
    let content = fs::read_to_string(path).map_err(Error::Io)?;
    let content = content.trim_end_matches('\n');
    parse_ref_content(content)
}

/// Parse the content of a ref file (without trailing newline).
pub(crate) fn parse_ref_content(content: &str) -> Result<Ref> {
    if let Some(target) = content.strip_prefix("ref: ") {
        Ok(Ref::Symbolic(target.trim().to_owned()))
    } else if content.len() == 40 && content.chars().all(|c| c.is_ascii_hexdigit()) {
        let oid: ObjectId = content.parse()?;
        Ok(Ref::Direct(oid))
    } else {
        Err(Error::InvalidRef(content.to_owned()))
    }
}

/// Resolve a reference to its target [`ObjectId`], following symbolic refs.
///
/// Dispatches to the reftable backend when `extensions.refStorage = reftable`.
///
/// # Parameters
///
/// - `git_dir` — path to the git directory.
/// - `refname` — reference name (e.g. `"HEAD"`, `"refs/heads/main"`).
///
/// # Errors
///
/// - [`Error::InvalidRef`] if the ref is malformed or forms a cycle.
/// - [`Error::ObjectNotFound`] if a symbolic target does not exist.
pub fn resolve_ref(git_dir: &Path, refname: &str) -> Result<ObjectId> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_resolve_ref(git_dir, refname);
    }
    let common = common_dir(git_dir);
    resolve_ref_depth(git_dir, common.as_deref(), refname, 0)
}

/// Determine the common git directory for worktree-aware ref resolution.
///
/// If `<git_dir>/commondir` exists, its contents point to the shared
/// git directory. Returns `None` when git_dir is already the common dir.
fn common_dir(git_dir: &Path) -> Option<PathBuf> {
    let commondir_file = git_dir.join("commondir");
    let raw = fs::read_to_string(commondir_file).ok()?;
    let rel = raw.trim();
    let path = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    path.canonicalize().ok()
}

/// Internal recursive resolver with cycle detection.
///
/// When operating inside a worktree, `common` points to the shared git
/// directory where most refs live.  The worktree-specific `git_dir` is
/// checked first for HEAD and per-worktree refs.
fn resolve_ref_depth(
    git_dir: &Path,
    common: Option<&Path>,
    refname: &str,
    depth: usize,
) -> Result<ObjectId> {
    if depth > 10 {
        return Err(Error::InvalidRef(format!(
            "ref symlink too deep: {refname}"
        )));
    }

    // First try as a loose ref file in git_dir
    let path = git_dir.join(refname);
    match read_ref_file(&path) {
        Ok(Ref::Direct(oid)) => return Ok(oid),
        Ok(Ref::Symbolic(target)) => {
            return resolve_ref_depth(git_dir, common, &target, depth + 1);
        }
        Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    // For worktrees, try the common dir for shared refs
    if let Some(cdir) = common {
        if cdir != git_dir {
            let cpath = cdir.join(refname);
            match read_ref_file(&cpath) {
                Ok(Ref::Direct(oid)) => return Ok(oid),
                Ok(Ref::Symbolic(target)) => {
                    return resolve_ref_depth(git_dir, common, &target, depth + 1);
                }
                Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }

    // Fall back to packed-refs (in common dir if available)
    let packed_dir = common.unwrap_or(git_dir);
    if let Some(oid) = lookup_packed_ref(packed_dir, refname)? {
        return Ok(oid);
    }
    // Also check git_dir packed-refs if different from common
    if common.is_some() && common != Some(git_dir) {
        if let Some(oid) = lookup_packed_ref(git_dir, refname)? {
            return Ok(oid);
        }
    }

    Err(Error::InvalidRef(format!("ref not found: {refname}")))
}

/// Look up a refname in `packed-refs`.
fn lookup_packed_ref(git_dir: &Path, refname: &str) -> Result<Option<ObjectId>> {
    let packed = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    };

    for line in content.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let hash = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        if name == refname && hash.len() == 40 {
            let oid: ObjectId = hash.parse()?;
            return Ok(Some(oid));
        }
    }
    Ok(None)
}

/// Write a ref, creating parent directories as needed.
///
/// Dispatches to the reftable backend when `extensions.refStorage = reftable`.
///
/// # Parameters
///
/// - `git_dir` — path to the git directory.
/// - `refname` — reference name (e.g. `"refs/heads/main"`).
/// - `oid` — the new target object ID.
///
/// # Errors
///
/// Returns [`Error::Io`] on filesystem errors.
pub fn write_ref(git_dir: &Path, refname: &str, oid: &ObjectId) -> Result<()> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_write_ref(git_dir, refname, oid, None, None);
    }
    let storage_dir = ref_storage_dir(git_dir, refname);
    let path = storage_dir.join(refname);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = format!("{oid}\n");
    // Write via lock file for atomicity
    let lock = path.with_extension("lock");
    fs::write(&lock, &content)?;
    fs::rename(&lock, &path)?;
    Ok(())
}

/// Delete a ref.
///
/// Dispatches to the reftable backend when `extensions.refStorage = reftable`.
///
/// # Errors
///
/// Returns [`Error::Io`] for errors other than "not found".
pub fn delete_ref(git_dir: &Path, refname: &str) -> Result<()> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_delete_ref(git_dir, refname);
    }
    let storage_dir = ref_storage_dir(git_dir, refname);
    // Remove the loose ref file
    let path = storage_dir.join(refname);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::Io(e)),
    }

    // Also remove the entry from packed-refs if present
    remove_packed_ref(&storage_dir, refname)?;

    Ok(())
}

/// Remove a single entry from the packed-refs file, rewriting it.
fn remove_packed_ref(git_dir: &Path, refname: &str) -> Result<()> {
    let packed_path = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed_path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e)),
    };

    let mut out = String::new();
    let mut skip_peeled = false;
    let mut changed = false;
    // Write a fresh header (don't preserve old comment lines — real git
    // regenerates the header on every rewrite).
    let mut header_written = false;

    for line in content.lines() {
        if skip_peeled {
            if line.starts_with('^') {
                changed = true;
                continue;
            }
            skip_peeled = false;
        }

        if line.starts_with('#') {
            // Skip old header lines — we'll write a fresh one
            continue;
        }
        if line.starts_with('^') {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Write fresh header before the first data line
        if !header_written {
            out.insert_str(0, "# pack-refs with: peeled fully-peeled sorted\n");
            header_written = true;
        }

        // Check if this line matches the ref to remove
        let mut parts = line.splitn(2, ' ');
        let _hash = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        if name == refname {
            changed = true;
            skip_peeled = true;
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    if changed {
        let lock = packed_path.with_extension("lock");
        fs::write(&lock, &out).map_err(Error::Io)?;
        fs::rename(&lock, &packed_path).map_err(Error::Io)?;
    }

    Ok(())
}

/// Read the symbolic ref target of `HEAD`.
///
/// Returns `None` if HEAD is detached (points directly to a commit hash).
///
/// # Errors
///
/// Returns [`Error::Io`] or [`Error::InvalidRef`] on failures.
pub fn read_head(git_dir: &Path) -> Result<Option<String>> {
    match read_ref_file(&git_dir.join("HEAD"))? {
        Ref::Symbolic(target) => Ok(Some(target)),
        Ref::Direct(_) => Ok(None),
    }
}

/// Read symbolic target of any ref.
///
/// Dispatches to the reftable backend when `extensions.refStorage = reftable`.
///
/// Returns `Ok(Some(target))` when `refname` exists and is symbolic,
/// `Ok(None)` when it is direct or missing.
pub fn read_symbolic_ref(git_dir: &Path, refname: &str) -> Result<Option<String>> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_read_symbolic_ref(git_dir, refname);
    }
    let path = git_dir.join(refname);
    match read_ref_file(&path) {
        Ok(Ref::Symbolic(target)) => Ok(Some(target)),
        Ok(Ref::Direct(_)) => Ok(None),
        Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(common) = common_dir(git_dir) {
                if common != git_dir {
                    let cpath = common.join(refname);
                    match read_ref_file(&cpath) {
                        Ok(Ref::Symbolic(target)) => return Ok(Some(target)),
                        Ok(Ref::Direct(_)) => return Ok(None),
                        Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Write a reflog entry.
///
/// Dispatches to the reftable backend when `extensions.refStorage = reftable`.
///
/// # Parameters
///
/// - `git_dir` — path to the git directory.
/// - `refname` — reference name (e.g. `"refs/heads/main"`).
/// - `old_oid` — previous OID (use `ObjectId::from_bytes(&[0;20])` for a new ref).
/// - `new_oid` — new OID.
/// - `identity` — `"Name <email> <timestamp> <tz>"` formatted string.
/// - `message` — short log message.
///
/// # Errors
///
/// Returns [`Error::Io`] on filesystem errors.
pub fn append_reflog(
    git_dir: &Path,
    refname: &str,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    identity: &str,
    message: &str,
) -> Result<()> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_append_reflog(
            git_dir, refname, old_oid, new_oid, identity, message,
        );
    }
    let storage_dir = ref_storage_dir(git_dir, refname);
    let log_path = storage_dir.join("logs").join(refname);
    let auto_create = refname == "HEAD" || reflog_auto_create_enabled(git_dir);
    if !auto_create && !log_path.exists() {
        return Ok(());
    }
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = format!("{old_oid} {new_oid} {identity}\t{message}\n");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    use io::Write;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn ref_storage_dir(git_dir: &Path, refname: &str) -> PathBuf {
    if refname == "HEAD" || refname.starts_with("refs/bisect/") {
        return git_dir.to_path_buf();
    }
    common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf())
}

/// Determine whether missing reflog files should be auto-created.
///
/// This follows Git's core.logAllRefUpdates behavior:
/// - explicit true/always/on/1 => create logs
/// - explicit false/never/off/0 => do not auto-create
/// - unset => true for non-bare repos, false for bare repos
fn reflog_auto_create_enabled(git_dir: &Path) -> bool {
    let config_dir = common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf());
    let config_path = config_dir.join("config");
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return true,
    };

    let mut in_core = false;
    let mut log_all_ref_updates: Option<bool> = None;
    let mut bare = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_core = trimmed.to_ascii_lowercase().starts_with("[core]");
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        match key.as_str() {
            "logallrefupdates" => {
                log_all_ref_updates = match value.as_str() {
                    "1" | "true" | "yes" | "on" | "always" => Some(true),
                    "0" | "false" | "no" | "off" | "never" => Some(false),
                    _ => None,
                };
            }
            "bare" => {
                bare = matches!(value.as_str(), "1" | "true" | "yes" | "on");
            }
            _ => {}
        }
    }

    log_all_ref_updates.unwrap_or(!bare)
}

/// List all refs under a given prefix (e.g. `"refs/heads/"`).
///
/// Dispatches to the reftable backend when `extensions.refStorage = reftable`.
///
/// Returns a sorted list of `(refname, ObjectId)` pairs.
///
/// # Errors
///
/// Returns [`Error::Io`] on directory traversal errors.
pub fn list_refs(git_dir: &Path, prefix: &str) -> Result<Vec<(String, ObjectId)>> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_list_refs(git_dir, prefix);
    }
    let mut results = Vec::new();
    let base = git_dir.join(prefix);
    collect_refs(&base, prefix, git_dir, &mut results)?;
    collect_packed_refs(git_dir, prefix, &mut results)?;

    // For worktrees, also collect refs from the common dir
    if let Some(cdir) = common_dir(git_dir) {
        if cdir != git_dir {
            let cbase = cdir.join(prefix);
            collect_refs(&cbase, prefix, &cdir, &mut results)?;
            collect_packed_refs(&cdir, prefix, &mut results)?;
            // Deduplicate: worktree-local refs take priority
            results.sort_by(|a, b| a.0.cmp(&b.0));
            results.dedup_by(|b, a| a.0 == b.0);
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

/// List refs matching a glob pattern (e.g. `refs/heads/topic/*`).
pub fn list_refs_glob(git_dir: &Path, pattern: &str) -> Result<Vec<(String, ObjectId)>> {
    let glob_pos = pattern.find(['*', '?', '[']);
    let prefix = match glob_pos {
        Some(pos) => match pattern[..pos].rfind('/') {
            Some(slash) => &pattern[..=slash],
            None => "",
        },
        None => pattern,
    };
    let all = list_refs(git_dir, prefix)?;
    let mut results = Vec::new();
    for (refname, oid) in all {
        if glob_match(pattern, &refname) {
            results.push((refname, oid));
        }
    }
    Ok(results)
}

/// Check whether a ref name matches a glob pattern.
///
/// Supports `*`, `?`, and `[…]` wildcards. An exact string match is also accepted.
pub fn ref_matches_glob(refname: &str, pattern: &str) -> bool {
    // For exact matches (no glob characters), check suffix match
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return refname == pattern
            || refname.ends_with(&format!("/{pattern}"))
            || refname.starts_with(&format!("{pattern}/"));
    }
    glob_match(pattern, refname)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pat = pattern.as_bytes();
    let txt = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0);
    while ti < txt.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

fn collect_refs(
    dir: &Path,
    prefix: &str,
    git_dir: &Path,
    out: &mut Vec<(String, ObjectId)>,
) -> Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e)),
    };

    for entry in read {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let refname = format!("{prefix}{name_str}");

        if file_type.is_dir() {
            collect_refs(&entry.path(), &format!("{refname}/"), git_dir, out)?;
        } else if file_type.is_file() {
            if let Ok(oid) = resolve_ref(git_dir, &refname) {
                out.push((refname, oid))
            }
        }
    }
    Ok(())
}

fn collect_packed_refs(git_dir: &Path, prefix: &str, out: &mut Vec<(String, ObjectId)>) -> Result<()> {
    let packed_path = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed_path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e)),
    };

    for line in content.lines() {
        if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let hash = parts.next().unwrap_or("");
        let refname = parts.next().unwrap_or("").trim();
        if !refname.starts_with(prefix) || hash.len() != 40 {
            continue;
        }
        let oid: ObjectId = hash.parse()?;
        out.push((refname.to_string(), oid));
    }
    Ok(())
}
