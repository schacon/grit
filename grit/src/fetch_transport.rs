//! Local fetch over `git-upload-pack` (protocol v0) with skipping negotiation.

use std::cell::Cell;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use grit_lib::diff::zero_oid;
use grit_lib::fetch_negotiator::SkippingNegotiator;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{peel_to_commit_for_merge_base, resolve_revision};
use grit_lib::unpack_objects::{unpack_objects, UnpackOptions};

use crate::grit_exe::grit_executable;
use crate::pkt_line;
use crate::protocol_wire;
use crate::wire_trace;

thread_local! {
    static PACKET_TRACE_IDENTITY: Cell<&'static str> = const { Cell::new("fetch") };
}

fn peel_commit_oid_for_negotiation(repo: &Repository, oid: ObjectId) -> Result<ObjectId> {
    peel_to_commit_for_merge_base(repo, oid).map_err(|e| match e {
        grit_lib::error::Error::InvalidRef(msg) => anyhow::anyhow!(msg),
        other => other.into(),
    })
}

/// Run `f` with `GIT_TRACE_PACKET` lines labeled as `identity` (`fetch`, `clone`, …).
pub fn with_packet_trace_identity<T>(
    identity: &'static str,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    struct Reset(&'static str);
    impl Drop for Reset {
        fn drop(&mut self) {
            PACKET_TRACE_IDENTITY.set(self.0);
        }
    }
    let prev = PACKET_TRACE_IDENTITY.get();
    PACKET_TRACE_IDENTITY.set(identity);
    let _guard = Reset(prev);
    f()
}

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
    let identity = PACKET_TRACE_IDENTITY.get();
    if identity == "clone" && direction == '>' && payload.starts_with("want ") {
        return;
    }
    wire_trace::trace_packet_line_ident(identity, direction, payload);
}

fn parse_ref_advertisement_line(line: &str) -> Option<(ObjectId, String, &str)> {
    let line = line.trim_end_matches('\n');
    if line.len() < 40 {
        return None;
    }
    let hex = &line[..40];
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let oid = ObjectId::from_hex(hex).ok()?;
    let mut rest = line[40..].trim_start();
    // Upstream `git-daemon` uses a single space after the OID; `upload-pack` often uses `\t`.
    rest = rest.trim_start_matches([' ', '\t']);
    let (refname, caps) = if let Some(i) = rest.find('\0') {
        (rest[..i].trim(), &rest[i + 1..])
    } else {
        (rest.trim(), "")
    };
    if refname.is_empty() {
        return None;
    }
    Some((oid, refname.to_string(), caps))
}

pub(crate) fn read_advertisement(
    child_stdout: &mut impl Read,
) -> Result<(Vec<(String, ObjectId)>, Option<String>)> {
    let mut out = Vec::new();
    let mut head_symref: Option<String> = None;
    loop {
        match pkt_line::read_packet(child_stdout)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                if let Some(ver) = line.strip_prefix("version ") {
                    if ver.trim().parse::<u8>().is_ok() {
                        trace_packet_fetch('<', line);
                        continue;
                    }
                }
                trace_packet_fetch('<', line);
                let Some((oid, refname, caps)) = parse_ref_advertisement_line(line) else {
                    continue;
                };
                if refname == "HEAD" {
                    for cap in caps.split_whitespace() {
                        if let Some(target) = cap.strip_prefix("symref=HEAD:") {
                            head_symref = Some(target.to_string());
                        }
                    }
                }
                out.push((refname, oid));
            }
            _ => {}
        }
    }
    Ok((out, head_symref))
}

