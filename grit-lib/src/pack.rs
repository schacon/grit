//! Pack and pack-index helpers for object counting and verification.
//!
//! This module implements a focused subset of pack functionality required by
//! `count-objects`, `verify-pack`, and `show-index`.

use crate::error::{Error, Result};
use crate::objects::{Object, ObjectId, ObjectKind};
use crate::odb::Odb;
use crate::unpack_objects::apply_delta;
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A parsed entry from an index file.
#[derive(Debug, Clone)]
pub struct PackIndexEntry {
    /// Object identifier.
    pub oid: ObjectId,
    /// Byte offset of the object in the corresponding `.pack`.
    pub offset: u64,
}

/// Parsed data from a `.idx` file (version 2).
#[derive(Debug, Clone)]
pub struct PackIndex {
    /// Absolute path to the `.idx` file.
    pub idx_path: PathBuf,
    /// Absolute path to the `.pack` file.
    pub pack_path: PathBuf,
    /// Parsed entries in index order.
    pub entries: Vec<PackIndexEntry>,
}

/// A single entry produced by `show-index`, with an optional CRC32.
///
/// Version-1 index files do not store CRC32 values; `crc32` is `None` for
/// those entries.  Version-2 index files always carry a CRC32.
#[derive(Debug, Clone)]
pub struct ShowIndexEntry {
    /// Object identifier.
    pub oid: ObjectId,
    /// Byte offset of the object in the corresponding `.pack` file.
    pub offset: u64,
    /// CRC32 of the compressed object data (v2 only).
    pub crc32: Option<u32>,
}

/// Parse a pack index from a reader (e.g. stdin) and return all entries in
/// index order.
///
/// Both version-1 (legacy) and version-2 index formats are supported.  Only
/// SHA-1 (20-byte hash) objects are supported; pass `hash_size = 20`.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] when the data cannot be parsed as a valid
/// pack index.
pub fn show_index_entries(reader: &mut dyn Read, hash_size: usize) -> Result<Vec<ShowIndexEntry>> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(Error::Io)?;

    if buf.len() < 8 {
        return Err(Error::CorruptObject(
            "unable to read header: index file too small".to_owned(),
        ));
    }

    let mut pos = 0usize;
    let first_u32 = read_u32_be(&buf, &mut pos)?;

    const PACK_IDX_SIGNATURE: u32 = 0xff74_4f63;

    if first_u32 == PACK_IDX_SIGNATURE {
        // Version 2 (or higher): read version word, then 256-entry fanout.
        let version = read_u32_be(&buf, &mut pos)?;
        if version != 2 {
            return Err(Error::CorruptObject(format!(
                "unknown index version: {version}"
            )));
        }
        show_index_v2(&buf, &mut pos, hash_size)
    } else {
        // Version 1: the two u32s we already started reading are the first two
        // fanout entries.  Re-read the whole fanout from the top.
        pos = 0;
        show_index_v1(&buf, &mut pos, hash_size)
    }
}

/// Parse version-1 pack index entries from `buf`.
fn show_index_v1(buf: &[u8], pos: &mut usize, hash_size: usize) -> Result<Vec<ShowIndexEntry>> {
    if buf.len() < 256 * 4 {
        return Err(Error::CorruptObject(
            "unable to read index: v1 fanout too short".to_owned(),
        ));
    }
    let mut fanout = [0u32; 256];
    for slot in &mut fanout {
        *slot = read_u32_be(buf, pos)?;
    }
    let object_count = fanout[255] as usize;

    let mut entries = Vec::with_capacity(object_count);
    for i in 0..object_count {
        // Each record: 4-byte big-endian offset + hash_size-byte OID.
        if *pos + 4 + hash_size > buf.len() {
            return Err(Error::CorruptObject(format!(
                "unable to read entry {i}/{object_count}: truncated"
            )));
        }
        let offset = read_u32_be(buf, pos)? as u64;
        let oid = ObjectId::from_bytes(&buf[*pos..*pos + hash_size])?;
        *pos += hash_size;
        entries.push(ShowIndexEntry {
            oid,
            offset,
            crc32: None,
        });
    }
    Ok(entries)
}

