//! Multi-pack-index (MIDX) file writing.
//!
//! Writes a Git-compatible `multi-pack-index` file (version 1, SHA-1) covering
//! all `pack-*.idx` files in a pack directory. Objects that appear in multiple
//! packs keep the last occurrence when pack names are sorted lexicographically
//! (deterministic).

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

// `git midx.h` (MIDX_LARGE_OFFSET_NEEDED).
const MIDX_LARGE_OFFSET_NEEDED: u32 = 0x8000_0000;

struct MidxEntry {
    oid: ObjectId,
    pack_id: u32,
    offset: u64,
}

/// Build `objects/pack/multi-pack-index` for all pack indexes in `pack_dir`.
///
/// Returns an error if there are no `.idx` files, if an object offset does not
/// fit in 31 bits (no `LOFF` chunk yet), or if I/O fails.
pub fn write_multi_pack_index(pack_dir: &Path) -> Result<()> {
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
            best.insert(e.oid, (pack_id, e.offset));
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

    let chunks: Vec<(u32, Vec<u8>)> = vec![
        (MIDX_CHUNKID_PACKNAMES, chunk_pnam),
        (MIDX_CHUNKID_OIDFANOUT, chunk_oidf),
        (MIDX_CHUNKID_OIDLOOKUP, chunk_oidl),
        (MIDX_CHUNKID_OBJECTOFFSETS, chunk_ooff),
    ];

    let num_chunks: u8 = chunks.len().try_into().map_err(|_| {
        Error::CorruptObject("too many MIDX chunks".to_owned())
    })?;

    let mut body = Vec::new();
    let mut cur_offset = MIDX_HEADER_SIZE as u64 + ((chunks.len() + 1) * CHUNK_TOC_ENTRY_SIZE) as u64;

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
    Ok(())
}
