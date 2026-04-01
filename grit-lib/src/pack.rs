//! Pack and pack-index helpers for object counting and verification.
//!
//! This module implements a focused subset of pack functionality required by
//! `count-objects` and `verify-pack`.

use crate::error::{Error, Result};
use crate::objects::ObjectId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
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
    let pack_bytes = fs::read(&idx.pack_path).map_err(Error::Io)?;
    if pack_bytes.len() < 12 + 20 {
        return Err(Error::CorruptObject(format!(
            "pack file {} is too small",
            idx.pack_path.display()
        )));
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
    read_alternates_inner(objects_dir, &mut visited, &mut out)?;
    Ok(out)
}

fn read_alternates_inner(
    objects_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
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
            read_alternates_inner(&candidate, visited, out)?;
        }
    }
    Ok(())
}

fn canonical_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
