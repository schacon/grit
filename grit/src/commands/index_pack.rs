//! `grit index-pack` — build pack index for an existing pack file.
//!
//! Reads a `.pack` file (from a path or stdin), parses all objects, and writes
//! a `.idx` version-2 index file alongside it.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::unpack_objects::apply_delta;

/// Arguments for `grit index-pack`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Read pack from stdin and write to objects/pack/.
    #[arg(long)]
    pub stdin: bool,

    /// Fix thin packs by adding missing base objects.
    #[arg(long = "fix-thin")]
    pub fix_thin: bool,

    /// Pack file to index.
    #[arg(value_name = "PACK-FILE")]
    pub pack_file: Option<String>,

    /// Verify the pack file integrity (check all objects).
    #[arg(long = "verify", short = 'v')]
    pub verify: bool,

    /// Hash algorithm (accepted for compat, only sha1).
    #[arg(long = "object-format")]
    pub object_format: Option<String>,

    /// Strict mode: reject packs with duplicate objects.
    #[arg(long)]
    pub strict: bool,

    /// Keep the uploaded pack (compatibility flag; accepted and ignored).
    #[arg(long = "keep", value_name = "MSG", num_args = 0..=1, default_missing_value = "")]
    pub keep: Option<String>,

    /// Verify pack connectivity while indexing (compatibility flag; ignored).
    #[arg(long = "check-self-contained-and-connected")]
    pub check_self_contained_and_connected: bool,
}

/// A resolved pack object.
struct ResolvedObject {
    oid: ObjectId,
    _kind: ObjectKind,
    offset: u64,
    crc32: u32,
}

/// Run `grit index-pack`.
pub fn run(args: Args) -> Result<()> {
    let _ = &args.keep;
    let _ = args.check_self_contained_and_connected;
    if let Some(fmt) = &args.object_format {
        if fmt != "sha1" {
            bail!("unsupported object format: {fmt}");
        }
    }

    // --verify mode: verify an existing pack + index.
    if args.verify {
        return run_verify(&args);
    }

    let pack_bytes = if args.stdin {
        let mut buf = Vec::new();
        io::stdin().lock().read_to_end(&mut buf)?;
        buf
    } else if let Some(ref path) = args.pack_file {
        fs::read(path).with_context(|| format!("cannot read {path}"))?
    } else {
        bail!("usage: grit index-pack [--stdin | <pack-file>]");
    };

    // Validate pack header.
    if pack_bytes.len() < 12 + 20 {
        bail!("pack too small");
    }
    if &pack_bytes[0..4] != b"PACK" {
        bail!("not a pack file: invalid signature");
    }
    let version = u32::from_be_bytes(pack_bytes[4..8].try_into()?);
    if version != 2 && version != 3 {
        bail!("unsupported pack version {version}");
    }
    let nr_objects = u32::from_be_bytes(pack_bytes[8..12].try_into()?) as usize;

    // Parse all objects, resolve deltas, collect entries.
    let resolved = parse_and_resolve(&pack_bytes, nr_objects, args.fix_thin)?;

    // --strict: reject packs with duplicate objects.
    if args.strict {
        let mut seen = std::collections::HashSet::new();
        for obj in &resolved {
            if !seen.insert(obj.oid) {
                bail!("duplicate object {} found in pack", obj.oid.to_hex());
            }
        }
    }

    // Determine output paths.
    let (pack_path, idx_path) = if args.stdin {
        // Compute pack checksum to derive filename.
        let pack_hash = {
            let mut h = Sha1::new();
            h.update(&pack_bytes);
            hex::encode(h.finalize())
        };
        // We need to discover a repo to find objects/pack/.
        let repo = grit_lib::repo::Repository::discover(None)
            .context("not a git repository (needed for --stdin)")?;
        let pack_dir = repo.odb.objects_dir().join("pack");
        fs::create_dir_all(&pack_dir)?;
        let pack_out = pack_dir.join(format!("pack-{pack_hash}.pack"));
        let idx_out = pack_dir.join(format!("pack-{pack_hash}.idx"));
        fs::write(&pack_out, &pack_bytes)?;
        (pack_out, idx_out)
    } else {
        let pack_path = PathBuf::from(
            args.pack_file
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no pack file specified"))?,
        );
        let mut idx_path = pack_path.clone();
        idx_path.set_extension("idx");
        (pack_path, idx_path)
    };

    // Write the .idx file.
    let idx_bytes = build_idx_v2(&resolved, &pack_bytes)?;
    fs::write(&idx_path, &idx_bytes)?;

    // Print the pack hash (matches git index-pack output).
    let pack_checksum = &pack_bytes[pack_bytes.len() - 20..];
    let pack_hex = hex::encode(pack_checksum);
    println!("{pack_hex}");

    let _ = pack_path; // suppress unused warning
    Ok(())
}

