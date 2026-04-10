//! Git index (staging area) reading and writing.
//!
//! The index file (`.git/index`) stores the current state of the staging area.
//! It uses a binary format with a 12-byte header, fixed-size index entries,
//! and optional extensions, followed by a trailing SHA-1 over the whole file.
//!
//! # Format version
//!
//! This implementation supports index versions 2 and 3. Requests for version 4
//! currently fall back to a non-compressed index on write because path
//! compression is not yet implemented.
//!
//! # References
//!
//! See `Documentation/technical/index-format.txt` in the Git source tree for
//! the authoritative format specification.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::config::ConfigSet;
use crate::error::{Error, Result};
use crate::objects::{parse_tree, ObjectId, ObjectKind, TreeEntry};
use crate::odb::Odb;
use crate::repo::Repository;
use crate::resolve_undo::{self, write_resolve_undo_payload, ResolveUndoRecord};
use crate::rev_parse;
use crate::untracked_cache;

/// File mode for a regular (non-executable) file.
pub const MODE_REGULAR: u32 = 0o100644;
/// File mode for an executable file.
pub const MODE_EXECUTABLE: u32 = 0o100755;
/// File mode for a symbolic link.
pub const MODE_SYMLINK: u32 = 0o120000;
/// File mode for a gitlink (submodule).
pub const MODE_GITLINK: u32 = 0o160000;
/// File mode for a directory (tree) entry — only used in tree objects, not index.
pub const MODE_TREE: u32 = 0o040000;

/// Git index extension signature `sdir` (sparse directory entries present).
const INDEX_EXT_SPARSE_DIRECTORIES: u32 = u32::from_be_bytes(*b"sdir");
/// Git index extension signature `UNTR` (untracked cache).
const INDEX_EXT_UNTRACKED: u32 = u32::from_be_bytes(*b"UNTR");
/// Git index extension signature `REUC` (resolve undo).
const INDEX_EXT_RESOLVE_UNDO: u32 = u32::from_be_bytes(*b"REUC");

/// A single entry in the Git index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Time the file metadata last changed (seconds since epoch).
    pub ctime_sec: u32,
    /// Nanosecond fraction of `ctime_sec`.
    pub ctime_nsec: u32,
    /// Time the file data last changed (seconds since epoch).
    pub mtime_sec: u32,
    /// Nanosecond fraction of `mtime_sec`.
    pub mtime_nsec: u32,
    /// Device number.
    pub dev: u32,
    /// Inode number.
    pub ino: u32,
    /// Unix file mode (`MODE_REGULAR`, `MODE_EXECUTABLE`, `MODE_SYMLINK`, …).
    pub mode: u32,
    /// Owner UID.
    pub uid: u32,
    /// Owner GID.
    pub gid: u32,
    /// File size in bytes (truncated to 32 bits).
    pub size: u32,
    /// SHA-1 of the blob object.
    pub oid: ObjectId,
    /// Entry flags (stage, assume-valid, extended, …).
    pub flags: u16,
    /// Extended flags (v3+ only).
    pub flags_extended: Option<u16>,
    /// Path relative to the repository root.  May contain `/` separators.
    pub path: Vec<u8>,
}

impl IndexEntry {
    /// Merge stage (0 = normal, 1–3 = conflict stages).
    #[must_use]
    pub fn stage(&self) -> u8 {
        ((self.flags >> 12) & 0x3) as u8
    }

    pub(crate) fn set_stage(&mut self, stage: u8) {
        self.flags = (self.flags & 0x0FFF) | ((stage as u16 & 0x3) << 12);
    }

    /// Whether the assume-unchanged bit is set.
    #[must_use]
    pub fn assume_unchanged(&self) -> bool {
        self.flags & 0x8000 != 0
    }

    /// Whether the skip-worktree bit is set (extended flags, v3+).
    #[must_use]
    pub fn skip_worktree(&self) -> bool {
        self.flags_extended
            .map(|f| f & 0x4000 != 0)
            .unwrap_or(false)
    }

    /// Set the assume-unchanged bit.
    pub fn set_assume_unchanged(&mut self, value: bool) {
        if value {
            self.flags |= 0x8000;
        } else {
            self.flags &= !0x8000;
        }
    }

    /// Set the skip-worktree bit (promotes entry to v3).
    pub fn set_skip_worktree(&mut self, value: bool) {
        let fe = self.flags_extended.get_or_insert(0);
        if value {
            *fe |= 0x4000;
        } else {
            *fe &= !0x4000;
            if *fe == 0 {
                self.flags_extended = None;
            }
        }
    }

    /// Whether the intent-to-add bit is set (extended flags, v3+).
    #[must_use]
    pub fn intent_to_add(&self) -> bool {
        self.flags_extended
            .map(|f| f & 0x2000 != 0)
            .unwrap_or(false)
    }

    /// Set the intent-to-add bit (promotes entry to v3).
    pub fn set_intent_to_add(&mut self, value: bool) {
        let fe = self.flags_extended.get_or_insert(0);
        if value {
            *fe |= 0x2000;
        } else {
            *fe &= !0x2000;
            if *fe == 0 {
                self.flags_extended = None;
            }
        }
    }

    /// Sparse-index placeholder: tree mode, stage 0, and `SKIP_WORKTREE` set.
    #[must_use]
    pub fn is_sparse_directory_placeholder(&self) -> bool {
        self.mode == MODE_TREE && self.stage() == 0 && self.skip_worktree()
    }

    /// In-memory only: `ls-files --with-tree` hides stage-1 overlay rows that duplicate stage 0.
    const FLAG_EXT_OVERLAY_TREE_SKIP: u16 = 0x8000;

    #[must_use]
    pub fn overlay_tree_skip_output(&self) -> bool {
        self.flags_extended
            .is_some_and(|fe| fe & Self::FLAG_EXT_OVERLAY_TREE_SKIP != 0)
    }

    fn set_overlay_tree_skip_output(&mut self, value: bool) {
        let fe = self.flags_extended.get_or_insert(0);
        if value {
            *fe |= Self::FLAG_EXT_OVERLAY_TREE_SKIP;
        } else {
            *fe &= !Self::FLAG_EXT_OVERLAY_TREE_SKIP;
            if *fe == 0 {
                self.flags_extended = None;
            }
        }
    }
}

/// The in-memory representation of the Git index file.
#[derive(Debug, Clone, Default)]
pub struct Index {
    /// Index format version (2 or 3).
    pub version: u32,
    /// Index entries, sorted by (path, stage).
    pub entries: Vec<IndexEntry>,
    /// When true, the on-disk index includes the `sdir` extension (sparse index).
    pub sparse_directories: bool,
    /// Optional untracked-cache extension (`UNTR`), matching Git's `istate->untracked`.
    pub untracked_cache: Option<untracked_cache::UntrackedCache>,
    /// Optional `REUC` resolve-undo extension (paths that were unmerged before a resolution).
    pub resolve_undo: Option<BTreeMap<Vec<u8>, ResolveUndoRecord>>,
}