/// Parse version-2 pack index entries from `buf` starting after the magic and
/// version words (fanout table is next).
fn show_index_v2(buf: &[u8], pos: &mut usize, hash_size: usize) -> Result<Vec<ShowIndexEntry>> {
    if buf.len() < *pos + 256 * 4 {
        return Err(Error::CorruptObject(
            "unable to read index: v2 fanout too short".to_owned(),
        ));
    }
    let mut fanout = [0u32; 256];
    for slot in &mut fanout {
        *slot = read_u32_be(buf, pos)?;
    }
    let object_count = fanout[255] as usize;

    // OID table.
    let mut oids = Vec::with_capacity(object_count);
    for i in 0..object_count {
        if *pos + hash_size > buf.len() {
            return Err(Error::CorruptObject(format!(
                "unable to read sha1 {i}/{object_count}: truncated"
            )));
        }
        let oid = ObjectId::from_bytes(&buf[*pos..*pos + hash_size])?;
        *pos += hash_size;
        oids.push(oid);
    }

    // CRC32 table.
    let mut crcs = Vec::with_capacity(object_count);
    for i in 0..object_count {
        if *pos + 4 > buf.len() {
            return Err(Error::CorruptObject(format!(
                "unable to read crc {i}/{object_count}: truncated"
            )));
        }
        crcs.push(read_u32_be(buf, pos)?);
    }

    // 32-bit offset table.
    let mut offsets32 = Vec::with_capacity(object_count);
    let mut large_count = 0usize;
    for i in 0..object_count {
        if *pos + 4 > buf.len() {
            return Err(Error::CorruptObject(format!(
                "unable to read 32b offset {i}/{object_count}: truncated"
            )));
        }
        let v = read_u32_be(buf, pos)?;
        if (v & 0x8000_0000) != 0 {
            large_count += 1;
        }
        offsets32.push(v);
    }

    // 64-bit large-offset table.
    let mut large_offsets = Vec::with_capacity(large_count);
    for i in 0..large_count {
        if *pos + 8 > buf.len() {
            return Err(Error::CorruptObject(format!(
                "unable to read 64b offset {i}: truncated"
            )));
        }
        large_offsets.push(read_u64_be(buf, pos)?);
    }

    let mut next_large = 0usize;
    let mut entries = Vec::with_capacity(object_count);
    for (i, oid) in oids.into_iter().enumerate() {
        let raw = offsets32[i];
        let offset = if (raw & 0x8000_0000) == 0 {
            raw as u64
        } else {
            let idx = (raw & 0x7fff_ffff) as usize;
            if idx != next_large {
                return Err(Error::CorruptObject(format!(
                    "inconsistent 64b offset index at entry {i}"
                )));
            }
            let off = large_offsets.get(next_large).copied().ok_or_else(|| {
                Error::CorruptObject(format!("missing large offset entry {next_large}"))
            })?;
            next_large += 1;
            off
        };
        entries.push(ShowIndexEntry {
            oid,
            offset,
            crc32: Some(crcs[i]),
        });
    }
    Ok(entries)
}

/// Basic information about local packs.
#[derive(Debug, Clone, Default)]
pub struct LocalPackInfo {
    /// Number of valid local packs.
    pub pack_count: usize,
    /// Total objects across all valid local packs.
    pub object_count: usize,
    /// Combined on-disk bytes of `.pack` + `.idx`.
    pub size_bytes: u64,
    /// Set of all object IDs present in local packs.
    pub object_ids: HashSet<ObjectId>,
}

/// Read all valid `.idx` files in `objects/pack`.
///
/// # Errors
///
/// Returns [`Error::Io`] for directory-level failures. Individual invalid pack
/// pairs are skipped.
pub fn read_local_pack_indexes(objects_dir: &Path) -> Result<Vec<PackIndex>> {
    let pack_dir = objects_dir.join("pack");
    let rd = match fs::read_dir(&pack_dir) {
        Ok(rd) => rd,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(Error::Io(err)),
    };

    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(Error::Io)?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("idx") {
            continue;
        }
        if let Ok(idx) = read_pack_index(&path) {
            // Ignore orphan `.idx` files (no `.pack`). They must not make `fsck` think objects
            // exist (`t7700-repack`); repack also skips them so a stray index does not block work.
            if !idx.pack_path.is_file() {
                continue;
            }
            out.push(idx);
        }
    }
    Ok(out)
}

/// Collect aggregate local pack metrics.
///
/// # Errors
///
/// Returns [`Error::Io`] when reading pack metadata fails.
pub fn collect_local_pack_info(objects_dir: &Path) -> Result<LocalPackInfo> {
    let indexes = read_local_pack_indexes(objects_dir)?;
    let mut info = LocalPackInfo::default();
    for idx in indexes {
        let pack_meta = fs::metadata(&idx.pack_path).map_err(Error::Io)?;
        let idx_meta = fs::metadata(&idx.idx_path).map_err(Error::Io)?;
        info.pack_count += 1;
        info.object_count += idx.entries.len();
        info.size_bytes += pack_meta.len() + idx_meta.len();
        for entry in idx.entries {
            info.object_ids.insert(entry.oid);
        }
    }
    Ok(info)
}

