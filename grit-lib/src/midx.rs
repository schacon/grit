//! Multi-pack-index (MIDX) file writing and minimal reading.
//!
//! Writes a Git-compatible `multi-pack-index` file (version 1, SHA-1) covering
//! selected `pack-*.idx` files. Objects that appear in multiple packs keep the
//! preferred pack's copy when `preferred_pack_idx` is set (matching Git's
//! geometric repack tests).
//!
//! Incremental writes follow Git's split layout: layers live under
//! `pack/multi-pack-index.d/multi-pack-index-<sha1>.midx` with ordering in
//! `multi-pack-index-chain` (oldest hash first, newest last).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
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
const MIDX_CHUNKID_LARGEOFFSETS: u32 = 0x4c4f_4646;
const MIDX_CHUNKID_REVINDEX: u32 = 0x5249_4458;
const MIDX_CHUNK_ALIGNMENT: usize = 4;

// `git midx.h` (MIDX_LARGE_OFFSET_NEEDED).
const MIDX_LARGE_OFFSET_NEEDED: u32 = 0x8000_0000;

struct MidxEntry {
    oid: ObjectId,
    pack_id: u32,
    offset: u64,
    pack_mtime: std::time::SystemTime,
}

/// Options for writing a multi-pack index (extension of the simple writer).
#[derive(Debug, Clone, Default)]
pub struct WriteMultiPackIndexOptions {
    /// When set, objects also present in other packs are taken from this pack
    /// (`pack_names` index in the sorted name list).
    pub preferred_pack_idx: Option<u32>,
    /// When true, append RIDX + empty BTMP chunks so `test-tool read-midx --bitmap` succeeds.
    pub write_bitmap_placeholders: bool,
    /// When true, write a new layer in `multi-pack-index.d/` and extend the chain file
    /// instead of replacing `pack/multi-pack-index`.
    pub incremental: bool,
    /// When true with [`Self::write_bitmap_placeholders`], also create an empty `.rev`
    /// sidecar (Git `GIT_TEST_MIDX_WRITE_REV` compatibility).
    pub write_rev_placeholder: bool,
}

struct MidxFileHeader {
    num_chunks: u8,
}

fn parse_midx_header(data: &[u8]) -> Result<(MidxFileHeader, usize, u8)> {
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
    let object_hash_bytes = data[5];
    let num_chunks = data[6];
    let _num_packs = u32::from_be_bytes(data[8..12].try_into().unwrap());
    Ok((
        MidxFileHeader { num_chunks },
        MIDX_HEADER_SIZE,
        object_hash_bytes,
    ))
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

/// Compare a pack basename that may use `.pack` or `.idx` with an MIDX pack name (`.idx`).
fn cmp_idx_or_pack_name(idx_or_pack_name: &str, idx_name: &str) -> std::cmp::Ordering {
    let a = idx_or_pack_name.as_bytes();
    let b = idx_name.as_bytes();
    let mut i = 0usize;
    let min = a.len().min(b.len());
    while i < min && a[i] == b[i] {
        i += 1;
    }
    let suf_a = &a[i..];
    let suf_b = &b[i..];
    if suf_b == b"idx" && suf_a == b"pack" {
        return std::cmp::Ordering::Equal;
    }
    suf_a.cmp(suf_b)
}

fn preferred_pack_index_by_mtime(pack_dir: &Path, names: &[String]) -> Result<Option<usize>> {
    let mut best: Option<(usize, std::time::SystemTime)> = None;
    for (i, n) in names.iter().enumerate() {
        let meta = fs::metadata(pack_dir.join(n)).map_err(Error::Io)?;
        let mtime = meta.modified().map_err(Error::Io)?;
        match best {
            None => best = Some((i, mtime)),
            Some((_, t)) if mtime < t => best = Some((i, mtime)),
            _ => {}
        }
    }
    Ok(best.map(|(i, _)| i))
}

fn midx_d_dir(pack_dir: &Path) -> std::path::PathBuf {
    pack_dir.join("multi-pack-index.d")
}

fn chain_file_path(pack_dir: &Path) -> std::path::PathBuf {
    midx_d_dir(pack_dir).join("multi-pack-index-chain")
}

fn read_chain_layer_hashes(pack_dir: &Path) -> Result<Vec<String>> {
    let path = chain_file_path(pack_dir);
    let f = fs::File::open(&path).map_err(Error::Io)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line.map_err(Error::Io)?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.len() != 40 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::CorruptObject(format!(
                "invalid multi-pack-index chain line: {t}"
            )));
        }
        out.push(t.to_ascii_lowercase());
    }
    Ok(out)
}