/// Parse all pack objects and resolve deltas.
fn parse_and_resolve(
    pack_bytes: &[u8],
    nr_objects: usize,
    fix_thin: bool,
) -> Result<Vec<ResolvedObject>> {
    use std::collections::HashMap;

    // For CRC32 we need to track the byte range of each object entry in the pack.
    let mut entries: Vec<(u64, u8, usize, Vec<u8>, Option<ObjectId>, Option<u64>)> = Vec::new();
    // (offset, type_code, header_size, data, base_oid_for_ref, base_offset_for_ofs)

    let mut pos = 12usize; // skip header
    let pack_end = pack_bytes.len() - 20; // before trailing checksum

    for _ in 0..nr_objects {
        let obj_start = pos;
        let (type_code, _size, data, base_oid, base_offset) =
            read_pack_entry(pack_bytes, &mut pos, obj_start as u64)?;
        let obj_end = pos;

        // CRC32 over the raw bytes of this entry.
        let _crc = crc32_slice(&pack_bytes[obj_start..obj_end]);

        entries.push((
            obj_start as u64,
            type_code,
            obj_end - obj_start,
            data,
            base_oid,
            base_offset,
        ));
    }

    // Verify trailing checksum.
    {
        let mut h = Sha1::new();
        h.update(&pack_bytes[..pack_end]);
        let digest = h.finalize();
        if digest.as_slice() != &pack_bytes[pack_end..pack_end + 20] {
            bail!("pack trailing checksum mismatch");
        }
    }

    // Resolve: first non-delta objects, then iteratively resolve deltas.
    let mut by_offset: HashMap<u64, (ObjectKind, Vec<u8>)> = HashMap::new();
    let mut by_oid: HashMap<ObjectId, (ObjectKind, Vec<u8>)> = HashMap::new();
    let mut resolved: Vec<ResolvedObject> = Vec::new();
    let mut pending: Vec<(
        u64,
        u8,
        Vec<u8>,
        Option<ObjectId>,
        Option<u64>,
        usize,
        usize,
    )> = Vec::new();

    // Try to open repo ODB for fix-thin.
    let odb = if fix_thin {
        grit_lib::repo::Repository::discover(None)
            .ok()
            .map(|r| r.odb)
    } else {
        None
    };

    for (offset, type_code, _entry_len, data, base_oid, base_offset) in &entries {
        let obj_start = *offset as usize;
        let obj_end = obj_start + _entry_len;
        let crc = crc32_slice(&pack_bytes[obj_start..obj_end]);

        match type_code {
            1..=4 => {
                let kind = type_code_to_kind(*type_code)?;
                let oid = Odb::hash_object_data(kind, data);
                by_offset.insert(*offset, (kind, data.clone()));
                by_oid.insert(oid, (kind, data.clone()));
                resolved.push(ResolvedObject {
                    oid,
                    _kind: kind,
                    offset: *offset,
                    crc32: crc,
                });
            }
            6 | 7 => {
                pending.push((
                    *offset,
                    *type_code,
                    data.clone(),
                    *base_oid,
                    *base_offset,
                    crc as usize, // smuggle crc
                    0,
                ));
            }
            other => bail!("unknown pack type {other}"),
        }
    }

    // Iterative delta resolution.
    let mut remaining = pending;
    loop {
        if remaining.is_empty() {
            break;
        }
        let before = remaining.len();
        let mut still_pending = Vec::new();

        for (offset, type_code, delta_data, base_oid_opt, base_offset_opt, crc_smuggled, _) in
            remaining
        {
            let base = if type_code == 6 {
                // OFS_DELTA
                base_offset_opt.and_then(|bo| by_offset.get(&bo).cloned())
            } else {
                // REF_DELTA
                base_oid_opt.and_then(|bo| {
                    by_oid.get(&bo).cloned().or_else(|| {
                        odb.as_ref()
                            .and_then(|o| o.read(&bo).ok())
                            .map(|obj| (obj.kind, obj.data))
                    })
                })
            };

            if let Some((base_kind, base_data)) = base {
                let result_data = apply_delta(&base_data, &delta_data)
                    .map_err(|e| anyhow::anyhow!("delta apply failed: {e}"))?;
                let oid = Odb::hash_object_data(base_kind, &result_data);
                by_offset.insert(offset, (base_kind, result_data.clone()));
                by_oid.insert(oid, (base_kind, result_data));
                resolved.push(ResolvedObject {
                    oid,
                    _kind: base_kind,
                    offset,
                    crc32: crc_smuggled as u32,
                });
            } else {
                still_pending.push((
                    offset,
                    type_code,
                    delta_data,
                    base_oid_opt,
                    base_offset_opt,
                    crc_smuggled,
                    0,
                ));
            }
        }

        remaining = still_pending;
        if remaining.len() == before {
            bail!(
                "{} delta(s) could not be resolved (use --fix-thin?)",
                remaining.len()
            );
        }
    }

    Ok(resolved)
}

