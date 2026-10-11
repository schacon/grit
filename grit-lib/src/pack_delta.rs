//! Parallel delta encoding and zlib compression for pack serialization.
//!
//! [`serialize_pack_with_deltas`] splits work into a parallel preparation stage
//! (delta bytes + deflate per planned entry) and a serial emission stage (headers,
//! OFS vs REF selection, trailer).

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::config::ConfigSet;
use crate::delta_encode::{encode_prefix_extension_delta, DeltaIndex};
use crate::error::{Error, Result};
use crate::objects::{HashAlgo, ObjectId, ObjectKind};
use crate::odb::Odb;
use crate::pack_objects::{
    append_pack_trailer, encode_ofs_delta_distance, encode_pack_object_header, pack_type_code,
    PackBuildOptions, PlannedEntry,
};
use crate::parallel::{
    try_par_map_force, ParallelHashError, ParallelOp, ParallelOptions, ThreadPolicy,
};

/// Per-entry compressed payload produced by the parallel stage.
enum PreparedBody {
    /// Entire pack object encoding copied verbatim (reused on-disk whole object).
    Raw(Vec<u8>),
    /// zlib-compressed bytes only (header written in the serial stage).
    Zlib(Vec<u8>),
}

struct PreparedEntry {
    body: PreparedBody,
    /// Uncompressed payload size for pack headers (ignored for [`PreparedBody::Raw`]).
    payload_len: usize,
    /// Set for freshly compressed whole objects.
    whole_kind: Option<ObjectKind>,
}

/// Resolve [`ParallelOptions`] for pack writing from repo config and an optional override.
pub(crate) fn pack_write_parallel_options(
    odb: &Odb,
    write_threads: Option<usize>,
) -> Result<ParallelOptions> {
    let cfg = pack_config(odb)?;
    let policy = ThreadPolicy::with_threads_override(write_threads);
    Ok(policy.try_resolve(ParallelOp::PackWrite, &cfg)?)
}

fn pack_config(odb: &Odb) -> Result<ConfigSet> {
    Ok(match odb.config_git_dir() {
        Some(git_dir) => {
            let env = crate::environment::Environment::empty();
            ConfigSet::load(&env, Some(git_dir), true)?
        }
        None => ConfigSet::new(),
    })
}

/// Serialize a delta plan into a PACK v2 stream.
pub(crate) fn serialize_pack_with_deltas(
    odb: &Odb,
    entries: &[PlannedEntry],
    opts: &PackBuildOptions,
    write_threads: Option<usize>,
) -> Result<Vec<u8>> {
    let par_opts = pack_write_parallel_options(odb, write_threads)?;
    let prepared = prepare_entries_parallel(odb, entries, opts, &par_opts)?;
    write_prepared_pack(odb, entries, &prepared, opts)
}

fn prepare_entries_parallel(
    odb: &Odb,
    entries: &[PlannedEntry],
    opts: &PackBuildOptions,
    par_opts: &ParallelOptions,
) -> Result<Vec<PreparedEntry>> {
    let payloads: HashMap<ObjectId, &[u8]> =
        entries.iter().map(|e| (e.oid, e.data.as_slice())).collect();
    let delta_indices = Arc::new(build_delta_indices(odb, entries, &payloads)?);

    let prepare = |entry: &PlannedEntry| -> Result<PreparedEntry> {
        prepare_one_entry(odb, entry, &payloads, &delta_indices, opts)
    };

    if par_opts.threads.get() <= 1 {
        entries.iter().map(prepare).collect()
    } else {
        try_par_map_force(entries, *par_opts, prepare).map_err(|e| match e {
            ParallelHashError::Task(err) => err,
        })
    }
}

fn build_delta_indices(
    odb: &Odb,
    entries: &[PlannedEntry],
    payloads: &HashMap<ObjectId, &[u8]>,
) -> Result<HashMap<ObjectId, Arc<DeltaIndex>>> {
    let mut map: HashMap<ObjectId, Arc<DeltaIndex>> = HashMap::new();
    for entry in entries {
        let Some(base_oid) = entry.base else {
            continue;
        };
        if entry.reused_delta_zlib.is_some() || entry.reused_delta.is_some() {
            continue;
        }
        let base_data: Vec<u8> = if let Some(d) = payloads.get(&base_oid) {
            (*d).to_vec()
        } else {
            odb.read(&base_oid)?.data
        };
        if entry.data.starts_with(&base_data) && entry.data.len() > base_data.len() {
            continue;
        }
        map.entry(base_oid).or_insert_with(|| {
            let base_ref = payloads
                .get(&base_oid)
                .copied()
                .unwrap_or(base_data.as_slice());
            Arc::new(DeltaIndex::new(base_ref))
        });
    }
    Ok(map)
}

fn prepare_one_entry(
    odb: &Odb,
    entry: &PlannedEntry,
    payloads: &HashMap<ObjectId, &[u8]>,
    delta_indices: &HashMap<ObjectId, Arc<DeltaIndex>>,
    opts: &PackBuildOptions,
) -> Result<PreparedEntry> {
    match entry.base {
        None => prepare_whole(odb, entry, opts),
        Some(base_oid) => prepare_delta(odb, entry, base_oid, payloads, delta_indices, opts),
    }
}

