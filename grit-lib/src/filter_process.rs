//! Long-running Git filter protocol (`filter.<name>.process`), matching `git-filter` v2.
//!
//! See Git's `convert.c` (`apply_multi_file_filter`) and `sub-process.c` (handshake).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::objects::ObjectId;
use crate::refs;
use crate::repo::Repository;

/// Max data bytes per pkt-line payload (Git `LARGE_PACKET_DATA_MAX`).
const LARGE_PACKET_DATA_MAX: usize = 65520 - 4;

const CAP_CLEAN: u32 = 1 << 0;
const CAP_SMUDGE: u32 = 1 << 1;
const CAP_DELAY: u32 = 1 << 2;

/// Result of a process-filter smudge: either output bytes or a deferred (`delayed`) response.
#[derive(Debug)]
pub enum ProcessSmudgeOutput {
    /// Smudged blob bytes to write to the working tree.
    Data(Vec<u8>),
    /// Filter chose to defer this path (Git parallel checkout / delay protocol; t2080).
    Delayed,
}

/// Optional metadata sent with smudge (ref, treeish, blob hex).
#[derive(Debug, Clone, Default)]
pub struct FilterSmudgeMeta {
    pub ref_name: Option<String>,
    pub treeish_hex: Option<String>,
    pub blob_hex: Option<String>,
}

/// Smudge metadata for path-only checkouts (`git checkout -- <paths>`): `blob=` only.
#[must_use]
pub fn smudge_meta_blob_only(blob_hex: &str) -> FilterSmudgeMeta {
    FilterSmudgeMeta {
        blob_hex: Some(blob_hex.to_string()),
        ..Default::default()
    }
}

/// Smudge metadata with `treeish=` only (e.g. `git reset --hard <commit>` / `git merge` checkout).
#[must_use]
pub fn smudge_meta_treeish_only(treeish_hex: &str, blob_hex: &str) -> FilterSmudgeMeta {
    FilterSmudgeMeta {
        treeish_hex: Some(treeish_hex.to_string()),
        blob_hex: Some(blob_hex.to_string()),
        ..Default::default()
    }
}

/// Process-smudge metadata for `git reset --hard <ref>` (t0021): `ref=` when the spec names a ref.
#[must_use]
pub fn smudge_meta_for_reset(
    repo: &Repository,
    commit_spec: &str,
    resolved_commit: &ObjectId,
    blob_hex: &str,
) -> FilterSmudgeMeta {
    let tip_hex = resolved_commit.to_string();
    let mut meta = FilterSmudgeMeta {
        treeish_hex: Some(tip_hex.clone()),
        blob_hex: Some(blob_hex.to_string()),
        ..Default::default()
    };
    let arg_lower = commit_spec.to_ascii_lowercase();
    let is_full_hex = arg_lower.len() == 40 && arg_lower.chars().all(|c| c.is_ascii_hexdigit());
    if is_full_hex && arg_lower == tip_hex.to_ascii_lowercase() {
        meta.ref_name = None;
        return meta;
    }
    let mut candidates: Vec<String> = Vec::new();
    if commit_spec == "HEAD" || commit_spec.starts_with("refs/") {
        candidates.push(commit_spec.to_string());
    } else {
        candidates.push(format!("refs/heads/{commit_spec}"));
        candidates.push(format!("refs/tags/{commit_spec}"));
        candidates.push(commit_spec.to_string());
    }
    for name in candidates {
        if let Ok(oid) = refs::resolve_ref(&repo.git_dir, &name) {
            if oid == *resolved_commit {
                meta.ref_name = Some(name);
                break;
            }
        }
    }
    meta
}

/// Process-smudge metadata for `git archive` (matches Git / t0021).
///
/// `tree_ish_arg` is the user's argument (`main`, full commit hex, or tree hex).
/// `resolved_tip` is the OID `archive` resolved; `tip_is_commit` is true when that object is a commit.
#[must_use]
pub fn smudge_meta_for_archive(
    repo: &Repository,
    tree_ish_arg: &str,
    resolved_tip: &ObjectId,
    tip_is_commit: bool,
    blob_hex: &str,
) -> FilterSmudgeMeta {
    let mut meta = FilterSmudgeMeta {
        blob_hex: Some(blob_hex.to_string()),
        ..Default::default()
    };
    if !tip_is_commit {
        meta.treeish_hex = Some(resolved_tip.to_string());
        return meta;
    }
    let tip_hex = resolved_tip.to_string();
    meta.treeish_hex = Some(tip_hex.clone());
    let arg_lower = tree_ish_arg.to_ascii_lowercase();
    let is_full_hex = arg_lower.len() == 40 && arg_lower.chars().all(|c| c.is_ascii_hexdigit());
    if is_full_hex && arg_lower == tip_hex.to_ascii_lowercase() {
        meta.ref_name = None;
        return meta;
    }
    if let Ok(oid) = refs::resolve_ref(&repo.git_dir, tree_ish_arg) {
        if oid == *resolved_tip {
            meta.ref_name = Some(tree_ish_arg.to_string());
            return meta;
        }
    }
    let heads = format!("refs/heads/{tree_ish_arg}");
    if let Ok(oid) = refs::resolve_ref(&repo.git_dir, &heads) {
        if oid == *resolved_tip {
            meta.ref_name = Some(heads);
        }
    }
    meta
}

