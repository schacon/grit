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

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::config::ConfigSet;
use crate::error::{Error, Result};
use crate::objects::ObjectId;

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
}

/// The in-memory representation of the Git index file.
#[derive(Debug, Clone, Default)]
pub struct Index {
    /// Index format version (2, 3, or 4).
    pub version: u32,
    /// Index entries, sorted by (path, stage).
    pub entries: Vec<IndexEntry>,
    /// Saved higher-stage entries used to recreate conflicts.
    pub resolve_undo: BTreeMap<Vec<u8>, ResolveUndoEntry>,
}

/// Saved stage 1/2/3 entries for a resolved path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolveUndoEntry {
    /// Mode and object ID for stages 1..=3. `None` means the stage was absent.
    pub stages: [Option<(u32, ObjectId)>; 3],
}

/// Default index version when `GIT_INDEX_VERSION` is unset or invalid.
const INDEX_FORMAT_DEFAULT: u32 = 2;
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
                 Using version {INDEX_FORMAT_DEFAULT}"
            );
            Some(INDEX_FORMAT_DEFAULT)
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
            resolve_undo: BTreeMap::new(),
        }
    }

    /// Create a new empty index, respecting config values for version.
    ///
    /// Priority: GIT_INDEX_VERSION env > index.version config > feature.manyFiles config > default (2).
    pub fn new_with_config(
        config_index_version: Option<&str>,
        config_many_files: Option<&str>,
    ) -> Self {
        // Env var takes highest priority
        if let Some(v) = get_index_format_from_env() {
            return Self {
                version: v,
                entries: Vec::new(),
                resolve_undo: BTreeMap::new(),
            };
        }
        // Config index.version
        if let Some(val) = config_index_version {
            if let Ok(v) = val.parse::<u32>() {
                if (INDEX_FORMAT_LB..=INDEX_FORMAT_UB).contains(&v) {
                    return Self {
                        version: v,
                        entries: Vec::new(),
                        resolve_undo: BTreeMap::new(),
                    };
                }
            }
            // Invalid config value
            eprintln!(
                "warning: index.version set, but the value is invalid.\n\
                 Using version {INDEX_FORMAT_DEFAULT}"
            );
            return Self {
                version: INDEX_FORMAT_DEFAULT,
                entries: Vec::new(),
                resolve_undo: BTreeMap::new(),
            };
        }
        // feature.manyFiles implies version 4
        if let Some(val) = config_many_files {
            let lowered = val.to_lowercase();
            let enabled = matches!(lowered.as_str(), "true" | "yes" | "1" | "on");
            if enabled {
                return Self {
                    version: 4,
                    entries: Vec::new(),
                    resolve_undo: BTreeMap::new(),
                };
            }
        }
        Self {
            version: 2,
            entries: Vec::new(),
            resolve_undo: BTreeMap::new(),
        }
    }

    /// Load an index from the given file path.
    ///
    /// Returns an empty index if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IndexError`] if the file is present but corrupt.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(data) => Self::parse(&data),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(Error::Io(e)),
        }
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

        // Verify trailing SHA-1 checksum
        let (body, checksum) = data.split_at(data.len() - 20);
        let checksum_is_zero = checksum.iter().all(|byte| *byte == 0);
        let mut hasher = Sha1::new();
        hasher.update(body);
        let computed = hasher.finalize();
        if !checksum_is_zero && computed.as_slice() != checksum {
            return Err(Error::IndexError("SHA-1 checksum mismatch".to_owned()));
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

        let mut index = Self {
            version,
            entries,
            resolve_undo: BTreeMap::new(),
        };

        while pos + 8 <= body.len() {
            let signature = &body[pos..pos + 4];
            pos += 4;
            let size = u32::from_be_bytes(
                body[pos..pos + 4]
                    .try_into()
                    .map_err(|_| Error::IndexError("cannot read extension size".to_owned()))?,
            ) as usize;
            pos += 4;
            if pos + size > body.len() {
                return Err(Error::IndexError("extension exceeds index size".to_owned()));
            }
            let extension = &body[pos..pos + size];
            pos += size;

            if signature == b"REUC" {
                index.resolve_undo = parse_resolve_undo(extension)?;
            }
        }

        Ok(index)
    }

    /// Write the index to a file, computing and appending the trailing SHA-1.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on filesystem errors.
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut body = Vec::new();
        self.serialize_into(&mut body)?;

        let checksum = if index_skip_hash_enabled(path) {
            [0u8; 20].to_vec()
        } else {
            let mut hasher = Sha1::new();
            hasher.update(&body);
            hasher.finalize().to_vec()
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
    fn serialize_into(&self, out: &mut Vec<u8>) -> Result<()> {
        // Determine which version to write.
        // Version 4 requires path compression, which we do not implement yet.
        // Downgrade to the newest format we can serialize correctly.
        let write_version = if self.version == 4 {
            4
        } else if self.version >= 3 {
            if self.entries.iter().any(|e| e.flags_extended.is_some()) {
                3
            } else {
                2
            }
        } else {
            self.version
        };
        // Header
        out.extend_from_slice(b"DIRC");
        out.extend_from_slice(&write_version.to_be_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

        let mut prev_path: &[u8] = &[];
        for entry in &self.entries {
            if write_version == 4 {
                serialize_entry_v4(entry, prev_path, out);
                prev_path = &entry.path;
            } else {
                serialize_entry(entry, write_version, out);
            }
        }
        if !self.resolve_undo.is_empty() {
            let mut reuc = Vec::new();
            serialize_resolve_undo(&self.resolve_undo, &mut reuc);
            out.extend_from_slice(b"REUC");
            out.extend_from_slice(&(reuc.len() as u32).to_be_bytes());
            out.extend_from_slice(&reuc);
        }
        Ok(())
    }

    /// Add or replace an entry (matched by path + stage).
    pub fn add_or_replace(&mut self, entry: IndexEntry) {
        let path = &entry.path;
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
    }

    /// Stage a file at stage 0, removing any conflict stage entries (1, 2, 3)
    /// for the same path. This is the correct behavior for `git add` on a
    /// conflicted file during merge/cherry-pick resolution.
    pub fn stage_file(&mut self, entry: IndexEntry) {
        let path = entry.path.clone();
        let mut saved = ResolveUndoEntry::default();
        let mut has_conflicts = false;
        for existing in &self.entries {
            if existing.path != path {
                continue;
            }
            let stage = existing.stage();
            if (1..=3).contains(&stage) {
                saved.stages[(stage - 1) as usize] = Some((existing.mode, existing.oid));
                has_conflicts = true;
            }
        }
        if has_conflicts {
            self.resolve_undo.insert(path.clone(), saved);
        }
        // Remove conflict stages first
        self.entries.retain(|e| e.path != path || e.stage() == 0);
        // Then add/replace stage-0 entry
        self.add_or_replace(entry);
    }

    /// Remove all entries matching the given path (all stages).
    ///
    /// Returns `true` if at least one entry was removed.
    pub fn remove(&mut self, path: &[u8]) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.path != path);
        self.resolve_undo.remove(path);
        self.entries.len() < before
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

    /// Restore higher-stage entries for a previously resolved path.
    #[must_use]
    pub fn unresolve(&mut self, path: &[u8]) -> bool {
        let Some(saved) = self.resolve_undo.remove(path) else {
            return false;
        };

        self.entries.retain(|e| e.path != path);
        for (index, stage) in saved.stages.iter().enumerate() {
            let Some((mode, oid)) = stage else {
                continue;
            };
            let stage_num = (index + 1) as u8;
            self.entries.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: *mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: *oid,
                flags: (path.len().min(0xFFF) as u16) | ((stage_num as u16) << 12),
                flags_extended: None,
                path: path.to_vec(),
            });
        }
        self.sort();
        true
    }
}

