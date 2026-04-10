//! Local fetch over `grit upload-pack` with skipping negotiation (protocol v0/v1 ref ads, or v2
//! capability preamble with ref names merged from the remote repository when needed).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::diff::zero_oid;
use grit_lib::fetch_negotiator::SkippingNegotiator;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{peel_to_commit_for_merge_base, resolve_revision};
use grit_lib::unpack_objects::{unpack_objects, UnpackOptions};

use crate::file_upload_pack_v2::{read_pkt_lines_until_flush, skip_v2_section_until_boundary};
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

/// Split a simple upload-pack command string into leading `VAR=value` tokens (shell-style, no
/// quotes) and the remainder. Used when rewriting `… git-upload-pack` to `grit upload-pack` so
/// tests like `GIT_TEST_ASSUME_DIFFERENT_OWNER=true git-upload-pack` keep their environment
/// (`t0411-clone-from-partial`, `t5605-clone-dirname`).
pub(crate) fn parse_leading_shell_env_assignments(template: &str) -> (Vec<(String, String)>, &str) {
    let mut env_pairs = Vec::new();
    let mut rest = template.trim();
    while !rest.is_empty() {
        let Some(token) = rest.split_whitespace().next() else {
            break;
        };
        let Some((key, val)) = token.split_once('=') else {
            break;
        };
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            break;
        }
        env_pairs.push((key.to_string(), val.to_string()));
        rest = rest[token.len()..].trim_start();
    }
    (env_pairs, rest)
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

/// Git blocks hostnames and ports that start with `-` so they cannot be mistaken for CLI flags when
/// passed to proxy or transport helpers (`connect.c`: `looks_like_command_line_option`).
fn looks_like_command_line_option(s: &str) -> bool {
    s.as_bytes().first() == Some(&b'-')
}

/// Resolve `GIT_PROXY_COMMAND` or the first matching `core.gitproxy` rule for `host`, mirroring
/// Git's `git_use_proxy` / `git_proxy_command_options` (`connect.c`).
fn resolve_git_proxy_command(host: &str, config: Option<&ConfigSet>) -> Option<String> {
    if let Ok(env_cmd) = std::env::var("GIT_PROXY_COMMAND") {
        let t = env_cmd.trim();
        return if t.is_empty() {
            None
        } else {
            Some(t.to_owned())
        };
    }
    let cfg = config?;
    for raw in cfg.get_all("core.gitproxy") {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        let (cmd_len, matched) = if let Some(idx) = value.find(" for ") {
            let pattern = value[idx + 5..].trim();
            if pattern.is_empty() {
                continue;
            }
            let rlen = host.len();
            let plen = pattern.len();
            let suffix_ok = if rlen < plen {
                false
            } else if host[rlen - plen..] == *pattern {
                rlen == plen || host.as_bytes()[rlen - plen - 1] == b'.'
            } else {
                false
            };
            if !suffix_ok {
                continue;
            }
            (idx, true)
        } else {
            (value.len(), true)
        };
        if !matched {
            continue;
        }
        let mut eff_len = cmd_len;
        if eff_len == 4 && value.as_bytes().get(..4) == Some(b"none") {
            eff_len = 0;
        }
        if eff_len == 0 {
            return None;
        }
        return Some(value[..eff_len].to_owned());
    }
    None
}

