//! Reference storage — files backend.
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
//! # Scope
//!
//! This module implements the **files backend** only (loose refs + read-only
//! packed-refs).  The reftable backend is out of scope for v1.

use std::fs;
use std::io;
use std::path::Path;

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
    resolve_ref_depth(git_dir, refname, 0)
}

/// Internal recursive resolver with cycle detection.
fn resolve_ref_depth(git_dir: &Path, refname: &str, depth: usize) -> Result<ObjectId> {
    if depth > 10 {
        return Err(Error::InvalidRef(format!(
            "ref symlink too deep: {refname}"
        )));
    }

    // First try as a loose ref file
    let path = git_dir.join(refname);
    match read_ref_file(&path) {
        Ok(Ref::Direct(oid)) => return Ok(oid),
        Ok(Ref::Symbolic(target)) => {
            return resolve_ref_depth(git_dir, &target, depth + 1);
        }
        Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    // Fall back to packed-refs
    if let Some(oid) = lookup_packed_ref(git_dir, refname)? {
        return Ok(oid);
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

/// Write a loose ref, creating parent directories as needed.
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
    let path = git_dir.join(refname);
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

/// Delete a loose ref file.
///
/// Returns `Ok(())` even if the file did not exist.
///
/// # Errors
///
/// Returns [`Error::Io`] for errors other than "not found".
pub fn delete_ref(git_dir: &Path, refname: &str) -> Result<()> {
    let path = git_dir.join(refname);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Io(e)),
    }
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

/// Read symbolic target of any loose ref.
///
/// Returns `Ok(Some(target))` when `refname` exists and is symbolic,
/// `Ok(None)` when it is direct or missing.
pub fn read_symbolic_ref(git_dir: &Path, refname: &str) -> Result<Option<String>> {
    let path = git_dir.join(refname);
    match read_ref_file(&path) {
        Ok(Ref::Symbolic(target)) => Ok(Some(target)),
        Ok(Ref::Direct(_)) => Ok(None),
        Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write a reflog entry.
///
/// Appends a line to `<git-dir>/logs/<refname>`.  Creates the log file and
/// parent directories if they do not exist.
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
    let log_path = git_dir.join("logs").join(refname);
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

/// List all loose refs under a given prefix (e.g. `"refs/heads/"`).
///
/// Returns a sorted list of `(refname, ObjectId)` pairs.
///
/// # Errors
///
/// Returns [`Error::Io`] on directory traversal errors.
pub fn list_refs(git_dir: &Path, prefix: &str) -> Result<Vec<(String, ObjectId)>> {
    let base = git_dir.join(prefix);
    let mut results = Vec::new();
    collect_refs(&base, prefix, git_dir, &mut results)?;
    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
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