fn parse_resolve_undo(data: &[u8]) -> Result<BTreeMap<Vec<u8>, ResolveUndoEntry>> {
    let mut pos = 0usize;
    let mut entries = BTreeMap::new();

    while pos < data.len() {
        let path_end = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::IndexError("resolve-undo path missing NUL".to_owned()))?;
        let path = data[pos..pos + path_end].to_vec();
        pos += path_end + 1;

        let mut modes = [0u32; 3];
        for mode in &mut modes {
            let mode_end = data[pos..]
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| Error::IndexError("resolve-undo mode missing NUL".to_owned()))?;
            let raw = std::str::from_utf8(&data[pos..pos + mode_end])
                .map_err(|_| Error::IndexError("resolve-undo mode is not UTF-8".to_owned()))?;
            *mode = u32::from_str_radix(raw, 8)
                .map_err(|_| Error::IndexError("invalid resolve-undo mode".to_owned()))?;
            pos += mode_end + 1;
        }

        let mut entry = ResolveUndoEntry::default();
        for (index, mode) in modes.into_iter().enumerate() {
            if mode == 0 {
                continue;
            }
            if pos + 20 > data.len() {
                return Err(Error::IndexError(
                    "resolve-undo object id exceeds extension size".to_owned(),
                ));
            }
            let oid = ObjectId::from_bytes(&data[pos..pos + 20])?;
            pos += 20;
            entry.stages[index] = Some((mode, oid));
        }

        entries.insert(path, entry);
    }

    Ok(entries)
}

