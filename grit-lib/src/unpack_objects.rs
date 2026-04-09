//! `unpack-objects`: unpack a pack stream into loose objects.
//!
//! Reads a pack-format byte stream, validates the trailing checksum, and
//! writes each object as a loose file in the object database.  Delta objects
//! (both `OFS_DELTA` and `REF_DELTA`) are resolved against already-unpacked
//! objects or objects already present in the ODB.

use std::collections::HashMap;
use std::io::Read;

use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::objects::{Object, ObjectId, ObjectKind};
use crate::odb::Odb;

/// Options controlling `unpack-objects` behaviour.
#[derive(Debug, Default)]
pub struct UnpackOptions {
    /// Validate and decompress objects but do not write them to the ODB.
    pub dry_run: bool,
    /// Suppress informational output.
    pub quiet: bool,
}

/// A delta that could not yet be resolved because its base was not yet known.
struct PendingDelta {
    /// Byte offset of this object in the pack stream (used to anchor
    /// `OFS_DELTA` back-references from later objects).
    offset: usize,
    /// For `REF_DELTA`: SHA-1 of the base object.
    base_oid: Option<ObjectId>,
    /// For `OFS_DELTA`: absolute byte offset of the base object.
    base_offset: Option<usize>,
    /// Decompressed delta data.
    delta_data: Vec<u8>,
}