fn prepare_whole(
    odb: &Odb,
    entry: &PlannedEntry,
    opts: &PackBuildOptions,
) -> Result<PreparedEntry> {
    if opts.reuse_objects && odb.hash_algo() == HashAlgo::Sha1 {
        if let Some(raw) = crate::pack::packed_full_object_slice(odb.objects_dir(), &entry.oid)? {
            return Ok(PreparedEntry {
                body: PreparedBody::Raw(raw.to_vec()),
                payload_len: 0,
                whole_kind: None,
            });
        }
    }
    let zlib = zlib_compress(&entry.data)?;
    Ok(PreparedEntry {
        body: PreparedBody::Zlib(zlib),
        payload_len: entry.data.len(),
        whole_kind: Some(entry.kind),
    })
}

fn prepare_delta(
    odb: &Odb,
    entry: &PlannedEntry,
    base_oid: ObjectId,
    payloads: &HashMap<ObjectId, &[u8]>,
    delta_indices: &HashMap<ObjectId, Arc<DeltaIndex>>,
    _opts: &PackBuildOptions,
) -> Result<PreparedEntry> {
    if let Some((delta_size, pack_bytes, zlib_start, zlib_end)) = &entry.reused_delta_zlib {
        let delta_len = usize::try_from(*delta_size)
            .map_err(|_| Error::CorruptObject("reused delta size exceeds usize".to_owned()))?;
        let zlib = pack_bytes[*zlib_start..*zlib_end].to_vec();
        return Ok(PreparedEntry {
            body: PreparedBody::Zlib(zlib),
            payload_len: delta_len,
            whole_kind: None,
        });
    }

    let delta = if let Some(reused) = &entry.reused_delta {
        reused.clone()
    } else {
        let base_data: Vec<u8> = if let Some(d) = payloads.get(&base_oid) {
            d.to_vec()
        } else {
            odb.read(&base_oid)?.data
        };
        if entry.data.starts_with(&base_data) && entry.data.len() > base_data.len() {
            encode_prefix_extension_delta(&base_data, &entry.data)?
        } else {
            let index = delta_indices
                .get(&base_oid)
                .ok_or_else(|| Error::CorruptObject("missing delta index for pack base".into()))?;
            index.encode(&entry.data, 0).ok_or_else(|| {
                Error::CorruptObject("failed to encode pack delta for object".into())
            })?
        }
    };

    let zlib = zlib_compress(&delta)?;
    Ok(PreparedEntry {
        body: PreparedBody::Zlib(zlib),
        payload_len: delta.len(),
        whole_kind: None,
    })
}

fn write_prepared_pack(
    odb: &Odb,
    entries: &[PlannedEntry],
    prepared: &[PreparedEntry],
    opts: &PackBuildOptions,
) -> Result<Vec<u8>> {
    let algo = odb.hash_algo();
    debug_assert_eq!(entries.len(), prepared.len());

    let mut buf = Vec::new();
    buf.extend_from_slice(b"PACK");
    buf.extend_from_slice(&2u32.to_be_bytes());
    let count = u32::try_from(entries.len())
        .map_err(|_| Error::CorruptObject("pack object count exceeds u32".to_owned()))?;
    buf.extend_from_slice(&count.to_be_bytes());

    let mut oid_to_offset: HashMap<ObjectId, u64> = HashMap::new();

    for (entry, prep) in entries.iter().zip(prepared) {
        let start = buf.len() as u64;
        match entry.base {
            None => match &prep.body {
                PreparedBody::Raw(raw) => buf.extend_from_slice(raw),
                PreparedBody::Zlib(zlib) => {
                    let kind = prep.whole_kind.unwrap_or(entry.kind);
                    encode_pack_object_header(&mut buf, pack_type_code(kind), prep.payload_len);
                    buf.extend_from_slice(zlib);
                }
            },
            Some(base_oid) => {
                let in_pack_offset = oid_to_offset.get(&base_oid).copied();
                if let Some(base_off) = in_pack_offset.filter(|_| opts.use_ofs_delta) {
                    let dist = start.checked_sub(base_off).ok_or_else(|| {
                        Error::CorruptObject("ofs-delta distance underflow".to_owned())
                    })?;
                    encode_pack_object_header(&mut buf, 6, prep.payload_len);
                    encode_ofs_delta_distance(&mut buf, dist);
                } else {
                    encode_pack_object_header(&mut buf, 7, prep.payload_len);
                    if base_oid.as_bytes().len() != algo.len() {
                        return Err(Error::CorruptObject(
                            "ref-delta base oid width mismatch".to_owned(),
                        ));
                    }
                    buf.extend_from_slice(base_oid.as_bytes());
                }
                match &prep.body {
                    PreparedBody::Raw(raw) => buf.extend_from_slice(raw),
                    PreparedBody::Zlib(zlib) => buf.extend_from_slice(zlib),
                }
            }
        }
        oid_to_offset.insert(entry.oid, start);
    }

    append_pack_trailer(&mut buf, algo);
    Ok(buf)
}

fn zlib_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).map_err(Error::Io)?;
    enc.finish().map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::ObjectKind;
    use std::num::NonZeroUsize;

    #[test]
    fn single_thread_prepare_matches_entry_count() {
        let entry = PlannedEntry {
            oid: ObjectId::from_hex(&"0".repeat(40)).unwrap(),
            kind: ObjectKind::Blob,
            data: b"hello".to_vec(),
            base: None,
            reused_delta: None,
            reused_delta_zlib: None,
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let objects = dir.path().join("objects");
        std::fs::create_dir_all(&objects).expect("objects");
        let odb = Odb::new(&objects);
        let opts = PackBuildOptions::default();
        let par = ParallelOptions {
            threads: NonZeroUsize::MIN,
            min_items: 0,
            min_total_bytes: None,
            signed_entry_threshold: None,
        };
        let got = prepare_entries_parallel(&odb, &[entry], &opts, &par).expect("prepare");
        assert_eq!(got.len(), 1);
    }
}