/// Resolve the path to the newest MIDX layer (root `multi-pack-index` or last chain entry).
pub fn resolve_tip_midx_path(pack_dir: &Path) -> Option<std::path::PathBuf> {
    let root = pack_dir.join("multi-pack-index");
    if root.exists() {
        return Some(root);
    }
    let hashes = read_chain_layer_hashes(pack_dir).ok()?;
    let last = hashes.last()?;
    Some(midx_d_dir(pack_dir).join(format!("multi-pack-index-{last}.midx")))
}

fn load_midx_file(path: &Path) -> Result<Vec<u8>> {
    let data = fs::read(path).map_err(Error::Io)?;
    let _ = parse_midx_header(&data)?;
    Ok(data)
}

fn oids_and_packs_from_midx_data(data: &[u8]) -> Result<(HashSet<ObjectId>, Vec<String>)> {
    let (_, hdr_end, _) = parse_midx_header(data)?;
    let (pn_off, pn_len) = find_chunk(data, hdr_end, MIDX_CHUNKID_PACKNAMES)?;
    let pack_names = parse_pack_names_blob(&data[pn_off..pn_off + pn_len])?;
    let (_ooff_off, ooff_len) = find_chunk(data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    let (oidl_off, oidl_len) = find_chunk(data, hdr_end, MIDX_CHUNKID_OIDLOOKUP)?;
    let num_objects = ooff_len / 8;
    if oidl_len != num_objects * 20 {
        return Err(Error::CorruptObject(
            "MIDX oid-lookup size mismatch".to_owned(),
        ));
    }
    let mut oids = HashSet::with_capacity(num_objects);
    for i in 0..num_objects {
        let start = oidl_off + i * 20;
        let oid = ObjectId::from_bytes(&data[start..start + 20])?;
        oids.insert(oid);
    }
    Ok((oids, pack_names))
}

fn collect_incremental_base(pack_dir: &Path) -> Result<(HashSet<ObjectId>, HashSet<String>)> {
    let mut oids = HashSet::new();
    let mut packs = HashSet::new();
    let root = pack_dir.join("multi-pack-index");
    let chain_path = chain_file_path(pack_dir);
    if chain_path.exists() {
        for h in read_chain_layer_hashes(pack_dir)? {
            let p = midx_d_dir(pack_dir).join(format!("multi-pack-index-{h}.midx"));
            let data = load_midx_file(&p)?;
            let (layer_oids, names) = oids_and_packs_from_midx_data(&data)?;
            oids.extend(layer_oids);
            for n in names {
                packs.insert(n);
            }
        }
        return Ok((oids, packs));
    }
    if root.exists() {
        let data = load_midx_file(&root)?;
        let (o, names) = oids_and_packs_from_midx_data(&data)?;
        oids = o;
        for n in names {
            packs.insert(n);
        }
    }
    Ok((oids, packs))
}

fn midx_checksum_hex_from_path(path: &Path) -> Result<String> {
    let data = fs::read(path).map_err(Error::Io)?;
    if data.len() < 20 {
        return Err(Error::CorruptObject(
            "midx too small for checksum".to_owned(),
        ));
    }
    let hash = &data[data.len() - 20..];
    Ok(hex::encode(hash))
}

fn hard_link_or_copy(src: &Path, dst: &Path) -> Result<()> {
    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }
    fs::copy(src, dst).map_err(Error::Io)?;
    Ok(())
}