/// Parse a version-2 pack index file.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] when format checks fail.
pub fn read_pack_index(idx_path: &Path) -> Result<PackIndex> {
    let bytes = fs::read(idx_path).map_err(Error::Io)?;
    if bytes.len() < 8 + 256 * 4 + 40 {
        return Err(Error::CorruptObject(format!(
            "index file {} is too small",
            idx_path.display()
        )));
    }

    let mut pos = 0usize;
    let magic = &bytes[pos..pos + 4];
    pos += 4;
    if magic != [0xff, b't', b'O', b'c'] {
        return Err(Error::CorruptObject(format!(
            "unsupported idx signature in {}",
            idx_path.display()
        )));
    }
    let version = read_u32_be(&bytes, &mut pos)?;
    if version != 2 {
        return Err(Error::CorruptObject(format!(
            "unsupported idx version {} in {}",
            version,
            idx_path.display()
        )));
    }

    let mut fanout = [0u32; 256];
    for slot in &mut fanout {
        *slot = read_u32_be(&bytes, &mut pos)?;
    }
    let object_count = fanout[255] as usize;

    let need = pos
        .saturating_add(object_count * 20)
        .saturating_add(object_count * 4)
        .saturating_add(object_count * 4)
        .saturating_add(40);
    if bytes.len() < need {
        return Err(Error::CorruptObject(format!(
            "truncated idx file {}",
            idx_path.display()
        )));
    }

    let mut oids = Vec::with_capacity(object_count);
    for _ in 0..object_count {
        let oid = ObjectId::from_bytes(&bytes[pos..pos + 20])?;
        pos += 20;
        oids.push(oid);
    }

    // Skip CRC table.
    pos += object_count * 4;

    let mut offsets32 = Vec::with_capacity(object_count);
    let mut large_count = 0usize;
    for _ in 0..object_count {
        let v = read_u32_be(&bytes, &mut pos)?;
        if (v & 0x8000_0000) != 0 {
            large_count += 1;
        }
        offsets32.push(v);
    }

    if bytes.len() < pos + large_count * 8 + 40 {
        return Err(Error::CorruptObject(format!(
            "truncated large offset table in {}",
            idx_path.display()
        )));
    }
    let mut large_offsets = Vec::with_capacity(large_count);
    for _ in 0..large_count {
        large_offsets.push(read_u64_be(&bytes, &mut pos)?);
    }

    let mut next_large = 0usize;
    let mut entries = Vec::with_capacity(object_count);
    for (i, oid) in oids.into_iter().enumerate() {
        let raw = offsets32[i];
        let offset = if (raw & 0x8000_0000) == 0 {
            raw as u64
        } else {
            let off = large_offsets.get(next_large).copied().ok_or_else(|| {
                Error::CorruptObject(format!("bad large offset index in {}", idx_path.display()))
            })?;
            next_large += 1;
            off
        };
        entries.push(PackIndexEntry { oid, offset });
    }

    let mut pack_path = idx_path.to_path_buf();
    pack_path.set_extension("pack");

    // Trailing 20 bytes are SHA-1 over all preceding index bytes (Git format).
    if bytes.len() < 20 {
        return Err(Error::CorruptObject(format!(
            "index file {} missing checksum",
            idx_path.display()
        )));
    }
    let idx_body_end = bytes.len() - 20;
    let mut h = Sha1::new();
    h.update(&bytes[..idx_body_end]);
    let digest = h.finalize();
    if digest.as_slice() != &bytes[idx_body_end..] {
        return Err(Error::CorruptObject(format!(
            "index checksum mismatch for {}",
            idx_path.display()
        )));
    }

    Ok(PackIndex {
        idx_path: idx_path.to_path_buf(),
        pack_path,
        entries,
    })
}

/// A pack object type as encoded in the packed stream header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedType {
    /// Commit object.
    Commit,
    /// Tree object.
    Tree,
    /// Blob object.
    Blob,
    /// Tag object.
    Tag,
    /// Offset delta.
    OfsDelta,
    /// Reference delta.
    RefDelta,
}

impl PackedType {
    /// Printable name used by `verify-pack -v` output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Tree => "tree",
            Self::Blob => "blob",
            Self::Tag => "tag",
            Self::OfsDelta => "ofs-delta",
            Self::RefDelta => "ref-delta",
        }
    }
}

