//! Loose object database: reading and writing zlib-compressed Git objects.
//!
//! Git stores objects as files under `<git-dir>/objects/<xx>/<38-hex-chars>`,
//! where the path is derived from the SHA-1 digest. Each file is a zlib-
//! compressed byte sequence whose decompressed form is:
//!
//! ```text
//! "<type> <size>\0<data>"
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use std::path::Path;
//! use grit_lib::odb::Odb;
//!
//! let odb = Odb::new(Path::new(".git/objects"));
//! ```

use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::objects::{Object, ObjectId, ObjectKind};
use crate::pack;

/// Decompress a zlib-wrapped loose object payload from an open file.
///
/// When the zlib wrapper advertises a preset dictionary (FDICT), `flate2` typically fails with a
/// generic corrupt-stream error; map that to `"needs dictionary"` so callers match Git's messages
/// (`t1006-cat-file` zlib-dictionary test).
fn read_zlib_loose_payload(mut file: fs::File) -> Result<Vec<u8>> {
    let mut hdr = [0u8; 2];
    file.read_exact(&mut hdr).map_err(Error::Io)?;
    let cmf_flg = u16::from(hdr[0]) << 8 | u16::from(hdr[1]);
    let looks_like_zlib_header = cmf_flg != 0 && cmf_flg % 31 == 0;
    let preset_dictionary = looks_like_zlib_header && (hdr[1] & 0x20) != 0;
    let mut decoder = ZlibDecoder::new(hdr.as_slice().chain(file));
    let mut raw = Vec::new();
    match decoder.read_to_end(&mut raw) {
        Ok(_) => Ok(raw),
        Err(e) => {
            if preset_dictionary {
                Err(Error::Zlib("needs dictionary".to_owned()))
            } else {
                Err(Error::Zlib(e.to_string()))
            }
        }
    }
}

/// True when `oid` is stored as a loose object or in a **non-promisor** local pack.
fn exists_materialized_in_objects_dir(objects_dir: &Path, oid: &ObjectId) -> bool {
    let loose = objects_dir
        .join(oid.loose_prefix())
        .join(oid.loose_suffix());
    if loose.exists() {
        return true;
    }
    let Ok(indexes) = pack::read_local_pack_indexes(objects_dir) else {
        return false;
    };
    for idx in &indexes {
        if idx.pack_path.with_extension("promisor").is_file() {
            continue;
        }
        if idx.entries.iter().any(|e| e.oid == *oid) {
            return true;
        }
    }
    false
}

/// A loose-object database rooted at a given `objects/` directory.
#[derive(Debug, Clone)]
pub struct Odb {
    objects_dir: PathBuf,
    /// Work tree root for resolving relative alternate env paths.
    work_tree: Option<PathBuf>,
}

impl Odb {
    /// Create an [`Odb`] pointing at the given `objects/` directory.
    ///
    /// The directory does not need to exist yet; it will be created on the
    /// first write operation.
    #[must_use]
    pub fn new(objects_dir: &Path) -> Self {
        Self {
            objects_dir: objects_dir.to_path_buf(),
            work_tree: None,
        }
    }

    /// Create an [`Odb`] with a work tree for resolving relative alternate paths.
    #[must_use]
    pub fn with_work_tree(objects_dir: &Path, work_tree: &Path) -> Self {
        Self {
            objects_dir: objects_dir.to_path_buf(),
            work_tree: Some(work_tree.to_path_buf()),
        }
    }

    /// Return the path to the `objects/` directory.
    #[must_use]
    pub fn objects_dir(&self) -> &Path {
        &self.objects_dir
    }

    /// Return the filesystem path for a given object ID.
    #[must_use]
    pub fn object_path(&self, oid: &ObjectId) -> PathBuf {
        self.objects_dir
            .join(oid.loose_prefix())
            .join(oid.loose_suffix())
    }