pub fn smudge_meta_for_checkout(repo: &Repository, blob_hex: &str) -> FilterSmudgeMeta {
    let mut meta = FilterSmudgeMeta {
        blob_hex: Some(blob_hex.to_string()),
        ..Default::default()
    };
    let Ok(content) = std::fs::read_to_string(repo.git_dir.join("HEAD")) else {
        return meta;
    };
    let content = content.trim();
    if let Some(sym) = content.strip_prefix("ref: ") {
        let sym = sym.trim();
        meta.ref_name = Some(sym.to_string());
        if let Ok(oid) = refs::resolve_ref(&repo.git_dir, sym) {
            meta.treeish_hex = Some(oid.to_string());
        }
    } else if content.len() == 40 {
        if let Ok(oid) = ObjectId::from_hex(content) {
            meta.treeish_hex = Some(oid.to_string());
        }
    }
    meta
}

struct RunningFilter {
    #[allow(dead_code)]
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    caps: u32,
}

fn process_registry() -> &'static Mutex<HashMap<String, Mutex<RunningFilter>>> {
    static REG: OnceLock<Mutex<HashMap<String, Mutex<RunningFilter>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_packet_header(len: usize, out: &mut [u8; 4]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out[0] = HEX[(len >> 12) & 0xf];
    out[1] = HEX[(len >> 8) & 0xf];
    out[2] = HEX[(len >> 4) & 0xf];
    out[3] = HEX[len & 0xf];
}

fn write_packet(stdin: &mut ChildStdin, payload: &[u8]) -> std::io::Result<()> {
    if payload.len() > LARGE_PACKET_DATA_MAX {
        return Err(std::io::Error::other("filter packet payload too large"));
    }
    let total = payload.len() + 4;
    let mut hdr = [0u8; 4];
    set_packet_header(total, &mut hdr);
    stdin.write_all(&hdr)?;
    stdin.write_all(payload)?;
    stdin.flush()?;
    Ok(())
}

fn write_packet_line(stdin: &mut ChildStdin, line: &str) -> std::io::Result<()> {
    let mut s = line.to_string();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    write_packet(stdin, s.as_bytes())
}

fn write_flush(stdin: &mut ChildStdin) -> std::io::Result<()> {
    stdin.write_all(b"0000")?;
    stdin.flush()
}

fn read_exact<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<()> {
    let mut off = 0;
    while off < buf.len() {
        let n = r.read(&mut buf[off..])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF reading pkt-line",
            ));
        }
        off += n;
    }
    Ok(())
}

fn read_packet_header(stdout: &mut ChildStdout) -> std::io::Result<Option<[u8; 4]>> {
    let mut hdr = [0u8; 4];
    let mut off = 0usize;
    while off < 4 {
        let n = stdout.read(&mut hdr[off..])?;
        if n == 0 {
            if off == 0 {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF reading pkt-line",
            ));
        }
        off += n;
    }
    Ok(Some(hdr))
}

fn read_packet_payload(stdout: &mut ChildStdout) -> std::io::Result<Option<Vec<u8>>> {
    let Some(hdr) = read_packet_header(stdout)? else {
        return Ok(None);
    };
    let hex = std::str::from_utf8(&hdr)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let total = usize::from_str_radix(hex, 16).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid pkt-line header")
    })?;
    if total == 0 {
        return Ok(None);
    }
    if total < 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid pkt-line length",
        ));
    }
    let len = total - 4;
    let mut payload = vec![0u8; len];
    read_exact(stdout, &mut payload)?;
    Ok(Some(payload))
}

