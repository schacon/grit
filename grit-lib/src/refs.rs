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

use std::collections::{BTreeSet, HashMap, HashSet};
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
pub fn common_dir(git_dir: &Path) -> Option<PathBuf> {
    let commondir_file = git_dir.join("commondir");
    let raw = fs::read_to_string(commondir_file).ok()?;
    let rel = raw.trim();
    // Match Git: `commondir` may be relative to this gitdir or an absolute path (see
    // `git worktree add` and `refs/files-backend.c`).
    let path = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    path.canonicalize().ok()
}

fn notes_merge_state_ref(refname: &str) -> bool {
    matches!(refname, "NOTES_MERGE_REF" | "NOTES_MERGE_PARTIAL")
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
        if notes_merge_state_ref(refname) {
            // These live only under this worktree's gitdir (see Git `is_root_ref` / per-worktree stores).
        } else if cdir != git_dir {
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

/// Outcome of a single storage-level ref lookup (Git `refs_read_raw_ref` style).
///
/// This checks whether a ref **name** exists in the ref store without applying
/// DWIM rules. A symbolic ref is considered to exist if its ref file (or
/// reftable record) is present, even when the target is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawRefLookup {
    /// A loose ref file, packed ref line, or reftable record exists for this name.
    Exists,
    /// No ref is recorded under this exact name.
    NotFound,
    /// A path component exists as a directory where a ref file was expected (e.g. `refs/heads`).
    IsDirectory,
}

/// Return whether `refname` exists as a ref in the repository's ref storage.
///
/// This matches `git refs exists` / `git show-ref --exists`: no DWIM, no
/// resolution of symbolic targets. Dispatches to the reftable backend when
/// configured.
///
/// # Parameters
///
/// - `git_dir` — path to the git directory (worktree gitdir or bare `.git`).
/// - `refname` — full ref name (e.g. `HEAD`, `refs/heads/main`, `CHERRY_PICK_HEAD`).
///
/// # Errors
///
/// Propagates I/O and reftable errors other than "not found".
pub fn read_raw_ref(git_dir: &Path, refname: &str) -> Result<RawRefLookup> {
    if crate::reftable::is_reftable_repo(git_dir) {
        read_raw_ref_reftable(git_dir, refname)
    } else {
        read_raw_ref_files(git_dir, refname)
    }
}

fn read_raw_ref_files(git_dir: &Path, refname: &str) -> Result<RawRefLookup> {
    let common = common_dir(git_dir);

    if let Some(lookup) = read_raw_ref_at(git_dir.join(refname))? {
        return Ok(lookup);
    }

    if let Some(cdir) = common.as_ref() {
        if *cdir != git_dir && !notes_merge_state_ref(refname) {
            if let Some(lookup) = read_raw_ref_at(cdir.join(refname))? {
                return Ok(lookup);
            }
        }
    }

    let packed_dir = common.as_deref().unwrap_or(git_dir);
    if packed_ref_name_exists(packed_dir, refname)? {
        return Ok(RawRefLookup::Exists);
    }
    if common.is_some()
        && common.as_deref() != Some(git_dir)
        && packed_ref_name_exists(git_dir, refname)?
    {
        return Ok(RawRefLookup::Exists);
    }

    Ok(RawRefLookup::NotFound)
}

/// Lock file path for a loose ref file (`<refpath>.lock`), matching Git's naming for nested refs.
#[must_use]
pub fn lock_path_for_ref(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

fn read_raw_ref_at(path: PathBuf) -> Result<Option<RawRefLookup>> {
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            if meta.is_dir() {
                return Ok(Some(RawRefLookup::IsDirectory));
            }
            Ok(Some(RawRefLookup::Exists))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(e)),
    }
}

fn packed_ref_with_prefix(git_dir: &Path, prefix_with_slash: &str) -> Result<Option<String>> {
    let packed = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    };
    let mut best: Option<String> = None;
    for line in content.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _oid = parts.next();
        let Some(name) = parts.next() else {
            continue;
        };
        let name = name.trim();
        if name.starts_with(prefix_with_slash) {
            let take = match &best {
                None => true,
                Some(b) => name < b.as_str(),
            };
            if take {
                best = Some(name.to_owned());
            }
        }
    }
    Ok(best)
}

