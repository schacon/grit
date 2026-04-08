//! Local fetch over `git-upload-pack` (protocol v0) with skipping negotiation.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use grit_lib::fetch_negotiator::SkippingNegotiator;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::unpack_objects::{unpack_objects, UnpackOptions};

use crate::grit_exe::grit_executable;
use crate::pkt_line;

const INITIAL_FLUSH: usize = 16;
const PIPESAFE_FLUSH: usize = 32;
fn next_flush_count(stateless_rpc: bool, count: usize) -> usize {
    if stateless_rpc {
        const LARGE_FLUSH: usize = 16384;
        if count < LARGE_FLUSH {
            count * 2
        } else {
            count * 11 / 10
        }
    } else if count < PIPESAFE_FLUSH {
        count * 2
    } else {
        count + PIPESAFE_FLUSH
    }
}

fn trace_packet_fetch(direction: char, payload: &str) {
    let Ok(dest) = std::env::var("GIT_TRACE_PACKET") else {
        return;
    };
    if dest.is_empty() || dest == "0" || dest.eq_ignore_ascii_case("false") {
        return;
    }
    let path = if dest == "1" {
        "/dev/stderr".to_string()
    } else {
        dest
    };
    let line = format!(
        "packet: {:>12}{} {}\n",
        "fetch",
        direction,
        payload.replace('\n', "")
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn read_advertisement(child_stdout: &mut impl Read) -> Result<Vec<(String, ObjectId)>> {
    let mut out = Vec::new();
    loop {
        match pkt_line::read_packet(child_stdout)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                let mut parts = line.splitn(2, '\t');
                let hex = parts.next().unwrap_or("").trim();
                let rest = parts.next().unwrap_or("");
                let refname = rest.split('\0').next().unwrap_or("").trim().to_string();
                if refname.is_empty() {
                    continue;
                }
                let oid =
                    ObjectId::from_hex(hex).with_context(|| format!("bad ref line: {line}"))?;
                out.push((refname, oid));
            }
            _ => {}
        }
    }
    Ok(out)
}

fn collect_wants(advertised: &[(String, ObjectId)], refspecs: &[String]) -> Result<Vec<ObjectId>> {
    if refspecs.is_empty() {
        let mut wants = Vec::new();
        for (name, oid) in advertised {
            if name.starts_with("refs/heads/") || name.starts_with("refs/tags/") {
                wants.push(*oid);
            }
        }
        wants.sort_by_key(|o| o.to_hex());
        wants.dedup();
        return Ok(wants);
    }
    let mut wants = Vec::new();
    for spec in refspecs {
        if spec.starts_with('^') {
            continue;
        }
        let spec_clean = spec.strip_prefix('+').unwrap_or(spec);
        let src = spec_clean
            .split_once(':')
            .map(|(a, _)| a)
            .unwrap_or(spec_clean);
        if src.contains('*') {
            bail!("glob refspec in upload-pack fetch not supported");
        }
        let remote_ref = if src.starts_with("refs/") {
            src.to_string()
        } else {
            format!("refs/heads/{src}")
        };
        let oid = advertised
            .iter()
            .find(|(n, _)| n == &remote_ref)
            .map(|(_, o)| *o)
            .or_else(|| {
                let tag_ref = format!("refs/tags/{src}");
                advertised
                    .iter()
                    .find(|(n, _)| n == &tag_ref)
                    .map(|(_, o)| *o)
            })
            .with_context(|| format!("could not find remote ref '{remote_ref}'"))?;
        wants.push(oid);
    }
    Ok(wants)
}