/// A decoded object header record used by `verify-pack`.
#[derive(Debug, Clone)]
pub struct VerifyObjectRecord {
    /// Object ID from the index.
    pub oid: ObjectId,
    /// Type from the pack stream header.
    pub packed_type: PackedType,
    /// Uncompressed object size from the pack header.
    pub size: u64,
    /// Total bytes in pack occupied by this object slot.
    pub size_in_pack: u64,
    /// Offset in pack file.
    pub offset: u64,
    /// Delta chain depth, if deltified.
    pub depth: Option<u64>,
    /// Base object for ref-delta objects.
    pub base_oid: Option<ObjectId>,
}

/// Verify one pack/index pair and optionally return object records.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] when the index or pack are malformed.
pub fn verify_pack_and_collect(idx_path: &Path) -> Result<Vec<VerifyObjectRecord>> {
    let idx = read_pack_index(idx_path)?;
    let idx_file_bytes = fs::read(idx_path).map_err(Error::Io)?;
    let pack_bytes = fs::read(&idx.pack_path).map_err(Error::Io)?;
    if pack_bytes.len() < 12 + 20 {
        return Err(Error::CorruptObject(format!(
            "pack file {} is too small",
            idx.pack_path.display()
        )));
    }
    let pack_end = pack_bytes.len() - 20;
    {
        let mut h = Sha1::new();
        h.update(&pack_bytes[..pack_end]);
        let digest = h.finalize();
        if digest.as_slice() != &pack_bytes[pack_end..] {
            return Err(Error::CorruptObject(format!(
                "pack trailing checksum mismatch for {}",
                idx.pack_path.display()
            )));
        }
    }
    if idx_file_bytes.len() >= 40 {
        let embedded = &idx_file_bytes[idx_file_bytes.len() - 40..idx_file_bytes.len() - 20];
        if embedded != &pack_bytes[pack_end..] {
            return Err(Error::CorruptObject(format!(
                "pack checksum in index does not match {}",
                idx.pack_path.display()
            )));
        }
    }
    if &pack_bytes[0..4] != b"PACK" {
        return Err(Error::CorruptObject(format!(
            "pack file {} has invalid signature",
            idx.pack_path.display()
        )));
    }
    let version = u32::from_be_bytes(pack_bytes[4..8].try_into().unwrap_or([0, 0, 0, 0]));
    if version != 2 && version != 3 {
        return Err(Error::CorruptObject(format!(
            "unsupported pack version {} in {}",
            version,
            idx.pack_path.display()
        )));
    }
    let count = u32::from_be_bytes(pack_bytes[8..12].try_into().unwrap_or([0, 0, 0, 0])) as usize;
    if count != idx.entries.len() {
        return Err(Error::CorruptObject(format!(
            "pack/index object count mismatch for {}",
            idx.pack_path.display()
        )));
    }

    let mut by_offset: BTreeMap<u64, ObjectId> = BTreeMap::new();
    for entry in &idx.entries {
        by_offset.insert(entry.offset, entry.oid);
    }
    let offsets: Vec<u64> = by_offset.keys().copied().collect();
    if offsets.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_oid: HashMap<ObjectId, usize> = HashMap::new();
    let mut records: Vec<VerifyObjectRecord> = Vec::with_capacity(offsets.len());
    for (i, offset) in offsets.iter().copied().enumerate() {
        let oid = by_offset.get(&offset).copied().ok_or_else(|| {
            Error::CorruptObject(format!("missing object id for offset {}", offset))
        })?;
        let next_off = offsets
            .get(i + 1)
            .copied()
            .unwrap_or((pack_bytes.len() - 20) as u64);
        if next_off <= offset || next_off > (pack_bytes.len() - 20) as u64 {
            return Err(Error::CorruptObject(format!(
                "invalid object boundaries at offset {} in {}",
                offset,
                idx.pack_path.display()
            )));
        }
        let mut p = offset as usize;
        let (packed_type, size) = parse_pack_object_header(&pack_bytes, &mut p)?;
        let mut base_oid = None;
        let mut depth = None;

        match packed_type {
            PackedType::RefDelta => {
                if p + 20 > pack_bytes.len() {
                    return Err(Error::CorruptObject(format!(
                        "truncated ref-delta base at offset {}",
                        offset
                    )));
                }
                base_oid = Some(ObjectId::from_bytes(&pack_bytes[p..p + 20])?);
            }
            PackedType::OfsDelta => {
                let base_offset = parse_ofs_delta_base(&pack_bytes, &mut p, offset)?;
                let base_depth = records
                    .iter()
                    .find(|r| r.offset == base_offset)
                    .and_then(|r| r.depth)
                    .unwrap_or(0);
                depth = Some(base_depth + 1);
            }
            PackedType::Commit | PackedType::Tree | PackedType::Blob | PackedType::Tag => {}
        }

        let size_in_pack = next_off - offset;
        records.push(VerifyObjectRecord {
            oid,
            packed_type,
            size,
            size_in_pack,
            offset,
            depth,
            base_oid,
        });
        by_oid.insert(oid, i);
    }

    // Fill ref-delta depths in a second pass once all base objects are known.
    for i in 0..records.len() {
        if records[i].packed_type != PackedType::RefDelta {
            continue;
        }
        let base = records[i]
            .base_oid
            .ok_or_else(|| Error::CorruptObject("ref-delta missing base oid".to_owned()))?;
        let base_depth = by_oid
            .get(&base)
            .and_then(|idx| records.get(*idx))
            .and_then(|r| r.depth)
            .unwrap_or(0);
        records[i].depth = Some(base_depth + 1);
    }

    // Confirm each index OID matches the resolved object bytes (catches swapped .idx/.pack pairs).
    for entry in &idx.entries {
        let obj = read_object_from_pack(&idx, &entry.oid)?;
        let computed = Odb::hash_object_data(obj.kind, &obj.data);
        if computed != entry.oid {
            return Err(Error::CorruptObject(format!(
                "pack object hash mismatch at offset {} (index says {})",
                entry.offset, entry.oid
            )));
        }
    }

    Ok(records)
}