fn packed_ref_name_exists(git_dir: &Path, refname: &str) -> Result<bool> {
    let packed = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::Io(e)),
    };
    for line in content.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _oid = parts.next();
        if let Some(name) = parts.next() {
            if name == refname {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Why a reference name cannot be created (Git `refs_verify_refname_available` style).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefnameUnavailable {
    /// An ancestor ref already exists in the store (e.g. `refs/foo` blocks `refs/foo/bar`).
    AncestorExists {
        /// Existing ref that blocks creation.
        blocking: String,
        /// Ref the caller tried to create.
        new_ref: String,
    },
    /// A descendant ref already exists (e.g. `refs/foo/bar` blocks `refs/foo`).
    DescendantExists {
        /// Existing ref under `new_ref/`.
        blocking: String,
        /// Ref the caller tried to create.
        new_ref: String,
    },
    /// Two refnames in the same batch are mutually incompatible (parent vs child).
    SameBatch {
        /// Ref being validated (Git prints this first).
        refname: String,
        /// Other ref in the batch (parent dirname or descendant name).
        other: String,
    },
}

impl RefnameUnavailable {
    /// Suffix after `cannot lock ref '<display_ref>': ` for stderr (no trailing newline).
    #[must_use]
    pub fn lock_message_suffix(&self) -> String {
        match self {
            RefnameUnavailable::AncestorExists { blocking, new_ref } => {
                format!("'{blocking}' exists; cannot create '{new_ref}'")
            }
            RefnameUnavailable::DescendantExists { blocking, new_ref } => {
                format!("'{blocking}' exists; cannot create '{new_ref}'")
            }
            RefnameUnavailable::SameBatch { refname, other } => {
                format!("cannot process '{refname}' and '{other}' at the same time")
            }
        }
    }
}

fn find_descendant_in_sorted_extras(
    dirname_with_slash: &str,
    extras: &BTreeSet<String>,
) -> Option<String> {
    let start = extras
        .range(dirname_with_slash.to_string()..)
        .next()
        .cloned()?;
    if start.starts_with(dirname_with_slash) {
        Some(start)
    } else {
        None
    }
}

/// Verify that `refname` can be created without directory/file conflicts with the ref store
/// and with other refnames queued in the same transaction (`extras`).
///
/// `skip` names are ignored when checking the filesystem (updates that delete or replace
/// those refs in the same batch). Matches Git's `refs_verify_refname_available`.
///
/// # Parameters
///
/// - `git_dir` — repository git directory.
/// - `refname` — full ref name to create.
/// - `extras` — other refnames touched in the same stdin batch / transaction (sorted set).
/// - `skip` — refnames that may be deleted or updated away in the same batch.
pub fn verify_refname_available_for_create(
    git_dir: &Path,
    refname: &str,
    extras: &BTreeSet<String>,
    skip: &HashSet<String>,
) -> std::result::Result<(), RefnameUnavailable> {
    // `Repository::git_dir` may be a relative path (e.g. `.git`); resolve so lookups match the
    // on-disk ref store regardless of process cwd (test harness runs from `trash/.git/...`).
    let git_dir = fs::canonicalize(git_dir).unwrap_or_else(|_| git_dir.to_path_buf());
    let mut seen_dirnames: HashSet<String> = HashSet::new();
    let segments: Vec<&str> = refname.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() <= 1 {
        // No slash-separated parent prefixes (e.g. `HEAD`).
    } else {
        let mut dirname = String::new();
        for part in &segments[..segments.len() - 1] {
            if !dirname.is_empty() {
                dirname.push('/');
            }
            dirname.push_str(part);

            if !seen_dirnames.insert(dirname.clone()) {
                continue;
            }

            if skip.contains(&dirname) {
                continue;
            }

            match read_raw_ref(&git_dir, &dirname) {
                Ok(RawRefLookup::Exists) => {
                    return Err(RefnameUnavailable::AncestorExists {
                        blocking: dirname.clone(),
                        new_ref: refname.to_owned(),
                    });
                }
                // A directory at `refs/prefix` is normal when storing `refs/prefix/child`; only a
                // real ref (loose file or packed line) blocks creating `refs/prefix/...`.
                Ok(RawRefLookup::NotFound | RawRefLookup::IsDirectory) => {}
                Err(_) => {}
            }

            if extras.contains(&dirname) {
                return Err(RefnameUnavailable::SameBatch {
                    refname: refname.to_owned(),
                    other: dirname.clone(),
                });
            }
        }
    }

    let mut leaf_dir = String::with_capacity(refname.len() + 1);
    leaf_dir.push_str(refname);
    leaf_dir.push('/');

    let under = list_refs(&git_dir, &leaf_dir).unwrap_or_default();
    if under.is_empty() {
        let packed_dir = common_dir(&git_dir).unwrap_or_else(|| git_dir.clone());
        if let Ok(Some(name)) = packed_ref_with_prefix(&packed_dir, &leaf_dir) {
            if !skip.contains(&name) {
                return Err(RefnameUnavailable::DescendantExists {
                    blocking: name,
                    new_ref: refname.to_owned(),
                });
            }
        }
        if packed_dir != git_dir {
            if let Ok(Some(name)) = packed_ref_with_prefix(&git_dir, &leaf_dir) {
                if !skip.contains(&name) {
                    return Err(RefnameUnavailable::DescendantExists {
                        blocking: name,
                        new_ref: refname.to_owned(),
                    });
                }
            }
        }
    }
    if under.is_empty()
        && fs::symlink_metadata(git_dir.join(refname))
            .map(|m| m.is_dir())
            .unwrap_or(false)
    {
        let mut blocking: Option<String> = None;
        let dir_path = git_dir.join(refname);
        if let Ok(read) = fs::read_dir(&dir_path) {
            for entry in read.flatten() {
                let path = entry.path();
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let full = format!("{refname}/{name}");
                blocking = Some(full);
                break;
            }
        }
        if let Some(b) = blocking {
            if !skip.contains(&b) {
                return Err(RefnameUnavailable::DescendantExists {
                    blocking: b,
                    new_ref: refname.to_owned(),
                });
            }
        }
    }

    for (existing, _) in under {
        if skip.contains(&existing) {
            continue;
        }
        return Err(RefnameUnavailable::DescendantExists {
            blocking: existing,
            new_ref: refname.to_owned(),
        });
    }

    if let Some(extra) = find_descendant_in_sorted_extras(&leaf_dir, extras) {
        if !skip.contains(&extra) {
            return Err(RefnameUnavailable::SameBatch {
                refname: refname.to_owned(),
                other: extra,
            });
        }
    }

    Ok(())
}

fn read_raw_ref_reftable(git_dir: &Path, refname: &str) -> Result<RawRefLookup> {
    if refname == "HEAD" {
        let head_path = git_dir.join("HEAD");
        match fs::symlink_metadata(&head_path) {
            Ok(meta) => {
                if meta.is_dir() {
                    return Ok(RawRefLookup::IsDirectory);
                }
                return Ok(RawRefLookup::Exists);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(RawRefLookup::NotFound),
            Err(e) => return Err(Error::Io(e)),
        }
    }

    if let Some(lookup) = read_raw_ref_at(git_dir.join(refname))? {
        return Ok(lookup);
    }

    let stack = crate::reftable::ReftableStack::open(git_dir)?;
    match stack.lookup_ref(refname)? {
        Some(rec) => match rec.value {
            crate::reftable::RefValue::Deletion => Ok(RawRefLookup::NotFound),
            _ => Ok(RawRefLookup::Exists),
        },
        None => Ok(RawRefLookup::NotFound),
    }
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
/// Write a symbolic ref (e.g. `NOTES_MERGE_REF` → `refs/notes/m`).
///
/// For reftable-backed repositories this dispatches to the reftable writer.
pub fn write_symbolic_ref(git_dir: &Path, refname: &str, target: &str) -> Result<()> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_write_symref(git_dir, refname, target, None, None);
    }
    let storage_dir = ref_storage_dir(git_dir, refname);
    let path = storage_dir.join(refname);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = format!("ref: {target}\n");
    let lock = lock_path_for_ref(&path);
    fs::write(&lock, &content)?;
    fs::rename(&lock, &path)?;
    Ok(())
}

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
    let lock = lock_path_for_ref(&path);
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

    let log_path = storage_dir.join("logs").join(refname);

    // Keep `logs/refs/heads/<name>` when deleting a branch so `branch -D` + later recreate can
    // retain history (matches upstream expectations in t1507 `log -g` with `@{now}`).
    if !refname.starts_with("refs/heads/") {
        let _ = fs::remove_file(&log_path);

        // Remove empty parent directories under `logs/refs/heads/` so a deleted nested ref
        // does not leave `logs/refs/heads/d` as a directory (which would block reflogs for
        // a later branch named `d`).
        let logs_heads = storage_dir.join("logs/refs/heads");
        let mut parent = log_path.parent();
        while let Some(p) = parent {
            if p == logs_heads.as_path() || !p.starts_with(&logs_heads) {
                break;
            }
            if fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
    }

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
        Err(Error::Io(ref e))
            if e.kind() == io::ErrorKind::NotFound
                || e.kind() == io::ErrorKind::NotADirectory
                || e.kind() == io::ErrorKind::IsADirectory =>
        {
            if !notes_merge_state_ref(refname) {
                if let Some(common) = common_dir(git_dir) {
                    if common != git_dir {
                        let cpath = common.join(refname);
                        match read_ref_file(&cpath) {
                            Ok(Ref::Symbolic(target)) => return Ok(Some(target)),
                            Ok(Ref::Direct(_)) => return Ok(None),
                            Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {}
                            Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::NotADirectory => {}
                            Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::IsADirectory => {}
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Core `logAllRefUpdates` modes (after config lookup), matching Git's `log_refs_config`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRefsConfig {
    /// `core.logAllRefUpdates` not set; resolved per-repo (bare vs non-bare).
    Unset,
    /// Explicitly disabled.
    None,
    /// `true` — log branch-like refs only (see [`should_autocreate_reflog`]).
    Normal,
    /// `always` — log updates to any ref.
    Always,
}

/// Read `[core] logAllRefUpdates` from the repository config.
///
/// Returns [`LogRefsConfig::Unset`] when the key is absent.
pub fn read_log_refs_config(git_dir: &Path) -> LogRefsConfig {
    let config_dir = common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf());
    let config_path = config_dir.join("config");
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return LogRefsConfig::Unset,
    };

    let mut in_core = false;
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
        if !key.trim().eq_ignore_ascii_case("logallrefupdates") {
            continue;
        }
        let v = value.trim();
        let lower = v.to_ascii_lowercase();
        return match lower.as_str() {
            "always" => LogRefsConfig::Always,
            "1" | "true" | "yes" | "on" => LogRefsConfig::Normal,
            "0" | "false" | "no" | "off" | "never" => LogRefsConfig::None,
            _ => LogRefsConfig::Unset,
        };
    }
    LogRefsConfig::Unset
}

fn read_core_bare(git_dir: &Path) -> bool {
    let config_dir = common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf());
    let config_path = config_dir.join("config");
    let Ok(content) = fs::read_to_string(config_path) else {
        return false;
    };
    let mut in_core = false;
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
        if key.trim().eq_ignore_ascii_case("bare") {
            let v = value.trim().to_ascii_lowercase();
            return matches!(v.as_str(), "1" | "true" | "yes" | "on");
        }
    }
    false
}

/// Effective `logAllRefUpdates` after applying Git's `LOG_REFS_UNSET` rule.
pub fn effective_log_refs_config(git_dir: &Path) -> LogRefsConfig {
    match read_log_refs_config(git_dir) {
        LogRefsConfig::Unset => {
            if read_core_bare(git_dir) {
                LogRefsConfig::None
            } else {
                LogRefsConfig::Normal
            }
        }
        other => other,
    }
}

/// Whether a new reflog file may be auto-created for `refname` given an already-resolved
/// `core.logAllRefUpdates` mode (including command-line config).
#[must_use]
pub fn should_autocreate_reflog_for_mode(refname: &str, mode: LogRefsConfig) -> bool {
    match mode {
        LogRefsConfig::Always => true,
        LogRefsConfig::Normal => {
            refname == "HEAD"
                || refname.starts_with("refs/heads/")
                || refname.starts_with("refs/remotes/")
                || refname.starts_with("refs/notes/")
        }
        LogRefsConfig::None | LogRefsConfig::Unset => false,
    }
}

/// Whether a new reflog file may be auto-created for `refname` (Git `should_autocreate_reflog`).
#[must_use]
pub fn should_autocreate_reflog(git_dir: &Path, refname: &str) -> bool {
    should_autocreate_reflog_for_mode(refname, effective_log_refs_config(git_dir))
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
/// - `force_create` — if true, create the log file even when [`should_autocreate_reflog`] would not.
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
    force_create: bool,
) -> Result<()> {
    if crate::reftable::is_reftable_repo(git_dir) {
        return crate::reftable::reftable_append_reflog(
            git_dir,
            refname,
            old_oid,
            new_oid,
            identity,
            message,
            force_create,
        );
    }
    let storage_dir = ref_storage_dir(git_dir, refname);
    let log_path = storage_dir.join("logs").join(refname);
    let may_write =
        force_create || should_autocreate_reflog(git_dir, refname) || !message.is_empty();
    if !may_write && !log_path.exists() {
        return Ok(());
    }
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = if message.is_empty() {
        format!("{old_oid} {new_oid} {identity}\n")
    } else {
        format!("{old_oid} {new_oid} {identity}\t{message}\n")
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    use io::Write;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn ref_storage_dir(git_dir: &Path, refname: &str) -> PathBuf {
    // Per-worktree refs live under this worktree's git dir; shared refs (including
    // `refs/bisect/*`) live in the common repository directory so all worktrees
    // see the same bisection state.
    if refname == "HEAD" || refname == "NOTES_MERGE_PARTIAL" || refname == "NOTES_MERGE_REF" {
        return git_dir.to_path_buf();
    }
    common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf())
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
    // Merge packed + loose so **loose always wins** for the same ref name (matches Git and
    // `resolve_ref`). Previously we concatenated packed then loose and never deduplicated the
    // main git dir case, so `pack-refs` could leave stale packed lines that shadowed updates.
    let mut by_name: HashMap<String, ObjectId> = HashMap::new();

    if let Some(cdir) = common_dir(git_dir) {
        if cdir != git_dir {
            collect_packed_refs_into_map(&cdir, prefix, &mut by_name)?;
            let cbase = cdir.join(prefix);
            collect_loose_refs_into_map(&cbase, prefix, &cdir, &mut by_name)?;
        }
    }

    collect_packed_refs_into_map(git_dir, prefix, &mut by_name)?;
    let base = git_dir.join(prefix);
    collect_loose_refs_into_map(&base, prefix, git_dir, &mut by_name)?;

    let mut results: Vec<(String, ObjectId)> = by_name.into_iter().collect();
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

fn collect_loose_refs_into_map(
    dir: &Path,
    prefix: &str,
    resolve_git_dir: &Path,
    out: &mut HashMap<String, ObjectId>,
) -> Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Error::Io(e)),
    };

    for entry in read {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let refname = format!("{prefix}{name_str}");
        let path = entry.path();
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_dir() {
            collect_loose_refs_into_map(&path, &format!("{refname}/"), resolve_git_dir, out)?;
        } else if meta.is_file() {
            if let Ok(oid) = resolve_ref(resolve_git_dir, &refname) {
                out.insert(refname, oid);
            }
        }
    }
    Ok(())
}

/// Resolve `@{-N}` syntax to the branch name (not an OID).
/// Returns the branch name of the Nth previously checked out branch.
pub fn resolve_at_n_branch(git_dir: &Path, spec: &str) -> Result<String> {
    // Parse the N from @{-N}
    let inner = spec
        .strip_prefix("@{-")
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| Error::InvalidRef(format!("not an @{{-N}} ref: {spec}")))?;
    let n: usize = inner
        .parse()
        .map_err(|_| Error::InvalidRef(format!("invalid N in {spec}")))?;
    if n == 0 {
        return Err(Error::InvalidRef("@{-0} is not valid".to_string()));
    }
    let entries = crate::reflog::read_reflog(git_dir, "HEAD")?;
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
        "{spec}: only {count} checkout(s) in reflog"
    )))
}