/// Protocol v2 ends the initial advertisement at a flush with no ref lines. Run `ls-refs` to
/// obtain the same ref list v0 would have advertised (heads, tags, `HEAD`), matching Git's
/// `fetch-pack` and fixing fetches that would otherwise see an empty ref map (e.g. t5525).
fn v2_ls_refs_for_fetch(
    stdin: &mut impl Write,
    stdout: &mut impl Read,
) -> Result<(Vec<(String, ObjectId)>, Option<String>)> {
    let default_hash = std::env::var("GIT_DEFAULT_HASH").unwrap_or_else(|_| "sha1".to_owned());
    let agent = format!("agent=git/{}-", crate::version_string());

    trace_packet_fetch('>', "command=ls-refs");
    pkt_line::write_line(stdin, "command=ls-refs")?;
    trace_packet_fetch('>', agent.trim_end());
    pkt_line::write_line(stdin, &agent)?;
    let of = format!("object-format={default_hash}");
    trace_packet_fetch('>', &of);
    pkt_line::write_line(stdin, &of)?;
    pkt_line::write_delim(stdin)?;
    trace_packet_fetch('>', "0001");
    trace_packet_fetch('>', "symrefs");
    pkt_line::write_line(stdin, "symrefs")?;
    trace_packet_fetch('>', "peel");
    pkt_line::write_line(stdin, "peel")?;
    trace_packet_fetch('>', "ref-prefix HEAD");
    pkt_line::write_line(stdin, "ref-prefix HEAD")?;
    trace_packet_fetch('>', "ref-prefix refs/heads/");
    pkt_line::write_line(stdin, "ref-prefix refs/heads/")?;
    trace_packet_fetch('>', "ref-prefix refs/tags/");
    pkt_line::write_line(stdin, "ref-prefix refs/tags/")?;
    pkt_line::write_flush(stdin)?;
    trace_packet_fetch('>', "0000");
    stdin.flush().context("flush ls-refs request")?;

    let mut buf = Vec::new();
    read_pkt_lines_until_flush(stdout, &mut buf, 512 * 1024).context("read ls-refs response")?;

    let mut cursor = std::io::Cursor::new(&buf);
    let mut advertised: Vec<(String, ObjectId)> = Vec::new();
    let mut head_symref: Option<String> = None;

    loop {
        let pkt = match pkt_line::read_packet(&mut cursor)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => line,
            Some(other) => bail!("unexpected ls-refs packet in fetch: {other:?}"),
        };
        let (name, oid, _peeled, symref_target) =
            crate::commands::ls_remote::parse_ls_refs_v2_line(&pkt)?;
        if name.contains("^{") {
            continue;
        }
        if name == "HEAD" {
            if let Some(t) = symref_target {
                head_symref = Some(t);
            }
            advertised.push((name, oid));
        } else if name.starts_with("refs/heads/") || name.starts_with("refs/tags/") {
            advertised.push((name, oid));
        }
    }

    Ok((advertised, head_symref))
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