/// Read alternates recursively, deduplicated in discovery order.
///
/// # Errors
///
/// Returns [`Error::Io`] when alternate files cannot be read.
pub fn read_alternates_recursive(objects_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    read_alternates_inner(objects_dir, &mut visited, &mut out, 0)?;
    Ok(out)
}

/// Maximum alternate chain depth (git uses 5).
const MAX_ALTERNATE_DEPTH: usize = 5;

fn read_alternates_inner(
    objects_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_ALTERNATE_DEPTH {
        return Ok(());
    }
    let canonical = canonical_or_self(objects_dir);
    let alt_file = canonical.join("info").join("alternates");
    let text = match fs::read_to_string(&alt_file) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(Error::Io(err)),
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let candidate = if Path::new(line).is_absolute() {
            PathBuf::from(line)
        } else {
            canonical.join(line)
        };
        let candidate = canonical_or_self(&candidate);
        if visited.insert(candidate.clone()) {
            out.push(candidate.clone());
            read_alternates_inner(&candidate, visited, out, depth + 1)?;
        }
    }
    Ok(())
}

fn canonical_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Convert a [`PackedType`] to an [`ObjectKind`] for non-delta types.
fn packed_type_to_kind(pt: PackedType) -> Result<ObjectKind> {
    match pt {
        PackedType::Commit => Ok(ObjectKind::Commit),
        PackedType::Tree => Ok(ObjectKind::Tree),
        PackedType::Blob => Ok(ObjectKind::Blob),
        PackedType::Tag => Ok(ObjectKind::Tag),
        PackedType::OfsDelta | PackedType::RefDelta => Err(Error::CorruptObject(
            "cannot convert delta type to object kind directly".to_owned(),
        )),
    }
}

/// Decompress zlib data from a byte slice starting at `pos`.
///
/// Returns the decompressed data and advances `pos` past the consumed
/// compressed bytes.
fn decompress_pack_data(bytes: &[u8], pos: &mut usize, expected_size: u64) -> Result<Vec<u8>> {
    let slice = &bytes[*pos..];
    let mut decoder = ZlibDecoder::new(slice);
    let mut out = Vec::with_capacity(expected_size as usize);
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::Zlib(e.to_string()))?;
    *pos += decoder.total_in() as usize;
    Ok(out)
}