/// Options for loading an index from disk.
#[derive(Debug, Clone, Copy)]
pub struct IndexLoadOptions {
    /// If the index contains sparse directory placeholders, expand them to full file entries.
    pub expand_sparse_directories: bool,
}

impl Default for IndexLoadOptions {
    fn default() -> Self {
        Self {
            expand_sparse_directories: true,
        }
    }
}

/// Version used after an invalid `GIT_INDEX_VERSION` value (matches Git stderr: "Using version 3").
const INDEX_ENV_INVALID_FALLBACK: u32 = 3;
/// Version used after an invalid `index.version` config value (same message as env).
const INDEX_CONFIG_INVALID_FALLBACK: u32 = 3;
/// Minimum supported index version.
const INDEX_FORMAT_LB: u32 = 2;
/// Maximum supported index version (version 4 requests are accepted and
/// downgraded on write).
const INDEX_FORMAT_UB: u32 = 4;

/// Read `GIT_INDEX_VERSION` and return the requested version.
///
/// If the environment variable is unset, returns `None`.
/// If it is set but invalid (non-numeric or out of range 2..=4), prints a
/// warning to stderr and returns the default version.
pub fn get_index_format_from_env() -> Option<u32> {
    let val = std::env::var("GIT_INDEX_VERSION").ok()?;
    if val.is_empty() {
        return None;
    }
    match val.parse::<u32>() {
        Ok(v) if (INDEX_FORMAT_LB..=INDEX_FORMAT_UB).contains(&v) => Some(v),
        _ => {
            eprintln!(
                "warning: GIT_INDEX_VERSION set, but the value is invalid.\n\
                 Using version {INDEX_ENV_INVALID_FALLBACK}"
            );
            Some(INDEX_ENV_INVALID_FALLBACK)
        }
    }
}

impl Index {
    /// Create a new, empty index.
    ///
    /// Respects `GIT_INDEX_VERSION` if set, otherwise defaults to version 2.
    #[must_use]
    pub fn new() -> Self {
        let version = get_index_format_from_env().unwrap_or(2);
        Self {
            version,
            entries: Vec::new(),
            sparse_directories: false,
            untracked_cache: None,
            resolve_undo: None,
        }
    }

    /// Create a new empty index, respecting config values for version.
    ///
    /// Priority matches Git's `prepare_repo_settings`: `GIT_INDEX_VERSION` env, then
    /// `feature.manyFiles` (implies version 4), then `index.version` (overrides version).
    pub fn new_with_config(
        config_index_version: Option<&str>,
        config_many_files: Option<&str>,
    ) -> Self {
        if let Some(v) = get_index_format_from_env() {
            return Self {
                version: v,
                entries: Vec::new(),
                sparse_directories: false,
                untracked_cache: None,
                resolve_undo: None,
            };
        }

        let many_files = config_truthy(config_many_files);
        let mut version = if many_files { 4 } else { 2 };

        if let Some(val) = config_index_version {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<u32>() {
                    Ok(v) if (INDEX_FORMAT_LB..=INDEX_FORMAT_UB).contains(&v) => {
                        version = v;
                    }
                    _ => {
                        eprintln!(
                            "warning: index.version set, but the value is invalid.\n\
                             Using version {INDEX_CONFIG_INVALID_FALLBACK}"
                        );
                        version = INDEX_CONFIG_INVALID_FALLBACK;
                    }
                }
            }
        }