/// Tests invoke `git-upload-pack`; use grit to serve grit-created object stores.
fn spawn_upload_pack(cmd_template: Option<&str>, repo_path: &Path) -> Result<std::process::Child> {
    let repo_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let rp = repo_path.to_string_lossy();
    let rp_escaped = rp.replace('\'', "'\"'\"'");

    let Some(cmd_template) = cmd_template else {
        return Command::new(grit_executable())
            .arg("upload-pack")
            .arg(rp.as_ref())
            .env_remove("GIT_TRACE_PACKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn grit upload-pack for {}", rp));
    };

    if cmd_template.contains("git-upload-pack") {
        return Command::new(grit_executable())
            .arg("upload-pack")
            .arg(rp.as_ref())
            .env_remove("GIT_TRACE_PACKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn grit upload-pack for {}", rp));
    }

    let trimmed = cmd_template.trim();
    if trimmed == "grit-upload-pack" || trimmed.ends_with("/grit-upload-pack") {
        return Command::new(trimmed)
            .arg(rp.as_ref())
            .env_remove("GIT_TRACE_PACKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn '{} {}'", trimmed, rp));
    }

    let full_cmd = cmd_template.replace('\'', "'\"'\"'");
    let script = format!("{full_cmd} '{rp_escaped}'");
    Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env_remove("GIT_TRACE_PACKET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn upload-pack: {script}"))
}

fn read_ack_round_with_negotiator(
    stdout: &mut impl Read,
    negotiator: &mut SkippingNegotiator,
) -> Result<()> {
    loop {
        let Some(pkt) = pkt_line::read_packet(stdout)? else {
            break;
        };
        let pkt_line::Packet::Data(ln) = pkt else {
            continue;
        };
        trace_packet_fetch('<', ln.trim_end());
        let Some((ack_oid, kind)) = parse_ack(&ln) else {
            break;
        };
        // Match `fetch-pack.c` `get_ack` + negotiation loop: only a bare `ACK <oid>`
        // ends the round without updating the negotiator; `common`, `continue`, and
        // `ready` all call `negotiator->ack` (see cases `ACK_common`, `ACK_continue`,
        // `ACK_ready`).
        if kind == AckKind::Bare {
            break;
        }
        let _ = negotiator.ack(ack_oid)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AckKind {
    /// `ACK <oid>` with no status suffix (post-`done` or legacy).
    Bare,
    Common,
    Continue,
    Ready,
}

fn parse_ack(line: &str) -> Option<(ObjectId, AckKind)> {
    if line == "NAK" {
        return None;
    }
    let rest = line.strip_prefix("ACK ")?;
    let hex = rest.split_whitespace().next()?;
    let oid = ObjectId::from_hex(hex).ok()?;
    let tail = rest.strip_prefix(hex).unwrap_or("").trim();
    let kind = if tail.contains("continue") {
        AckKind::Continue
    } else if tail.contains("common") {
        AckKind::Common
    } else if tail.contains("ready") {
        AckKind::Ready
    } else {
        AckKind::Bare
    };
    Some((oid, kind))
}

fn read_pkt_payload_raw(r: &mut impl Read) -> std::io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len_str = std::str::from_utf8(&len_buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len = usize::from_str_radix(len_str, 16)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    match len {
        0 => Ok(Some(Vec::new())),
        1 | 2 => Ok(Some(Vec::new())),
        n if n <= 4 => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid pkt-line length: {n}"),
        )),
        n => {
            let payload_len = n - 4;
            let mut buf = vec![0u8; payload_len];
            r.read_exact(&mut buf)?;
            Ok(Some(buf))
        }
    }
}

fn read_sideband_pack_until_done(r: &mut impl Read, out: &mut Vec<u8>) -> Result<()> {
    let mut seen_pack = false;
    loop {
        let Some(payload) = read_pkt_payload_raw(r)? else {
            break;
        };
        if payload.is_empty() {
            if seen_pack {
                break;
            }
            continue;
        }
        match payload[0] {
            1 => {
                seen_pack = true;
                out.extend_from_slice(&payload[1..]);
            }
            2 | 3 => {}
            _ => {
                if !seen_pack && payload.starts_with(b"PACK") {
                    seen_pack = true;
                    out.extend_from_slice(&payload);
                } else if seen_pack {
                    out.extend_from_slice(&payload);
                }
            }
        }
    }
    Ok(())
}