/// When the child speaks protocol v2, [`read_advertisement`] only skips capability lines and never
/// records ref advertisements. Merge `refs/heads/*` and `refs/tags/*` from the on-disk remote so
/// [`collect_wants_for_upload_pack`] can request missing objects and the fetch command can update
/// remote-tracking refs (t5506-remote-groups).
fn merge_remote_refs_into_upload_pack_advertisement(
    remote_repo_path: &Path,
    advertised: &mut Vec<(String, ObjectId)>,
) -> Result<()> {
    // `remote_repo_path` is the repository root (bare) or work tree (non-bare); `list_refs` needs
    // the git directory. `Repository::open` expects `.git` for normal repos.
    let remote_git: PathBuf = {
        let dot_git = remote_repo_path.join(".git");
        if dot_git.is_dir() {
            dot_git
        } else {
            remote_repo_path.to_path_buf()
        }
    };
    if advertised.iter().any(|(n, _)| n.starts_with("refs/heads/")) {
        return Ok(());
    }
    let mut by_name: HashMap<String, ObjectId> =
        advertised.iter().map(|(n, o)| (n.clone(), *o)).collect();
    for (n, o) in refs::list_refs(&remote_git, "refs/heads/")? {
        by_name.insert(n, o);
    }
    for (n, o) in refs::list_refs(&remote_git, "refs/tags/")? {
        by_name.insert(n, o);
    }
    *advertised = by_name.into_iter().collect();
    advertised.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(())
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

    let (leading_env, after_env) = parse_leading_shell_env_assignments(cmd_template);
    if after_env.contains("git-upload-pack") {
        let mut c = Command::new(grit_executable());
        for (k, v) in leading_env {
            c.env(k, v);
        }
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

/// Spawn `upload-pack` for local pipe negotiation ([`fetch_via_upload_pack_skipping`], etc.).
///
/// Always uses **protocol v0** ref advertisement for the child (`GIT_PROTOCOL` cleared), even when
/// the user's `protocol.version` is 2. The local fetch client reads the v0 pkt-line ref list via
/// [`read_advertisement`]; a v2 server would emit `version 2` capability lines first and no ref
/// rows, which breaks refspec resolution (e.g. `t5501-fetch-push-alternates`).
pub(crate) fn spawn_upload_pack(
    cmd_template: Option<&str>,
    repo_path: &Path,
) -> Result<std::process::Child> {
    // Local fetch/clone uses protocol v0 pkt-line negotiation (`want`/`have`/`done`). Always
    // spawn the server side without forcing `GIT_PROTOCOL=version=2`, even when the client
    // defaults to `protocol.version=2` for HTTP/file v2 — otherwise `upload-pack` enters the v2
    // path and rejects v0 `want` lines as "unknown capability" (t0411 lazy-fetch re-enable).
    // Force protocol 0 on the wire so the ref advertisement matches [`read_advertisement`] (t5501).
    spawn_upload_pack_with_proto(cmd_template, repo_path, 0)
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
        // Flush / delim / response-end — not a data payload; side-band readers stop at flush.
        0 | 1 | 2 => Ok(None),
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
    // Progress and pack data share side-band channel 1; the `PACK` magic may start mid-chunk or
    // span chunk boundaries (65515-byte framing), so scan a small carry buffer until we find it.
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let Some(payload) = read_pkt_payload_raw(r)? else {
            break;
        };
        // `read_pkt_payload_raw` returns `None` on flush/EOF; empty payloads should not occur.
        if payload.is_empty() {
            continue;
        }
        match payload[0] {
            1 => {
                let data = &payload[1..];
                if !seen_pack {
                    pending.extend_from_slice(data);
                    if let Some(pos) = pending.windows(4).position(|w| w == b"PACK") {
                        seen_pack = true;
                        out.extend_from_slice(&pending[pos..]);
                        pending.clear();
                    } else if pending.len() > 3 {
                        let keep_from = pending.len() - 3;
                        pending.drain(..keep_from);
                    }
                } else {
                    out.extend_from_slice(data);
                }
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

/// Read a protocol v2 `fetch` response: skip non-pack sections, demux side-band-64k pack data.
fn read_v2_fetch_pack_response(stdout: &mut impl Read, out: &mut Vec<u8>) -> Result<()> {
    loop {
        let hdr = match pkt_line::read_packet(stdout)? {
            Some(pkt_line::Packet::Data(s)) => s,
            Some(pkt_line::Packet::Flush) => return Ok(()),
            None => return Ok(()),
            Some(other) => bail!("unexpected v2 fetch response: {other:?}"),
        };
        trace_packet_fetch('<', hdr.trim_end());
        match hdr.as_str() {
            "acknowledgments" | "wanted-refs" | "shallow-info" | "packfile-uris" => {
                skip_v2_section_until_boundary(stdout)?;
            }
            "packfile" => {
                read_sideband_pack_until_done(stdout, out)?;
                let _ = pkt_line::read_packet(stdout)?;
                return Ok(());
            }
            other => bail!("unexpected v2 fetch section: {other}"),
        }
    }
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
    // Force v0 advertisement (immediate ref pkt-lines). v2 stops after `version 2` until `ls-refs`,
    // which leaves `read_advertisement` with an empty ref list and breaks fetch.
    let mut child = spawn_upload_pack_with_proto(upload_pack_cmd, remote_repo_path, 0)?;
    let mut stdin = child.stdin.take().context("upload-pack stdin")?;
    let mut stdout = child.stdout.take().context("upload-pack stdout")?;

    // Must match `spawn_upload_pack`: the child is always protocol v0 (`GIT_PROTOCOL` cleared).
    let (mut advertised, head_symref) = read_advertisement(&mut stdout)?;
    if !has_cli_refspecs {
        merge_remote_refs_into_upload_pack_advertisement(remote_repo_path, &mut advertised)?;
    }
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

    // When the client already has every wanted object, `pack-objects --thin` can stream an empty
    // body (or only the 12-byte PACK header). That is still a successful fetch (ref updates only).
    if !pack_buf.is_empty() && (pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK") {
        bail!("did not receive a pack file from upload-pack");
    }

    let odb = Odb::new(&local_git_dir.join("objects"));
    if pack_buf.len() > 12 {
        append_pack_to_git_trace_packfile(&pack_buf)?;
        unpack_objects(&mut pack_buf.as_slice(), &odb, &UnpackOptions::default())?;
    }

    Ok((remote_heads, remote_tags, head_symref, head_advertised_oid))
}

fn append_pack_to_git_trace_packfile(pack: &[u8]) -> anyhow::Result<()> {
    let Ok(path) = std::env::var("GIT_TRACE_PACKFILE") else {
        return Ok(());
    };
    if path.is_empty() {
        return Ok(());
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("GIT_TRACE_PACKFILE: open {}", path))?;
    f.write_all(pack)
        .with_context(|| format!("GIT_TRACE_PACKFILE: write {}", path))?;
    Ok(())
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

    if !pack_buf.is_empty() && (pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK") {
        bail!("did not receive a pack file from upload-pack");
    }

    append_pack_to_git_trace_packfile(&pack_buf)?;

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

fn fetch_git_daemon_upload_pack_over_streams(
    local_git_dir: &Path,
    refspecs: &[String],
    parsed: &GitDaemonUrl,
    stream_w: &mut impl Write,
    stream: &mut impl Read,
) -> Result<(
    Vec<(String, ObjectId)>,
    Vec<(String, ObjectId)>,
    Option<String>,
    Option<ObjectId>,
)> {
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
    pkt_line::write_packet_raw(stream_w, &inner).context("write git:// request")?;
    stream_w.flush().ok();

    let trace_show = String::from_utf8_lossy(&inner)
        .replace('\0', "\\0")
        .replace('\n', "");
    trace_packet_fetch('>', &trace_show);

    let (advertised, head_symref) = read_advertisement(stream)?;
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
        stream_w,
        stream,
        &wants,
    )?;

    if !pack_buf.is_empty() && (pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK") {
        bail!("did not receive a pack file from upload-pack");
    }

    let odb = Odb::new(&local_git_dir.join("objects"));
    if pack_buf.len() > 12 {
        append_pack_to_git_trace_packfile(&pack_buf)?;
        unpack_objects(&mut pack_buf.as_slice(), &odb, &UnpackOptions::default())?;
    }

    Ok((remote_heads, remote_tags, head_symref, head_advertised_oid))
}

/// Fetch over `git://` (native daemon) using upload-pack negotiation.
///
/// When `GIT_PROXY_COMMAND` or `core.gitproxy` selects a proxy, the proxy is spawned with
/// `host` and `port` as trailing arguments (Git-compatible). Pass `config` so repository
/// `core.gitproxy` rules apply; use `None` for clone before a config exists.
///
/// `proxy_cwd` is the directory used as the child's working directory when spawning the proxy
/// (typically the repository work tree). This matches Git so relative `core.gitproxy` commands
/// like `./proxy` resolve even when the process CWD is not the work tree (e.g. `GIT_DIR` set).
pub fn fetch_via_git_protocol_skipping(
    local_git_dir: &Path,
    url: &str,
    refspecs: &[String],
    config: Option<&ConfigSet>,
    proxy_cwd: Option<&Path>,
) -> Result<(
    Vec<(String, ObjectId)>,
    Vec<(String, ObjectId)>,
    Option<String>,
    Option<ObjectId>,
)> {
    let parsed = parse_git_url(url)?;
    if looks_like_command_line_option(&parsed.host) {
        bail!("strange hostname '{}' blocked", parsed.host);
    }
    let port_str = parsed.port.to_string();
    if looks_like_command_line_option(&port_str) {
        bail!("strange port '{}' blocked", port_str);
    }

    if let Some(proxy_cmd) = resolve_git_proxy_command(&parsed.host, config) {
        let words = shell_words::split(&proxy_cmd)
            .with_context(|| format!("invalid proxy command: {proxy_cmd:?}"))?;
        if words.is_empty() {
            bail!("empty proxy command");
        }
        let mut cmd = Command::new(&words[0]);
        for arg in words.iter().skip(1) {
            cmd.arg(arg);
        }
        cmd.arg(&parsed.host).arg(port_str.as_str());
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(dir) = proxy_cwd {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("cannot start proxy {proxy_cmd}"))?;
        let mut stream_w = child.stdin.take().context("proxy stdin")?;
        let mut stream = child.stdout.take().context("proxy stdout")?;

        let fetch_res = fetch_git_daemon_upload_pack_over_streams(
            local_git_dir,
            refspecs,
            &parsed,
            &mut stream_w,
            &mut stream,
        );
        drop(stream_w);
        drop(stream);
        let status = child.wait().context("wait for proxy")?;
        let out = fetch_res?;
        if !status.success() {
            bail!("proxy exited with status {status}");
        }
        Ok(out)
    } else {
        let addr = format!("{}:{}", parsed.host, port_str)
            .to_socket_addrs()
            .with_context(|| format!("could not resolve git://{}:{}", parsed.host, parsed.port))?
            .next()
            .with_context(|| format!("no addresses for git://{}:{}", parsed.host, parsed.port))?;
        let mut stream =
            TcpStream::connect_timeout(&addr, Duration::from_secs(30)).with_context(|| {
                format!("could not connect to git://{}:{}", parsed.host, parsed.port)
            })?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(600)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(600)));

        let mut stream_w = stream
            .try_clone()
            .context("dup git:// socket for simultaneous read/write")?;

        fetch_git_daemon_upload_pack_over_streams(
            local_git_dir,
            refspecs,
            &parsed,
            &mut stream_w,
            &mut stream,
        )
    }
}