        Self {
            version,
            entries: Vec::new(),
            sparse_directories: false,
            untracked_cache: None,
            resolve_undo: None,
        }
    }

    /// New empty index using a loaded [`ConfigSet`] (includes `-c` / `GIT_CONFIG_PARAMETERS`).
    ///
    /// Same precedence as [`Self::new_with_config`], but reads `feature.manyFiles` and
    /// `index.version` from `config`.
    #[must_use]
    pub fn new_from_config(config: &ConfigSet) -> Self {
        if let Some(v) = get_index_format_from_env() {
            return Self {
                version: v,
                entries: Vec::new(),
                sparse_directories: false,
                untracked_cache: None,
                resolve_undo: None,
            };
        }

        let many_files = config
            .get_bool("feature.manyFiles")
            .and_then(|r| r.ok())
            .unwrap_or(false);
        let mut version = if many_files { 4 } else { 2 };

        if let Some(val) = config.get("index.version") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<u32>() {
                    Ok(v) if (INDEX_FORMAT_LB..=INDEX_FORMAT_UB).contains(&v) => {
                        version = v;
                    }
                    _ => {
                        eprintln!(
                            "warning: index.version set, but the value is invalid.\n\
                             Using version {INDEX_CONFIG_INVALID_FALLBACK}"
                        );
                        version = INDEX_CONFIG_INVALID_FALLBACK;
                    }
                }
            }
        }

        Self {
            version,
            entries: Vec::new(),
            sparse_directories: false,
            untracked_cache: None,
            resolve_undo: None,
        }
    }

    /// Load an index from the given file path without expanding sparse-directory placeholders.
    ///
    /// Returns an empty index if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IndexError`] if the file is present but corrupt.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(data) => Self::parse(&data),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self {
                sparse_directories: false,
                ..Self::new()
            }),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Load an index and expand sparse-directory placeholders using the object database.
    ///
    /// After a successful return, [`Index::sparse_directories`] is cleared and every
    /// placeholder is replaced by the blob entries from the referenced tree.
    pub fn load_expand_sparse(path: &Path, odb: &Odb) -> Result<Self> {
        let mut idx = Self::load(path)?;
        idx.expand_sparse_directory_placeholders(odb)?;
        Ok(idx)
    }

    /// Like [`Index::load_expand_sparse`], but treats a missing index or Git's
    /// `"file too short"` placeholder as an empty index.
    pub fn load_expand_sparse_optional(path: &Path, odb: &Odb) -> Result<Self> {
        let mut idx = match fs::read(path) {
            Ok(data) => Self::parse(&data).or_else(|e| match e {
                Error::IndexError(msg) if msg == "file too short" => Ok(Self::new()),
                other => Err(other),
            })?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::new(),
            Err(e) => return Err(Error::Io(e)),
        };
        idx.expand_sparse_directory_placeholders(odb)?;
        Ok(idx)
    }

    /// Returns true if the index contains sparse-index tree placeholders (`MODE_TREE` + skip-worktree).
    #[must_use]
    pub fn has_sparse_directory_placeholders(&self) -> bool {
        self.entries
            .iter()
            .any(IndexEntry::is_sparse_directory_placeholder)
    }

    /// Replace sparse-directory placeholder entries with all blob paths from their trees.
    ///
    /// Each placeholder must reference a tree object. New entries are marked skip-worktree like Git's
    /// expanded index, except we keep `sparse_directories` false in memory after expansion.
    pub fn expand_sparse_directory_placeholders(&mut self, odb: &Odb) -> Result<()> {
        if !self.has_sparse_directory_placeholders() {
            return Ok(());
        }
        let mut out: Vec<IndexEntry> = Vec::with_capacity(self.entries.len());
        for entry in self.entries.drain(..) {
            if entry.is_sparse_directory_placeholder() {
                let prefix = trim_trailing_slash_bytes(&entry.path);
                let blobs = flatten_tree_blobs(odb, &entry.oid, prefix)?;
                out.extend(blobs);
            } else {
                out.push(entry);
            }
        }
        self.entries = out;
        self.sparse_directories = false;
        self.sort();
        Ok(())
    }

    /// Collapse consecutive skip-worktree subtrees into sparse-directory placeholders when
    /// `cone_mode` is true and each directory is outside the sparse cone.
    ///
    /// `head_tree` is the tree OID at `HEAD`. When `enable_sparse_index` is false, clears
    /// [`Index::sparse_directories`] and returns without collapsing.
    pub fn try_collapse_sparse_directories(
        &mut self,
        odb: &Odb,
        head_tree: &ObjectId,
        patterns: &[String],
        cone_mode: bool,
        enable_sparse_index: bool,
    ) -> Result<()> {
        if !enable_sparse_index || !cone_mode {
            self.sparse_directories = false;
            return Ok(());
        }

        let mut prefixes = BTreeSet::<Vec<u8>>::new();
        for e in &self.entries {
            if e.stage() != 0 || e.mode == MODE_TREE || !e.skip_worktree() {
                continue;
            }
            collect_directory_prefixes(&e.path, &mut prefixes);
        }

        let mut collapsed_any = false;
        // Deepest prefixes first so nested dirs collapse before parents.
        let mut ordered: Vec<Vec<u8>> = prefixes.into_iter().collect();
        ordered.sort_by_key(|p| std::cmp::Reverse(p.len()));

        for pref in ordered {
            let pref_str = String::from_utf8_lossy(&pref);
            if directory_in_cone(&pref_str, patterns, cone_mode) {
                continue;
            }
            let Some(subtree_oid) = tree_oid_for_prefix(odb, head_tree, &pref)? else {
                continue;
            };
            let expected = collect_sparse_aware_expected_blobs(
                odb,
                &subtree_oid,
                &pref,
                patterns,
                cone_mode,
                &self.entries,
            )?;
            if expected.is_empty() {
                continue;
            }
            let mut matched = Vec::new();
            for e in &self.entries {
                if e.stage() != 0 {
                    continue;
                }
                if path_under_prefix(&e.path, &pref) && e.mode != MODE_TREE {
                    matched.push(e.clone());
                }
            }
            if matched.len() != expected.len() {
                continue;
            }
            matched.sort_by(|a, b| a.path.cmp(&b.path));
            let mut exp_sorted = expected;
            exp_sorted.sort_by(|a, b| a.path.cmp(&b.path));
            if !matched
                .iter()
                .zip(exp_sorted.iter())
                .all(|(a, b)| a.path == b.path && a.oid == b.oid && a.mode == b.mode)
            {
                continue;
            }
            if !matched.iter().all(|e| e.skip_worktree()) {
                continue;
            }

            let mut path_with_slash = pref.clone();
            if !path_with_slash.ends_with(b"/") {
                path_with_slash.push(b'/');
            }
            self.entries
                .retain(|e| e.stage() != 0 || !path_under_prefix(&e.path, &pref));
            let mut placeholder = IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: MODE_TREE,
                uid: 0,
                gid: 0,
                size: 0,
                oid: subtree_oid,
                flags: path_with_slash.len().min(0xFFF) as u16,
                flags_extended: Some(0),
                path: path_with_slash,
            };
            placeholder.set_skip_worktree(true);
            self.add_or_replace(placeholder);
            collapsed_any = true;
        }

        if collapsed_any {
            self.sort();
            self.sparse_directories = true;
        } else {
            self.sparse_directories = false;
        }
        Ok(())
    }

    /// Parse index bytes (the whole file including trailing SHA-1).
    ///
    /// # Errors
    ///
    /// Returns [`Error::IndexError`] on structural problems.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(Error::IndexError("file too short".to_owned()));
        }

        // Trailing SHA-1: normal index is a hash of the body; Git may write all zeros when
        // `index.skipHash` / `feature.manyFiles` skips computing the checksum.
        let (body, checksum) = data.split_at(data.len() - 20);
        if !checksum.iter().all(|&b| b == 0) {
            let mut hasher = Sha1::new();
            hasher.update(body);
            let computed = hasher.finalize();
            if computed.as_slice() != checksum {
                return Err(Error::IndexError("SHA-1 checksum mismatch".to_owned()));
            }
        }

        // Header
        let magic = &body[..4];
        if magic != b"DIRC" {
            return Err(Error::IndexError("bad magic: expected DIRC".to_owned()));
        }
        let version = u32::from_be_bytes(
            body[4..8]
                .try_into()
                .map_err(|_| Error::IndexError("cannot read version".to_owned()))?,
        );
        if version != 2 && version != 3 && version != 4 {
            return Err(Error::IndexError(format!(
                "unsupported index version {version}"
            )));
        }
        let count = u32::from_be_bytes(
            body[8..12]
                .try_into()
                .map_err(|_| Error::IndexError("cannot read entry count".to_owned()))?,
        );

        let mut pos = 12usize;
        let mut entries = Vec::with_capacity(count as usize);

        let mut prev_path: Vec<u8> = Vec::new();
        for _ in 0..count {
            let (entry, consumed) = parse_entry(&body[pos..], version, &prev_path)?;
            prev_path = entry.path.clone();
            entries.push(entry);
            pos += consumed;
        }

        let mut sparse_directories = false;
        let mut untracked_cache = None;
        let mut resolve_undo = None;
        while pos + 8 <= body.len() {
            let sig = u32::from_be_bytes(
                body[pos..pos + 4]
                    .try_into()
                    .map_err(|_| Error::IndexError("truncated extension sig".to_owned()))?,
            );
            let ext_sz = u32::from_be_bytes(
                body[pos + 4..pos + 8]
                    .try_into()
                    .map_err(|_| Error::IndexError("truncated extension size".to_owned()))?,
            ) as usize;
            pos += 8;
            if pos + ext_sz > body.len() {
                return Err(Error::IndexError(
                    "extension overruns index body".to_owned(),
                ));
            }
            if sig == INDEX_EXT_SPARSE_DIRECTORIES {
                sparse_directories = true;
            } else if sig == INDEX_EXT_UNTRACKED {
                let ext_data = &body[pos..pos + ext_sz];
                untracked_cache = untracked_cache::parse_untracked_extension(ext_data);
            } else if sig == INDEX_EXT_RESOLVE_UNDO {
                let ext_data = &body[pos..pos + ext_sz];
                resolve_undo = Some(resolve_undo::parse_resolve_undo_payload(ext_data)?);
            }
            pos += ext_sz;
        }
        if pos != body.len() {
            return Err(Error::IndexError("junk after index extensions".to_owned()));
        }

        Ok(Self {
            version,
            entries,
            sparse_directories,
            untracked_cache,
            resolve_undo,
        })
    }

    /// Write the index to a file, computing and appending the trailing SHA-1.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on filesystem errors.
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut sorted = self.clone();
        sorted.sort();

        let mut body = Vec::new();
        sorted.serialize_into(&mut body)?;

        let git_dir = path.parent();
        let config = git_dir.and_then(|d| ConfigSet::load(Some(d), true).ok());
        let skip_hash = index_skip_hash_for_write(config.as_ref());
        let checksum: [u8; 20] = if skip_hash {
            [0u8; 20]
        } else {
            let mut hasher = Sha1::new();
            hasher.update(&body);
            hasher.finalize().into()
        };

        let tmp_path = path.with_extension("lock");
        let pid_path = pid_path_for_lock(&tmp_path);
        let lockfile_pid_enabled = lockfile_pid_enabled(path);

        let mut lock_file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let message = build_lock_exists_message(&tmp_path, &pid_path, &e);
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    message,
                )));
            }
            Err(e) => return Err(Error::Io(e)),
        };

        let mut wrote_pid_file = false;
        if lockfile_pid_enabled {
            if let Err(e) = write_lock_pid_file(&pid_path) {
                let _ = fs::remove_file(&tmp_path);
                return Err(Error::Io(e));
            }
            wrote_pid_file = true;
        }

        if let Err(e) = (|| -> io::Result<()> {
            lock_file.write_all(&body)?;
            lock_file.write_all(&checksum)?;
            Ok(())
        })() {
            let _ = fs::remove_file(&tmp_path);
            if wrote_pid_file {
                let _ = fs::remove_file(&pid_path);
            }
            return Err(Error::Io(e));
        }
        drop(lock_file);

        if let Err(e) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            if wrote_pid_file {
                let _ = fs::remove_file(&pid_path);
            }
            return Err(Error::Io(e));
        }
        {
            if wrote_pid_file {
                let _ = fs::remove_file(&pid_path);
            }
        }
        Ok(())
    }

    /// Serialise the index body (without trailing checksum) into `out`.
    ///
    /// Callers must have sorted entries when using format 4 (path compression depends on order).
    fn serialize_into(&self, out: &mut Vec<u8>) -> Result<()> {
        let has_extended_flags = self.entries.iter().any(|e| e.flags_extended.is_some());
        let write_version = if self.version >= 4 {
            4
        } else if has_extended_flags {
            3
        } else if self.version >= 3 {
            2
        } else {
            self.version
        };
        // Header
        out.extend_from_slice(b"DIRC");
        out.extend_from_slice(&write_version.to_be_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

        if write_version == 4 {
            let mut previous_path: Vec<u8> = Vec::new();
            for entry in &self.entries {
                serialize_entry_v4(entry, &mut previous_path, out);
            }
        } else {
            for entry in &self.entries {
                serialize_entry(entry, write_version, out);
            }
        }
        if self.sparse_directories {
            out.extend_from_slice(&INDEX_EXT_SPARSE_DIRECTORIES.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
        }
        if let Some(uc) = &self.untracked_cache {
            let payload = untracked_cache::write_untracked_extension(uc);
            out.extend_from_slice(&INDEX_EXT_UNTRACKED.to_be_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            out.extend_from_slice(&payload);
        }
        if let Some(ru) = &self.resolve_undo {
            let payload = write_resolve_undo_payload(ru);
            if !payload.is_empty() {
                out.extend_from_slice(&INDEX_EXT_RESOLVE_UNDO.to_be_bytes());
                out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                out.extend_from_slice(&payload);
            }
        }
        Ok(())
    }

    /// Add or replace an entry (matched by path + stage).
    pub fn add_or_replace(&mut self, entry: IndexEntry) {
        let path = entry.path.clone();
        let stage = entry.stage();
        // Binary search for the insertion point by (path, stage)
        let result = self.entries.binary_search_by(|e| {
            e.path
                .as_slice()
                .cmp(path.as_slice())
                .then_with(|| e.stage().cmp(&stage))
        });
        match result {
            Ok(pos) => {
                // Exact match — replace in place
                self.entries[pos] = entry;
            }
            Err(pos) => {
                // Not found — insert at sorted position
                self.entries.insert(pos, entry);
            }
        }
        if stage == 0 {
            if let Ok(p) = std::str::from_utf8(&path) {
                self.invalidate_untracked_cache_for_path(p);
            }
        }
    }

    /// Stage a file at stage 0, removing any conflict stage entries (1, 2, 3)
    /// for the same path. This is the correct behavior for `git add` on a
    /// conflicted file during merge/cherry-pick resolution.
    pub fn stage_file(&mut self, entry: IndexEntry) {
        let path = entry.path.clone();
        for e in &self.entries {
            if e.path == path && e.stage() != 0 {
                resolve_undo::record_resolve_undo_for_entry(&mut self.resolve_undo, e);
            }
        }
        // Remove conflict stages first
        self.entries.retain(|e| e.path != path || e.stage() == 0);
        // Then add/replace stage-0 entry
        self.add_or_replace(entry);
    }

    /// Drop all resolve-undo records (matches Git `resolve_undo_clear_index`).
    pub fn clear_resolve_undo(&mut self) {
        self.resolve_undo = None;
    }

    /// Remove and return the resolve-undo record for `path`, if any.
    pub fn take_resolve_undo_record(&mut self, path: &[u8]) -> Option<ResolveUndoRecord> {
        let map = self.resolve_undo.as_mut()?;
        let ru = map.remove(path)?;
        if map.is_empty() {
            self.resolve_undo = None;
        }
        Some(ru)
    }

    /// Replace all index entries for `path` with unmerged stages from `record`.
    pub fn install_unmerged_from_resolve_undo(&mut self, path: &[u8], record: &ResolveUndoRecord) {
        self.entries.retain(|e| e.path != path);
        for stage in 1u8..=3u8 {
            let i = (stage - 1) as usize;
            if record.modes[i] == 0 {
                continue;
            }
            let entry = IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: record.modes[i],
                uid: 0,
                gid: 0,
                size: 0,
                oid: record.oids[i],
                flags: path.len().min(0xFFF) as u16 | ((stage as u16) << 12),
                flags_extended: None,
                path: path.to_vec(),
            };
            self.add_or_replace(entry);
        }
        self.sort();
    }

    /// Re-create unmerged index entries for `path` from the resolve-undo extension.
    ///
    /// Returns `true` when a resolve-undo record existed and was consumed (Git `unmerge_one`).
    pub fn unmerge_path_from_resolve_undo(&mut self, path: &[u8]) -> bool {
        let Some(record) = self.take_resolve_undo_record(path) else {
            return false;
        };
        self.install_unmerged_from_resolve_undo(path, &record);
        true
    }

    /// Remove all entries matching the given path (all stages).
    ///
    /// Returns `true` if at least one entry was removed.
    pub fn remove(&mut self, path: &[u8]) -> bool {
        let mut removed_any = false;
        for e in &self.entries {
            if e.path == path {
                if e.stage() != 0 {
                    resolve_undo::record_resolve_undo_for_entry(&mut self.resolve_undo, e);
                }
                removed_any = true;
            }
        }
        if !removed_any {
            return false;
        }
        self.entries.retain(|e| e.path != path);
        if let Ok(p) = std::str::from_utf8(path) {
            self.invalidate_untracked_cache_for_path(p);
        }
        true
    }

    /// Remove every index entry for `path` (all merge stages), like `remove_file_from_index`.
    ///
    /// Returns whether any entry was removed.
    pub fn remove_path_all_stages(&mut self, path: &[u8]) -> bool {
        self.remove(path)
    }

    /// Invalidate UNTR nodes affected by an index change (Git `untracked_cache_*_index`).
    pub fn invalidate_untracked_cache_for_path(&mut self, path: &str) {
        if let Some(uc) = self.untracked_cache.as_mut() {
            untracked_cache::invalidate_path(uc, path);
        }
    }

    /// Remove every index entry whose path lies strictly under `path` (all stages).
    ///
    /// Used when staging a file at `path` that replaces a former directory: Git removes
    /// tracked paths like `path/child` from the index so they do not remain alongside
    /// the new blob entry.
    pub fn remove_descendants_under_path(&mut self, path: &str) {
        let prefix = path.as_bytes();
        if prefix.is_empty() {
            return;
        }
        let plen = prefix.len();
        let had_descendant = self.entries.iter().any(|e| {
            let ep = e.path.as_slice();
            ep.len() > plen && ep.starts_with(prefix) && ep[plen] == b'/'
        });
        self.entries.retain(|e| {
            let ep = e.path.as_slice();
            if ep.len() <= plen {
                return true;
            }
            if !ep.starts_with(prefix) {
                return true;
            }
            // Drop paths strictly under `prefix/` (keep same-length prefix matches like "d-other").
            ep[plen] != b'/'
        });
        if had_descendant {
            self.invalidate_untracked_cache_for_path(path);
        }
    }

    /// Sort entries in Git's canonical order: by path, then by stage.
    pub fn sort(&mut self) {
        self.entries
            .sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.stage().cmp(&b.stage())));
    }

    /// Find an entry by path and stage (0 for normal entries).
    #[must_use]
    pub fn get(&self, path: &[u8], stage: u8) -> Option<&IndexEntry> {
        self.entries
            .iter()
            .find(|e| e.path == path && e.stage() == stage)
    }

    /// Find a mutable entry by path and stage.
    pub fn get_mut(&mut self, path: &[u8], stage: u8) -> Option<&mut IndexEntry> {
        self.entries
            .iter_mut()
            .find(|e| e.path == path && e.stage() == stage)
    }

    /// Merge tree contents from `treeish` into this index as virtual stage-1 entries, matching
    /// Git's `overlay_tree_on_index` used by `git ls-files --with-tree`.
    ///
    /// Existing unmerged entries (stages 1–3) are shifted to stage 3 so stage 1 is free for the
    /// overlay. Stage-1 paths that already exist at stage 0 are marked so `ls-files` can skip
    /// them (Git's `CE_UPDATE` on the stage-1 entry).
    ///
    /// # Parameters
    ///
    /// - `repo` — repository whose object database is used to read the tree.
    /// - `treeish` — revision or tree OID string (`HEAD`, `HEAD~1`, full SHA, etc.).
    /// - `prefix` — optional path prefix (bytes, no trailing slash except empty); only paths under
    ///   this prefix are considered from the tree. Pass empty slice for the full tree.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if `treeish` cannot be resolved, the tree cannot be read, or an object is
    /// missing from the ODB.
    pub fn overlay_tree_on_index(
        &mut self,
        repo: &Repository,
        treeish: &str,
        prefix: &[u8],
    ) -> Result<()> {
        let oid = rev_parse::resolve_revision(repo, treeish)?;
        let tree_oid = peel_to_tree_oid(repo, oid)?;
        for e in self.entries.iter_mut() {
            if e.stage() != 0 {
                e.set_stage(3);
            }
        }
        self.sort();
        let has_stage1 = self.entries.iter().any(|e| e.stage() == 1);
        let mut appended: Vec<IndexEntry> = Vec::new();
        read_tree_into_overlay(repo, &tree_oid, prefix, &[], has_stage1, &mut appended)?;
        for e in appended {
            self.add_or_replace(e);
        }
        if !has_stage1 {
            self.sort();
        }
        let mut last_stage0: Option<&[u8]> = None;
        for e in &mut self.entries {
            match e.stage() {
                0 => {
                    last_stage0 = Some(e.path.as_slice());
                }
                1 => {
                    if last_stage0.is_some_and(|p| p == e.path.as_slice()) {
                        e.set_overlay_tree_skip_output(true);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn peel_to_tree_oid(repo: &Repository, oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&oid)?;
    match obj.kind {
        ObjectKind::Tree => Ok(oid),
        ObjectKind::Commit => {
            let commit = crate::objects::parse_commit(&obj.data)?;
            Ok(commit.tree)
        }
        ObjectKind::Tag => {
            let tag = crate::objects::parse_tag(&obj.data)?;
            peel_to_tree_oid(repo, tag.object)
        }
        _ => Err(Error::ObjectNotFound(format!(
            "cannot peel {oid} to tree for --with-tree"
        ))),
    }
}

fn read_tree_into_overlay(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &[u8],
    rel_base: &[u8],
    use_replace_path: bool,
    out: &mut Vec<IndexEntry>,
) -> Result<()> {
    let obj = repo.odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Err(Error::ObjectNotFound(format!(
            "object {tree_oid} is not a tree"
        )));
    }
    let entries = parse_tree(&obj.data)?;
    for TreeEntry { mode, name, oid } in entries {
        if mode == MODE_TREE {
            let mut path = rel_base.to_vec();
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(&name);
            if !prefix_under_or_equal(prefix, &path) {
                continue;
            }
            read_tree_into_overlay(repo, &oid, prefix, &path, use_replace_path, out)?;
            continue;
        }
        if mode == MODE_GITLINK {
            continue;
        }
        let mut path = rel_base.to_vec();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(&name);
        if !prefix_under_or_equal(prefix, &path) {
            continue;
        }
        let entry = synthetic_stage1_index_entry(mode, &path, oid);
        if use_replace_path {
            if let Some(pos) = out.iter().position(|e| e.path == path && e.stage() == 1) {
                out[pos] = entry;
            } else {
                out.push(entry);
            }
        } else {
            out.push(entry);
        }
    }
    Ok(())
}

fn prefix_under_or_equal(prefix: &[u8], path: &[u8]) -> bool {
    if prefix.is_empty() {
        return true;
    }
    if path == prefix {
        return true;
    }
    path.len() > prefix.len() && path.starts_with(prefix) && path[prefix.len()] == b'/'
}

fn synthetic_stage1_index_entry(mode: u32, path: &[u8], oid: ObjectId) -> IndexEntry {
    let path_len = path.len().min(0xFFF) as u16;
    let flags = (1u16 << 12) | path_len;
    IndexEntry {
        ctime_sec: 0,
        ctime_nsec: 0,
        mtime_sec: 0,
        mtime_nsec: 0,
        dev: 0,
        ino: 0,
        mode,
        uid: 0,
        gid: 0,
        size: 0,
        oid,
        flags,
        flags_extended: None,
        path: path.to_vec(),
    }
}

fn config_truthy(raw: Option<&str>) -> bool {
    let Some(val) = raw else {
        return false;
    };
    let lowered = val.trim().to_lowercase();
    matches!(lowered.as_str(), "true" | "yes" | "1" | "on")
}

/// Whether to write 20 zero bytes instead of the SHA-1 of the index body.
///
/// Mirrors Git `prepare_repo_settings`: `feature.manyFiles` enables skip-hash unless
/// `index.skipHash` / `index.skiphash` is explicitly false; otherwise honor true `index.skipHash`.
fn index_skip_hash_for_write(config: Option<&ConfigSet>) -> bool {
    let Some(config) = config else {
        return false;
    };
    let many_files = config
        .get_bool("feature.manyFiles")
        .and_then(|r| r.ok())
        .unwrap_or(false);
    if many_files {
        if let Some(Ok(false)) = config.get_bool("index.skipHash") {
            return false;
        }
        if let Some(Ok(false)) = config.get_bool("index.skiphash") {
            return false;
        }
        return true;
    }
    for key in ["index.skipHash", "index.skiphash"] {
        if let Some(Ok(true)) = config.get_bool(key) {
            return true;
        }
    }
    false
}

fn trim_trailing_slash_bytes(path: &[u8]) -> &[u8] {
    path.strip_suffix(b"/").unwrap_or(path)
}

fn path_under_prefix(path: &[u8], prefix: &[u8]) -> bool {
    if path == prefix {
        return true;
    }
    if prefix.is_empty() {
        return true;
    }
    path.len() > prefix.len() && path.starts_with(prefix) && path[prefix.len()] == b'/'
}

fn directory_in_cone(dir_path: &str, patterns: &[String], cone_mode: bool) -> bool {
    crate::sparse_checkout::path_matches_sparse_patterns(dir_path, patterns, cone_mode)
}

fn collect_directory_prefixes(path: &[u8], out: &mut BTreeSet<Vec<u8>>) {
    for (i, &b) in path.iter().enumerate() {
        if b == b'/' {
            out.insert(path[..i].to_vec());
        }
    }
}

fn tree_oid_for_prefix(odb: &Odb, root_tree: &ObjectId, prefix: &[u8]) -> Result<Option<ObjectId>> {
    if prefix.is_empty() {
        return Ok(Some(*root_tree));
    }
    let pref_str = String::from_utf8_lossy(prefix);
    let components: Vec<&str> = pref_str.split('/').filter(|c| !c.is_empty()).collect();
    let mut current = *root_tree;
    for comp in components {
        let obj = odb.read(&current)?;
        if obj.kind != ObjectKind::Tree {
            return Ok(None);
        }
        let entries = parse_tree(&obj.data)?;
        let mut next = None;
        for e in entries {
            if e.name == comp.as_bytes() {
                if e.mode == MODE_TREE {
                    next = Some(e.oid);
                }
                break;
            }
        }
        current = match next {
            Some(o) => o,
            None => return Ok(None),
        };
    }
    Ok(Some(current))
}

/// Build the list of blob index entries under `prefix` that match `HEAD` at `tree_oid`,
/// treating existing sparse-directory placeholders in `entries` as opaque subtrees (like Git).
fn collect_sparse_aware_expected_blobs(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &[u8],
    patterns: &[String],
    cone_mode: bool,
    entries: &[IndexEntry],
) -> Result<Vec<IndexEntry>> {
    let mut out = Vec::new();
    walk_sparse_aware(
        odb, tree_oid, prefix, patterns, cone_mode, entries, &mut out,
    )?;
    Ok(out)
}

fn walk_sparse_aware(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &[u8],
    patterns: &[String],
    cone_mode: bool,
    entries: &[IndexEntry],
    out: &mut Vec<IndexEntry>,
) -> Result<()> {
    let obj = odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Err(Error::IndexError(format!("expected tree at {}", tree_oid)));
    }
    let tree_entries = parse_tree(&obj.data)?;
    for te in tree_entries {
        let path = if prefix.is_empty() {
            te.name.clone()
        } else {
            let mut p = prefix.to_vec();
            p.push(b'/');
            p.extend_from_slice(&te.name);
            p
        };
        if te.mode == MODE_TREE {
            let path_slash = {
                let mut p = path.clone();
                p.push(b'/');
                p
            };
            if entries.iter().any(|e| {
                e.stage() == 0
                    && e.is_sparse_directory_placeholder()
                    && e.path == path_slash
                    && e.oid == te.oid
            }) {
                continue;
            }
            walk_sparse_aware(odb, &te.oid, &path, patterns, cone_mode, entries, out)?;
        } else {
            let path_len = path.len().min(0xFFF) as u16;
            let path_str = String::from_utf8_lossy(&path);
            if crate::sparse_checkout::path_matches_sparse_patterns(&path_str, patterns, cone_mode)
            {
                continue;
            }
            let mut e = IndexEntry {
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
                flags: path_len,
                flags_extended: Some(0),
                path,
            };
            e.set_skip_worktree(true);
            out.push(e);
        }
    }
    Ok(())
}

fn flatten_tree_blobs(odb: &Odb, tree_oid: &ObjectId, prefix: &[u8]) -> Result<Vec<IndexEntry>> {
    let obj = odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Err(Error::IndexError(format!("expected tree at {}", tree_oid)));
    }
    let entries = parse_tree(&obj.data)?;
    let mut out = Vec::new();
    for te in entries {
        let path = if prefix.is_empty() {
            te.name.clone()
        } else {
            let mut p = prefix.to_vec();
            p.push(b'/');
            p.extend_from_slice(&te.name);
            p
        };
        if te.mode == MODE_TREE {
            let sub = flatten_tree_blobs(odb, &te.oid, &path)?;
            out.extend(sub);
        } else {
            let path_len = path.len().min(0xFFF) as u16;
            let mut e = IndexEntry {
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
                flags: path_len,
                flags_extended: Some(0),
                path,
            };
            e.set_skip_worktree(true);
            out.push(e);
        }
    }
    Ok(out)
}

fn lockfile_pid_enabled(index_path: &Path) -> bool {
    let git_dir = match index_path.parent() {
        Some(dir) => dir,
        None => return false,
    };

    ConfigSet::load(Some(git_dir), true)
        .ok()
        .and_then(|cfg| cfg.get_bool("core.lockfilepid"))
        .and_then(|res| res.ok())
        .unwrap_or(false)
}

fn pid_path_for_lock(lock_path: &Path) -> std::path::PathBuf {
    let file_name = lock_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "index.lock".to_owned());
    let pid_name = if let Some(base) = file_name.strip_suffix(".lock") {
        format!("{base}~pid.lock")
    } else {
        format!("{file_name}~pid.lock")
    };
    lock_path.with_file_name(pid_name)
}