/// Fetch via `upload-pack` using explicit object IDs (e.g. lazy promisor fetch).
///
/// Negotiates using the same skipping strategy as [`fetch_via_upload_pack_skipping`], but the
/// client `want` lines are exactly `wants` (typically a single OID not advertised as a ref).
/// Returns the raw `PACK` bytes (side-band demultiplexed).
pub fn fetch_upload_pack_explicit_wants(
    local_git_dir: &Path,
    remote_repo_path: &Path,
    upload_pack_cmd: Option<&str>,
    wants: &[ObjectId],
) -> Result<Vec<u8>> {
    if wants.is_empty() {
        bail!("nothing to fetch (empty want list)");
    }
    fetch_upload_pack_negotiate_pack_bytes(local_git_dir, remote_repo_path, upload_pack_cmd, wants)
}

/// Fetch via `upload-pack` using skipping negotiation; unpack pack into `local_git_dir`.
///
/// Returns remote heads and tags from the ref advertisement.
pub fn fetch_via_upload_pack_skipping(
    local_git_dir: &Path,
    remote_repo_path: &Path,
    upload_pack_cmd: Option<&str>,
    refspecs: &[String],
) -> Result<(Vec<(String, ObjectId)>, Vec<(String, ObjectId)>)> {
    let mut child = spawn_upload_pack(upload_pack_cmd, remote_repo_path)?;
    let mut stdin = child.stdin.take().context("upload-pack stdin")?;
    let mut stdout = child.stdout.take().context("upload-pack stdout")?;

    let advertised = read_advertisement(&mut stdout)?;
    let wants = collect_wants(&advertised, refspecs)?;
    if wants.is_empty() {
        bail!("nothing to fetch (advertised {} ref(s))", advertised.len());
    }

    let remote_heads: Vec<_> = advertised
        .iter()
        .filter(|(n, _)| n.starts_with("refs/heads/"))
        .cloned()
        .collect();
    let remote_tags: Vec<_> = advertised
        .iter()
        .filter(|(n, _)| n.starts_with("refs/tags/"))
        .cloned()
        .collect();

    let pack_buf = fetch_upload_pack_negotiate_pack_bytes_with_streams(
        local_git_dir,
        &advertised,
        &mut stdin,
        &mut stdout,
        &wants,
    )?;

    let status = child.wait()?;
    if !status.success() {
        bail!("upload-pack exited with {}", status);
    }

    if pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK" {
        bail!("did not receive a pack file from upload-pack");
    }

    let odb = Odb::new(&local_git_dir.join("objects"));
    unpack_objects(&mut pack_buf.as_slice(), &odb, &UnpackOptions::default())?;

    Ok((remote_heads, remote_tags))
}

fn fetch_upload_pack_negotiate_pack_bytes(
    local_git_dir: &Path,
    remote_repo_path: &Path,
    upload_pack_cmd: Option<&str>,
    wants: &[ObjectId],
) -> Result<Vec<u8>> {
    let mut child = spawn_upload_pack(upload_pack_cmd, remote_repo_path)?;
    let mut stdin = child.stdin.take().context("upload-pack stdin")?;
    let mut stdout = child.stdout.take().context("upload-pack stdout")?;

    let advertised = read_advertisement(&mut stdout)?;
    let pack_buf = fetch_upload_pack_negotiate_pack_bytes_with_streams(
        local_git_dir,
        &advertised,
        &mut stdin,
        &mut stdout,
        wants,
    )?;

    let status = child.wait()?;
    if !status.success() {
        bail!("upload-pack exited with {}", status);
    }

    if pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK" {
        bail!("did not receive a pack file from upload-pack");
    }

    Ok(pack_buf)
}