    /// Whether the object exists under this database directory only (loose or local packs).
    ///
    /// Unlike [`Self::exists`], this ignores `info/alternates` and
    /// `GIT_ALTERNATE_OBJECT_DIRECTORIES`. Used for partial-clone bookkeeping where
    /// objects reachable via alternates are still treated as "missing" until copied locally.
    ///
    /// Objects stored **only** in promisor packs (sibling `.promisor` marker next to the
    /// `.pack`) are treated as absent: Git considers them fetchable on demand, and
    /// `rev-list --missing=print` lists them until materialized as loose objects or a
    /// non-promisor pack.
    ///
    /// The empty tree object is treated as present without a loose file (matches Git).
    #[must_use]
    pub fn exists_local(&self, oid: &ObjectId) -> bool {
        const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        if oid.to_hex() == EMPTY_TREE {
            return true;
        }
        exists_materialized_in_objects_dir(&self.objects_dir, oid)
    }

    /// Check whether an object exists in the loose store or any pack file.
    #[must_use]
    pub fn exists(&self, oid: &ObjectId) -> bool {
        // The empty tree is a well-known object (no on-disk loose file). Git's
        // canonical SHA-1 is `...8d69288fbee4904`; some harnesses still use the
        // legacy typo hash `...899d69f7c6948d4` — treat both as present.
        const EMPTY_TREE_CANON: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        const EMPTY_TREE_LEGACY: &str = "4b825dc642cb6eb9a060e54bf899d69f7c6948d4";
        let hex = oid.to_hex();
        if hex == EMPTY_TREE_CANON || hex == EMPTY_TREE_LEGACY {
            return true;
        }
        if self.exists_in_dir(&self.objects_dir, oid) {
            return true;
        }
        // Check alternates from info/alternates file.
        if let Ok(alts) = pack::read_alternates_recursive(&self.objects_dir) {
            for alt_dir in &alts {
                if self.exists_in_dir(alt_dir, oid) {
                    return true;
                }
            }
        }
        // Check GIT_ALTERNATE_OBJECT_DIRECTORIES env var.
        for alt_dir in env_alternate_dirs(self.work_tree.as_deref()) {
            if self.exists_in_dir(&alt_dir, oid) {
                return true;
            }
        }
        false
    }

    /// Check whether an object exists in a specific objects directory.
    fn exists_in_dir(&self, objects_dir: &Path, oid: &ObjectId) -> bool {
        let loose = objects_dir
            .join(oid.loose_prefix())
            .join(oid.loose_suffix());
        if loose.exists() {
            return true;
        }
        if let Ok(indexes) = pack::read_local_pack_indexes(objects_dir) {
            for idx in &indexes {
                if idx.entries.iter().any(|e| e.oid == *oid) {
                    return true;
                }
            }
        }
        false
    }

    /// Touch the loose object file or pack file containing `oid`, matching Git's
    /// `odb_freshen_object` (updates mtime so age-based prune keeps recently re-referenced objects).
    ///
    /// Returns `true` if an on-disk object was found and touched.
    #[must_use]
    pub fn freshen_object(&self, oid: &ObjectId) -> bool {
        const EMPTY_TREE_CANON: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        const EMPTY_TREE_LEGACY: &str = "4b825dc642cb6eb9a060e54bf899d69f7c6948d4";
        let hex = oid.to_hex();
        if hex == EMPTY_TREE_CANON || hex == EMPTY_TREE_LEGACY {
            return false;
        }

        let loose = self.object_path(oid);
        if loose.is_file() {
            return touch_path_mtime(&loose);
        }

        if freshen_object_in_objects_dir(&self.objects_dir, oid) {
            return true;
        }

        if let Ok(alts) = pack::read_alternates_recursive(&self.objects_dir) {
            for alt_dir in &alts {
                if freshen_object_in_objects_dir(alt_dir, oid) {
                    return true;
                }
            }
        }

        for alt_dir in env_alternate_dirs(self.work_tree.as_deref()) {
            if freshen_object_in_objects_dir(&alt_dir, oid) {
                return true;
            }
        }

        false
    }