fn write_lock_pid_file(pid_path: &Path) -> io::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(pid_path)?;
    writeln!(file, "pid {}", std::process::id())?;
    Ok(())
}

fn build_lock_exists_message(lock_path: &Path, pid_path: &Path, err: &io::Error) -> String {
    let mut msg = format!("Unable to create '{}': {}.\n\n", lock_path.display(), err);

    if let Some(pid) = read_lock_pid(pid_path) {
        if is_process_running(pid) {
            msg.push_str(&format!(
                "Lock is held by process {pid}; if no git process is running, the lock file may be stale (PIDs can be reused)"
            ));
        } else {
            msg.push_str(&format!(
                "Lock was held by process {pid}, which is no longer running; the lock file appears to be stale"
            ));
        }
    } else {
        msg.push_str(
            "Another git process seems to be running in this repository, or the lock file may be stale",
        );
    }

    msg
}

fn read_lock_pid(pid_path: &Path) -> Option<u64> {
    let raw = fs::read_to_string(pid_path).ok()?;
    let trimmed = raw.trim();
    if let Some(v) = trimmed.strip_prefix("pid ") {
        return v.trim().parse::<u64>().ok();
    }
    trimmed.parse::<u64>().ok()
}

fn is_process_running(pid: u64) -> bool {
    #[cfg(target_os = "linux")]
    {
        let proc_path = std::path::PathBuf::from(format!("/proc/{pid}"));
        proc_path.exists()
    }

    #[cfg(not(target_os = "linux"))]
    {
        let status = std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status();
        status.map(|s| s.success()).unwrap_or(false)
    }
}

