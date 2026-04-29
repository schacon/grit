//! Receive-side pack ingestion helpers shared by `receive-pack` and local `push`.
//!
//! Matches Git `receive-pack` behaviour: choose `unpack-objects` vs `index-pack` from
//! `receive.unpacklimit` / `transfer.unpacklimit`, and enforce `receive.maxInputSize`.

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::receive_pack::{max_input_size_from_config, should_use_unpack_objects};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::grit_exe;

/// Ingest a pack into `git_dir` using the same unpack path as Git `receive-pack`.
///
/// When `strict` is false (e.g. `receive-pack --skip-connectivity-check`), `unpack-objects` runs
/// without `--strict` so thin packs can store tips without requiring all bases in the ODB.
pub fn ingest_received_pack(
    git_dir: &Path,
    pack: &[u8],
    remote_cfg: &ConfigSet,
    strict: bool,
) -> Result<()> {
    let max_input_bytes = max_input_size_from_config(remote_cfg);

    if should_use_unpack_objects(pack, remote_cfg) {
        ingest_via_unpack_objects_subprocess(git_dir, pack, max_input_bytes, strict)
    } else {
        ingest_via_index_pack_subprocess(git_dir, pack, max_input_bytes)
    }
}

fn ingest_via_unpack_objects_subprocess(
    git_dir: &Path,
    pack: &[u8],
    max_input: Option<u64>,
    strict: bool,
) -> Result<()> {
    let mut cmd = Command::new(grit_exe::grit_executable());
    grit_exe::strip_trace2_env(&mut cmd);
    cmd.arg(format!("--git-dir={}", git_dir.display()));
    if strict {
        cmd.args(["unpack-objects", "-q", "--strict"]);
    } else {
        cmd.args(["unpack-objects", "-q"]);
    }
    cmd.stdin(Stdio::piped())
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