fn serialize_resolve_undo(entries: &BTreeMap<Vec<u8>, ResolveUndoEntry>, out: &mut Vec<u8>) {
    for (path, entry) in entries {
        out.extend_from_slice(path);
        out.push(0);
        for stage in &entry.stages {
            let mode = stage.map(|(mode, _)| mode).unwrap_or(0);
            out.extend_from_slice(format!("{mode:o}").as_bytes());
            out.push(0);
        }
        for stage in &entry.stages {
            if let Some((_, oid)) = stage {
                out.extend_from_slice(oid.as_bytes());
            }
        }
    }
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

fn index_skip_hash_enabled(index_path: &Path) -> bool {
    let git_dir = match index_path.parent() {
        Some(dir) => dir,
        None => return false,
    };

    let config = match ConfigSet::load(Some(git_dir), true) {
        Ok(config) => config,
        Err(_) => return false,
    };

    if let Some(value) = config.get_bool("index.skipHash").and_then(|res| res.ok()) {
        return value;
    }

    config
        .get_bool("feature.manyFiles")
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

fn serialize_entry_v4(entry: &IndexEntry, prev_path: &[u8], out: &mut Vec<u8>) {
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

    let shared_prefix_len = prev_path
        .iter()
        .zip(entry.path.iter())
        .take_while(|(lhs, rhs)| lhs == rhs)
        .count();
    let strip_len = prev_path.len().saturating_sub(shared_prefix_len);
    write_varint(strip_len, out);
    out.extend_from_slice(&entry.path[shared_prefix_len..]);
    out.push(0);
}

fn write_varint(mut value: usize, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
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
    fn requested_v4_writes_a_compatible_index_format() {
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
        assert_eq!(loaded.entries[0].path, b"one");
        assert_eq!(loaded.entries[1].path, b"two/one");
    }

    #[test]
    fn loads_index_with_zeroed_trailing_hash() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("index");

        let mut idx = Index::new();
        idx.add_or_replace(make_entry("foo"));
        idx.write(&path).unwrap();

        let mut data = fs::read(&path).unwrap();
        let len = data.len();
        data[len - 20..].fill(0);
        fs::write(&path, data).unwrap();

        let loaded = Index::load(&path).unwrap();
        assert_eq!(loaded.entries[0].path, b"foo");
    }
}