fn fetch_upload_pack_negotiate_pack_bytes_with_streams(
    local_git_dir: &Path,
    advertised: &[(String, ObjectId)],
    stdin: &mut impl Write,
    stdout: &mut impl Read,
    wants: &[ObjectId],
) -> Result<Vec<u8>> {
    let local_repo = Repository::open(local_git_dir, None)
        .with_context(|| format!("open local repository {}", local_git_dir.display()))?;

    let want_set: HashSet<ObjectId> = wants.iter().copied().collect();

    let first_want = wants[0];
    let caps = " multi_ack_detailed thin-pack ofs-delta side-band-64k no-progress";
    let mut req = Vec::new();
    let w0 = format!("want {}{}", first_want.to_hex(), caps);
    trace_packet_fetch('>', w0.as_str());
    pkt_line::write_line_to_vec(&mut req, &w0)?;
    for w in wants.iter().skip(1) {
        let line = format!("want {}", w.to_hex());
        trace_packet_fetch('>', line.as_str());
        pkt_line::write_line_to_vec(&mut req, &line)?;
    }
    req.extend_from_slice(b"0000");
    stdin.write_all(&req).context("write wants")?;
    stdin.flush()?;

    let mut negotiator = SkippingNegotiator::new(local_repo);
    for (_, oid) in advertised {
        if want_set.contains(oid) {
            continue;
        }
        if negotiator.repo().odb.read(oid).is_ok() {
            negotiator.known_common(*oid)?;
        }
    }

    let mut tips: Vec<ObjectId> = Vec::new();
    for prefix in ["refs/heads/", "refs/tags/"] {
        if let Ok(entries) = refs::list_refs(local_git_dir, prefix) {
            for (name, oid) in entries {
                if let Ok(resolved) = resolve_revision(negotiator.repo(), &name) {
                    tips.push(resolved);
                } else {
                    tips.push(oid);
                }
            }
        }
    }
    if let Ok(h) = refs::resolve_ref(local_git_dir, "HEAD") {
        tips.push(h);
    }
    for sym in ["HEAD", "MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        if let Ok(oid) = resolve_revision(negotiator.repo(), sym) {
            tips.push(oid);
        }
    }
    tips.sort_by_key(|o| o.to_hex());
    tips.dedup();
    for t in tips {
        if want_set.contains(&t) {
            continue;
        }
        if negotiator.repo().odb.read(&t).is_err() {
            continue;
        }
        negotiator.add_tip(t)?;
    }

    let mut count: usize = 0;
    let mut flush_at: usize = INITIAL_FLUSH;
    let mut pending = Vec::new();
    let stateless_rpc = false;
    let mut flushes: i32 = 0;

    while let Some(oid) = negotiator.next_have()? {
        let h = format!("have {}", oid.to_hex());
        trace_packet_fetch('>', h.as_str());
        pkt_line::write_line_to_vec(&mut pending, &h)?;
        count += 1;
        if flush_at <= count {
            pending.extend_from_slice(b"0000");
            stdin.write_all(&pending).context("write have flush")?;
            stdin.flush()?;
            pending.clear();
            flush_at = next_flush_count(stateless_rpc, count);
            flushes += 1;

            // Match fetch-pack: skip reading ACKs after the first flush so one window stays ahead.
            if !stateless_rpc && count == INITIAL_FLUSH {
                continue;
            }

            read_ack_round_with_negotiator(stdout, &mut negotiator)?;
            flushes -= 1;
        }
    }

    if !pending.is_empty() {
        pending.extend_from_slice(b"0000");
        stdin.write_all(&pending).context("final have flush")?;
        stdin.flush()?;
        flushes += 1;
    }

    while flushes > 0 {
        read_ack_round_with_negotiator(stdout, &mut negotiator)?;
        flushes -= 1;
    }

    let mut tail = Vec::new();
    pkt_line::write_line_to_vec(&mut tail, "done")?;
    trace_packet_fetch('>', "done");
    tail.extend_from_slice(b"0000");
    stdin.write_all(&tail).context("write done")?;
    stdin.flush()?;

    let mut pack_buf = Vec::new();
    read_sideband_pack_until_done(stdout, &mut pack_buf)?;

    Ok(pack_buf)
}