fn read_packet_line(stdout: &mut ChildStdout) -> std::io::Result<Option<String>> {
    let Some(payload) = read_packet_payload(stdout)? else {
        return Ok(None);
    };
    let s = String::from_utf8_lossy(&payload).into_owned();
    Ok(Some(s.trim_end_matches('\n').to_string()))
}

/// Read pkt-lines until flush; updates `acc` only when a `status=` line appears (matches Git
/// `subprocess_read_status` — if the segment is empty, `acc` is left unchanged).
fn read_status(stdout: &mut ChildStdout, acc: &mut String) -> std::io::Result<()> {
    loop {
        let Some(line) = read_packet_line(stdout)? else {
            break;
        };
        if let Some(rest) = line.strip_prefix("status=") {
            *acc = rest.to_string();
        }
    }
    Ok(())
}

fn read_packetized(stdout: &mut ChildStdout) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let Some(chunk) = read_packet_payload(stdout)? else {
            break;
        };
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn handshake(stdout: &mut ChildStdout, stdin: &mut ChildStdin) -> std::io::Result<u32> {
    // Match Git's test-tool rot13-filter: client sends only `version=2` before the first flush.
    write_packet_line(stdin, "git-filter-client")?;
    write_packet_line(stdin, "version=2")?;
    write_flush(stdin)?;

    let Some(server) = read_packet_line(stdout)? else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "expected git-filter-server",
        ));
    };
    if server != "git-filter-server" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unexpected filter server line: {server}"),
        ));
    }
    let Some(ver_line) = read_packet_line(stdout)? else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "expected version line",
        ));
    };
    let ver = ver_line
        .strip_prefix("version=")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "expected version="))?;
    if ver != "2" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unsupported filter protocol version {ver}"),
        ));
    }
    if read_packet_line(stdout)?.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expected flush after version",
        ));
    }

    write_packet_line(stdin, "capability=clean")?;
    write_packet_line(stdin, "capability=smudge")?;
    write_packet_line(stdin, "capability=delay")?;
    write_flush(stdin)?;

    let mut caps = 0u32;
    loop {
        let Some(line) = read_packet_line(stdout)? else {
            break;
        };
        if let Some(name) = line.strip_prefix("capability=") {
            match name {
                "clean" => caps |= CAP_CLEAN,
                "smudge" => caps |= CAP_SMUDGE,
                "delay" => caps |= CAP_DELAY,
                _ => {}
            }
        }
    }

    Ok(caps)
}

fn spawn_running(cmd: &str) -> std::io::Result<RunningFilter> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("filter process missing stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("filter process missing stdout"))?;

    let caps = handshake(&mut stdout, &mut stdin)?;

    Ok(RunningFilter {
        child,
        stdin: Some(stdin),
        stdout: Some(stdout),
        caps,
    })
}

fn ensure_started(cmd: &str) -> Result<(), String> {
    let mut reg = process_registry()
        .lock()
        .map_err(|_| "filter registry poisoned".to_string())?;
    if reg.contains_key(cmd) {
        return Ok(());
    }
    let rf = spawn_running(cmd).map_err(|e| e.to_string())?;
    reg.insert(cmd.to_string(), Mutex::new(rf));
    Ok(())
}

fn write_packetized(stdin: &mut ChildStdin, data: &[u8]) -> std::io::Result<()> {
    let mut off = 0usize;
    while off < data.len() {
        let end = (off + LARGE_PACKET_DATA_MAX).min(data.len());
        write_packet(stdin, &data[off..end])?;
        off = end;
    }
    Ok(())
}

/// Run clean via long-running filter `cmd` for `path` and `input`.
pub fn apply_process_clean(cmd: &str, path: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    ensure_started(cmd)?;
    let reg = process_registry()
        .lock()
        .map_err(|_| "filter registry poisoned".to_string())?;
    let proc_mutex = reg
        .get(cmd)
        .ok_or_else(|| "filter process not registered".to_string())?;
    let mut rf = proc_mutex
        .lock()
        .map_err(|_| "filter process mutex poisoned".to_string())?;
    if rf.caps & CAP_CLEAN == 0 {
        return Err("filter process does not support clean".to_string());
    }

    let mut stdin = rf
        .stdin
        .take()
        .ok_or_else(|| "filter stdin missing".to_string())?;
    let mut stdout = rf
        .stdout
        .take()
        .ok_or_else(|| "filter stdout missing".to_string())?;

    let result = (|| {
        write_packet_line(&mut stdin, "command=clean").map_err(|e| e.to_string())?;
        write_packet_line(&mut stdin, &format!("pathname={path}")).map_err(|e| e.to_string())?;
        write_flush(&mut stdin).map_err(|e| e.to_string())?;
        write_packetized(&mut stdin, input).map_err(|e| e.to_string())?;
        write_flush(&mut stdin).map_err(|e| e.to_string())?;

        let mut st = String::new();
        read_status(&mut stdout, &mut st).map_err(|e| e.to_string())?;
        if st != "success" {
            return Err(format!("filter status: {st}"));
        }
        let out = read_packetized(&mut stdout).map_err(|e| e.to_string())?;
        read_status(&mut stdout, &mut st).map_err(|e| e.to_string())?;
        if st != "success" {
            return Err(format!("filter tail status: {st}"));
        }
        Ok(out)
    })();

    rf.stdin = Some(stdin);
    rf.stdout = Some(stdout);
    result
}