/// Parse a single index entry from `data`, returning `(entry, bytes_consumed)`.
fn parse_entry(data: &[u8], version: u32, prev_path: &[u8]) -> Result<(IndexEntry, usize)> {
    if data.len() < 62 {
        return Err(Error::IndexError("entry too short".to_owned()));
    }

    let mut pos = 0;

    macro_rules! read_u32 {
        () => {{
            let v = u32::from_be_bytes(
                data[pos..pos + 4]
                    .try_into()
                    .map_err(|_| Error::IndexError("truncated u32".to_owned()))?,
            );
            pos += 4;
            v
        }};
    }

    let ctime_sec = read_u32!();
    let ctime_nsec = read_u32!();
    let mtime_sec = read_u32!();
    let mtime_nsec = read_u32!();
    let dev = read_u32!();
    let ino = read_u32!();
    let mode = read_u32!();
    let uid = read_u32!();
    let gid = read_u32!();
    let size = read_u32!();

    let oid = ObjectId::from_bytes(&data[pos..pos + 20])?;
    pos += 20;

    let flags = u16::from_be_bytes(
        data[pos..pos + 2]
            .try_into()
            .map_err(|_| Error::IndexError("truncated flags".to_owned()))?,
    );
    pos += 2;

    let flags_extended = if version >= 3 && flags & 0x4000 != 0 {
        let fe = u16::from_be_bytes(
            data[pos..pos + 2]
                .try_into()
                .map_err(|_| Error::IndexError("truncated extended flags".to_owned()))?,
        );
        pos += 2;
        Some(fe)
    } else {
        None
    };

    let path;
    if version == 4 {
        // V4: prefix-compressed path
        let (strip_len, varint_bytes) = read_varint(&data[pos..]);
        pos += varint_bytes;
        let nul = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::IndexError("v4 entry path missing NUL".to_owned()))?;
        let suffix = &data[pos..pos + nul];
        pos += nul + 1;
        let keep = prev_path.len().saturating_sub(strip_len);
        let mut full_path = prev_path[..keep].to_vec();
        full_path.extend_from_slice(suffix);
        path = full_path;
    } else {
        // V2/V3: NUL-terminated full path + padding
        let nul = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::IndexError("entry path missing NUL terminator".to_owned()))?;
        path = data[pos..pos + nul].to_vec();
        pos += nul + 1;
        let entry_start = 0usize;
        let entry_len = pos - entry_start;
        let padded = (entry_len + 7) & !7;
        let padding = padded.saturating_sub(entry_len);
        pos += padding;
    }

    Ok((
        IndexEntry {
            ctime_sec,
            ctime_nsec,
            mtime_sec,
            mtime_nsec,
            dev,
            ino,
            mode,
            uid,
            gid,
            size,
            oid,
            flags,
            flags_extended,
            path,
        },
        pos,
    ))
}

