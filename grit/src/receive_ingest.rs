//! Receive-side pack ingestion helpers shared by `receive-pack` and local `push`.
//!
//! Matches Git `receive-pack` behaviour: choose `unpack-objects` vs `index-pack` from
//! `receive.unpacklimit` / `transfer.unpacklimit`, and enforce `receive.maxInputSize`.

use anyhow::{bail, Context, Result};
use grit_lib::config::{parse_git_config_int_strict, ConfigSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::grit_exe;

/// Effective `receive.unpacklimit` / `transfer.unpacklimit` / default (100), matching Git
/// `receive-pack` (`receive` wins when set non-negative).
pub fn receive_unpack_limit(cfg: &ConfigSet) -> i32 {
    let recv = cfg
        .get("receive.unpacklimit")
        .and_then(|v| parse_git_config_int_strict(&v).ok());
    let xfer = cfg
        .get("transfer.unpacklimit")
        .and_then(|v| parse_git_config_int_strict(&v).ok());
    match (recv, xfer) {
        (Some(r), _) if r >= 0 => r as i32,
        (_, Some(t)) if t >= 0 => t as i32,
        _ => 100,
    }
}

/// `receive.maxInputSize` as a byte cap (`0` or unset = unlimited).
pub fn max_input_size_from_config(cfg: &ConfigSet) -> Option<u64> {
    match cfg.get_i64("receive.maxinputsize") {
        None => None,
        Some(Ok(v)) if v <= 0 => None,
        Some(Ok(v)) => Some(v as u64),
        Some(Err(_)) => None,
    }
}

pub fn pack_object_count(pack: &[u8]) -> Option<u32> {
    if pack.len() < 12 || &pack[0..4] != b"PACK" {
        return None;
    }
    Some(u32::from_be_bytes(pack[8..12].try_into().ok()?))
}

/// Ingest a pack into `git_dir` using the same unpack path as Git `receive-pack`.
pub fn ingest_received_pack(git_dir: &Path, pack: &[u8], remote_cfg: &ConfigSet) -> Result<()> {
    let unpack_limit = receive_unpack_limit(remote_cfg);
    let max_input_bytes = max_input_size_from_config(remote_cfg);
    let nr_objects = pack_object_count(pack).unwrap_or(0);
    let use_unpack_objects =
        i64::from(unpack_limit) > 0 && (nr_objects as i64) < i64::from(unpack_limit);

    if use_unpack_objects {
        ingest_via_unpack_objects_subprocess(git_dir, pack, max_input_bytes)
    } else {
        ingest_via_index_pack_subprocess(git_dir, pack, max_input_bytes)
    }
}

fn ingest_via_unpack_objects_subprocess(
    git_dir: &Path,
    pack: &[u8],
    max_input: Option<u64>,
) -> Result<()> {
    let mut cmd = Command::new(grit_exe::grit_executable());
    grit_exe::strip_trace2_env(&mut cmd);
    cmd.arg(format!("--git-dir={}", git_dir.display()))
        .args(["unpack-objects", "-q", "--strict"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("GIT_DIR", git_dir.as_os_str());
    if let Some(n) = max_input {
        cmd.arg(format!("--max-input-size={n}"));
    }
    let mut child = cmd.spawn().context("spawn grit unpack-objects")?;
    let mut stdin = child.stdin.take().context("unpack-objects stdin")?;
    stdin
        .write_all(pack)
        .context("write pack to unpack-objects stdin")?;
    drop(stdin);
    let out = child
        .wait_with_output()
        .context("wait for unpack-objects")?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    bail!("unpack-objects abnormal exit: {stderr}");
}

fn ingest_via_index_pack_subprocess(
    git_dir: &Path,
    pack: &[u8],
    max_input: Option<u64>,
) -> Result<()> {
    let mut cmd = Command::new(grit_exe::grit_executable());
    grit_exe::strip_trace2_env(&mut cmd);
    cmd.arg(format!("--git-dir={}", git_dir.display()))
        .args(["index-pack", "--stdin", "--fix-thin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_DIR", git_dir.as_os_str());
    if let Some(n) = max_input {
        cmd.arg(format!("--max-input-size={n}"));
    }
    let mut child = cmd.spawn().context("spawn grit index-pack")?;
    let mut stdin = child.stdin.take().context("index-pack stdin")?;
    stdin
        .write_all(pack)
        .context("write pack to index-pack stdin")?;
    drop(stdin);
    let out = child.wait_with_output().context("wait for index-pack")?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    bail!("index-pack abnormal exit: {stderr}");
}