/// Run smudge via long-running filter.
pub fn apply_process_smudge(
    cmd: &str,
    path: &str,
    input: &[u8],
    meta: Option<&FilterSmudgeMeta>,
) -> Result<Vec<u8>, String> {
    match apply_process_smudge_extended(cmd, path, input, meta, false)? {
        ProcessSmudgeOutput::Data(d) => Ok(d),
        ProcessSmudgeOutput::Delayed => {
            Err("delayed checkout not supported by grit process filter".to_string())
        }
    }
}

/// Like [`apply_process_smudge`] but surfaces `delayed` for callers that skip writing the path.
///
/// When `send_can_delay` is true and the filter advertised the delay capability, sends
/// `can-delay=1` after the metadata lines (matches Git `convert.c`; required for
/// `test-tool rot13-filter --always-delay` in t2080).
pub fn apply_process_smudge_extended(
    cmd: &str,
    path: &str,
    input: &[u8],
    meta: Option<&FilterSmudgeMeta>,
    send_can_delay: bool,
) -> Result<ProcessSmudgeOutput, String> {
    ensure_started(cmd)?;
    let reg = process_registry()
        .lock()
        .map_err(|_| "filter registry poisoned".to_string())?;
    let proc_mutex = reg
        .get(cmd)
        .ok_or_else(|| "filter process not registered".to_string())?;
    let mut rf = proc_mutex
        .lock()
        .map_err(|_| "filter process mutex poisoned".to_string())?;
    let mut stdin = rf
        .stdin
        .take()
        .ok_or_else(|| "filter stdin missing".to_string())?;
    let mut stdout = rf
        .stdout
        .take()
        .ok_or_else(|| "filter stdout missing".to_string())?;

    let result = (|| {
        if rf.caps & CAP_SMUDGE == 0 {
            return Ok(ProcessSmudgeOutput::Data(input.to_vec()));
        }
        write_packet_line(&mut stdin, "command=smudge").map_err(|e| e.to_string())?;
        write_packet_line(&mut stdin, &format!("pathname={path}")).map_err(|e| e.to_string())?;
        if let Some(m) = meta {
            if let Some(r) = &m.ref_name {
                write_packet_line(&mut stdin, &format!("ref={r}")).map_err(|e| e.to_string())?;
            }
            if let Some(t) = &m.treeish_hex {
                write_packet_line(&mut stdin, &format!("treeish={t}"))
                    .map_err(|e| e.to_string())?;
            }
            if let Some(b) = &m.blob_hex {
                write_packet_line(&mut stdin, &format!("blob={b}")).map_err(|e| e.to_string())?;
            }
        }
        if send_can_delay && (rf.caps & CAP_DELAY) != 0 {
            write_packet_line(&mut stdin, "can-delay=1").map_err(|e| e.to_string())?;
        }
        write_flush(&mut stdin).map_err(|e| e.to_string())?;
        write_packetized(&mut stdin, input).map_err(|e| e.to_string())?;
        write_flush(&mut stdin).map_err(|e| e.to_string())?;

        let mut st = String::new();
        read_status(&mut stdout, &mut st).map_err(|e| e.to_string())?;
        if st == "delayed" {
            return Ok(ProcessSmudgeOutput::Delayed);
        }
        if st != "success" {
            return Err(format!("filter status: {st}"));
        }
        let out = read_packetized(&mut stdout).map_err(|e| e.to_string())?;
        read_status(&mut stdout, &mut st).map_err(|e| e.to_string())?;
        if st != "success" {
            return Err(format!("filter tail status: {st}"));
        }
        Ok(ProcessSmudgeOutput::Data(out))
    })();

    rf.stdin = Some(stdin);
    rf.stdout = Some(stdout);
    result
}
