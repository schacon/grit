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
const MIDX_CHUNKID_REVINDEX: u32 = 0x5249_4458;
const MIDX_CHUNKID_BITMAPPED_PACKS: u32 = 0x4254_4d50;

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
    /// Basename of the preferred pack (e.g. `pack-abc.idx` or `pack-abc.pack`); resolved against
    /// the working pack name list after optional subset filtering.
    pub preferred_pack_name: Option<String>,
    /// If set, only these `pack-*.idx` basenames are included, in this order (Git `--stdin-packs`).
    pub pack_names_subset_ordered: Option<Vec<String>>,
    /// When true, append RIDX + empty BTMP chunks so `test-tool read-midx --bitmap` succeeds.
    pub write_bitmap_placeholders: bool,
    /// When true, write a new layer in `multi-pack-index.d/` and extend the chain file
    /// instead of replacing `pack/multi-pack-index`.
    pub incremental: bool,
    /// When true with [`Self::write_bitmap_placeholders`], also create an empty `.rev`
    /// sidecar (Git `GIT_TEST_MIDX_WRITE_REV` compatibility).
    pub write_rev_placeholder: bool,
}

fn normalize_pack_idx_basename(raw: &str) -> Result<String> {
    let t = raw.trim();
    let t = std::path::Path::new(t)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(t);
    let t = t.strip_prefix("./").unwrap_or(t);
    if t.ends_with(".idx") {
        Ok(t.to_string())
    } else if t.ends_with(".pack") {
        Ok(format!("{}.idx", t.strip_suffix(".pack").unwrap_or(t)))
    } else {
        Ok(format!("{t}.idx"))
    }
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
    let (_, hdr_end) = parse_midx_header(data)?;
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

fn build_midx_bytes(
    idx_names: &[String],
    indexes: &[PackIndex],
    preferred_idx: Option<usize>,
    write_bitmap_placeholders: bool,
) -> Result<Vec<u8>> {
    let preferred_pack_idx = preferred_idx.map(|p| p as u32);

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
    for name in idx_names {
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

    // BTMP: per-pack (bitmap_pos, bitmap_nr) in the pseudo-bitmap namespace, matching Git's
    // `write_midx_bitmapped_packs` (cumulative start + object count per pack).
    let chunk_btmp: Vec<u8> = if write_bitmap_placeholders {
        let mut v = Vec::new();
        let mut cumulative = 0u32;
        for idx in indexes {
            let n = u32::try_from(idx.entries.len()).map_err(|_| {
                Error::CorruptObject("too many objects in pack for MIDX BTMP".to_owned())
            })?;
            v.extend_from_slice(&cumulative.to_be_bytes());
            v.extend_from_slice(&n.to_be_bytes());
            cumulative = cumulative.saturating_add(n);
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
    if pref.is_some() || write_bitmap_placeholders {
        chunks.push((MIDX_CHUNKID_REVINDEX, chunk_ridx));
    }
    if write_bitmap_placeholders {
        chunks.push((MIDX_CHUNKID_BITMAPPED_PACKS, chunk_btmp));
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
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end) = parse_midx_header(&data)?;
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

/// OID rows from the active multi-pack-index, plus reverse-index order for pack-reuse bitmap bits.
///
/// Git assigns each object a **global bitmap bit** equal to its position in the MIDX reverse index
/// (`RIDX` chunk) traversal order — not its position in the pack `.idx` file. Helpers on this struct
/// map [`ObjectId`] → global bit the same way as `midx-write.c` (`midx_pack_order`).
#[derive(Debug, Clone)]
pub struct MidxReuseTables {
    /// OIDs in MIDX lexicographic order (same order as the OID lookup chunk).
    pub oids: Vec<ObjectId>,
    /// `(pack_int_id, in-pack offset)` parallel to `oids`.
    pub pack_and_offset: Vec<(u32, u64)>,
    /// `rid_order[rank]` is the OID-table index of the object at global bitmap rank `rank`.
    pub rid_order: Vec<u32>,
    /// Inverse map: global bitmap rank for each OID-table index.
    pub oid_idx_to_rank: Vec<u32>,
}

/// Load OID / object-offset / reverse-index tables from the tip MIDX (root or chain tip).
///
/// Returns [`None`] when there is no MIDX or no `RIDX` chunk (no pseudo-bitmap ordering).
pub fn load_midx_reuse_tables(objects_dir: &Path) -> Result<Option<MidxReuseTables>> {
    let pack_dir = objects_dir.join("pack");
    let Some(path) = resolve_tip_midx_path(&pack_dir) else {
        return Ok(None);
    };
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end) = parse_midx_header(&data)?;
    let (oidl_off, oid_l_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDLOOKUP)?;
    let (ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    let Ok((ridx_off, ridx_len)) = find_chunk(&data, hdr_end, MIDX_CHUNKID_REVINDEX) else {
        return Ok(None);
    };
    if oid_l_len % 20 != 0 || ooff_len != oid_l_len / 20 * 8 {
        return Err(Error::CorruptObject(
            "MIDX OID / offset chunk size mismatch".to_owned(),
        ));
    }
    let num_objects = oid_l_len / 20;
    if ridx_len != num_objects.saturating_mul(4) {
        return Err(Error::CorruptObject(
            "MIDX reverse index length does not match object count".to_owned(),
        ));
    }
    if num_objects == 0 {
        return Ok(None);
    }

    let mut oids = Vec::with_capacity(num_objects);
    for i in 0..num_objects {
        let base = oidl_off + i * 20;
        oids.push(ObjectId::from_bytes(&data[base..base + 20])?);
    }

    let mut pack_and_offset = Vec::with_capacity(num_objects);
    for i in 0..num_objects {
        let ob = ooff_off + i * 8;
        let pack_id = u32::from_be_bytes(data[ob..ob + 4].try_into().unwrap());
        let off32 = u32::from_be_bytes(data[ob + 4..ob + 8].try_into().unwrap());
        pack_and_offset.push((pack_id, u64::from(off32)));
    }

    let mut rid_order = Vec::with_capacity(num_objects);
    for i in 0..num_objects {
        let base = ridx_off + i * 4;
        rid_order.push(u32::from_be_bytes(data[base..base + 4].try_into().unwrap()));
    }

    let mut oid_idx_to_rank = vec![0u32; num_objects];
    for (rank, &oid_idx) in rid_order.iter().enumerate() {
        let idx = usize::try_from(oid_idx)
            .map_err(|_| Error::CorruptObject("bad MIDX reverse index entry".to_owned()))?;
        if idx >= num_objects {
            return Err(Error::CorruptObject(
                "MIDX reverse index out of range".to_owned(),
            ));
        }
        oid_idx_to_rank[idx] = u32::try_from(rank)
            .map_err(|_| Error::CorruptObject("too many MIDX objects".to_owned()))?;
    }

    Ok(Some(MidxReuseTables {
        oids,
        pack_and_offset,
        rid_order,
        oid_idx_to_rank,
    }))
}

impl MidxReuseTables {
    /// Global pseudo-bitmap index for `oid`, or [`None`] if the object is not in this MIDX.
    #[must_use]
    pub fn global_bitmap_bit(&self, oid: &ObjectId) -> Option<u32> {
        let oid_idx = self.oids.binary_search(oid).ok()?;
        Some(self.oid_idx_to_rank[oid_idx])
    }
}

/// One pack's slice of the MIDX pseudo-bitmap namespace (`BTMP` chunk).
#[derive(Debug, Clone, Copy)]
pub struct MidxBtmpPackRange {
    /// Pack index in the MIDX pack-names list.
    pub pack_id: u32,
    /// First bit index assigned to this pack (cumulative object order).
    pub bitmap_pos: u32,
    /// Number of objects in this pack (same as `.idx` entry count).
    pub bitmap_nr: u32,
}

/// Read per-pack `(bitmap_pos, bitmap_nr)` from the active MIDX `BTMP` chunk.
///
/// Returns an empty vector when the MIDX has no bitmapped-packs chunk.
pub fn read_midx_btmp_ranges(objects_dir: &Path) -> Result<Vec<MidxBtmpPackRange>> {
    let pack_dir = objects_dir.join("pack");
    let Some(path) = resolve_tip_midx_path(&pack_dir) else {
        return Ok(Vec::new());
    };
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end) = parse_midx_header(&data)?;
    let Ok((btmp_off, btmp_len)) = find_chunk(&data, hdr_end, MIDX_CHUNKID_BITMAPPED_PACKS) else {
        return Ok(Vec::new());
    };
    if btmp_len == 0 || btmp_len % 8 != 0 {
        return Err(Error::CorruptObject(
            "invalid MIDX BTMP chunk length".to_owned(),
        ));
    }
    let num_packs = u32::from_be_bytes(data[8..12].try_into().unwrap());
    let n_entries = btmp_len / 8;
    if u32::try_from(n_entries).ok() != Some(num_packs) {
        return Err(Error::CorruptObject(
            "MIDX BTMP entry count does not match num_packs".to_owned(),
        ));
    }
    let mut out = Vec::with_capacity(n_entries);
    for i in 0..n_entries {
        let base = btmp_off + i * 8;
        let bitmap_pos = u32::from_be_bytes(data[base..base + 4].try_into().unwrap());
        let bitmap_nr = u32::from_be_bytes(data[base + 4..base + 8].try_into().unwrap());
        out.push(MidxBtmpPackRange {
            pack_id: u32::try_from(i)
                .map_err(|_| Error::CorruptObject("too many packs in MIDX BTMP".to_owned()))?,
            bitmap_pos,
            bitmap_nr,
        });
    }
    Ok(out)
}

/// Look up which pack and in-pack offset holds `oid` according to the active MIDX.
pub fn midx_lookup_pack_and_offset(objects_dir: &Path, oid: &ObjectId) -> Result<(u32, u64)> {
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
    let data = fs::read(&path).map_err(Error::Io)?;
    let (_, hdr_end) = parse_midx_header(&data)?;
    let (fanout_off, fanout_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDFANOUT)?;
    let (oidl_off, oid_l_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OIDLOOKUP)?;
    let (ooff_off, ooff_len) = find_chunk(&data, hdr_end, MIDX_CHUNKID_OBJECTOFFSETS)?;
    if fanout_len != 256 * 4 || oid_l_len % 20 != 0 || ooff_len != oid_l_len / 20 * 8 {
        return Err(Error::CorruptObject("truncated MIDX OID chunks".to_owned()));
    }
    let num_objects = oid_l_len / 20;
    let first = oid.as_bytes()[0] as usize;
    let j0 = if first == 0 {
        0usize
    } else {
        u32::from_be_bytes(
            data[fanout_off + (first - 1) * 4..fanout_off + first * 4]
                .try_into()
                .unwrap(),
        ) as usize
    };
    let j1 = u32::from_be_bytes(
        data[fanout_off + first * 4..fanout_off + (first + 1) * 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let mut lo = j0;
    let mut hi = j1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let base = oidl_off + mid * 20;
        let cmp = data[base..base + 20].cmp(oid.as_bytes());
        if cmp == std::cmp::Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo >= num_objects {
        return Err(Error::CorruptObject(format!(
            "object {} not in multi-pack-index",
            oid.to_hex()
        )));
    }
    let base = oidl_off + lo * 20;
    if data[base..base + 20] != *oid.as_bytes() {
        return Err(Error::CorruptObject(format!(
            "object {} not in multi-pack-index",
            oid.to_hex()
        )));
    }
    let ob = ooff_off + lo * 8;
    let pack_id = u32::from_be_bytes(data[ob..ob + 4].try_into().unwrap());
    let off32 = u32::from_be_bytes(data[ob + 4..ob + 8].try_into().unwrap());
    Ok((pack_id, u64::from(off32)))
}

pub fn read_midx_preferred_idx_name(objects_dir: &Path) -> Result<String> {
    let pack_dir = objects_dir.join("pack");
    let path = resolve_tip_midx_path(&pack_dir)
        .ok_or_else(|| Error::CorruptObject("no multi-pack-index found".to_owned()))?;
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

    let idx_names: Vec<String> = if let Some(sub) = &opts.pack_names_subset_ordered {
        let mut out = Vec::new();
        for line in sub {
            let want = normalize_pack_idx_basename(line)?;
            let found = idx_names
                .iter()
                .find(|n| **n == want)
                .cloned()
                .ok_or_else(|| {
                    Error::CorruptObject(format!("pack index not in repository: {want}"))
                })?;
            if !out.contains(&found) {
                out.push(found);
            }
        }
        if out.is_empty() {
            return Err(Error::CorruptObject(
                "stdin-packs list produced empty pack set".to_owned(),
            ));
        }
        out
    } else {
        idx_names
    };

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
    if preferred_idx.is_none() {
        if let Some(ref raw) = opts.preferred_pack_name {
            let want = normalize_pack_idx_basename(raw)?;
            preferred_idx = work_names.iter().position(|n| *n == want);
            if preferred_idx.is_none() {
                return Err(Error::CorruptObject(format!(
                    "preferred pack not in MIDX pack list: {want}"
                )));
            }
        }
    }
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

    let mut best: HashMap<ObjectId, (u32, u64)> = HashMap::new();
    for (pack_id, idx) in indexes.iter().enumerate() {
        let pack_id = u32::try_from(pack_id).map_err(|_| {
            Error::CorruptObject("too many pack files for multi-pack-index".to_owned())
        })?;
        for e in &idx.entries {
            if opts.incremental && base_oids.contains(&e.oid) {
                continue;
            }
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