/// Read and fully resolve one object from a pack file given its offset.
///
/// Handles OFS_DELTA and REF_DELTA by recursively reading the base object.
/// The `idx` is used for REF_DELTA resolution (to find a base by OID).
fn read_pack_object_at(
    pack_bytes: &[u8],
    offset: u64,
    idx: &PackIndex,
    depth: usize,
) -> Result<(ObjectKind, Vec<u8>)> {
    if depth > 50 {
        return Err(Error::CorruptObject(
            "delta chain too deep (>50)".to_owned(),
        ));
    }
    let mut pos = offset as usize;
    let (packed_type, size) = parse_pack_object_header(pack_bytes, &mut pos)?;

    match packed_type {
        PackedType::Commit | PackedType::Tree | PackedType::Blob | PackedType::Tag => {
            let data = decompress_pack_data(pack_bytes, &mut pos, size)?;
            let kind = packed_type_to_kind(packed_type)?;
            Ok((kind, data))
        }
        PackedType::OfsDelta => {
            let base_offset = parse_ofs_delta_base(pack_bytes, &mut pos, offset)?;
            let delta_data = decompress_pack_data(pack_bytes, &mut pos, size)?;
            let (base_kind, base_data) =
                read_pack_object_at(pack_bytes, base_offset, idx, depth + 1)?;
            let result = apply_delta(&base_data, &delta_data)?;
            Ok((base_kind, result))
        }
        PackedType::RefDelta => {
            if pos + 20 > pack_bytes.len() {
                return Err(Error::CorruptObject(
                    "truncated ref-delta base OID".to_owned(),
                ));
            }
            let base_oid = ObjectId::from_bytes(&pack_bytes[pos..pos + 20])?;
            pos += 20;
            let delta_data = decompress_pack_data(pack_bytes, &mut pos, size)?;
            // Find the base in the same pack index
            let base_entry = idx
                .entries
                .iter()
                .find(|e| e.oid == base_oid)
                .ok_or_else(|| {
                    Error::CorruptObject(format!("ref-delta base {} not found in pack", base_oid))
                })?;
            let (base_kind, base_data) =
                read_pack_object_at(pack_bytes, base_entry.offset, idx, depth + 1)?;
            let result = apply_delta(&base_data, &delta_data)?;
            Ok((base_kind, result))
        }
    }
}

/// Read an object from a pack file by its OID.
///
/// Searches the given pack index for the OID, then reads and decompresses
/// the object from the corresponding pack file, resolving delta chains.
///
/// # Errors
///
/// Returns [`Error::ObjectNotFound`] if the OID is not in this pack.
pub fn read_object_from_pack(idx: &PackIndex, oid: &ObjectId) -> Result<Object> {
    let entry = idx
        .entries
        .iter()
        .find(|e| e.oid == *oid)
        .ok_or_else(|| Error::ObjectNotFound(oid.to_hex()))?;

    let pack_bytes = fs::read(&idx.pack_path).map_err(Error::Io)?;
    let (kind, data) = read_pack_object_at(&pack_bytes, entry.offset, idx, 0)?;
    Ok(Object::new(kind, data))
}

/// Search all pack indexes in `objects_dir` for the given OID and read it.
///
/// # Errors
///
/// Returns [`Error::ObjectNotFound`] if no pack contains the OID.
pub fn read_object_from_packs(objects_dir: &Path, oid: &ObjectId) -> Result<Object> {
    let indexes = read_local_pack_indexes(objects_dir)?;
    for idx in &indexes {
        if idx.entries.iter().any(|e| e.oid == *oid) {
            return read_object_from_pack(idx, oid);
        }
    }
    Err(Error::ObjectNotFound(oid.to_hex()))
}

/// When `oid` is stored as a delta in a pack, return its delta base object id.
/// Returns [`None`] for loose objects and for non-delta packed objects.
/// If `oid` is stored as `REF_DELTA` or `OFS_DELTA` in a local pack and its base OID is in
/// `packed_set`, return the base OID and the **uncompressed** delta payload (Git binary delta).
///
/// Callers re-zlib when writing a new pack so we do not depend on copying raw deflate streams.
///
/// # Errors
///
/// Returns [`Error::CorruptObject`] when the pack stream is malformed.
pub fn packed_ref_delta_reuse_slice(
    objects_dir: &Path,
    oid: &ObjectId,
    packed_set: &HashSet<ObjectId>,
) -> Result<Option<(ObjectId, Vec<u8>)>> {
    let mut indexes = read_local_pack_indexes(objects_dir)?;
    sort_pack_indexes_oldest_first(&mut indexes);
    for idx in indexes {
        let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) else {
            continue;
        };
        let pack_bytes = fs::read(&idx.pack_path).map_err(Error::Io)?;
        let mut p = entry.offset as usize;
        let (packed_type, _size) = parse_pack_object_header(&pack_bytes, &mut p)?;
        let base = match packed_type {
            PackedType::RefDelta => {
                if p + 20 > pack_bytes.len() {
                    return Err(Error::CorruptObject(
                        "truncated ref-delta base oid while scanning for reuse".to_owned(),
                    ));
                }
                let oid = ObjectId::from_bytes(&pack_bytes[p..p + 20])?;
                p += 20;
                oid
            }
            PackedType::OfsDelta => {
                let base_off = parse_ofs_delta_base(&pack_bytes, &mut p, entry.offset)?;
                let Some(base_entry) = idx.entries.iter().find(|e| e.offset == base_off) else {
                    continue;
                };
                base_entry.oid
            }
            _ => {
                // Same OID may exist as a full object in an older pack and as a delta in a newer
                // one; keep scanning packs.
                continue;
            }
        };
        if !packed_set.contains(&base) {
            continue;
        }
        let zlib_start = p;
        let mut end_pos = zlib_start;
        if skip_one_pack_object(&pack_bytes, &mut end_pos, entry.offset).is_err() {
            continue;
        }
        let compressed = &pack_bytes[zlib_start..end_pos];
        let mut dec = ZlibDecoder::new(compressed);
        let mut delta = Vec::new();
        if dec.read_to_end(&mut delta).is_err() {
            continue;
        }
        return Ok(Some((base, delta)));
    }
    Ok(None)
}