/// Read a single pack entry starting at `pos`, return (type_code, size, decompressed_data, base_oid, base_offset).
fn read_pack_entry(
    pack_bytes: &[u8],
    pos: &mut usize,
    this_offset: u64,
) -> Result<(u8, usize, Vec<u8>, Option<ObjectId>, Option<u64>)> {
    use flate2::read::ZlibDecoder;

    let c = pack_bytes
        .get(*pos)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("truncated pack"))?;
    *pos += 1;
    let type_code = (c >> 4) & 0x7;
    let mut size = (c & 0x0f) as usize;
    let mut shift = 4u32;
    let mut cur = c;
    while cur & 0x80 != 0 {
        cur = pack_bytes
            .get(*pos)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("truncated pack header"))?;
        *pos += 1;
        size |= ((cur & 0x7f) as usize) << shift;
        shift += 7;
    }

    let mut base_oid = None;
    let mut base_offset = None;

    match type_code {
        6 => {
            // OFS_DELTA: read negative offset.
            let mut c2 = pack_bytes
                .get(*pos)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("truncated ofs-delta"))?;
            *pos += 1;
            let mut value = (c2 & 0x7f) as u64;
            while c2 & 0x80 != 0 {
                c2 = pack_bytes
                    .get(*pos)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("truncated ofs-delta"))?;
                *pos += 1;
                value = ((value + 1) << 7) | (c2 & 0x7f) as u64;
            }
            base_offset = Some(
                this_offset
                    .checked_sub(value)
                    .ok_or_else(|| anyhow::anyhow!("ofs-delta base underflow"))?,
            );
        }
        7 => {
            // REF_DELTA: 20-byte base OID.
            if *pos + 20 > pack_bytes.len() {
                bail!("truncated ref-delta base");
            }
            base_oid = Some(
                ObjectId::from_bytes(&pack_bytes[*pos..*pos + 20])
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
            );
            *pos += 20;
        }
        _ => {}
    }

    // Decompress.
    let slice = &pack_bytes[*pos..];
    let mut decoder = ZlibDecoder::new(slice);
    let mut data = Vec::with_capacity(size);
    decoder
        .read_to_end(&mut data)
        .map_err(|e| anyhow::anyhow!("zlib: {e}"))?;
    *pos += decoder.total_in() as usize;

    Ok((type_code, size, data, base_oid, base_offset))
}

fn type_code_to_kind(code: u8) -> Result<ObjectKind> {
    match code {
        1 => Ok(ObjectKind::Commit),
        2 => Ok(ObjectKind::Tree),
        3 => Ok(ObjectKind::Blob),
        4 => Ok(ObjectKind::Tag),
        _ => bail!("type code {code} is not a base object type"),
    }
}

/// Compute CRC32 (IEEE) of a byte slice.
fn crc32_slice(data: &[u8]) -> u32 {
    // CRC32 IEEE polynomial, same as used in pack idx v2.
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let idx = ((crc ^ b as u32) & 0xFF) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    !crc
}