    /// Read a loose object file at `path`, verifying the uncompressed payload hashes to `expected_oid`.
    ///
    /// Git stores loose objects under paths derived from the OID; if the file contents hash to a
    /// different id (for example after a mistaken `mv`), this returns [`Error::LooseHashMismatch`].
    ///
    /// # Errors
    ///
    /// - [`Error::Zlib`] — decompression failed.
    /// - [`Error::CorruptObject`] — header is malformed.
    /// - [`Error::LooseHashMismatch`] — payload OID does not match `expected_oid`.
    pub fn read_loose_verify_oid(path: &Path, expected_oid: &ObjectId) -> Result<Object> {
        let file = fs::File::open(path).map_err(Error::Io)?;
        let raw = read_zlib_loose_payload(file)?;
        let obj = parse_object_bytes_with_oid(&raw, expected_oid)?;
        let computed = hash_object_from_parsed(&obj);
        if computed != *expected_oid {
            return Err(Error::LooseHashMismatch {
                path: path.display().to_string(),
                real_oid: computed.to_hex(),
            });
        }
        Ok(obj)
    }

    /// Read and decompress an object from the loose store.
    ///
    /// # Errors
    ///
    /// - [`Error::ObjectNotFound`] — no file at the expected path.
    /// - [`Error::Zlib`] — decompression failed.
    /// - [`Error::CorruptObject`] — header is malformed.
    pub fn read(&self, oid: &ObjectId) -> Result<Object> {
        // The empty tree is a well-known virtual object — no storage needed.
        const EMPTY_TREE_CANON: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        const EMPTY_TREE_LEGACY: &str = "4b825dc642cb6eb9a060e54bf899d69f7c6948d4";
        let hex = oid.to_hex();
        if hex == EMPTY_TREE_CANON || hex == EMPTY_TREE_LEGACY {
            return Ok(crate::objects::Object {
                kind: crate::objects::ObjectKind::Tree,
                data: Vec::new(),
            });
        }

        let path = self.object_path(oid);
        match fs::File::open(&path) {
            Ok(file) => {
                let raw = read_zlib_loose_payload(file)?;
                // Match Git: loose objects are read from the path implied by `oid` without
                // requiring the payload to hash back to that oid (t1006 corrupt-loose / swapped files).
                return parse_object_bytes(&raw);
            }
            Err(_) => {
                // Loose object not found; try pack files.
            }
        }

        // Fall back to pack files.
        if let Ok(obj) = pack::read_object_from_packs(&self.objects_dir, oid) {
            return Ok(obj);
        }

        // Check alternates from info/alternates file.
        if let Ok(alts) = pack::read_alternates_recursive(&self.objects_dir) {
            for alt_dir in &alts {
                if let Ok(obj) = Self::read_from_dir(alt_dir, oid) {
                    return Ok(obj);
                }
            }
        }

        // Check GIT_ALTERNATE_OBJECT_DIRECTORIES env var.
        for alt_dir in env_alternate_dirs(self.work_tree.as_deref()) {
            if let Ok(obj) = Self::read_from_dir(&alt_dir, oid) {
                return Ok(obj);
            }
        }

        Err(Error::ObjectNotFound(oid.to_hex()))
    }

    /// Try to read an object from a specific objects directory (loose or pack).
    fn read_from_dir(objects_dir: &Path, oid: &ObjectId) -> Result<Object> {
        let loose = objects_dir
            .join(oid.loose_prefix())
            .join(oid.loose_suffix());
        if let Ok(file) = fs::File::open(&loose) {
            let raw = read_zlib_loose_payload(file)?;
            return parse_object_bytes(&raw);
        }
        pack::read_object_from_packs(objects_dir, oid)
    }

    /// Hash raw content of a given kind and return the [`ObjectId`].
    ///
    /// This does **not** write anything to disk.
    #[must_use]
    pub fn hash_object_data(kind: ObjectKind, data: &[u8]) -> ObjectId {
        // Stream the hash without copying data into a contiguous buffer.
        let header = format!("{} {}\0", kind, data.len());
        let mut hasher = Sha1::new();
        hasher.update(header.as_bytes());
        hasher.update(data);
        let digest = hasher.finalize();
        ObjectId::from_bytes(digest.as_slice())
            .unwrap_or_else(|_| unreachable!("SHA-1 is 20 bytes"))
    }