/// Unpack a pack stream from `reader` into `odb`.
///
/// Reads the complete pack from `reader`, validates the trailing SHA-1
/// checksum, unpacks all objects (including full delta-chain resolution), and —
/// unless [`UnpackOptions::dry_run`] is set — writes each object to `odb`.
///
/// Returns the total number of objects processed.
///
/// # Errors
///
/// - [`Error::CorruptObject`] — invalid pack format, checksum mismatch, or
///   unresolvable delta chains.
/// - [`Error::Io`] — I/O failure reading from `reader`.
/// - [`Error::Zlib`] — decompression failure.
pub fn unpack_objects(reader: &mut dyn Read, odb: &Odb, opts: &UnpackOptions) -> Result<usize> {
    let mut raw = Vec::new();
    reader.read_to_end(&mut raw).map_err(Error::Io)?;

    let mut rd = PackReader::new(raw);

    // Validate magic and version.
    let sig = rd.read_exact(4)?;
    if sig != b"PACK" {
        return Err(Error::CorruptObject(
            "not a pack stream: invalid signature".to_owned(),
        ));
    }
    let version = rd.read_u32_be()?;
    if version != 2 && version != 3 {
        return Err(Error::CorruptObject(format!(
            "unsupported pack version {version}"
        )));
    }
    let nr_objects = rd.read_u32_be()? as usize;

    // Maps used for delta resolution.
    // pack-stream offset → (kind, decompressed-data) for objects resolved in
    // this pass (needed to service OFS_DELTA back-references).
    let mut by_offset: HashMap<usize, (ObjectKind, Vec<u8>)> = HashMap::new();
    // ObjectId → (kind, data) for in-pack objects (REF_DELTA resolution).
    let mut by_oid: HashMap<ObjectId, (ObjectKind, Vec<u8>)> = HashMap::new();

    let mut pending: Vec<PendingDelta> = Vec::new();
    let mut count = 0usize;

    for _ in 0..nr_objects {
        let obj_offset = rd.pos;
        let (type_code, size) = rd.read_type_size()?;

        match type_code {
            1..=4 => {
                let kind = type_code_to_kind(type_code)?;
                let data = rd.decompress(size)?;
                let oid = write_or_hash(kind, &data, odb, opts.dry_run)?;
                by_offset.insert(obj_offset, (kind, data.clone()));
                by_oid.insert(oid, (kind, data));
                count += 1;
            }
            6 => {
                // OFS_DELTA: base at a negative encoded offset from this object.
                let neg = rd.read_ofs_neg_offset()?;
                let base_offset = obj_offset.checked_sub(neg).ok_or_else(|| {
                    Error::CorruptObject("ofs-delta base offset underflow".to_owned())
                })?;
                let delta_data = rd.decompress(size)?;
                pending.push(PendingDelta {
                    offset: obj_offset,
                    base_oid: None,
                    base_offset: Some(base_offset),
                    delta_data,
                });
            }
            7 => {
                // REF_DELTA: base identified by its SHA-1.
                let base_bytes = rd.read_exact(20)?;
                let base_oid = ObjectId::from_bytes(base_bytes)?;
                let delta_data = rd.decompress(size)?;
                pending.push(PendingDelta {
                    offset: obj_offset,
                    base_oid: Some(base_oid),
                    base_offset: None,
                    delta_data,
                });
            }
            other => {
                return Err(Error::CorruptObject(format!(
                    "unknown packed-object type {other}"
                )))
            }
        }
    }

    // Verify the pack trailing checksum: SHA-1 over all bytes consumed so far.
    let consumed = rd.pos;
    {
        let mut hasher = Sha1::new();
        hasher.update(&rd.data[..consumed]);
        let digest = hasher.finalize();
        let trailing = rd.read_exact(20)?;
        if digest.as_slice() != trailing {
            return Err(Error::CorruptObject(
                "pack trailing checksum mismatch".to_owned(),
            ));
        }
    }

    // Resolve pending deltas iteratively.  Each pass resolves all deltas whose
    // base is now known; repeat until none remain or we stall (corrupt pack).
    let mut remaining = pending;
    loop {
        if remaining.is_empty() {
            break;
        }
        let before = remaining.len();
        let mut still_pending: Vec<PendingDelta> = Vec::new();

        for delta in remaining {
            let base = if let Some(base_off) = delta.base_offset {
                by_offset.get(&base_off).cloned()
            } else if let Some(ref base_id) = delta.base_oid {
                if let Some(entry) = by_oid.get(base_id) {
                    Some(entry.clone())
                } else if !opts.dry_run {
                    odb.read(base_id).ok().map(|obj| (obj.kind, obj.data))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some((base_kind, base_data)) = base {
                let result = apply_delta(&base_data, &delta.delta_data)?;
                let oid = write_or_hash(base_kind, &result, odb, opts.dry_run)?;
                by_offset.insert(delta.offset, (base_kind, result.clone()));
                by_oid.insert(oid, (base_kind, result));
                count += 1;
            } else {
                still_pending.push(delta);
            }
        }

        remaining = still_pending;
        if remaining.len() == before {
            return Err(Error::CorruptObject(format!(
                "{} delta(s) could not be resolved",
                remaining.len()
            )));
        }
    }

    Ok(count)
}

/// Parse a pack byte stream and return every resolved object (after delta resolution) keyed by OID.
///
/// Does not write to any object database. Used for receive-pack connectivity checks before
/// applying a push to the permanent ODB.
///
/// Thin-pack bases may be resolved from `odb` when they are not present in the pack.
pub fn pack_bytes_to_object_map(data: &[u8], odb: &Odb) -> Result<HashMap<ObjectId, Object>> {
    let rd = PackReader::new(data.to_vec());
    build_pack_object_map(rd, odb)
}

fn build_pack_object_map(mut rd: PackReader, odb: &Odb) -> Result<HashMap<ObjectId, Object>> {
    let sig = rd.read_exact(4)?;
    if sig != b"PACK" {
        return Err(Error::CorruptObject(
            "not a pack stream: invalid signature".to_owned(),
        ));
    }
    let version = rd.read_u32_be()?;
    if version != 2 && version != 3 {
        return Err(Error::CorruptObject(format!(
            "unsupported pack version {version}"
        )));
    }
    let nr_objects = rd.read_u32_be()? as usize;

    let mut by_offset: HashMap<usize, (ObjectKind, Vec<u8>)> = HashMap::new();
    let mut by_oid: HashMap<ObjectId, (ObjectKind, Vec<u8>)> = HashMap::new();
    let mut pending: Vec<PendingDelta> = Vec::new();

    fn base_from_pack_or_odb(
        by_oid: &HashMap<ObjectId, (ObjectKind, Vec<u8>)>,
        odb: &Odb,
        id: &ObjectId,
    ) -> Option<(ObjectKind, Vec<u8>)> {
        if let Some(e) = by_oid.get(id) {
            return Some(e.clone());
        }
        odb.read(id).ok().map(|o| (o.kind, o.data))
    }

    for _ in 0..nr_objects {
        let obj_offset = rd.pos;
        let (type_code, size) = rd.read_type_size()?;

        match type_code {
            1..=4 => {
                let kind = type_code_to_kind(type_code)?;
                let data = rd.decompress(size)?;
                let oid = Odb::hash_object_data(kind, &data);
                by_offset.insert(obj_offset, (kind, data.clone()));
                by_oid.insert(oid, (kind, data));
            }
            6 => {
                let neg = rd.read_ofs_neg_offset()?;
                let base_offset = obj_offset.checked_sub(neg).ok_or_else(|| {
                    Error::CorruptObject("ofs-delta base offset underflow".to_owned())
                })?;
                let delta_data = rd.decompress(size)?;
                pending.push(PendingDelta {
                    offset: obj_offset,
                    base_oid: None,
                    base_offset: Some(base_offset),
                    delta_data,
                });
            }
            7 => {
                let base_bytes = rd.read_exact(20)?;
                let base_oid = ObjectId::from_bytes(base_bytes)?;
                let delta_data = rd.decompress(size)?;
                pending.push(PendingDelta {
                    offset: obj_offset,
                    base_oid: Some(base_oid),
                    base_offset: None,
                    delta_data,
                });
            }
            other => {
                return Err(Error::CorruptObject(format!(
                    "unknown packed-object type {other}"
                )))
            }
        }
    }

    let consumed = rd.pos;
    {
        let mut hasher = Sha1::new();
        hasher.update(&rd.data[..consumed]);
        let digest = hasher.finalize();
        let trailing = rd.read_exact(20)?;
        if digest.as_slice() != trailing {
            return Err(Error::CorruptObject(
                "pack trailing checksum mismatch".to_owned(),
            ));
        }
    }

    let mut remaining = pending;
    loop {
        if remaining.is_empty() {
            break;
        }
        let before = remaining.len();
        let mut still_pending: Vec<PendingDelta> = Vec::new();

        for delta in remaining {
            let base = if let Some(base_off) = delta.base_offset {
                by_offset.get(&base_off).cloned()
            } else if let Some(ref base_id) = delta.base_oid {
                base_from_pack_or_odb(&by_oid, odb, base_id)
            } else {
                None
            };

            if let Some((base_kind, base_data)) = base {
                let result = apply_delta(&base_data, &delta.delta_data)?;
                let oid = Odb::hash_object_data(base_kind, &result);
                by_offset.insert(delta.offset, (base_kind, result.clone()));
                by_oid.insert(oid, (base_kind, result));
            } else {
                still_pending.push(delta);
            }
        }

        remaining = still_pending;
        if remaining.len() == before {
            return Err(Error::CorruptObject(format!(
                "{} delta(s) could not be resolved",
                remaining.len()
            )));
        }
    }

    Ok(by_oid
        .into_iter()
        .map(|(oid, (kind, data))| (oid, Object::new(kind, data)))
        .collect())
}

/// Either write `data` as a loose object (if `!dry_run`) or just compute its
/// [`ObjectId`] without touching the filesystem.
fn write_or_hash(kind: ObjectKind, data: &[u8], odb: &Odb, dry_run: bool) -> Result<ObjectId> {
    if dry_run {
        Ok(Odb::hash_object_data(kind, data))
    } else {
        odb.write(kind, data)
    }
}

/// Convert a pack object type code to an [`ObjectKind`].
fn type_code_to_kind(code: u8) -> Result<ObjectKind> {
    match code {
        1 => Ok(ObjectKind::Commit),
        2 => Ok(ObjectKind::Tree),
        3 => Ok(ObjectKind::Blob),
        4 => Ok(ObjectKind::Tag),
        _ => Err(Error::CorruptObject(format!(
            "type code {code} is not a regular object type"
        ))),
    }
}

/// Low-level cursor over a buffered pack byte stream.
struct PackReader {
    data: Vec<u8>,
    pos: usize,
}

impl PackReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    /// Read exactly `n` bytes and advance the cursor, returning a slice into
    /// the internal buffer.
    fn read_exact(&mut self, n: usize) -> Result<&[u8]> {
        if self.pos + n > self.data.len() {
            return Err(Error::CorruptObject(format!(
                "pack stream truncated: need {n} bytes at offset {}",
                self.pos
            )));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Read a single byte and advance the cursor.
    fn read_byte(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            return Err(Error::CorruptObject(
                "unexpected end of pack stream".to_owned(),
            ));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    /// Read a big-endian `u32`.
    fn read_u32_be(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| {
            Error::CorruptObject("u32 read failed".to_owned())
        })?))
    }

    /// Read the packed-object type + size header (variable-length big-endian
    /// encoding with the type in bits 4-6 of the first byte).
    ///
    /// Returns `(type_code, uncompressed_size)`.
    fn read_type_size(&mut self) -> Result<(u8, usize)> {
        let c = self.read_byte()?;
        let type_code = (c >> 4) & 0x7;
        let mut size = (c & 0x0f) as usize;
        let mut shift = 4u32;
        let mut cur = c;
        while cur & 0x80 != 0 {
            cur = self.read_byte()?;
            size |= ((cur & 0x7f) as usize) << shift;
            shift += 7;
        }
        Ok((type_code, size))
    }

    /// Read an `OFS_DELTA` negative-offset value.
    ///
    /// The encoding uses a big-endian variable-length integer with a +1 bias
    /// on each continuation byte, yielding values ≥ 1.
    fn read_ofs_neg_offset(&mut self) -> Result<usize> {
        let mut c = self.read_byte()?;
        let mut value = (c & 0x7f) as usize;
        while c & 0x80 != 0 {
            c = self.read_byte()?;
            value = (value + 1) << 7 | (c & 0x7f) as usize;
        }
        Ok(value)
    }

    /// Decompress zlib-compressed data starting at the current cursor position.
    ///
    /// Advances the cursor by exactly the number of compressed bytes consumed.
    /// Returns an error if the decompressed length differs from `expected_size`.
    fn decompress(&mut self, expected_size: usize) -> Result<Vec<u8>> {
        let slice = &self.data[self.pos..];
        let mut decoder = ZlibDecoder::new(slice);
        let mut out = Vec::with_capacity(expected_size);
        decoder
            .read_to_end(&mut out)
            .map_err(|e| Error::Zlib(e.to_string()))?;
        if out.len() != expected_size {
            return Err(Error::CorruptObject(format!(
                "decompressed {} bytes but expected {}",
                out.len(),
                expected_size
            )));
        }
        self.pos += decoder.total_in() as usize;
        Ok(out)
    }
}

/// Apply a git "patch delta" to `base`, producing the patched result.
///
/// The delta binary format is:
/// 1. Source size: variable-length little-endian integer (must equal
///    `base.len()`).
/// 2. Destination size: variable-length little-endian integer.
/// 3. A sequence of COPY (MSB set) and INSERT (MSB clear) instructions.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] if the delta is malformed, the source-size
/// field does not match `base.len()`, or the result length does not match the
/// declared destination size.
pub fn apply_delta(base: &[u8], delta: &[u8]) -> Result<Vec<u8>> {
    let mut pos = 0usize;

    let src_size = read_delta_varint(delta, &mut pos)?;
    if src_size != base.len() {
        return Err(Error::CorruptObject(format!(
            "delta source size {src_size} != base size {}",
            base.len()
        )));
    }
    let dest_size = read_delta_varint(delta, &mut pos)?;
    let mut result = Vec::with_capacity(dest_size);

    while pos < delta.len() {
        let cmd = delta[pos];
        pos += 1;
        if cmd == 0 {
            return Err(Error::CorruptObject(
                "reserved opcode 0 in delta stream".to_owned(),
            ));
        }
        if cmd & 0x80 != 0 {
            // COPY instruction: up to 4 offset bytes (bits 0-3) and up to 3
            // size bytes (bits 4-6) are present, each controlled by a flag bit.
            let mut offset = 0usize;
            let mut size = 0usize;

            macro_rules! maybe_read_byte {
                ($flag:expr, $shift:expr, $target:expr) => {
                    if cmd & $flag != 0 {
                        let b = *delta.get(pos).ok_or_else(|| {
                            Error::CorruptObject("truncated delta COPY operand".to_owned())
                        })?;
                        pos += 1;
                        $target |= (b as usize) << $shift;
                    }
                };
            }

            maybe_read_byte!(0x01, 0, offset);
            maybe_read_byte!(0x02, 8, offset);
            maybe_read_byte!(0x04, 16, offset);
            maybe_read_byte!(0x08, 24, offset);
            maybe_read_byte!(0x10, 0, size);
            maybe_read_byte!(0x20, 8, size);
            maybe_read_byte!(0x40, 16, size);

            if size == 0 {
                size = 0x10000;
            }

            let end = offset.checked_add(size).ok_or_else(|| {
                Error::CorruptObject("delta COPY range overflows usize".to_owned())
            })?;
            let chunk = base.get(offset..end).ok_or_else(|| {
                Error::CorruptObject(format!(
                    "delta COPY [{offset},{end}) out of range (base is {} bytes)",
                    base.len()
                ))
            })?;
            result.extend_from_slice(chunk);
        } else {
            // INSERT instruction: copy the next `cmd` literal bytes verbatim.
            let n = cmd as usize;
            let chunk = delta
                .get(pos..pos + n)
                .ok_or_else(|| Error::CorruptObject("truncated delta INSERT data".to_owned()))?;
            result.extend_from_slice(chunk);
            pos += n;
        }
    }

    if result.len() != dest_size {
        return Err(Error::CorruptObject(format!(
            "delta produced {} bytes but expected {dest_size}",
            result.len()
        )));
    }

    Ok(result)
}

/// Read a variable-length little-endian integer from `data` starting at `*pos`.
///
/// Advances `*pos` past the consumed bytes.
fn read_delta_varint(data: &[u8], pos: &mut usize) -> Result<usize> {
    let mut value = 0usize;
    let mut shift = 0u32;
    loop {
        let b = *data
            .get(*pos)
            .ok_or_else(|| Error::CorruptObject("truncated delta varint".to_owned()))?;
        *pos += 1;
        value |= ((b & 0x7f) as usize) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            break;
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a minimal pack from a list of (kind, data) pairs.
    // Returns the raw pack bytes.
    fn make_pack(objects: &[(ObjectKind, &[u8])]) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let mut entries: Vec<Vec<u8>> = Vec::new();
        for (kind, data) in objects {
            let type_code: u8 = match kind {
                ObjectKind::Commit => 1,
                ObjectKind::Tree => 2,
                ObjectKind::Blob => 3,
                ObjectKind::Tag => 4,
            };
            // Encode type+size header.
            let mut header = Vec::new();
            let mut size = data.len();
            let first = ((type_code & 0x7) << 4) as u8 | (size & 0x0f) as u8;
            size >>= 4;
            if size > 0 {
                header.push(first | 0x80);
                while size > 0 {
                    let b = (size & 0x7f) as u8;
                    size >>= 7;
                    header.push(if size > 0 { b | 0x80 } else { b });
                }
            } else {
                header.push(first);
            }
            // zlib-compress data.
            let mut enc = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(data).unwrap();
            let compressed = enc.finish().unwrap();
            let mut entry = header;
            entry.extend_from_slice(&compressed);
            entries.push(entry);
        }

        // Assemble: PACK + version(2) + count + entries + SHA-1.
        let mut pack = Vec::new();
        pack.extend_from_slice(b"PACK");
        pack.extend_from_slice(&2u32.to_be_bytes());
        pack.extend_from_slice(&(objects.len() as u32).to_be_bytes());
        for entry in &entries {
            pack.extend_from_slice(entry);
        }
        let mut hasher = Sha1::new();
        hasher.update(&pack);
        let digest = hasher.finalize();
        pack.extend_from_slice(digest.as_slice());
        pack
    }

    #[test]
    fn test_apply_delta_simple() {
        // Build a trivial delta: insert "hello world".
        let base = b"hello";
        let mut delta = Vec::new();
        // src_size = 5
        delta.push(5u8);
        // dest_size = 11
        delta.push(11u8);
        // COPY instruction: copy base[0..5]
        // cmd = 0x80 | 0x01 (offset present, byte 0) | 0x10 (size byte 0)
        delta.push(0x80 | 0x01 | 0x10); // 0x91
        delta.push(0u8); // offset = 0
        delta.push(5u8); // size = 5
                         // INSERT " world" (6 bytes)
        delta.push(6u8);
        delta.extend_from_slice(b" world");

        let result = apply_delta(base, &delta).unwrap();
        assert_eq!(result, b"hello world");
    }

    #[test]
    fn test_apply_delta_insert_only() {
        let base = b"";
        let mut delta = Vec::new();
        delta.push(0u8); // src_size = 0
        delta.push(5u8); // dest_size = 5
        delta.push(5u8); // INSERT 5 bytes
        delta.extend_from_slice(b"hello");

        let result = apply_delta(base, &delta).unwrap();
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_apply_delta_copy_only() {
        let base = b"abcdef";
        let mut delta = Vec::new();
        delta.push(6u8); // src_size = 6
        delta.push(3u8); // dest_size = 3
                         // COPY base[2..5]: offset=2, size=3
                         // cmd = 0x80 | 0x01 | 0x10
        delta.push(0x91u8);
        delta.push(2u8); // offset = 2
        delta.push(3u8); // size = 3

        let result = apply_delta(base, &delta).unwrap();
        assert_eq!(result, b"cde");
    }

    #[test]
    fn test_apply_delta_size_zero_means_65536() {
        // A COPY with size bytes all zero means 0x10000 = 65536.
        let base = vec![0xABu8; 65536];
        let mut delta = Vec::new();
        // src_size = 65536, encoded as 3 bytes little-endian varint
        delta.push(0x80 | (65536 & 0x7f) as u8); // 0
        delta.push(0x80 | ((65536 >> 7) & 0x7f) as u8); // 0x80
        delta.push(((65536 >> 14) & 0x7f) as u8); // 4
                                                  // dest_size = 65536, same
        delta.push(0x80 | (65536 & 0x7f) as u8);
        delta.push(0x80 | ((65536 >> 7) & 0x7f) as u8);
        delta.push(((65536 >> 14) & 0x7f) as u8);
        // COPY: offset=0 (no offset bytes), size=0 (no size bytes) → means 0x10000
        // cmd = 0x80 (no offset/size bytes present at all → offset=0, size=0→65536)
        delta.push(0x80u8);

        let result = apply_delta(&base, &delta).unwrap();
        assert_eq!(result.len(), 65536);
        assert!(result.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn test_unpack_objects_blobs() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let objects_dir = tmp.path().join("objects");
        std::fs::create_dir_all(&objects_dir).unwrap();
        let odb = Odb::new(&objects_dir);

        let pack = make_pack(&[
            (ObjectKind::Blob, b"hello\n"),
            (ObjectKind::Blob, b"world\n"),
        ]);

        let opts = UnpackOptions::default();
        let count = unpack_objects(&mut pack.as_slice(), &odb, &opts).unwrap();
        assert_eq!(count, 2);

        // Verify both blobs can be read back.
        let oid1 = Odb::hash_object_data(ObjectKind::Blob, b"hello\n");
        let oid2 = Odb::hash_object_data(ObjectKind::Blob, b"world\n");
        let obj1 = odb.read(&oid1).unwrap();
        let obj2 = odb.read(&oid2).unwrap();
        assert_eq!(obj1.data, b"hello\n");
        assert_eq!(obj2.data, b"world\n");
    }

    #[test]
    fn test_unpack_objects_dry_run_writes_nothing() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let objects_dir = tmp.path().join("objects");
        std::fs::create_dir_all(&objects_dir).unwrap();
        let odb = Odb::new(&objects_dir);

        let pack = make_pack(&[(ObjectKind::Blob, b"test content")]);

        let opts = UnpackOptions {
            dry_run: true,
            quiet: true,
        };
        let count = unpack_objects(&mut pack.as_slice(), &odb, &opts).unwrap();
        assert_eq!(count, 1);

        // Nothing should be written.
        let oid = Odb::hash_object_data(ObjectKind::Blob, b"test content");
        assert!(!odb.exists(&oid));
    }

    #[test]
    fn test_unpack_objects_bad_signature() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let objects_dir = tmp.path().join("objects");
        std::fs::create_dir_all(&objects_dir).unwrap();
        let odb = Odb::new(&objects_dir);

        let mut bad = b"NOPE\x00\x00\x00\x02\x00\x00\x00\x00".to_vec();
        bad.extend_from_slice(&[0u8; 20]);
        let opts = UnpackOptions::default();
        let err = unpack_objects(&mut bad.as_slice(), &odb, &opts).unwrap_err();
        assert!(err.to_string().contains("invalid signature"));
    }

    #[test]
    fn test_unpack_objects_checksum_mismatch() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let objects_dir = tmp.path().join("objects");
        std::fs::create_dir_all(&objects_dir).unwrap();
        let odb = Odb::new(&objects_dir);

        let mut pack = make_pack(&[(ObjectKind::Blob, b"data")]);
        // Corrupt the trailing checksum.
        let n = pack.len();
        pack[n - 1] ^= 0xFF;

        let opts = UnpackOptions::default();
        let err = unpack_objects(&mut pack.as_slice(), &odb, &opts).unwrap_err();
        assert!(err.to_string().contains("checksum"));
    }

    #[test]
    fn test_apply_delta_source_size_mismatch() {
        let base = b"hi";
        let delta = [3u8, 2u8, 2u8, b'h', b'i']; // src_size=3 != base.len()=2
        let err = apply_delta(base, &delta).unwrap_err();
        assert!(err.to_string().contains("source size"));
    }
}
