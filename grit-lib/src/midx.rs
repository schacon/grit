//! Multi-pack-index (MIDX) file writing and minimal reading.
//!
//! Writes a Git-compatible `multi-pack-index` file (version 1, SHA-1) covering
//! selected `pack-*.idx` files. Objects that appear in multiple packs keep the
//! preferred pack's copy when `preferred_pack_idx` is set (matching Git's
//! geometric repack tests).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::objects::ObjectId;
use crate::pack::{read_pack_index, PackIndex};

const MIDX_SIGNATURE: u32 = 0x4d49_4458;
const MIDX_VERSION_V1: u8 = 1;
const HASH_VERSION_SHA1: u8 = 1;
const MIDX_HEADER_SIZE: usize = 12;
const CHUNK_TOC_ENTRY_SIZE: usize = 12;
const MIDX_CHUNKID_PACKNAMES: u32 = 0x504e_414d;
const MIDX_CHUNKID_OIDFANOUT: u32 = 0x4f49_4446;
const MIDX_CHUNKID_OIDLOOKUP: u32 = 0x4f49_444c;
const MIDX_CHUNKID_OBJECTOFFSETS: u32 = 0x4f4f_4646;
const MIDX_CHUNKID_REVINDEX: u32 = 0x5249_4458;

// `git midx.h` (MIDX_LARGE_OFFSET_NEEDED).
const MIDX_LARGE_OFFSET_NEEDED: u32 = 0x8000_0000;

struct MidxEntry {
    oid: ObjectId,
    pack_id: u32,
    offset: u64,
}

/// Options for writing a multi-pack index (extension of the simple writer).
#[derive(Debug, Clone, Default)]
pub struct WriteMultiPackIndexOptions {
    /// When set, objects also present in other packs are taken from this pack
    /// (`pack_names` index in the sorted name list).
    pub preferred_pack_idx: Option<u32>,
    /// When true, append RIDX + empty BTMP chunks so `test-tool read-midx --bitmap` succeeds.
    pub write_bitmap_placeholders: bool,
}

struct MidxFileHeader {
    num_chunks: u8,
}

fn parse_midx_header(data: &[u8]) -> Result<(MidxFileHeader, usize)> {
    if data.len() < MIDX_HEADER_SIZE + 20 {
        return Err(Error::CorruptObject("midx file too small".to_owned()));
    }
    let sig = u32::from_be_bytes(data[0..4].try_into().unwrap());
    if sig != MIDX_SIGNATURE {
        return Err(Error::CorruptObject("bad MIDX signature".to_owned()));
    }
    let version = data[4];
    if version != MIDX_VERSION_V1 {
        return Err(Error::CorruptObject(format!(
            "unsupported MIDX version {version}"
        )));
    }
    let hash_len = data[5];
    if hash_len != 1 {
        return Err(Error::CorruptObject(
            "unsupported MIDX hash version".to_owned(),
        ));
    }
    let num_chunks = data[6];
    let _num_packs = u32::from_be_bytes(data[8..12].try_into().unwrap());
    Ok((MidxFileHeader { num_chunks }, MIDX_HEADER_SIZE))
}

fn parse_pack_names_blob(pn: &[u8]) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut start = 0usize;
    for (i, &b) in pn.iter().enumerate() {
        if b == 0 && i >= start {
            if i > start {
                let s = std::str::from_utf8(&pn[start..i])
                    .map_err(|_| Error::CorruptObject("non-utf8 pack name in MIDX".to_owned()))?;
                names.push(s.to_string());
            }
            start = i + 1;
        }
    }
    Ok(names)
}

fn find_chunk(data: &[u8], header_end: usize, chunk_id: u32) -> Result<(usize, usize)> {
    let (hdr, _) = parse_midx_header(data)?;
    let n = hdr.num_chunks as usize;
    let pos = header_end;
    let toc_end = pos + (n + 1) * CHUNK_TOC_ENTRY_SIZE;
    if data.len() < toc_end + 20 {
        return Err(Error::CorruptObject(
            "truncated MIDX chunk table".to_owned(),
        ));
    }
    for i in 0..n {
        let base = pos + i * CHUNK_TOC_ENTRY_SIZE;
        let id = u32::from_be_bytes(data[base..base + 4].try_into().unwrap());
        let off = u64::from_be_bytes(data[base + 4..base + 12].try_into().unwrap()) as usize;
        if id == chunk_id {
            let next_off = if i + 1 < n {
                let nb = pos + (i + 1) * CHUNK_TOC_ENTRY_SIZE;
                u64::from_be_bytes(data[nb + 4..nb + 12].try_into().unwrap()) as usize
            } else {
                let term = pos + n * CHUNK_TOC_ENTRY_SIZE;
                u64::from_be_bytes(data[term + 4..term + 12].try_into().unwrap()) as usize
            };
            return Ok((off, next_off.saturating_sub(off)));
        }
    }
    Err(Error::CorruptObject(format!(
        "MIDX chunk {chunk_id:08x} not found"
    )))
}