/// Serialise a single index entry into `out`.
/// Read a variable-length integer (git's index v4 varint encoding).
/// Returns (value, bytes_consumed).
fn write_varint(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let mut b = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            b |= 0x80;
        }
        out.push(b);
        if value == 0 {
            break;
        }
    }
}

fn read_varint(data: &[u8]) -> (usize, usize) {
    let mut value: usize = 0;
    let mut shift = 0usize;
    let mut pos = 0;
    loop {
        if pos >= data.len() {
            break;
        }
        let byte = data[pos] as usize;
        pos += 1;
        value |= (byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        // Prevent infinite loops on malformed data
        if shift > 28 {
            break;
        }
    }
    (value, pos)
}

fn serialize_entry_v4(entry: &IndexEntry, previous_path: &mut Vec<u8>, out: &mut Vec<u8>) {
    let write_u32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_be_bytes());

    write_u32(out, entry.ctime_sec);
    write_u32(out, entry.ctime_nsec);
    write_u32(out, entry.mtime_sec);
    write_u32(out, entry.mtime_nsec);
    write_u32(out, entry.dev);
    write_u32(out, entry.ino);
    write_u32(out, entry.mode);
    write_u32(out, entry.uid);
    write_u32(out, entry.gid);
    write_u32(out, entry.size);
    out.extend_from_slice(entry.oid.as_bytes());

    let mut flags = entry.flags;
    if entry.flags_extended.is_some() {
        flags |= 0x4000;
    } else {
        flags &= !0x4000;
    }
    let path_len = entry.path.len().min(0xFFF) as u16;
    flags = (flags & 0xF000) | path_len;
    out.extend_from_slice(&flags.to_be_bytes());

    if let Some(fe) = entry.flags_extended {
        out.extend_from_slice(&fe.to_be_bytes());
    }

    let common = previous_path
        .iter()
        .zip(entry.path.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let to_remove = previous_path.len().saturating_sub(common);
    write_varint(out, to_remove);
    out.extend_from_slice(&entry.path[common..]);
    out.push(0);

    previous_path.clear();
    previous_path.extend_from_slice(&entry.path);
}

fn serialize_entry(entry: &IndexEntry, version: u32, out: &mut Vec<u8>) {
    let start = out.len();

    let write_u32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_be_bytes());

    write_u32(out, entry.ctime_sec);
    write_u32(out, entry.ctime_nsec);
    write_u32(out, entry.mtime_sec);
    write_u32(out, entry.mtime_nsec);
    write_u32(out, entry.dev);
    write_u32(out, entry.ino);
    write_u32(out, entry.mode);
    write_u32(out, entry.uid);
    write_u32(out, entry.gid);
    write_u32(out, entry.size);
    out.extend_from_slice(entry.oid.as_bytes());

    // Set or clear the extended-flags bit in flags
    let mut flags = entry.flags;
    if version >= 3 && entry.flags_extended.is_some() {
        flags |= 0x4000;
    } else {
        flags &= !0x4000;
    }
    // Overwrite path length bits (bottom 12)
    let path_len = entry.path.len().min(0xFFF) as u16;
    flags = (flags & 0xF000) | path_len;
    out.extend_from_slice(&flags.to_be_bytes());

    if version >= 3 {
        if let Some(fe) = entry.flags_extended {
            out.extend_from_slice(&fe.to_be_bytes());
        }
    }

    out.extend_from_slice(&entry.path);
    out.push(0);

    // Pad to 8-byte boundary
    let entry_len = out.len() - start;
    let padded = (entry_len + 7) & !7;
    let padding = padded - entry_len;
    for _ in 0..padding {
        out.push(0);
    }
}