pub(crate) fn collect_wants(
    advertised: &[(String, ObjectId)],
    refspecs: &[String],
) -> Result<Vec<ObjectId>> {
    if refspecs.is_empty() {
        let mut wants = Vec::new();
        for (name, oid) in advertised {
            if name.starts_with("refs/heads/") || name.starts_with("refs/tags/") {
                wants.push(*oid);
            }
        }
        if wants.is_empty() {
            if let Some((_, oid)) = advertised.iter().find(|(n, _)| n == "HEAD") {
                wants.push(*oid);
            }
        }
        if wants.is_empty() {
            for (name, oid) in advertised {
                if name == "HEAD" {
                    continue;
                }
                if name.starts_with("refs/") {
                    wants.push(*oid);
                }
            }
        }
        wants.retain(|o| *o != zero_oid());
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

/// Pushes `oid` onto `wants` if it is not already present (order-preserving).
pub(crate) fn push_want_unique(wants: &mut Vec<ObjectId>, oid: ObjectId) {
    if !wants.contains(&oid) {
        wants.push(oid);
    }
}

/// Resolve CLI refspec sources to `want` OIDs for upload-pack (matches [`collect_wants`]).
pub(crate) fn collect_wants_cli(
    _remote_git_dir: &Path,
    advertised: &[(String, ObjectId)],
    cli_refspecs: &[String],
) -> Result<Vec<ObjectId>> {
    collect_wants(advertised, cli_refspecs)
}

/// Tests invoke `git-upload-pack`; use grit to serve grit-created object stores.
///
/// `client_proto` is passed to [`protocol_wire::merge_git_protocol_env_for_child`] (use `0` when
/// the reader expects a v0 ref advertisement, e.g. `ext::` transport).
pub(crate) fn spawn_upload_pack_with_proto(
    cmd_template: Option<&str>,
    repo_path: &Path,
    client_proto: u8,
) -> Result<std::process::Child> {
    let repo_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let rp = repo_path.to_string_lossy();
    let rp_escaped = rp.replace('\'', "'\"'\"'");

    let apply_proto_env = |c: &mut Command| {
        if client_proto == 0 {
            c.env_remove("GIT_PROTOCOL");
        } else {
            protocol_wire::merge_git_protocol_env_for_child(c, client_proto);
        }
    };

    let Some(cmd_template) = cmd_template else {
        let mut c = Command::new(grit_executable());
        c.arg("upload-pack")
            .arg(rp.as_ref())
            .env_remove("GIT_TRACE_PACKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        apply_proto_env(&mut c);
        return c
            .spawn()
            .with_context(|| format!("failed to spawn grit upload-pack for {}", rp));
    };

    if cmd_template.contains("git-upload-pack") {
        let mut c = Command::new(grit_executable());
        c.arg("upload-pack")
            .arg(rp.as_ref())
            .env_remove("GIT_TRACE_PACKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        apply_proto_env(&mut c);
        return c
            .spawn()
            .with_context(|| format!("failed to spawn grit upload-pack for {}", rp));
    }

    let trimmed = cmd_template.trim();
    if trimmed == "grit-upload-pack" || trimmed.ends_with("/grit-upload-pack") {
        let mut c = Command::new(trimmed);
        c.arg(rp.as_ref())
            .env_remove("GIT_TRACE_PACKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        apply_proto_env(&mut c);
        return c
            .spawn()
            .with_context(|| format!("failed to spawn '{} {}'", trimmed, rp));
    }

    let full_cmd = cmd_template.replace('\'', "'\"'\"'");
    let script = format!("{full_cmd} '{rp_escaped}'");
    let mut c = Command::new("sh");
    c.arg("-c")
        .arg(&script)
        .env_remove("GIT_TRACE_PACKET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    apply_proto_env(&mut c);
    c.spawn()
        .with_context(|| format!("failed to spawn upload-pack: {script}"))
}

pub(crate) fn spawn_upload_pack(
    cmd_template: Option<&str>,
    repo_path: &Path,
) -> Result<std::process::Child> {
    spawn_upload_pack_with_proto(
        cmd_template,
        repo_path,
        protocol_wire::effective_client_protocol_version(),
    )
}

pub(crate) fn drain_child_stdout_to_eof(r: &mut impl Read) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

fn read_ack_round_with_negotiator(
    stdout: &mut impl Read,
    negotiator: &mut SkippingNegotiator,
) -> Result<()> {
    loop {
        let Some(pkt) = pkt_line::read_packet(stdout)? else {
            break;
        };
        match pkt {
            pkt_line::Packet::Flush => break,
            pkt_line::Packet::Data(ln) => {
                trace_packet_fetch('<', ln.trim_end());
                if ln.trim_end() == "NAK" {
                    // `upload-pack` sends `NAK` as the last pkt-line of a negotiation round but does
                    // not follow it with a flush; waiting for another packet would block forever while
                    // the server waits for our next `have` batch or `done`.
                    break;
                }
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
            _ => {}
        }
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

pub(crate) fn read_pkt_payload_raw(r: &mut impl Read) -> std::io::Result<Option<Vec<u8>>> {
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
/// `compute_wants` builds the OID list sent as `want` lines (configured fetch, CLI refspecs, or
/// tag-following). When `has_cli_refspecs` is false, empty wants follow the same early-exit rules as
/// a fetch with no CLI refspecs.
///
/// Returns remote heads and tags from the ref advertisement, plus `HEAD` symref target
/// from capabilities when present (e.g. `symref=HEAD:refs/heads/main`).
pub fn fetch_via_upload_pack_skipping(
    local_git_dir: &Path,
    remote_repo_path: &Path,
    upload_pack_cmd: Option<&str>,
    compute_wants: impl FnOnce(&[(String, ObjectId)]) -> Result<Vec<ObjectId>>,
    has_cli_refspecs: bool,
) -> Result<(
    Vec<(String, ObjectId)>,
    Vec<(String, ObjectId)>,
    Option<String>,
    Option<ObjectId>,
)> {
    let mut child = spawn_upload_pack(upload_pack_cmd, remote_repo_path)?;
    let mut stdin = child.stdin.take().context("upload-pack stdin")?;
    let mut stdout = child.stdout.take().context("upload-pack stdout")?;

    let (advertised, head_symref) = read_advertisement(&mut stdout)?;
    let wants = compute_wants(&advertised)?;
    if wants.is_empty() {
        if !has_cli_refspecs && advertised.is_empty() {
            drop(stdin);
            let _ = drain_child_stdout_to_eof(&mut stdout);
            let status = child.wait()?;
            if !status.success() {
                bail!("upload-pack exited with {}", status);
            }
            return Ok((Vec::new(), Vec::new(), head_symref, None));
        }
        // No pack to transfer (local already has all objects), but when the remote advertised
        // refs we must still return those heads/tags so `git fetch` can update
        // `refs/remotes/<remote>/*` to match the remote repository (Git behavior; submodule
        // `update --remote` depends on this).
        if !has_cli_refspecs {
            drop(stdin);
            let _ = drain_child_stdout_to_eof(&mut stdout);
            let status = child.wait()?;
            if !status.success() {
                bail!("upload-pack exited with {}", status);
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
            let head_advertised_oid = advertised
                .iter()
                .find(|(n, _)| n == "HEAD")
                .map(|(_, o)| *o);
            return Ok((remote_heads, remote_tags, head_symref, head_advertised_oid));
        }
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

    let head_advertised_oid = advertised
        .iter()
        .find(|(n, _)| n == "HEAD")
        .map(|(_, o)| *o);

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
    if pack_buf.len() > 12 {
        unpack_objects(&mut pack_buf.as_slice(), &odb, &UnpackOptions::default())?;
    }

    Ok((remote_heads, remote_tags, head_symref, head_advertised_oid))
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

    let (advertised, _head_symref) = read_advertisement(&mut stdout)?;
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

pub(crate) fn fetch_upload_pack_negotiate_pack_bytes_with_streams(
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
    let agent = crate::version_string();
    // Match `git fetch-pack` capability order on the first `want` line (see pkt traces in t5700).
    let caps = format!(
        " multi_ack_detailed side-band-64k thin-pack no-progress include-tag ofs-delta deepen-since deepen-not agent=git/{agent}"
    );
    let mut req = Vec::new();
    let w0 = format!("want {}{}", first_want.to_hex(), caps);
    trace_packet_fetch('>', w0.as_str());
    pkt_line::write_line_to_vec(&mut req, &w0)?;
    for w in wants.iter().skip(1) {
        let line = format!("want {}", w.to_hex());
        trace_packet_fetch('>', line.as_str());
        pkt_line::write_line_to_vec(&mut req, &line)?;
    }
    // Match `git fetch-pack`: when only one unique OID is wanted, send a second bare `want`
    // line (same as the first) before the flush. Some servers (notably `git-daemon`) expect this.
    if wants.len() == 1 {
        let line = format!("want {}", first_want.to_hex());
        trace_packet_fetch('>', line.as_str());
        pkt_line::write_line_to_vec(&mut req, &line)?;
    }
    req.extend_from_slice(b"0000");
    stdin.write_all(&req).context("write wants")?;
    stdin.flush()?;

    let mut negotiator = SkippingNegotiator::new(local_repo);

    if let Ok(entries) = refs::list_refs(local_git_dir, "refs/bundles/") {
        for (name, oid) in entries {
            let t = if let Ok(resolved) = resolve_revision(negotiator.repo(), &name) {
                resolved
            } else {
                oid
            };
            if negotiator.repo().odb.read(&t).is_ok() {
                let c = peel_commit_oid_for_negotiation(negotiator.repo(), t)?;
                negotiator.add_tip(c)?;
            }
        }
    }

    for w in wants {
        if negotiator.repo().odb.read(w).is_ok() {
            let c = peel_commit_oid_for_negotiation(negotiator.repo(), *w)?;
            negotiator.add_tip(c)?;
        }
    }

    let mut tips: Vec<ObjectId> = Vec::new();
    for prefix in ["refs/heads/", "refs/tags/"] {
        if let Ok(entries) = refs::list_refs(local_git_dir, prefix) {
            for (name, oid) in entries {
                let tip = if let Ok(resolved) = resolve_revision(negotiator.repo(), &name) {
                    resolved
                } else {
                    oid
                };
                if negotiator.repo().odb.read(&tip).is_err() {
                    continue;
                }
                tips.push(peel_commit_oid_for_negotiation(negotiator.repo(), tip)?);
            }
        }
    }
    if let Ok(h) = refs::resolve_ref(local_git_dir, "HEAD") {
        if negotiator.repo().odb.read(&h).is_ok() {
            tips.push(peel_commit_oid_for_negotiation(negotiator.repo(), h)?);
        }
    }
    for sym in ["HEAD", "MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        if let Ok(oid) = resolve_revision(negotiator.repo(), sym) {
            if negotiator.repo().odb.read(&oid).is_ok() {
                tips.push(peel_commit_oid_for_negotiation(negotiator.repo(), oid)?);
            }
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

    // With no `have` lines, Git's upload-pack does not send `NAK` until it sees `done`
    // (`upload-pack.c` `get_common_commits`). Reading ACKs here deadlocks the child on a pipe.
    for (_, oid) in advertised {
        if want_set.contains(oid) {
            continue;
        }
        if negotiator.repo().odb.read(oid).is_ok() {
            let c = peel_commit_oid_for_negotiation(negotiator.repo(), *oid)?;
            negotiator.known_common(c)?;
        }
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

    // Match `fetch-pack.c` `find_common`: send `done` as a single pkt-line with no trailing flush
    // before reading the server's `ACK`/`NAK` and the pack (a stray `0000` leaves a flush on the
    // wire and breaks side-band demux).
    let mut tail = Vec::new();
    pkt_line::write_line_to_vec(&mut tail, "done")?;
    trace_packet_fetch('>', "done");
    stdin.write_all(&tail).context("write done")?;
    stdin.flush()?;

    // `upload-pack` responds to `done` with `ACK <oid>` or `NAK` before streaming the pack.
    match pkt_line::read_packet(stdout)? {
        None => bail!("unexpected EOF from upload-pack after done"),
        Some(pkt_line::Packet::Flush) => {
            bail!("unexpected flush from upload-pack after done");
        }
        Some(pkt_line::Packet::Data(ln)) => {
            trace_packet_fetch('<', ln.trim_end());
            if ln.trim_end() == "NAK" {
                // Expected when we had nothing in common.
            } else if let Some((ack_oid, kind)) = parse_ack(&ln) {
                if kind != AckKind::Bare {
                    let _ = negotiator.ack(ack_oid)?;
                }
            }
        }
        Some(_) => {}
    }

    let mut pack_buf = Vec::new();
    read_sideband_pack_until_done(stdout, &mut pack_buf)?;

    Ok(pack_buf)
}

/// When tests run `git-daemon` with `--base-path=<GIT_DAEMON_DOCUMENT_ROOT_PATH>`, map a
/// `git://host:port/repo` URL to that on-disk repository so local commands can open it.
pub fn try_local_path_for_git_daemon_url(url: &str) -> Option<std::path::PathBuf> {
    let root = std::env::var("GIT_DAEMON_DOCUMENT_ROOT_PATH").ok()?;
    let parsed = parse_git_url(url).ok()?;
    let rel = parsed.path.trim_start_matches('/');
    if rel.is_empty() {
        return None;
    }
    Some(std::path::Path::new(&root).join(rel))
}

/// Parsed `git://host[:port]/path` (path includes leading `/`).
pub struct GitDaemonUrl {
    pub host: String,
    pub port: u16,
    pub path: String,
}

/// Parse `git://` URLs for the native Git daemon transport.
pub fn parse_git_url(url: &str) -> Result<GitDaemonUrl> {
    let rest = url
        .strip_prefix("git://")
        .with_context(|| format!("not a git:// URL: {url}"))?;
    let (authority, path_part) = rest
        .find('/')
        .map(|i| (&rest[..i], &rest[i..]))
        .unwrap_or((rest, "/"));
    if path_part.is_empty() || path_part == "/" {
        bail!("git:// URL missing repository path");
    }
    let path = path_part.to_string();
    let (host, port) = if authority.starts_with('[') {
        let end = authority
            .find(']')
            .with_context(|| format!("invalid git:// authority: {authority}"))?;
        let host = authority[1..end].to_string();
        let port = if let Some(p) = authority[end + 1..].strip_prefix(':') {
            p.parse::<u16>()
                .with_context(|| format!("invalid port in git:// URL: {url}"))?
        } else {
            9418
        };
        (host, port)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        let h = h.trim_end_matches(':');
        if p.is_empty() {
            (h.to_string(), 9418)
        } else if p.chars().all(|c| c.is_ascii_digit()) {
            (
                h.to_string(),
                p.parse::<u16>()
                    .with_context(|| format!("invalid port in git:// URL: {url}"))?,
            )
        } else {
            (authority.to_string(), 9418)
        }
    } else {
        (authority.to_string(), 9418)
    };
    if host.is_empty() {
        bail!("git:// URL has empty host");
    }
    Ok(GitDaemonUrl { host, port, path })
}

/// Fetch over `git://` (native daemon) using upload-pack negotiation.
pub fn fetch_via_git_protocol_skipping(
    local_git_dir: &Path,
    url: &str,
    refspecs: &[String],
) -> Result<(
    Vec<(String, ObjectId)>,
    Vec<(String, ObjectId)>,
    Option<String>,
    Option<ObjectId>,
)> {
    let parsed = parse_git_url(url)?;
    let addr = format!("{}:{}", parsed.host, parsed.port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve git://{}:{}", parsed.host, parsed.port))?
        .next()
        .with_context(|| format!("no addresses for git://{}:{}", parsed.host, parsed.port))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(30))
        .with_context(|| format!("could not connect to git://{}:{}", parsed.host, parsed.port))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(600)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(600)));

    let mut stream_w = stream
        .try_clone()
        .context("dup git:// socket for simultaneous read/write")?;
    let client_proto = protocol_wire::effective_client_protocol_version();
    let virtual_host = std::env::var("GIT_OVERRIDE_VIRTUAL_HOST")
        .unwrap_or_else(|_| format!("{}:{}", parsed.host, parsed.port));
    let mut inner: Vec<u8> = Vec::new();
    inner.extend_from_slice(b"git-upload-pack ");
    inner.extend_from_slice(parsed.path.as_bytes());
    inner.push(0);
    inner.extend_from_slice(b"host=");
    inner.extend_from_slice(virtual_host.as_bytes());
    inner.push(0);
    if client_proto > 0 {
        inner.push(0);
        inner.extend_from_slice(format!("version={client_proto}\0").as_bytes());
    }
    pkt_line::write_packet_raw(&mut stream_w, &inner).context("write git:// request")?;
    stream_w.flush().ok();

    let trace_show = String::from_utf8_lossy(&inner)
        .replace('\0', "\\0")
        .replace('\n', "");
    trace_packet_fetch('>', &trace_show);

    let (advertised, head_symref) = read_advertisement(&mut stream)?;
    if advertised.is_empty() {
        bail!("nothing to fetch (advertised 0 ref(s))");
    }
    let wants = collect_wants(&advertised, refspecs)?;
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

    let head_advertised_oid = advertised
        .iter()
        .find(|(n, _)| n == "HEAD")
        .map(|(_, o)| *o);

    if wants.is_empty() {
        if !refspecs.is_empty() {
            bail!(
                "nothing to fetch (advertised {} ref(s), empty want list)",
                advertised.len()
            );
        }
        return Ok((remote_heads, remote_tags, head_symref, head_advertised_oid));
    }

    let pack_buf = fetch_upload_pack_negotiate_pack_bytes_with_streams(
        local_git_dir,
        &advertised,
        &mut stream_w,
        &mut stream,
        &wants,
    )?;

    if pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK" {
        bail!("did not receive a pack file from upload-pack");
    }

    let odb = Odb::new(&local_git_dir.join("objects"));
    if pack_buf.len() > 12 {
        unpack_objects(&mut pack_buf.as_slice(), &odb, &UnpackOptions::default())?;
    }

    Ok((remote_heads, remote_tags, head_symref, head_advertised_oid))
}