fn collect_packed_refs_into_map(
    git_dir: &Path,
    prefix: &str,
    out: &mut HashMap<String, ObjectId>,
) -> Result<()> {
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
        out.insert(refname.to_string(), oid);
    }
    Ok(())
}

#[cfg(test)]
mod refname_available_tests {
    use super::*;
    use std::collections::{BTreeSet, HashSet};
    use tempfile::tempdir;

    #[test]
    fn loose_parent_blocks_child_create() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/1l")).unwrap();
        fs::write(
            git_dir.join("refs/1l/c"),
            "67bf698f3ab735e92fb011a99cff3497c44d30c1\n",
        )
        .unwrap();
        assert_eq!(
            read_raw_ref(git_dir, "refs/1l/c").unwrap(),
            RawRefLookup::Exists
        );
        let extras = BTreeSet::from([
            "refs/1l/b".to_string(),
            "refs/1l/c/x".to_string(),
            "refs/1l/d".to_string(),
        ]);
        let skip = HashSet::new();
        let err = verify_refname_available_for_create(git_dir, "refs/1l/c/x", &extras, &skip)
            .unwrap_err();
        assert!(matches!(
            err,
            RefnameUnavailable::AncestorExists { ref blocking, .. } if blocking == "refs/1l/c"
        ));
    }

    #[test]
    fn verify_sees_loose_ref_after_canonical_git_dir() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(git_dir.join("refs/1l")).unwrap();
        fs::write(
            git_dir.join("refs/1l/c"),
            "67bf698f3ab735e92fb011a99cff3497c44d30c1\n",
        )
        .unwrap();
        let skip = HashSet::new();
        let extras = BTreeSet::new();
        let err = verify_refname_available_for_create(&git_dir, "refs/1l/c/x", &extras, &skip)
            .unwrap_err();
        assert!(matches!(
            err,
            RefnameUnavailable::AncestorExists { ref blocking, .. } if blocking == "refs/1l/c"
        ));
    }

    #[test]
    fn list_refs_finds_sibling_under_parent_directory() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/ns/p")).unwrap();
        fs::write(
            git_dir.join("refs/ns/p/x"),
            "67bf698f3ab735e92fb011a99cff3497c44d30c1\n",
        )
        .unwrap();
        let listed = list_refs(git_dir, "refs/ns/p/").unwrap();
        assert!(
            listed.iter().any(|(n, _)| n == "refs/ns/p/x"),
            "got {listed:?}"
        );
    }

    #[test]
    fn verify_blocks_parent_when_child_ref_exists() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/ns/p")).unwrap();
        fs::write(
            git_dir.join("refs/ns/p/x"),
            "67bf698f3ab735e92fb011a99cff3497c44d30c1\n",
        )
        .unwrap();
        let extras = BTreeSet::from(["refs/ns/p".to_string()]);
        let skip = HashSet::new();
        let err =
            verify_refname_available_for_create(git_dir, "refs/ns/p", &extras, &skip).unwrap_err();
        assert!(matches!(
            err,
            RefnameUnavailable::DescendantExists { ref blocking, .. }
                if blocking == "refs/ns/p/x"
        ));
    }

    #[test]
    fn verify_blocks_parent_git_style_nested_path() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/3l/c")).unwrap();
        fs::write(
            git_dir.join("refs/3l/c/x"),
            "67bf698f3ab735e92fb011a99cff3497c44d30c1\n",
        )
        .unwrap();
        let extras = BTreeSet::from(["refs/3l/c".to_string()]);
        let skip = HashSet::new();
        let err =
            verify_refname_available_for_create(git_dir, "refs/3l/c", &extras, &skip).unwrap_err();
        assert!(matches!(
            err,
            RefnameUnavailable::DescendantExists { ref blocking, .. }
                if blocking == "refs/3l/c/x"
        ));
    }

    #[test]
    fn intermediate_directory_does_not_block_nested_create() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/ns")).unwrap();
        fs::write(
            git_dir.join("refs/ns/existing"),
            "67bf698f3ab735e92fb011a99cff3497c44d30c1\n",
        )
        .unwrap();
        assert_eq!(
            read_raw_ref(git_dir, "refs/ns").unwrap(),
            RawRefLookup::IsDirectory
        );
        let extras = BTreeSet::from(["refs/ns/newchild".to_string()]);
        let skip = HashSet::new();
        verify_refname_available_for_create(git_dir, "refs/ns/newchild", &extras, &skip).unwrap();
    }
}

#[cfg(test)]
mod read_raw_ref_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loose_ref_file_is_exists() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        fs::write(
            git_dir.join("refs/heads/side"),
            "0000000000000000000000000000000000000000\n",
        )
        .unwrap();
        assert_eq!(
            read_raw_ref(git_dir, "refs/heads/side").unwrap(),
            RawRefLookup::Exists
        );
    }

    #[test]
    fn missing_ref_is_not_found() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        assert_eq!(
            read_raw_ref(git_dir, "refs/heads/nope").unwrap(),
            RawRefLookup::NotFound
        );
    }

    #[test]
    fn directory_where_ref_expected_is_is_directory() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        assert_eq!(
            read_raw_ref(git_dir, "refs/heads").unwrap(),
            RawRefLookup::IsDirectory
        );
    }

    #[test]
    fn packed_ref_name_is_exists() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path();
        fs::write(
            git_dir.join("packed-refs"),
            "# pack-refs with: peeled fully-peeled \n\
             0000000000000000000000000000000000000000 refs/heads/packed\n",
        )
        .unwrap();
        assert_eq!(
            read_raw_ref(git_dir, "refs/heads/packed").unwrap(),
            RawRefLookup::Exists
        );
    }
}
