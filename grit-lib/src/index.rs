//! Git index (staging area) reading and writing.
//!
//! The index file (`.git/index`) stores the current state of the staging area.
//! It uses a binary format with a 12-byte header, fixed-size index entries,
//! and optional extensions, followed by a trailing SHA-1 over the whole file.
//!
//! # Format version
//!
//! This implementation supports index versions 2 and 3.  Version 4 (path
//! compression) is not yet implemented.
//!
//! # References
//!
//! See `Documentation/technical/index-format.txt` in the Git source tree for
//! the authoritative format specification.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use sha1::{Digest, Sha1};

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
}

impl Index {
    /// Create a new, empty index.
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: 2,
            entries: Vec::new(),
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
        let mut hasher = Sha1::new();
        hasher.update(body);
        let computed = hasher.finalize();
        if computed.as_slice() != checksum {
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
        if version != 2 && version != 3 {
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

        for _ in 0..count {
            let (entry, consumed) = parse_entry(&body[pos..], version)?;
            entries.push(entry);
            pos += consumed;
        }

        Ok(Self { version, entries })
    }

    /// Write the index to a file, computing and appending the trailing SHA-1.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on filesystem errors.
    pub fn write(&self, path: &Path) -> Result<()> {
        let mut body = Vec::new();
        self.serialize_into(&mut body)?;

        let mut hasher = Sha1::new();
        hasher.update(&body);
        let checksum = hasher.finalize();

        let tmp_path = path.with_extension("lock");
        {
            let mut f = fs::File::create(&tmp_path)?;
            f.write_all(&body)?;
            f.write_all(&checksum)?;
        }
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Serialise the index body (without trailing checksum) into `out`.
    fn serialize_into(&self, out: &mut Vec<u8>) -> Result<()> {
        // Header
        out.extend_from_slice(b"DIRC");
        out.extend_from_slice(&self.version.to_be_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

        for entry in &self.entries {
            serialize_entry(entry, self.version, out);
        }
        Ok(())
    }

    /// Add or replace an entry (matched by path + stage).
    pub fn add_or_replace(&mut self, entry: IndexEntry) {
        let path = &entry.path;
        let stage = entry.stage();
        // Binary search for the insertion point by (path, stage)
        let result = self.entries.binary_search_by(|e| {
            e.path.as_slice().cmp(path.as_slice()).then_with(|| e.stage().cmp(&stage))
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

    /// Remove all entries matching the given path (all stages).
    ///
    /// Returns `true` if at least one entry was removed.
    pub fn remove(&mut self, path: &[u8]) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.path != path);
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
}

/// Parse a single index entry from `data`, returning `(entry, bytes_consumed)`.
fn parse_entry(data: &[u8], version: u32) -> Result<(IndexEntry, usize)> {
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

    // Path: null-terminated
    let nul = data[pos..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::IndexError("entry path missing NUL terminator".to_owned()))?;
    let path = data[pos..pos + nul].to_vec();
    pos += nul + 1;

    // Pad to 8-byte boundary (from start of entry)
    let entry_start = 0usize;
    let entry_len = pos - entry_start;
    let padded = (entry_len + 7) & !7;
    let padding = padded.saturating_sub(entry_len);
    pos += padding;

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
}