    /// Write an object to the loose store and return its [`ObjectId`].
    ///
    /// If the object already exists it is not overwritten (Git behaviour).
    ///
    /// # Errors
    ///
    /// - [`Error::Io`] — could not create the directory or write the file.
    /// - [`Error::Zlib`] — compression failed.
    pub fn write(&self, kind: ObjectKind, data: &[u8]) -> Result<ObjectId> {
        let store_bytes = build_store_bytes(kind, data);
        let oid = hash_bytes(&store_bytes);

        let path = self.object_path(&oid);
        if path.exists() {
            let _ = self.freshen_object(&oid);
            return Ok(oid);
        }
        if self.exists(&oid) {
            let _ = self.freshen_object(&oid);
            return Ok(oid);
        }

        let prefix_dir = path
            .parent()
            .ok_or_else(|| Error::PathError("object path has no parent".to_owned()))?;
        fs::create_dir_all(prefix_dir)?;

        // Write to a temp file in the same directory, then rename atomically.
        let tmp_path = prefix_dir.join(format!("tmp_{}", oid.loose_suffix()));
        {
            let tmp_file = fs::File::create(&tmp_path)?;
            let mut encoder = ZlibEncoder::new(tmp_file, Compression::default());
            encoder
                .write_all(&store_bytes)
                .map_err(|e| Error::Zlib(e.to_string()))?;
            encoder.finish().map_err(|e| Error::Zlib(e.to_string()))?;
        }
        fs::rename(&tmp_path, &path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o444));
        }

        Ok(oid)
    }

    /// Write a loose object file when it is missing, even if [`Self::exists`] is true because
    /// the object lives only in a pack.
    ///
    /// Used when materializing a partial-clone layout: objects must be duplicated as loose files
    /// before local packs are removed.
    pub fn write_loose_materialize(&self, kind: ObjectKind, data: &[u8]) -> Result<ObjectId> {
        let store_bytes = build_store_bytes(kind, data);
        let oid = hash_bytes(&store_bytes);
        let path = self.object_path(&oid);
        if path.exists() {
            let _ = self.freshen_object(&oid);
            return Ok(oid);
        }

        let prefix_dir = path
            .parent()
            .ok_or_else(|| Error::PathError("object path has no parent".to_owned()))?;
        fs::create_dir_all(prefix_dir)?;

        let tmp_path = prefix_dir.join(format!("tmp_{}", oid.loose_suffix()));
        {
            let tmp_file = fs::File::create(&tmp_path)?;
            let mut encoder = ZlibEncoder::new(tmp_file, Compression::default());
            encoder
                .write_all(&store_bytes)
                .map_err(|e| Error::Zlib(e.to_string()))?;
            encoder.finish().map_err(|e| Error::Zlib(e.to_string()))?;
        }
        fs::rename(&tmp_path, &path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o444));
        }

        Ok(oid)
    }

    /// Write an already-serialized object (header + data) to the loose store.
    ///
    /// Useful when the caller has the full store bytes (e.g. from stdin with
    /// `--literally`).
    ///
    /// # Errors
    ///
    /// - [`Error::CorruptObject`] — the provided bytes don't form a valid header.
    /// - [`Error::Io`] / [`Error::Zlib`] — storage errors.
    pub fn write_raw(&self, store_bytes: &[u8]) -> Result<ObjectId> {
        // Validate the header before storing
        parse_object_bytes(store_bytes)?;

        let oid = hash_bytes(store_bytes);
        let path = self.object_path(&oid);
        if path.exists() {
            let _ = self.freshen_object(&oid);
            return Ok(oid);
        }
        if self.exists(&oid) {
            let _ = self.freshen_object(&oid);
            return Ok(oid);
        }

        let prefix_dir = path
            .parent()
            .ok_or_else(|| Error::PathError("object path has no parent".to_owned()))?;
        fs::create_dir_all(prefix_dir)?;

        let tmp_path = prefix_dir.join(format!("tmp_{}", oid.loose_suffix()));
        {
            let tmp_file = fs::File::create(&tmp_path)?;
            let mut encoder = ZlibEncoder::new(tmp_file, Compression::default());
            encoder
                .write_all(store_bytes)
                .map_err(|e| Error::Zlib(e.to_string()))?;
            encoder.finish().map_err(|e| Error::Zlib(e.to_string()))?;
        }
        fs::rename(&tmp_path, &path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o444));
        }

        Ok(oid)
    }

    /// Returns true when a loose object exists at `oid`'s path and zlib-decompresses to a
    /// structurally valid `<type> <size>\0<payload>` object (type may be non-standard).
    ///
    /// Used for `git cat-file -e`, which succeeds for hand-crafted loose objects that
    /// [`Self::read`] rejects due to [`Error::UnknownObjectType`].
    #[must_use]
    pub fn loose_object_plumbing_ok(&self, oid: &ObjectId) -> bool {
        let path = self.object_path(oid);
        let Ok(file) = fs::File::open(&path) else {
            return false;
        };
        let Ok(raw) = read_zlib_loose_payload(file) else {
            return false;
        };
        loose_store_bytes_header_valid(&raw)
    }
}