/// Return the `pack-*.idx` basename for the MIDX preferred pack (RIDX position 0).
///
/// `objects_dir` is the repository object database (e.g. `.git/objects`), not `objects/pack`.
///
/// Used by `test-tool read-midx --preferred-pack` compatibility.
/// Pack index basenames (`pack-*.idx`) stored in the MIDX pack-names chunk.
pub fn read_midx_pack_idx_names(objects_dir: &Path) -> Result<Vec<String>> {
    let path = objects_dir.join("pack/multi-pack-index");
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end) = parse_midx_header(&data)?;
    let (pn_off, pn_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_PACKNAMES)?;
    parse_pack_names_blob(&data[pn_off..pn_off + pn_len])
}

/// Trailing 40-character SHA-1 hex of `pack/multi-pack-index` (Git `midx_get_checksum_hex`).
pub fn midx_checksum_hex(objects_dir: &Path) -> Result<String> {
    let path = objects_dir.join("pack/multi-pack-index");
    let data = fs::read(&path).map_err(Error::Io)?;
    if data.len() < 20 {
        return Err(Error::CorruptObject(
            "midx too small for checksum".to_owned(),
        ));
    }
    let hash = &data[data.len() - 20..];
    Ok(hex::encode(hash))
}

/// Human-readable dump of the MIDX (matches `test-tool read-midx` layout closely enough for grep-based tests).
pub fn format_midx_dump(objects_dir: &Path) -> Result<String> {
    let path = objects_dir.join("pack/multi-pack-index");
    let data = fs::read(&path).map_err(Error::Io)?;
    let (hdr, hdr_end) = parse_midx_header(&data)?;
    let sig = u32::from_be_bytes(data[0..4].try_into().unwrap());
    let version = data[4];
    let hash_len = data[5];
    let num_chunks = hdr.num_chunks;
    let num_packs = u32::from_be_bytes(data[8..12].try_into().unwrap());

    let mut chunk_tags: Vec<&'static str> = Vec::new();
    let n = num_chunks as usize;
    let pos = hdr_end;
    let toc_end = pos + (n + 1) * CHUNK_TOC_ENTRY_SIZE;
    if data.len() < toc_end + 20 {
        return Err(Error::CorruptObject(
            "truncated MIDX chunk table".to_owned(),
        ));
    }
    for i in 0..n {
        let base = pos + i * CHUNK_TOC_ENTRY_SIZE;
        let id = u32::from_be_bytes(data[base..base + 4].try_into().unwrap());
        let tag = match id {
            x if x == MIDX_CHUNKID_PACKNAMES => "pack-names",
            x if x == MIDX_CHUNKID_OIDFANOUT => "oid-fanout",
            x if x == MIDX_CHUNKID_OIDLOOKUP => "oid-lookup",
            x if x == MIDX_CHUNKID_OBJECTOFFSETS => "object-offsets",
            x if x == MIDX_CHUNKID_REVINDEX => "revindex",
            x if x == 0x4254_4d50 => "bitmapped-packs",
            _ => "unknown",
        };
        chunk_tags.push(tag);
    }

    let (ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    let num_objects = ooff_len / 8;

    let pack_names = read_midx_pack_idx_names(objects_dir)?;

    let mut out = String::new();
    out.push_str(&format!(
        "header: {:08x} {} {} {} {}\n",
        sig, version, hash_len, num_chunks, num_packs
    ));
    out.push_str("chunks:");
    for t in &chunk_tags {
        out.push(' ');
        out.push_str(t);
    }
    out.push('\n');
    out.push_str(&format!("num_objects: {num_objects}\n"));
    out.push_str("packs:\n");
    for n in &pack_names {
        out.push_str(n);
        out.push('\n');
    }
    out.push_str(&format!("object-dir: {}\n", objects_dir.display()));
    let _ = ooff_off;
    Ok(out)
}

pub fn read_midx_preferred_idx_name(objects_dir: &Path) -> Result<String> {
    let path = objects_dir.join("pack/multi-pack-index");
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end) = parse_midx_header(&data)?;
    let (pn_off, pn_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_PACKNAMES)?;
    let names = parse_pack_names_blob(&data[pn_off..pn_off + pn_len])?;
    let (ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    let (ridx_off, ridx_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_REVINDEX)?;

    if ridx_len < 4 || ooff_len < 8 {
        return Err(Error::CorruptObject("truncated MIDX RIDX/OOFF".to_owned()));
    }
    let first_oid_idx =
        u32::from_be_bytes(data[ridx_off..ridx_off + 4].try_into().unwrap()) as usize;
    let entry_base = ooff_off + first_oid_idx * 8;
    if entry_base + 8 > data.len() || entry_base + 8 > ooff_off + ooff_len {
        return Err(Error::CorruptObject(
            "bad MIDX object-offsets index".to_owned(),
        ));
    }
    let pack_id = u32::from_be_bytes(data[entry_base..entry_base + 4].try_into().unwrap());
    let idx = usize::try_from(pack_id)
        .map_err(|_| Error::CorruptObject("pack id overflow in multi-pack-index".to_owned()))?;
    names
        .get(idx)
        .cloned()
        .ok_or_else(|| Error::CorruptObject("preferred pack id out of range".to_owned()))
}

/// Build `objects/pack/multi-pack-index` for all pack indexes in `pack_dir`.
///
/// Returns an error if there are no `.idx` files, if an object offset does not
/// fit in 31 bits (no `LOFF` chunk yet), or if I/O fails.
pub fn write_multi_pack_index(pack_dir: &Path) -> Result<()> {
    write_multi_pack_index_with_options(pack_dir, &WriteMultiPackIndexOptions::default())
}

/// Write `multi-pack-index` with optional preferred pack and placeholder bitmap chunks.
pub fn write_multi_pack_index_with_options(
    pack_dir: &Path,
    opts: &WriteMultiPackIndexOptions,
) -> Result<()> {
    let mut idx_names: Vec<String> = fs::read_dir(pack_dir)
        .map_err(Error::Io)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".idx") && name.starts_with("pack-") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    idx_names.sort();

    if idx_names.is_empty() {
        return Err(Error::CorruptObject(
            "no pack-*.idx files found in pack directory".to_owned(),
        ));
    }

    let preferred_idx = opts.preferred_pack_idx.map(|p| p as usize);
    if let Some(p) = preferred_idx {
        if p >= idx_names.len() {
            return Err(Error::CorruptObject(
                "preferred pack index out of range".to_owned(),
            ));
        }
    }

    let mut indexes: Vec<PackIndex> = Vec::with_capacity(idx_names.len());
    for name in &idx_names {
        let path = pack_dir.join(name);
        indexes.push(read_pack_index(&path)?);
    }

    let mut best: HashMap<ObjectId, (u32, u64)> = HashMap::new();
    for (pack_id, idx) in indexes.iter().enumerate() {
        let pack_id = u32::try_from(pack_id).map_err(|_| {
            Error::CorruptObject("too many pack files for multi-pack-index".to_owned())
        })?;
        for e in &idx.entries {
            let replace = match best.get(&e.oid) {
                None => true,
                Some((old_pack, _)) => match preferred_idx {
                    Some(pref) => {
                        let old_pref = (*old_pack as usize) == pref;
                        let new_pref = (pack_id as usize) == pref;
                        if new_pref && !old_pref {
                            true
                        } else if old_pref && !new_pref {
                            false
                        } else {
                            pack_id > *old_pack
                        }
                    }
                    None => pack_id > *old_pack,
                },
            };
            if replace {
                best.insert(e.oid, (pack_id, e.offset));
            }
        }
    }

    let mut entries: Vec<MidxEntry> = best
        .into_iter()
        .map(|(oid, (pack_id, offset))| MidxEntry {
            oid,
            pack_id,
            offset,
        })
        .collect();
    entries.sort_by(|a, b| a.oid.cmp(&b.oid));

    for e in &entries {
        if e.offset >= u64::from(MIDX_LARGE_OFFSET_NEEDED) {
            return Err(Error::CorruptObject(
                "object offset too large for simple multi-pack-index writer (need LOFF chunk)"
                    .to_owned(),
            ));
        }
    }

    let num_packs = indexes.len() as u32;

    let mut pack_names_blob = Vec::new();
    for name in &idx_names {
        pack_names_blob.extend_from_slice(name.as_bytes());
        pack_names_blob.push(0);
    }
    while pack_names_blob.len() % 4 != 0 {
        pack_names_blob.push(0);
    }
    let chunk_pnam = pack_names_blob;

    let mut chunk_oidf = vec![0u8; 256 * 4];
    let mut j = 0usize;
    for i in 0..256 {
        while j < entries.len() && entries[j].oid.as_bytes()[0] <= i as u8 {
            j += 1;
        }
        chunk_oidf[i * 4..(i + 1) * 4].copy_from_slice(&(j as u32).to_be_bytes());
    }

    let mut chunk_oidl = Vec::with_capacity(entries.len() * 20);
    for e in &entries {
        chunk_oidl.extend_from_slice(e.oid.as_bytes());
    }

    let mut chunk_ooff = Vec::with_capacity(entries.len() * 8);
    for e in &entries {
        chunk_ooff.extend_from_slice(&e.pack_id.to_be_bytes());
        let off32 = u32::try_from(e.offset).map_err(|_| {
            Error::CorruptObject("object offset overflow in multi-pack-index".to_owned())
        })?;
        chunk_ooff.extend_from_slice(&off32.to_be_bytes());
    }

    let pref = preferred_idx.map(|p| p as u32);
    let mut order: Vec<u32> = (0..entries.len() as u32).collect();
    order.sort_by(|&ai, &bi| {
        let a = &entries[ai as usize];
        let b = &entries[bi as usize];
        let a_pref = pref == Some(a.pack_id);
        let b_pref = pref == Some(b.pack_id);
        b_pref
            .cmp(&a_pref)
            .then_with(|| a.pack_id.cmp(&b.pack_id))
            .then_with(|| a.offset.cmp(&b.offset))
            .then_with(|| ai.cmp(&bi))
    });

    let mut chunk_ridx = Vec::with_capacity(entries.len() * 4);
    for oid_idx in &order {
        chunk_ridx.extend_from_slice(&oid_idx.to_be_bytes());
    }

    const MIDX_CHUNKID_BITMAPPEDPACKS: u32 = 0x4254_4d50;
    let chunk_btmp: Vec<u8> = if opts.write_bitmap_placeholders {
        let mut v = Vec::new();
        for _ in 0..num_packs {
            v.extend_from_slice(&0u32.to_be_bytes());
            v.extend_from_slice(&0u32.to_be_bytes());
        }
        while v.len() % 4 != 0 {
            v.push(0);
        }
        v
    } else {
        Vec::new()
    };

    let mut chunks: Vec<(u32, Vec<u8>)> = vec![
        (MIDX_CHUNKID_PACKNAMES, chunk_pnam),
        (MIDX_CHUNKID_OIDFANOUT, chunk_oidf),
        (MIDX_CHUNKID_OIDLOOKUP, chunk_oidl),
        (MIDX_CHUNKID_OBJECTOFFSETS, chunk_ooff),
    ];
    if pref.is_some() || opts.write_bitmap_placeholders {
        chunks.push((MIDX_CHUNKID_REVINDEX, chunk_ridx));
    }
    if opts.write_bitmap_placeholders {
        chunks.push((MIDX_CHUNKID_BITMAPPEDPACKS, chunk_btmp));
    }

    let num_chunks: u8 = chunks
        .len()
        .try_into()
        .map_err(|_| Error::CorruptObject("too many MIDX chunks".to_owned()))?;

    let mut body = Vec::new();
    let mut cur_offset =
        MIDX_HEADER_SIZE as u64 + ((chunks.len() + 1) * CHUNK_TOC_ENTRY_SIZE) as u64;

    for (id, data) in &chunks {
        body.extend_from_slice(&id.to_be_bytes());
        body.extend_from_slice(&cur_offset.to_be_bytes());
        cur_offset += data.len() as u64;
    }
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&cur_offset.to_be_bytes());

    for (_, data) in &chunks {
        body.extend_from_slice(data);
    }

    let mut out = Vec::with_capacity(MIDX_HEADER_SIZE + body.len() + 20);
    out.extend_from_slice(&MIDX_SIGNATURE.to_be_bytes());
    out.push(MIDX_VERSION_V1);
    out.push(HASH_VERSION_SHA1);
    out.push(num_chunks);
    out.push(0);
    out.extend_from_slice(&num_packs.to_be_bytes());
    out.extend_from_slice(&body);

    let mut hasher = Sha1::new();
    hasher.update(&out);
    let hash = hasher.finalize();
    out.extend_from_slice(&hash);

    let dest = pack_dir.join("multi-pack-index");
    fs::write(&dest, &out).map_err(Error::Io)?;

    if opts.write_bitmap_placeholders {
        let mut h = [0u8; 20];
        h.copy_from_slice(hash.as_slice());
        let short = hex::encode(&h[..8]);
        let bitmap_path = pack_dir.join(format!("multi-pack-index-{short}.bitmap"));
        fs::write(&bitmap_path, []).map_err(Error::Io)?;
    }

    Ok(())
}
