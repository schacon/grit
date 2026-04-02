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

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::objects::{Object, ObjectId, ObjectKind};

/// A loose-object database rooted at a given `objects/` directory.
#[derive(Debug, Clone)]
pub struct Odb {
    objects_dir: PathBuf,
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

    /// Check whether an object exists in the loose store.
    #[must_use]
    pub fn exists(&self, oid: &ObjectId) -> bool {
        self.object_path(oid).exists()
    }

    /// Read and decompress an object from the loose store.
    ///
    /// # Errors
    ///
    /// - [`Error::ObjectNotFound`] — no file at the expected path.
    /// - [`Error::Zlib`] — decompression failed.
    /// - [`Error::CorruptObject`] — header is malformed.
    pub fn read(&self, oid: &ObjectId) -> Result<Object> {
        let path = self.object_path(oid);
        let file = fs::File::open(&path).map_err(|_| Error::ObjectNotFound(oid.to_hex()))?;

        let mut decoder = ZlibDecoder::new(file);
        let mut raw = Vec::new();
        decoder
            .read_to_end(&mut raw)
            .map_err(|e| Error::Zlib(e.to_string()))?;

        parse_object_bytes(&raw)
    }

    /// Hash raw content of a given kind and return the [`ObjectId`].
    ///
    /// This does **not** write anything to disk.
    #[must_use]
    pub fn hash_object_data(kind: ObjectKind, data: &[u8]) -> ObjectId {
        let store_bytes = build_store_bytes(kind, data);
        hash_bytes(&store_bytes)
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

        Ok(oid)
    }
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