/// Prefer older packs when the same OID exists as a full object in a fresh repack and as a delta
/// in an earlier thin pack (t5316).
fn sort_pack_indexes_oldest_first(indexes: &mut [PackIndex]) {
    indexes.sort_by(|a, b| {
        let ta = fs::metadata(&a.pack_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let tb = fs::metadata(&b.pack_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        ta.cmp(&tb).then_with(|| a.pack_path.cmp(&b.pack_path))
    });
}

fn sort_pack_indexes_newest_first(indexes: &mut [PackIndex]) {
    indexes.sort_by(|a, b| {
        let ta = fs::metadata(&a.pack_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let tb = fs::metadata(&b.pack_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        tb.cmp(&ta).then_with(|| b.pack_path.cmp(&a.pack_path))
    });
}

pub fn packed_delta_base_oid(objects_dir: &Path, oid: &ObjectId) -> Result<Option<ObjectId>> {
    let mut indexes = read_local_pack_indexes(objects_dir)?;
    sort_pack_indexes_newest_first(&mut indexes);
    for idx in &indexes {
        let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) else {
            continue;
        };
        let pack_bytes = fs::read(&idx.pack_path).map_err(Error::Io)?;
        let mut p = entry.offset as usize;
        let (packed_type, _) = parse_pack_object_header(&pack_bytes, &mut p)?;
        match packed_type {
            PackedType::RefDelta => {
                if p + 20 > pack_bytes.len() {
                    return Err(Error::CorruptObject("truncated ref-delta base".to_owned()));
                }
                return Ok(Some(ObjectId::from_bytes(&pack_bytes[p..p + 20])?));
            }
            PackedType::OfsDelta => {
                let base_off = parse_ofs_delta_base(&pack_bytes, &mut p, entry.offset)?;
                return Ok(idx
                    .entries
                    .iter()
                    .find(|e| e.offset == base_off)
                    .map(|e| e.oid));
            }
            _ => continue,
        }
    }
    Ok(None)
}

fn parse_pack_object_header(bytes: &[u8], pos: &mut usize) -> Result<(PackedType, u64)> {
    let first = *bytes.get(*pos).ok_or_else(|| {
        Error::CorruptObject("unexpected end of pack header while decoding object".to_owned())
    })?;
    *pos += 1;

    let type_code = (first >> 4) & 0x7;
    let mut size = (first & 0x0f) as u64;
    let mut shift = 4u32;
    let mut c = first;
    while (c & 0x80) != 0 {
        c = *bytes.get(*pos).ok_or_else(|| {
            Error::CorruptObject("unexpected end of variable size header".to_owned())
        })?;
        *pos += 1;
        size |= ((c & 0x7f) as u64) << shift;
        shift += 7;
    }

    let packed_type = match type_code {
        1 => PackedType::Commit,
        2 => PackedType::Tree,
        3 => PackedType::Blob,
        4 => PackedType::Tag,
        6 => PackedType::OfsDelta,
        7 => PackedType::RefDelta,
        _ => {
            return Err(Error::CorruptObject(format!(
                "unsupported packed object type {}",
                type_code
            )))
        }
    };
    Ok((packed_type, size))
}

/// Dependency of a packed delta object at `object_offset` within `pack_bytes`.
#[derive(Debug, Clone, Copy)]
pub enum PackedDeltaDependency {
    /// OFS_DELTA: base object offset within the same pack.
    OfsBase {
        /// Pack offset of the base object.
        base_offset: u64,
    },
    /// REF_DELTA: base object id (may live in another pack).
    RefBase {
        /// OID of the delta base.
        base_oid: ObjectId,
    },
}

/// If the object at `object_offset` is a delta, return how it refers to its base.
pub fn read_packed_delta_dependency(
    pack_bytes: &[u8],
    object_offset: u64,
) -> Result<Option<PackedDeltaDependency>> {
    let mut pos = object_offset as usize;
    let (ty, _) = parse_pack_object_header(pack_bytes, &mut pos)?;
    match ty {
        PackedType::OfsDelta => {
            let base = parse_ofs_delta_base(pack_bytes, &mut pos, object_offset)?;
            Ok(Some(PackedDeltaDependency::OfsBase { base_offset: base }))
        }
        PackedType::RefDelta => {
            if pos + 20 > pack_bytes.len() {
                return Err(Error::CorruptObject("truncated ref-delta base oid".into()));
            }
            let base_oid = ObjectId::from_bytes(&pack_bytes[pos..pos + 20])?;
            Ok(Some(PackedDeltaDependency::RefBase { base_oid }))
        }
        _ => Ok(None),
    }
}

fn parse_ofs_delta_base(bytes: &[u8], pos: &mut usize, this_offset: u64) -> Result<u64> {
    let mut c = *bytes
        .get(*pos)
        .ok_or_else(|| Error::CorruptObject("truncated ofs-delta header".to_owned()))?;
    *pos += 1;
    let mut value = (c & 0x7f) as u64;
    while (c & 0x80) != 0 {
        c = *bytes
            .get(*pos)
            .ok_or_else(|| Error::CorruptObject("truncated ofs-delta header".to_owned()))?;
        *pos += 1;
        value = ((value + 1) << 7) | (c & 0x7f) as u64;
    }
    this_offset
        .checked_sub(value)
        .ok_or_else(|| Error::CorruptObject("invalid ofs-delta base offset".to_owned()))
}

/// Advance `pos` past one packed object (including zlib payload).
///
/// `object_start_offset` is the byte offset of this object within the pack file
/// (used for `OFS_DELTA` base resolution).
/// Raw bytes of one packed object (header + zlib payload) starting at `object_start_offset`.
#[must_use]
pub fn slice_one_pack_object(bytes: &[u8], object_start_offset: u64) -> Result<&[u8]> {
    let start = object_start_offset as usize;
    let mut pos = start;
    skip_one_pack_object(bytes, &mut pos, object_start_offset)?;
    Ok(&bytes[start..pos])
}

pub fn skip_one_pack_object(bytes: &[u8], pos: &mut usize, object_start_offset: u64) -> Result<()> {
    let (packed_type, size) = parse_pack_object_header(bytes, pos)?;
    match packed_type {
        PackedType::Commit | PackedType::Tree | PackedType::Blob | PackedType::Tag => {
            let mut dec = ZlibDecoder::new(&bytes[*pos..]);
            let mut tmp = Vec::with_capacity(size as usize);
            dec.read_to_end(&mut tmp)
                .map_err(|e| Error::Zlib(e.to_string()))?;
            *pos += dec.total_in() as usize;
        }
        PackedType::RefDelta => {
            if *pos + 20 > bytes.len() {
                return Err(Error::CorruptObject("truncated ref-delta base oid".into()));
            }
            *pos += 20;
            let mut dec = ZlibDecoder::new(&bytes[*pos..]);
            let mut tmp = Vec::with_capacity(size as usize);
            dec.read_to_end(&mut tmp)
                .map_err(|e| Error::Zlib(e.to_string()))?;
            *pos += dec.total_in() as usize;
        }
        PackedType::OfsDelta => {
            let _base_off = parse_ofs_delta_base(bytes, pos, object_start_offset)?;
            let mut dec = ZlibDecoder::new(&bytes[*pos..]);
            let mut tmp = Vec::with_capacity(size as usize);
            dec.read_to_end(&mut tmp)
                .map_err(|e| Error::Zlib(e.to_string()))?;
            *pos += dec.total_in() as usize;
        }
    }
    Ok(())
}

fn read_u32_be(bytes: &[u8], pos: &mut usize) -> Result<u32> {
    if bytes.len() < *pos + 4 {
        return Err(Error::CorruptObject(
            "unexpected end of idx while reading u32".to_owned(),
        ));
    }
    let v = u32::from_be_bytes(
        bytes[*pos..*pos + 4]
            .try_into()
            .map_err(|_| Error::CorruptObject("failed to parse u32".to_owned()))?,
    );
    *pos += 4;
    Ok(v)
}

fn read_u64_be(bytes: &[u8], pos: &mut usize) -> Result<u64> {
    if bytes.len() < *pos + 8 {
        return Err(Error::CorruptObject(
            "unexpected end of idx while reading u64".to_owned(),
        ));
    }
    let v = u64::from_be_bytes(
        bytes[*pos..*pos + 8]
            .try_into()
            .map_err(|_| Error::CorruptObject("failed to parse u64".to_owned()))?,
    );
    *pos += 8;
    Ok(v)
}

/// Read all object IDs from a `.idx` file.
pub fn read_idx_object_ids(idx_path: &Path) -> Result<Vec<ObjectId>> {
    let index = read_pack_index(idx_path)?;
    Ok(index.entries.into_iter().map(|e| e.oid).collect())
}