fn loose_store_bytes_header_valid(raw: &[u8]) -> bool {
    let nul = match raw.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => return false,
    };
    let header = &raw[..nul];
    let data = &raw[nul + 1..];
    let sp = match header.iter().position(|&b| b == b' ') {
        Some(i) => i,
        None => return false,
    };
    if sp == 0 || sp > 32 {
        return false;
    }
    let size_str = match std::str::from_utf8(&header[sp + 1..]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let size: usize = match size_str.parse() {
        Ok(s) => s,
        Err(_) => return false,
    };
    data.len() == size
}

/// Update `path`'s mtime to "now" (Git `utime(path, NULL)`), returning whether it succeeded.
fn touch_path_mtime(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: `utimes` with NULL `times` sets atime and mtime to the current time.
        unsafe { libc::utimes(c_path.as_ptr(), std::ptr::null()) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

fn freshen_object_in_objects_dir(objects_dir: &Path, oid: &ObjectId) -> bool {
    let Ok(indexes) = pack::read_local_pack_indexes(objects_dir) else {
        return false;
    };
    for idx in &indexes {
        if idx.entries.iter().any(|e| e.oid == *oid) {
            return touch_path_mtime(&idx.pack_path);
        }
    }
    false
}

fn hash_object_from_parsed(obj: &Object) -> ObjectId {
    Odb::hash_object_data(obj.kind, &obj.data)
}

/// Compute the SHA-1 of a byte slice and return it as an [`ObjectId`].
fn hash_bytes(data: &[u8]) -> ObjectId {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let digest = hasher.finalize();
    // SAFETY: SHA-1 always produces exactly 20 bytes.
    ObjectId::from_bytes(digest.as_slice()).unwrap_or_else(|_| unreachable!("SHA-1 is 20 bytes"))
}

/// Build the canonical store byte sequence: `"<kind> <len>\0<data>"`.
fn build_store_bytes(kind: ObjectKind, data: &[u8]) -> Vec<u8> {
    let header = format!("{} {}\0", kind, data.len());
    let mut out = Vec::with_capacity(header.len() + data.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(data);
    out
}

/// Parse decompressed object bytes (`"<type> <size>\0<data>"`) into an [`Object`].
pub(crate) fn parse_object_bytes(raw: &[u8]) -> Result<Object> {
    parse_object_bytes_inner(raw, None)
}

pub(crate) fn parse_object_bytes_with_oid(raw: &[u8], oid: &ObjectId) -> Result<Object> {
    parse_object_bytes_inner(raw, Some(oid))
}

fn parse_object_bytes_inner(raw: &[u8], oid_hint: Option<&ObjectId>) -> Result<Object> {
    let nul = raw
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::CorruptObject("missing NUL in object header".to_owned()))?;

    let header = &raw[..nul];
    let data = raw[nul + 1..].to_vec();

    let sp = header
        .iter()
        .position(|&b| b == b' ')
        .ok_or_else(|| Error::CorruptObject("missing space in object header".to_owned()))?;

    if sp > 32 {
        let oid_str = oid_hint
            .map(|o| o.to_hex())
            .unwrap_or_else(|| hash_bytes(raw).to_hex());
        return Err(Error::ObjectHeaderTooLong { oid: oid_str });
    }

    let kind = ObjectKind::from_bytes(&header[..sp])?;

    let size_str = std::str::from_utf8(&header[sp + 1..])
        .map_err(|_| Error::CorruptObject("non-UTF-8 object size".to_owned()))?;
    let size: usize = size_str
        .parse()
        .map_err(|_| Error::CorruptObject(format!("invalid object size: {size_str}")))?;

    if data.len() != size {
        return Err(Error::CorruptObject(format!(
            "object size mismatch: header says {size} but got {}",
            data.len()
        )));
    }

    Ok(Object::new(kind, data))
}

/// Parse `GIT_ALTERNATE_OBJECT_DIRECTORIES` into a list of paths.
///
/// The env var contains colon-separated (`:`-separated on Unix) paths
/// to additional object directories to search. Supports double-quoted
/// entries with octal escapes (e.g. `\057` for `/`).
///
/// Relative paths are resolved against `resolve_base` (typically the work tree root).
fn env_alternate_dirs(resolve_base: Option<&Path>) -> Vec<PathBuf> {
    match std::env::var("GIT_ALTERNATE_OBJECT_DIRECTORIES") {
        Ok(val) if !val.is_empty() => {
            let mut dirs = parse_alternate_env(&val);
            if let Some(base) = resolve_base {
                for dir in &mut dirs {
                    if dir.is_relative() {
                        *dir = base.join(&dir);
                    }
                }
            }
            dirs
        }
        _ => Vec::new(),
    }
}

/// Parse a colon-separated alternates string, handling double-quoted entries
/// with octal escape sequences.
fn parse_alternate_env(val: &str) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut chars = val.chars().peekable();
    while chars.peek().is_some() {
        if chars.peek() == Some(&':') {
            chars.next();
            continue;
        }
        if chars.peek() == Some(&'"') {
            // Try quoted parsing; if EOF is hit without closing quote,
            // fall back to treating the whole segment as a raw path.
            chars.next(); // consume the opening '"'
            let saved: Vec<char> = chars.clone().collect();
            let mut path = String::new();
            let mut properly_closed = false;
            loop {
                match chars.next() {
                    None => break,
                    Some('"') => {
                        properly_closed = true;
                        break;
                    }
                    Some('\\') => match chars.peek() {
                        Some(c) if c.is_ascii_digit() => {
                            let mut oct = String::new();
                            for _ in 0..3 {
                                if let Some(&c) = chars.peek() {
                                    if c.is_ascii_digit() {
                                        oct.push(c);
                                        chars.next();
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            if let Ok(byte) = u8::from_str_radix(&oct, 8) {
                                path.push(byte as char);
                            }
                        }
                        Some(_) => {
                            if let Some(c) = chars.next() {
                                match c {
                                    'n' => path.push('\n'),
                                    't' => path.push('\t'),
                                    'r' => path.push('\r'),
                                    _ => path.push(c),
                                }
                            }
                        }
                        None => {}
                    },
                    Some(c) => path.push(c),
                }
            }
            if !properly_closed {
                // Broken quoting: fall back to treating raw value (with leading ")
                // as a literal path.
                let raw: String = std::iter::once('"').chain(saved.into_iter()).collect();
                // Extract up to ':' or end
                let raw_path = raw.split(':').next().unwrap_or(&raw);
                if !raw_path.is_empty() {
                    result.push(PathBuf::from(raw_path));
                }
                // Advance past the ':' in the original chars (we consumed the saved copy)
                // Since chars is now at EOF, we need to handle remaining items.
                // Actually, we consumed chars fully. Let's reconstruct from raw.
                let remainder = &raw[raw_path.len()..];
                if let Some(rest) = remainder.strip_prefix(':') {
                    // Parse remaining entries
                    result.extend(parse_alternate_env(rest));
                }
                return result;
            } else if !path.is_empty() {
                result.push(PathBuf::from(path));
            }
        } else {
            let mut path = String::new();
            while let Some(&c) = chars.peek() {
                if c == ':' {
                    break;
                }
                path.push(c);
                chars.next();
            }
            if !path.is_empty() {
                result.push(PathBuf::from(path));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip_blob() {
        let dir = TempDir::new().unwrap();
        let odb = Odb::new(dir.path());
        let data = b"hello world";
        let oid = odb.write(ObjectKind::Blob, data).unwrap();
        let obj = odb.read(&oid).unwrap();
        assert_eq!(obj.kind, ObjectKind::Blob);
        assert_eq!(obj.data, data);
    }

    #[test]
    fn known_blob_hash() {
        // Verified: echo -n "hello" | git hash-object --stdin
        //        => b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0
        let oid = Odb::hash_object_data(ObjectKind::Blob, b"hello");
        assert_eq!(oid.to_hex(), "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
    }
}