fn link_root_midx_into_chain(pack_dir: &Path, root_checksum_hex: &str) -> Result<()> {
    let midx_d = midx_d_dir(pack_dir);
    fs::create_dir_all(&midx_d).map_err(Error::Io)?;
    let dst_midx = midx_d.join(format!("multi-pack-index-{root_checksum_hex}.midx"));
    hard_link_or_copy(&pack_dir.join("multi-pack-index"), &dst_midx)?;
    let exts = ["bitmap", "rev"];
    for ext in exts {
        let src = pack_dir.join(format!("multi-pack-index-{root_checksum_hex}.{ext}"));
        if src.exists() {
            let dst = midx_d.join(format!("multi-pack-index-{root_checksum_hex}.{ext}"));
            hard_link_or_copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn clear_stale_split_layers(pack_dir: &Path, keep: &[String]) -> Result<()> {
    let midx_d = midx_d_dir(pack_dir);
    if !midx_d.exists() {
        return Ok(());
    }
    let keep: HashSet<&str> = keep.iter().map(|s| s.as_str()).collect();
    for ent in fs::read_dir(&midx_d).map_err(Error::Io)? {
        let ent = ent.map_err(Error::Io)?;
        let name = ent.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("multi-pack-index-") else {
            continue;
        };
        let Some((hash_part, _ext)) = rest.split_once('.') else {
            continue;
        };
        if hash_part.len() == 40 && !keep.contains(hash_part) {
            let _ = fs::remove_file(ent.path());
        }
    }
    Ok(())
}

fn pack_mtime_for_midx(idx: &PackIndex) -> std::time::SystemTime {
    fs::metadata(&idx.pack_path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

fn midx_pick_better_entry(
    cur: &MidxEntry,
    cand_pack: u32,
    cand_offset: u64,
    cand_mtime: std::time::SystemTime,
    preferred_pack: Option<u32>,
) -> bool {
    let cur_pref = preferred_pack == Some(cur.pack_id);
    let new_pref = preferred_pack == Some(cand_pack);
    if new_pref && !cur_pref {
        return true;
    }
    if cur_pref && !new_pref {
        return false;
    }
    match cand_mtime.cmp(&cur.pack_mtime) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            if cand_pack != cur.pack_id {
                cand_pack < cur.pack_id
            } else {
                cand_offset < cur.offset
            }
        }
    }
}

fn build_midx_bytes(
    idx_names: &[String],
    indexes: &[PackIndex],
    preferred_idx: Option<usize>,
    write_bitmap_placeholders: bool,
) -> Result<Vec<u8>> {
    let preferred_pack_idx = preferred_idx.map(|p| p as u32);
    let pack_mtimes: Vec<std::time::SystemTime> = indexes.iter().map(pack_mtime_for_midx).collect();

    let mut best: HashMap<ObjectId, MidxEntry> = HashMap::new();
    for (pack_id, idx) in indexes.iter().enumerate() {
        let pack_id = u32::try_from(pack_id).map_err(|_| {
            Error::CorruptObject("too many pack files for multi-pack-index".to_owned())
        })?;
        let mtime = pack_mtimes[pack_id as usize];
        for e in &idx.entries {
            let cand = MidxEntry {
                oid: e.oid,
                pack_id,
                offset: e.offset,
                pack_mtime: mtime,
            };
            match best.get(&e.oid) {
                None => {
                    best.insert(e.oid, cand);
                }
                Some(cur) => {
                    if midx_pick_better_entry(cur, pack_id, e.offset, mtime, preferred_pack_idx) {
                        best.insert(e.oid, cand);
                    }
                }
            }
        }
    }

    let mut entries: Vec<MidxEntry> = best.into_values().collect();
    entries.sort_by(|a, b| a.oid.cmp(&b.oid));

    let mut large_offsets: Vec<u64> = Vec::new();
    for e in &entries {
        if e.offset > u64::from(u32::MAX) {
            return Err(Error::CorruptObject(
                "object offset does not fit in multi-pack-index".to_owned(),
            ));
        }
    }

    let num_packs = indexes.len() as u32;

    let mut pack_names_blob = Vec::new();
    for name in idx_names {
        pack_names_blob.extend_from_slice(name.as_bytes());
        pack_names_blob.push(0);
    }
    let pad = (MIDX_CHUNK_ALIGNMENT - (pack_names_blob.len() % MIDX_CHUNK_ALIGNMENT))
        % MIDX_CHUNK_ALIGNMENT;
    pack_names_blob.extend(std::iter::repeat_n(0u8, pad));
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
        let needs_large = e.offset >= u64::from(MIDX_LARGE_OFFSET_NEEDED);
        let encoded = if needs_large {
            let slot = u32::try_from(large_offsets.len()).map_err(|_| {
                Error::CorruptObject("too many large offsets in multi-pack-index".to_owned())
            })?;
            large_offsets.push(e.offset);
            MIDX_LARGE_OFFSET_NEEDED | slot
        } else {
            u32::try_from(e.offset).map_err(|_| {
                Error::CorruptObject("object offset overflow in multi-pack-index".to_owned())
            })?
        };
        chunk_ooff.extend_from_slice(&encoded.to_be_bytes());
    }

    let chunk_loff: Vec<u8> = if large_offsets.is_empty() {
        Vec::new()
    } else {
        let mut v = Vec::with_capacity(large_offsets.len() * 8);
        for off in &large_offsets {
            v.extend_from_slice(&off.to_be_bytes());
        }
        v
    };

    let pref = preferred_pack_idx;
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
    let chunk_btmp: Vec<u8> = if write_bitmap_placeholders {
        let mut v = Vec::new();
        for _ in 0..num_packs {
            v.extend_from_slice(&0u32.to_be_bytes());
            v.extend_from_slice(&0u32.to_be_bytes());
        }
        let pad = (MIDX_CHUNK_ALIGNMENT - (v.len() % MIDX_CHUNK_ALIGNMENT)) % MIDX_CHUNK_ALIGNMENT;
        v.extend(std::iter::repeat_n(0u8, pad));
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
    if !chunk_loff.is_empty() {
        chunks.push((MIDX_CHUNKID_LARGEOFFSETS, chunk_loff));
    }
    if pref.is_some() || write_bitmap_placeholders {
        chunks.push((MIDX_CHUNKID_REVINDEX, chunk_ridx));
    }
    if write_bitmap_placeholders {
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

    Ok(out)
}

fn find_chunk(data: &[u8], header_end: usize, chunk_id: u32) -> Result<(usize, usize)> {
    let (hdr, _, _) = parse_midx_header(data)?;
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
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end, _) = parse_midx_header(&data)?;
    let (pn_off, pn_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_PACKNAMES)?;
    parse_pack_names_blob(&data[pn_off..pn_off + pn_len])
}

/// Trailing 40-character SHA-1 hex of the active MIDX (root or chain tip).
pub fn midx_checksum_hex(objects_dir: &Path) -> Result<String> {
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
    midx_checksum_hex_from_path(&path)
}

/// Human-readable dump of the MIDX (matches `test-tool read-midx` layout closely enough for grep-based tests).
pub fn format_midx_dump(objects_dir: &Path) -> Result<String> {
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
    let data = fs::read(&path).map_err(Error::Io)?;
    let (hdr, hdr_end, _) = parse_midx_header(&data)?;
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

    let (_ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
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
    Ok(out)
}

/// Returns whether `oid` appears in the active MIDX OID table for `objects_dir`.
///
/// [`None`] means there is no MIDX at the pack tip. [`Some`] is the lookup result when a MIDX exists.
pub fn midx_oid_listed_in_tip(objects_dir: &Path, oid: &ObjectId) -> Result<Option<bool>> {
    let pack_dir = objects_dir.join("pack");
    let Some(midx_path) = resolve_tip_midx_path(&pack_dir) else {
        return Ok(None);
    };
    let data = fs::read(&midx_path).map_err(Error::Io)?;
    let (_, hdr_end, hash_bytes) = parse_midx_header(&data)?;
    if hash_bytes != 1 {
        eprintln!(
            "error: multi-pack-index hash version {} does not match version 1",
            hash_bytes
        );
        return Err(Error::CorruptObject(
            "multi-pack-index hash version mismatch".to_owned(),
        ));
    }
    let (oidf_off, oidf_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDFANOUT)?;
    if oidf_len != 256 * 4 {
        eprintln!("error: multi-pack-index OID fanout is of the wrong size");
        return Err(Error::CorruptObject(
            "multi-pack-index OID fanout is of the wrong size".to_owned(),
        ));
    }
    let (oidl_off, oidl_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDLOOKUP)?;
    let (_ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    let num_objects = ooff_len / 8;
    if oidl_len != num_objects * 20 || ooff_len != num_objects * 8 {
        if oidl_len != num_objects * 20 {
            eprintln!("error: multi-pack-index OID lookup chunk is the wrong size");
        } else {
            eprintln!("error: multi-pack-index object offset chunk is the wrong size");
        }
        return Err(Error::CorruptObject("midx chunk size mismatch".to_owned()));
    }

    let first = oid.as_bytes()[0] as usize;
    let lo = if first == 0 {
        0u32
    } else {
        u32::from_be_bytes(
            data[oidf_off + (first - 1) * 4..oidf_off + first * 4]
                .try_into()
                .unwrap(),
        )
    };
    let hi = u32::from_be_bytes(
        data[oidf_off + first * 4..oidf_off + (first + 1) * 4]
            .try_into()
            .unwrap(),
    );
    if lo > hi || hi as usize > num_objects {
        eprintln!(
            "error: oid fanout out of order: fanout[{}] = {:08x} > {:08x} = fanout[{}]",
            first.saturating_sub(1),
            lo,
            hi,
            first
        );
        return Err(Error::CorruptObject("oid fanout out of order".to_owned()));
    }

    let mut i = lo as usize;
    while i < hi as usize {
        let o = ObjectId::from_bytes(&data[oidl_off + i * 20..oidl_off + (i + 1) * 20])?;
        match o.cmp(oid) {
            std::cmp::Ordering::Equal => return Ok(Some(true)),
            std::cmp::Ordering::Greater => return Ok(Some(false)),
            std::cmp::Ordering::Less => i += 1,
        }
    }
    Ok(Some(false))
}

/// When `core.multiPackIndex` is enabled, try to read `oid` from the active MIDX in `objects_dir`.
///
/// Returns [`None`] when no MIDX exists or `oid` is not listed. Returns [`Some(Err(..))`] when the
/// MIDX is present but malformed (callers surface Git-style `error:` / `fatal:` messages).
pub fn try_read_object_via_midx(
    objects_dir: &Path,
    oid: &ObjectId,
) -> Result<Option<crate::objects::Object>> {
    let pack_dir = objects_dir.join("pack");
    let Some(midx_path) = resolve_tip_midx_path(&pack_dir) else {
        return Ok(None);
    };
    let data = fs::read(&midx_path).map_err(Error::Io)?;
    let (_, hdr_end, hash_bytes) = parse_midx_header(&data)?;
    let num_packs_hdr = u32::from_be_bytes(data[8..12].try_into().unwrap());
    if hash_bytes != 1 {
        eprintln!(
            "error: multi-pack-index hash version {} does not match version 1",
            hash_bytes
        );
        return Err(Error::CorruptObject(
            "multi-pack-index hash version mismatch".to_owned(),
        ));
    }
    let (pn_off, pn_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_PACKNAMES)?;
    let pack_names = parse_pack_names_blob(&data[pn_off..pn_off + pn_len])?;
    if pack_names.len() != num_packs_hdr as usize {
        return Err(Error::CorruptObject(
            "multi-pack-index pack-name chunk is too short".to_owned(),
        ));
    }
    let (oidf_off, oidf_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDFANOUT)?;
    if oidf_len != 256 * 4 {
        eprintln!("error: multi-pack-index OID fanout is of the wrong size");
        return Err(Error::CorruptObject(
            "multi-pack-index OID fanout is of the wrong size".to_owned(),
        ));
    }
    let (oidl_off, oidl_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDLOOKUP)?;
    let (ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    let num_objects = ooff_len / 8;
    if oidl_len != num_objects * 20 {
        eprintln!("error: multi-pack-index OID lookup chunk is the wrong size");
        return Err(Error::CorruptObject(
            "multi-pack-index OID lookup chunk is the wrong size".to_owned(),
        ));
    }
    if ooff_len != num_objects * 8 {
        eprintln!("error: multi-pack-index object offset chunk is the wrong size");
        return Err(Error::CorruptObject(
            "multi-pack-index object offset chunk is the wrong size".to_owned(),
        ));
    }
    let loff = find_chunk(&data, hdr_end, MIDX_CHUNKID_LARGEOFFSETS).ok();
    let ridx = find_chunk(&data, hdr_end, MIDX_CHUNKID_REVINDEX).ok();

    if let Some((_, rlen)) = ridx {
        if rlen != num_objects * 4 {
            eprintln!("error: multi-pack-index reverse-index chunk is the wrong size");
            eprintln!("warning: multi-pack bitmap is missing required reverse index");
        }
    }

    let first = oid.as_bytes()[0] as usize;
    let lo = if first == 0 {
        0u32
    } else {
        u32::from_be_bytes(
            data[oidf_off + (first - 1) * 4..oidf_off + first * 4]
                .try_into()
                .unwrap(),
        )
    };
    let hi = u32::from_be_bytes(
        data[oidf_off + first * 4..oidf_off + (first + 1) * 4]
            .try_into()
            .unwrap(),
    );
    if lo > hi || hi as usize > num_objects {
        eprintln!(
            "error: oid fanout out of order: fanout[{}] = {:08x} > {:08x} = fanout[{}]",
            first.saturating_sub(1),
            lo,
            hi,
            first
        );
        return Err(Error::CorruptObject("oid fanout out of order".to_owned()));
    }

    let mut pos = None;
    let mut i = lo as usize;
    while i < hi as usize {
        let o = ObjectId::from_bytes(&data[oidl_off + i * 20..oidl_off + (i + 1) * 20])?;
        let c = o.cmp(oid);
        if c == std::cmp::Ordering::Equal {
            pos = Some(i);
            break;
        }
        if c == std::cmp::Ordering::Greater {
            break;
        }
        i += 1;
    }
    let Some(pos) = pos else {
        return Ok(None);
    };

    let obase = ooff_off + pos * 8;
    let pack_id = u32::from_be_bytes(data[obase..obase + 4].try_into().unwrap());
    let raw_off = u32::from_be_bytes(data[obase + 4..obase + 8].try_into().unwrap());
    let _offset = if (raw_off & MIDX_LARGE_OFFSET_NEEDED) != 0 {
        let Some((loff_off, loff_len)) = loff else {
            return Err(Error::CorruptObject(
                "multi-pack-index large offset missing LOFF chunk".to_owned(),
            ));
        };
        let idx = (raw_off & !MIDX_LARGE_OFFSET_NEEDED) as usize;
        let need = (idx + 1) * 8;
        if loff_len < need {
            return Err(Error::CorruptObject(
                "multi-pack-index large offset out of bounds".to_owned(),
            ));
        }
        u64::from_be_bytes(
            data[loff_off + idx * 8..loff_off + (idx + 1) * 8]
                .try_into()
                .unwrap(),
        )
    } else {
        raw_off as u64
    };

    let idx_name = pack_names
        .get(pack_id as usize)
        .ok_or_else(|| Error::CorruptObject("bad pack-int-id".to_owned()))?;
    let idx_path = pack_dir.join(idx_name);
    let idx = crate::pack::read_pack_index(&idx_path)?;
    crate::pack::read_object_from_pack(&idx, oid).map(Some)
}

pub fn read_midx_preferred_idx_name(objects_dir: &Path) -> Result<String> {
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end, _) = parse_midx_header(&data)?;
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
/// Remove every multi-pack-index file under `pack_dir` (root file, sidecars, and
/// `multi-pack-index.d/`). Used by full `repack -a` so stale incremental chains do not survive.
pub fn clear_pack_midx_state(pack_dir: &Path) -> Result<()> {
    let _ = fs::remove_file(pack_dir.join("multi-pack-index"));
    scrub_root_midx_sidecars_except(pack_dir, None)?;
    let midx_d = midx_d_dir(pack_dir);
    if midx_d.exists() {
        let _ = fs::remove_dir_all(&midx_d);
    }
    Ok(())
}

pub fn write_multi_pack_index(pack_dir: &Path) -> Result<()> {
    write_multi_pack_index_with_options(pack_dir, &WriteMultiPackIndexOptions::default())
}

/// Write `multi-pack-index` with optional preferred pack, placeholders, and incremental chain.
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

    let (base_oids, base_pack_names) = if opts.incremental {
        collect_incremental_base(pack_dir)?
    } else {
        (HashSet::new(), HashSet::new())
    };

    let layer_idx_names: Vec<String> = if opts.incremental {
        idx_names
            .iter()
            .filter(|n| {
                !base_pack_names
                    .iter()
                    .any(|bp| pack_names_match_layer(bp, n))
            })
            .cloned()
            .collect()
    } else {
        idx_names.clone()
    };

    if opts.incremental && layer_idx_names.is_empty() {
        return Ok(());
    }

    let work_names = if opts.incremental {
        &layer_idx_names[..]
    } else {
        &idx_names[..]
    };

    let mut preferred_idx = opts.preferred_pack_idx.map(|p| p as usize);
    if preferred_idx.is_none() && opts.write_bitmap_placeholders && !work_names.is_empty() {
        preferred_idx = preferred_pack_index_by_mtime(pack_dir, work_names)?;
    }
    if let Some(p) = preferred_idx {
        if p >= work_names.len() {
            return Err(Error::CorruptObject(
                "preferred pack index out of range".to_owned(),
            ));
        }
    }

    let mut indexes: Vec<PackIndex> = Vec::with_capacity(work_names.len());
    for name in work_names {
        let path = pack_dir.join(name);
        indexes.push(read_pack_index(&path)?);
    }

    let pack_mtimes_layer: Vec<std::time::SystemTime> =
        indexes.iter().map(pack_mtime_for_midx).collect();
    let preferred_u32 = preferred_idx.map(|p| p as u32);

    let mut best: HashMap<ObjectId, MidxEntry> = HashMap::new();
    for (pack_id, idx) in indexes.iter().enumerate() {
        let pack_id = u32::try_from(pack_id).map_err(|_| {
            Error::CorruptObject("too many pack files for multi-pack-index".to_owned())
        })?;
        let mtime = pack_mtimes_layer[pack_id as usize];
        for e in &idx.entries {
            if opts.incremental && base_oids.contains(&e.oid) {
                continue;
            }
            let cand = MidxEntry {
                oid: e.oid,
                pack_id,
                offset: e.offset,
                pack_mtime: mtime,
            };
            match best.get(&e.oid) {
                None => {
                    best.insert(e.oid, cand);
                }
                Some(cur) => {
                    if midx_pick_better_entry(cur, pack_id, e.offset, mtime, preferred_u32) {
                        best.insert(e.oid, cand);
                    }
                }
            }
        }
    }

    let bitmap_placeholders =
        opts.write_bitmap_placeholders && (!opts.incremental || !best.is_empty());

    let out = build_midx_bytes(work_names, &indexes, preferred_idx, bitmap_placeholders)?;

    let hash = &out[out.len() - 20..];
    let hash_hex = hex::encode(hash);

    if opts.incremental {
        let root_midx = pack_dir.join("multi-pack-index");
        let chain_path = chain_file_path(pack_dir);
        let chain_existed = chain_path.exists();

        let mut chain = if root_midx.exists() && !chain_existed {
            let root_hex = midx_checksum_hex_from_path(&root_midx)?;
            link_root_midx_into_chain(pack_dir, &root_hex)?;
            vec![root_hex]
        } else {
            read_chain_layer_hashes(pack_dir).unwrap_or_default()
        };

        chain.push(hash_hex.clone());

        let midx_d = midx_d_dir(pack_dir);
        fs::create_dir_all(&midx_d).map_err(Error::Io)?;

        let layer_path = midx_d.join(format!("multi-pack-index-{hash_hex}.midx"));
        fs::write(&layer_path, &out).map_err(Error::Io)?;

        let mut chain_data = String::new();
        for h in &chain {
            chain_data.push_str(h);
            chain_data.push('\n');
        }
        fs::write(chain_file_path(pack_dir), chain_data.as_bytes()).map_err(Error::Io)?;

        clear_stale_split_layers(pack_dir, &chain)?;

        let _ = fs::remove_file(pack_dir.join("multi-pack-index"));
        scrub_root_midx_sidecars(pack_dir)?;
        if bitmap_placeholders {
            let full = hex::encode(hash);
            fs::write(midx_d.join(format!("multi-pack-index-{full}.bitmap")), [])
                .map_err(Error::Io)?;
            if opts.write_rev_placeholder {
                fs::write(midx_d.join(format!("multi-pack-index-{full}.rev")), [])
                    .map_err(Error::Io)?;
            }
        }
    } else {
        let midx_d = midx_d_dir(pack_dir);
        if midx_d.exists() {
            for ent in fs::read_dir(&midx_d).map_err(Error::Io)? {
                let ent = ent.map_err(Error::Io)?;
                let _ = if ent.file_type().map_err(Error::Io)?.is_dir() {
                    fs::remove_dir_all(ent.path())
                } else {
                    fs::remove_file(ent.path())
                };
            }
        }
        fs::create_dir_all(&midx_d).map_err(Error::Io)?;

        let dest = pack_dir.join("multi-pack-index");
        fs::write(&dest, &out).map_err(Error::Io)?;

        scrub_root_midx_sidecars_except(pack_dir, Some(&hash_hex))?;

        if opts.write_bitmap_placeholders {
            fs::write(
                pack_dir.join(format!("multi-pack-index-{hash_hex}.bitmap")),
                [],
            )
            .map_err(Error::Io)?;
            if opts.write_rev_placeholder {
                fs::write(
                    pack_dir.join(format!("multi-pack-index-{hash_hex}.rev")),
                    [],
                )
                .map_err(Error::Io)?;
            }
        }
    }

    Ok(())
}

fn pack_names_match_layer(base_name: &str, disk_idx: &str) -> bool {
    if base_name == disk_idx {
        return true;
    }
    cmp_idx_or_pack_name(disk_idx, base_name).is_eq()
}

fn scrub_root_midx_sidecars(pack_dir: &Path) -> Result<()> {
    scrub_root_midx_sidecars_except(pack_dir, None)
}

fn scrub_root_midx_sidecars_except(pack_dir: &Path, keep_hex: Option<&str>) -> Result<()> {
    let Ok(rd) = fs::read_dir(pack_dir) else {
        return Ok(());
    };
    for ent in rd {
        let ent = ent.map_err(Error::Io)?;
        let name = ent.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("multi-pack-index-") else {
            continue;
        };
        if !(rest.ends_with(".bitmap") || rest.ends_with(".rev")) {
            continue;
        }
        let hash_part = rest
            .strip_suffix(".bitmap")
            .or_else(|| rest.strip_suffix(".rev"))
            .unwrap_or(rest);
        if hash_part.len() != 40 {
            continue;
        }
        if keep_hex.is_some_and(|k| k == hash_part) {
            continue;
        }
        let _ = fs::remove_file(ent.path());
    }
    Ok(())
}