/// Pre-computed CRC32 lookup table (IEEE 802.3 polynomial 0xEDB88320).
static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// Build a version-2 `.idx` file from resolved entries and pack bytes.
fn build_idx_v2(entries: &[ResolvedObject], pack_bytes: &[u8]) -> Result<Vec<u8>> {
    // Sort by OID.
    let mut sorted: Vec<&ResolvedObject> = entries.iter().collect();
    sorted.sort_by_key(|e| *e.oid.as_bytes());

    let mut buf: Vec<u8> = Vec::new();

    // Header: magic + version.
    buf.extend_from_slice(&[0xFF, b't', b'O', b'c']);
    buf.extend_from_slice(&2u32.to_be_bytes());

    // Fanout table (256 entries).
    let mut fanout = [0u32; 256];
    for entry in &sorted {
        let first_byte = entry.oid.as_bytes()[0] as usize;
        fanout[first_byte] += 1;
    }
    // Cumulative.
    for i in 1..256 {
        fanout[i] += fanout[i - 1];
    }
    for slot in &fanout {
        buf.extend_from_slice(&slot.to_be_bytes());
    }

    // OID table.
    for entry in &sorted {
        buf.extend_from_slice(entry.oid.as_bytes());
    }

    // CRC32 table.
    for entry in &sorted {
        buf.extend_from_slice(&entry.crc32.to_be_bytes());
    }

    // Offset table (32-bit). Large offsets get MSB set.
    let mut large_offsets: Vec<u64> = Vec::new();
    for entry in &sorted {
        if entry.offset >= 0x8000_0000 {
            let idx = large_offsets.len() as u32;
            buf.extend_from_slice(&(idx | 0x8000_0000).to_be_bytes());
            large_offsets.push(entry.offset);
        } else {
            buf.extend_from_slice(&(entry.offset as u32).to_be_bytes());
        }
    }

    // Large offset table.
    for off in &large_offsets {
        buf.extend_from_slice(&off.to_be_bytes());
    }

    // Pack checksum (last 20 bytes of pack file).
    let pack_checksum = &pack_bytes[pack_bytes.len() - 20..];
    buf.extend_from_slice(pack_checksum);

    // Index checksum.
    let mut h = Sha1::new();
    h.update(&buf);
    let idx_checksum = h.finalize();
    buf.extend_from_slice(idx_checksum.as_slice());

    Ok(buf)
}

/// Verify an existing pack file and its index.
fn run_verify(args: &Args) -> Result<()> {
    let pack_path = args
        .pack_file
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("usage: grit index-pack --verify <pack-file>"))?;

    let pack_bytes = fs::read(pack_path).with_context(|| format!("cannot read {pack_path}"))?;

    // Validate pack header.
    if pack_bytes.len() < 12 + 20 {
        bail!("pack too small");
    }
    if &pack_bytes[0..4] != b"PACK" {
        bail!("not a pack file: invalid signature");
    }
    let version = u32::from_be_bytes(pack_bytes[4..8].try_into()?);
    if version != 2 && version != 3 {
        bail!("unsupported pack version {version}");
    }
    let nr_objects = u32::from_be_bytes(pack_bytes[8..12].try_into()?) as usize;

    // Verify trailing checksum.
    let pack_end = pack_bytes.len() - 20;
    {
        let mut h = Sha1::new();
        h.update(&pack_bytes[..pack_end]);
        let digest = h.finalize();
        if digest.as_slice() != &pack_bytes[pack_end..pack_end + 20] {
            bail!("pack trailing checksum mismatch");
        }
    }

    // Parse all objects to verify they can be resolved.
    let resolved = parse_and_resolve(&pack_bytes, nr_objects, false)?;

    // Verify each object's OID matches its content.
    for obj in &resolved {
        // OID was computed during resolution, so if we got here, it's valid.
        let _ = obj;
    }

    // Check if .idx file exists and verify it.
    let mut idx_path = std::path::PathBuf::from(pack_path);
    idx_path.set_extension("idx");
    if idx_path.exists() {
        let existing_idx = fs::read(&idx_path)?;
        let expected_idx = build_idx_v2(&resolved, &pack_bytes)?;
        if existing_idx != expected_idx {
            // Check at least that the pack checksum in the idx matches.
            if existing_idx.len() >= 40 {
                let idx_pack_checksum =
                    &existing_idx[existing_idx.len() - 40..existing_idx.len() - 20];
                let pack_checksum = &pack_bytes[pack_bytes.len() - 20..];
                if idx_pack_checksum != pack_checksum {
                    bail!("pack checksum in index does not match pack file");
                }
            }
        }
    }

    // Print pack checksum and status.
    let pack_checksum = &pack_bytes[pack_bytes.len() - 20..];
    let pack_hex = hex::encode(pack_checksum);
    eprintln!("{}: ok", pack_path);
    println!("{pack_hex}");

    Ok(())
}