/// Build an [`IndexEntry`] by stat-ing a file on disk.
///
/// # Parameters
///
/// - `path` — absolute path to the file.
/// - `rel_path` — path relative to the repo root (stored in the index).
/// - `oid` — the object ID of the file's blob.
/// - `mode` — file mode (use [`MODE_REGULAR`], [`MODE_EXECUTABLE`], etc.).
///
/// # Errors
///
/// Returns [`Error::Io`] if `stat` fails.
pub fn entry_from_stat(
    path: &Path,
    rel_path: &[u8],
    oid: ObjectId,
    mode: u32,
) -> Result<IndexEntry> {
    let meta = fs::symlink_metadata(path)?;
    Ok(entry_from_metadata(&meta, rel_path, oid, mode))
}

/// Build an [`IndexEntry`] from already-obtained metadata.
///
/// This avoids a redundant `stat()` call when the caller already has
/// filesystem metadata (e.g. from `symlink_metadata`).
#[must_use]
pub fn entry_from_metadata(
    meta: &fs::Metadata,
    rel_path: &[u8],
    oid: ObjectId,
    mode: u32,
) -> IndexEntry {
    use std::os::unix::fs::MetadataExt;
    IndexEntry {
        ctime_sec: meta.ctime() as u32,
        ctime_nsec: meta.ctime_nsec() as u32,
        mtime_sec: meta.mtime() as u32,
        mtime_nsec: meta.mtime_nsec() as u32,
        dev: meta.dev() as u32,
        ino: meta.ino() as u32,
        mode,
        uid: meta.uid(),
        gid: meta.gid(),
        size: meta.size() as u32,
        oid,
        flags: rel_path.len().min(0xFFF) as u16,
        flags_extended: None,
        path: rel_path.to_vec(),
    }
}

