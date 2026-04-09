//! `test-tool pack-deltas` — build a pack with `REF_DELTA` lines on stdin (see `git/t/helper/test-pack-deltas.c`).

use anyhow::{bail, Context, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use grit_lib::delta_encode::encode_lcp_delta;
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;
use sha1::{Digest, Sha1};
use std::io::{BufRead, Write};

const OBJ_REF_DELTA: u32 = 7;

/// Encode Git's per-object pack header (`type << 4 | size_low_4`, then 7-bit size continuation).
fn encode_pack_object_header(typ: u32, size: u64) -> Vec<u8> {
    debug_assert!((1..=7).contains(&typ));
    let mut hdr = Vec::with_capacity(16);
    let mut s = size;
    let mut c: u8 = ((typ << 4) as u8) | (u8::try_from(s & 0x0f).unwrap());
    s >>= 4;
    while s != 0 {
        hdr.push(c | 0x80);
        c = u8::try_from(s & 0x7f).unwrap();
        s >>= 7;
    }
    hdr.push(c);
    hdr
}

fn zlib_compress_level1(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(1));
    enc.write_all(data)
        .map_err(|e| anyhow::anyhow!("zlib compress: {e}"))?;
    enc.finish()
        .map_err(|e| anyhow::anyhow!("zlib finish: {e}"))
}

/// Run `test-tool pack-deltas` after argv has been preprocessed (`-C` applied).
///
/// # Arguments
///
/// `rest` — argv slice starting with `"pack-deltas"`, then `-n` / `--num-objects <n>`.
///
/// # Errors
///
/// Returns an error on bad CLI, missing objects, unknown line types, or I/O failure.
pub fn run(rest: &[String]) -> Result<()> {
    let mut num_objects: Option<u32> = None;
    let mut it = rest.iter().skip(1);
    while let Some(arg) = it.next() {
        if let Some(v) = arg.strip_prefix("--num-objects=") {
            num_objects = Some(
                v.parse()
                    .with_context(|| format!("invalid --num-objects value: {v}"))?,
            );
            continue;
        }
        if let Some(tail) = arg.strip_prefix("-n") {
            if !tail.is_empty() {
                num_objects = Some(
                    tail.parse()
                        .with_context(|| format!("invalid -n value: {tail}"))?,
                );
                continue;
            }
        }
        match arg.as_str() {
            "-n" => {
                let v = it
                    .next()
                    .with_context(|| format!("{arg} requires a value"))?;
                num_objects = Some(
                    v.parse()
                        .with_context(|| format!("invalid -n value: {v}"))?,
                );
            }
            "--num-objects" => {
                let v = it
                    .next()
                    .with_context(|| format!("{arg} requires a value"))?;
                num_objects = Some(
                    v.parse()
                        .with_context(|| format!("invalid --num-objects value: {v}"))?,
                );
            }
            other => bail!("test-tool pack-deltas: unknown argument '{other}'"),
        }
    }
    let Some(num_objects) = num_objects else {
        bail!("usage: test-tool pack-deltas --num-objects <num-objects>");
    };

    let repo = Repository::discover(None).context("pack-deltas: not a git repository")?;
    let odb = &repo.odb;

    let stdin = std::io::stdin().lock();
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"PACK");
    body.extend_from_slice(&2u32.to_be_bytes());
    body.extend_from_slice(&num_objects.to_be_bytes());

    for line in stdin.lines() {
        let line = line.context("read stdin")?;
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            bail!("invalid input format: {line}");
        }
        let kind = parts[0];
        let content_hex = parts[1];
        let content_oid: ObjectId = content_hex
            .parse()
            .with_context(|| format!("invalid object: {content_hex}"))?;

        match kind {
            "REF_DELTA" => {
                let base_hex = parts
                    .get(2)
                    .copied()
                    .with_context(|| format!("REF_DELTA requires base oid: {line}"))?;
                let base_oid: ObjectId = base_hex
                    .parse()
                    .with_context(|| format!("invalid object: {base_hex}"))?;

                let obj = odb
                    .read(&content_oid)
                    .with_context(|| format!("unable to read {content_oid}"))?;
                let base_obj = odb
                    .read(&base_oid)
                    .with_context(|| format!("unable to read {base_oid}"))?;

                let delta = encode_lcp_delta(&base_obj.data, &obj.data).map_err(|e| {
                    anyhow::anyhow!("delta encode {content_oid} against {base_oid}: {e}")
                })?;
                let compressed = zlib_compress_level1(&delta)?;
                let hdrlen = encode_pack_object_header(OBJ_REF_DELTA, delta.len() as u64);
                body.extend_from_slice(&hdrlen);
                body.extend_from_slice(base_oid.as_bytes());
                body.extend_from_slice(&compressed);
            }
            "OFS_DELTA" => bail!("OFS_DELTA not implemented"),
            "FULL" => bail!("FULL not implemented"),
            other => bail!("unknown pack type: {other}"),
        }
    }

    let mut hasher = Sha1::new();
    hasher.update(&body);
    let digest = hasher.finalize();
    body.extend_from_slice(&digest);

    std::io::stdout()
        .write_all(&body)
        .context("write pack to stdout")?;
    Ok(())
}