/// Convert a `stat` mode to the Git index mode, normalised to one of the
/// known constants ([`MODE_REGULAR`], [`MODE_EXECUTABLE`], [`MODE_SYMLINK`]).
///
/// Only the `S_IFMT` and execute bits are inspected; all other permission bits
/// are discarded (Git stores only 644 or 755 for regular files).
///
/// # Parameters
///
/// - `raw_mode` — the raw `st_mode` value from `stat(2)`.
#[must_use]
pub fn normalize_mode(raw_mode: u32) -> u32 {
    const S_IFMT: u32 = 0o170000;
    const S_IFLNK: u32 = 0o120000;
    const S_IFREG: u32 = 0o100000;

    let fmt = raw_mode & S_IFMT;
    if fmt == S_IFLNK {
        return MODE_SYMLINK;
    }
    if fmt == S_IFREG {
        // Executable if any execute bit is set
        if raw_mode & 0o111 != 0 {
            return MODE_EXECUTABLE;
        }
        return MODE_REGULAR;
    }
    // Fallback for everything else (devices, etc.) — treat as regular
    MODE_REGULAR
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use tempfile::TempDir;

    fn dummy_oid() -> ObjectId {
        ObjectId::from_bytes(&[0u8; 20]).unwrap()
    }

    fn make_entry(path: &str) -> IndexEntry {
        IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode: MODE_REGULAR,
            uid: 0,
            gid: 0,
            size: 0,
            oid: dummy_oid(),
            flags: path.len().min(0xFFF) as u16,
            flags_extended: None,
            path: path.as_bytes().to_vec(),
        }
    }

    #[test]
    fn round_trip_empty_index() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index");

        let idx = Index::new();
        idx.write(&path).unwrap();

        let loaded = Index::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), 0);
    }

    #[test]
    fn round_trip_with_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index");

        let mut idx = Index::new();
        idx.add_or_replace(make_entry("foo.txt"));
        idx.add_or_replace(make_entry("bar/baz.txt"));
        idx.write(&path).unwrap();

        let loaded = Index::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].path, b"bar/baz.txt");
        assert_eq!(loaded.entries[1].path, b"foo.txt");
    }

    #[test]
    fn remove_descendants_under_path_drops_nested_only() {
        let mut idx = Index::new();
        idx.add_or_replace(make_entry("d/e"));
        idx.add_or_replace(make_entry("d-other"));
        idx.add_or_replace(make_entry("prefix/d"));
        idx.remove_descendants_under_path("d");
        let paths: Vec<_> = idx.entries.iter().map(|e| e.path.as_slice()).collect();
        assert_eq!(paths, vec![b"d-other".as_slice(), b"prefix/d".as_slice()]);
    }

    #[test]
    fn requested_v4_writes_v4_on_disk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index");

        let mut idx = Index {
            version: 4,
            ..Index::default()
        };
        idx.add_or_replace(make_entry("one"));
        idx.add_or_replace(make_entry("two/one"));
        idx.write(&path).unwrap();

        let data = fs::read(&path).unwrap();
        assert_eq!(&data[4..8], &4u32.to_be_bytes());

        let loaded = Index::load(&path).unwrap();
        assert_eq!(loaded.version, 4);
        assert_eq!(loaded.entries[0].path, b"one");
        assert_eq!(loaded.entries[1].path, b"two/one");
    }
}
