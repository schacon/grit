//! Smart HTTP client for `git clone` / `git fetch` (protocol v2 over HTTP).
//!
//! Used when the repository URL is `http://` or `https://`. Emits trace2 `child_start`
//! lines compatible with `test_remote_https_urls` in the test harness.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use grit_lib::fetch_negotiator::SkippingNegotiator;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;

use crate::http_bundle_uri::strip_v0_service_advertisement_if_present;
use crate::pkt_line;

const SERVICE: &str = "git-upload-pack";

static TRACED_HTTPS_URLS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Clear deduplication state for `GIT_TRACE2_EVENT` `child_start` lines (new top-level command).
pub fn clear_trace2_https_url_dedup() {
    if let Some(m) = TRACED_HTTPS_URLS.get() {
        m.lock().ok().map(|mut g| g.clear());
    }
}

/// Emit a single JSON trace2 line (for deduplicated bundle fetches).
pub fn trace2_child_start_git_remote_https(url: &str) {
    let Ok(path) = std::env::var("GIT_TRACE2_EVENT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let set = TRACED_HTTPS_URLS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = set.lock().ok();
    if let Some(ref mut g) = guard {
        if !g.insert(url.to_string()) {
            return;
        }
    }
    let now = crate::trace2_json_now();
    let esc = url.replace('\\', "\\\\").replace('"', "\\\"");
    let line = format!(
        r#"{{"event":"child_start","sid":"grit-0","time":"{}","argv":["git-remote-https","{}"]}}"#,
        now, esc
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{line}"));
}

pub(crate) fn agent_header() -> String {
    format!("grit/{}", crate::version_string())
}

fn http_get(client: &crate::http_client::HttpClientContext, url: &str) -> Result<Vec<u8>> {
    trace2_child_start_git_remote_https(url);
    client.get_with_git_protocol(url, Some("version=2"))
}

fn http_get_discovery(
    client: &crate::http_client::HttpClientContext,
    url: &str,
) -> Result<Vec<u8>> {
    trace2_child_start_git_remote_https(url);
    client.get(url)
}

fn http_post(
    client: &crate::http_client::HttpClientContext,
    url: &str,
    content_type: &str,
    accept: &str,
    body: &[u8],
) -> Result<Vec<u8>> {
    trace2_child_start_git_remote_https(url);
    client.post_with_git_protocol(url, content_type, accept, body, Some("version=2"))
}

fn http_post_discovery(
    client: &crate::http_client::HttpClientContext,
    url: &str,
    content_type: &str,
    accept: &str,
    body: &[u8],
    git_protocol_header: Option<&str>,
) -> Result<Vec<u8>> {
    trace2_child_start_git_remote_https(url);
    client.post_with_git_protocol(url, content_type, accept, body, git_protocol_header)
}

fn read_v2_caps(body: &[u8]) -> Result<Vec<String>> {
    let mut cur = Cursor::new(body);
    let first = match pkt_line::read_packet(&mut cur)? {
        None => bail!("empty v2 capability block"),
        Some(pkt_line::Packet::Data(s)) => s,
        Some(other) => bail!("expected version line, got {other:?}"),
    };
    if first != "version 2" {
        bail!("expected 'version 2', got {first:?}");
    }
    let mut caps = vec![first];
    loop {
        match pkt_line::read_packet(&mut cur)? {
            None => bail!("unexpected EOF in v2 capabilities"),
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(s)) => caps.push(s),
            Some(other) => bail!("unexpected packet in v2 caps: {other:?}"),
        }
    }
    Ok(caps)
}

fn parse_v0_v1_advertisement(
    body: &[u8],
) -> Result<(Vec<LsRefEntry>, std::collections::HashSet<String>)> {
    let mut cur = Cursor::new(body);
    let mut refs = Vec::new();
    let mut caps = std::collections::HashSet::new();
    let mut first_ref_line = true;
    loop {
        match pkt_line::read_packet(&mut cur)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                if line.starts_with("version ") {
                    continue;
                }
                let (payload, cap_part) = match line.split_once('\0') {
                    Some((p, c)) => (p.trim(), Some(c)),
                    None => (line.trim(), None),
                };
                let (oid_hex, refname) = payload
                    .split_once('\t')
                    .or_else(|| payload.split_once(' '))
                    .ok_or_else(|| anyhow::anyhow!("malformed v0/v1 advertisement: {line}"))?;
                let oid = ObjectId::from_hex(oid_hex.trim())
                    .with_context(|| format!("bad oid in v0/v1 advertisement: {oid_hex}"))?;
                let refname = refname.trim();
                if refname.is_empty() {
                    continue;
                }
                if first_ref_line {
                    if let Some(raw_caps) = cap_part {
                        for cap in raw_caps.split_whitespace() {
                            caps.insert(cap.to_string());
                        }
                    }
                    first_ref_line = false;
                }
                refs.push(LsRefEntry {
                    name: refname.to_string(),
                    oid,
                });
            }
            Some(other) => bail!("unexpected packet in v0/v1 advertisement: {other:?}"),
        }
    }
    Ok((refs, caps))
}

enum HttpDiscovery {
    V2 {
        caps: Vec<String>,
        object_format: String,
    },
    V0V1 {
        advertised: Vec<LsRefEntry>,
        caps: std::collections::HashSet<String>,
    },
}

fn discover_http_protocol(pkt_body: &[u8]) -> Result<HttpDiscovery> {
    let mut cur = Cursor::new(pkt_body);
    let first = match pkt_line::read_packet(&mut cur)? {
        None => bail!("empty smart-http advertisement"),
        Some(pkt_line::Packet::Data(s)) => s,
        Some(other) => bail!("unexpected first advertisement packet: {other:?}"),
    };
    if first == "version 2" {
        let caps = read_v2_caps(pkt_body)?;
        let object_format = caps
            .iter()
            .find_map(|c| c.strip_prefix("object-format="))
            .unwrap_or("sha1")
            .to_string();
        return Ok(HttpDiscovery::V2 {
            caps,
            object_format,
        });
    }
    let (advertised, caps) = parse_v0_v1_advertisement(pkt_body)?;
    Ok(HttpDiscovery::V0V1 { advertised, caps })
}

fn cap_lines_for_client_request(caps: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in caps {
        if line.starts_with("agent=") {
            out.push(line.clone());
        } else if let Some(fmt) = line.strip_prefix("object-format=") {
            out.push(format!("object-format={fmt}"));
        }
    }
    out
}

fn skip_to_flush(r: &mut Cursor<&[u8]>) -> Result<()> {
    loop {
        match pkt_line::read_packet(r)? {
            None => return Ok(()),
            Some(pkt_line::Packet::Flush) => return Ok(()),
            Some(pkt_line::Packet::Data(_)) => {}
            Some(_) => {}
        }
    }
}

/// Ref advertisement from protocol v2 `ls-refs`.
#[derive(Clone, Debug)]
pub struct LsRefEntry {
    pub name: String,
    pub oid: ObjectId,
}

/// Run `ls-refs` over smart HTTP and return advertised refs.
pub fn http_ls_refs(
    repo_url: &str,
    client: &crate::http_client::HttpClientContext,
) -> Result<Vec<LsRefEntry>> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
    refs_url.push_str(&format!("service={SERVICE}"));

    let body = http_get(client, &refs_url)?;
    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;
    let (caps, object_format) = match discover_http_protocol(pkt_body)? {
        HttpDiscovery::V2 {
            caps,
            object_format,
        } => (caps, object_format),
        HttpDiscovery::V0V1 { advertised, .. } => return Ok(advertised),
    };

    let mut req = Vec::new();
    pkt_line::write_line_to_vec(&mut req, "command=ls-refs")?;
    pkt_line::write_line_to_vec(&mut req, &format!("object-format={object_format}"))?;
    for line in cap_lines_for_client_request(&caps) {
        pkt_line::write_line_to_vec(&mut req, &line)?;
    }
    pkt_line::write_delim(&mut req)?;
    pkt_line::write_line_to_vec(&mut req, "peel")?;
    pkt_line::write_line_to_vec(&mut req, "symrefs")?;
    pkt_line::write_flush(&mut req)?;

    let post_url = format!("{base}/{SERVICE}");
    let resp = http_post(
        client,
        &post_url,
        &format!("application/x-{SERVICE}-request"),
        &format!("application/x-{SERVICE}-result"),
        &req,
    )?;

    parse_ls_refs_v2_response(&resp)
}

fn parse_ls_refs_v2_response(data: &[u8]) -> Result<Vec<LsRefEntry>> {
    let mut cur = Cursor::new(data);
    let mut out = Vec::new();
    loop {
        let pkt = match pkt_line::read_packet(&mut cur)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => line,
            Some(other) => bail!("unexpected ls-refs packet: {other:?}"),
        };
        let (oid_hex, rest) = pkt
            .split_once(' ')
            .ok_or_else(|| anyhow::anyhow!("bad ls-refs line: {pkt}"))?;
        let oid = ObjectId::from_hex(oid_hex.trim())?;
        let name = rest.split_whitespace().next().unwrap_or(rest).to_string();
        if name.is_empty() {
            continue;
        }
        out.push(LsRefEntry { name, oid });
    }
    Ok(out)
}

fn collect_wants_from_advertised(
    advertised: &[LsRefEntry],
    refspecs: &[String],
) -> Result<Vec<ObjectId>> {
    if refspecs.is_empty() {
        let mut wants = Vec::new();
        for e in advertised {
            if e.name.starts_with("refs/heads/") || e.name.starts_with("refs/tags/") {
                wants.push(e.oid);
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
            bail!("glob refspec in HTTP fetch not supported");
        }
        let remote_ref = if src.starts_with("refs/") {
            src.to_string()
        } else {
            format!("refs/heads/{src}")
        };
        let oid = advertised
            .iter()
            .find(|e| e.name == remote_ref)
            .map(|e| e.oid)
            .or_else(|| {
                let tag_ref = format!("refs/tags/{src}");
                advertised.iter().find(|e| e.name == tag_ref).map(|e| e.oid)
            })
            .with_context(|| format!("could not find remote ref '{remote_ref}'"))?;
        wants.push(oid);
    }
    Ok(wants)
}

fn build_fetch_caps_v0(caps: &std::collections::HashSet<String>) -> String {
    let mut enabled = Vec::new();
    for want in [
        "multi_ack_detailed",
        "side-band-64k",
        "thin-pack",
        "no-progress",
        "include-tag",
        "ofs-delta",
    ] {
        if caps.contains(want) {
            enabled.push(want);
        }
    }
    if enabled.is_empty() {
        String::new()
    } else {
        format!(" {}", enabled.join(" "))
    }
}

fn fetch_pack_v0_v1_stateless_http(
    local_git_dir: &Path,
    base: &str,
    advertised: &[LsRefEntry],
    refspecs: &[String],
    caps: &std::collections::HashSet<String>,
    filter_active: bool,
    client: &crate::http_client::HttpClientContext,
) -> Result<(Vec<LsRefEntry>, Vec<LsRefEntry>, Vec<LsRefEntry>)> {
    let wants = collect_wants_from_advertised(advertised, refspecs)?;
    if wants.is_empty() {
        bail!("nothing to fetch (empty want list)");
    }

    let remote_heads: Vec<_> = advertised
        .iter()
        .filter(|e| e.name.starts_with("refs/heads/"))
        .cloned()
        .collect();
    let remote_tags: Vec<_> = advertised
        .iter()
        .filter(|e| e.name.starts_with("refs/tags/"))
        .cloned()
        .collect();
    let all_advertised = advertised.to_vec();

    let fetch_caps = build_fetch_caps_v0(caps);
    let mut req = Vec::new();
    let first = wants[0];
    pkt_line::write_line_to_vec(&mut req, &format!("want {}{}", first.to_hex(), fetch_caps))?;
    for w in wants.iter().skip(1) {
        pkt_line::write_line_to_vec(&mut req, &format!("want {}", w.to_hex()))?;
    }
    pkt_line::write_line_to_vec(&mut req, "done")?;
    pkt_line::write_flush(&mut req)?;

    let post_url = format!("{base}/{SERVICE}");
    let resp = http_post(
        client,
        &post_url,
        &format!("application/x-{SERVICE}-request"),
        &format!("application/x-{SERVICE}-result"),
        &req,
    )?;

    let mut cur = Cursor::new(resp.as_slice());
    let mut first_pkt = None::<String>;
    if let Some(pkt_line::Packet::Data(line)) = pkt_line::read_packet(&mut cur)? {
        let line = line.trim_end_matches('\n').to_string();
        if line.starts_with("ERR ") {
            bail!("remote upload-pack error: {}", line.trim_start_matches("ERR "));
        }
        first_pkt = Some(line);
    }
    let mut pack_buf = Vec::new();
    if caps.contains("side-band-64k") {
        read_sideband_pack_until_done(&mut cur, &mut pack_buf)?;
    } else {
        let pos = cur.position() as usize;
        if pos < resp.len() {
            pack_buf.extend_from_slice(&resp[pos..]);
        }
    }

    if !pack_buf.is_empty() {
        if pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK" {
            bail!("did not receive a pack file from HTTP v0/v1 fetch");
        }
        crate::fetch_transport::unpack_upload_pack_bytes(local_git_dir, &pack_buf, filter_active)?;
    } else if let Some(line) = first_pkt {
        let normalized = line.trim();
        if normalized != "NAK" && !normalized.starts_with("ACK ") {
            bail!("unexpected v0/v1 fetch response: {normalized}");
        }
    }

    Ok((remote_heads, remote_tags, all_advertised))
}

fn trace_clone_negotiation_line(line: &str) {
    crate::trace_packet::trace_packet_line(line.as_bytes());
}

fn read_sideband_pack_until_done(r: &mut impl Read, out: &mut Vec<u8>) -> Result<()> {
    let mut seen_pack = false;
    loop {
        let Some(payload) = crate::fetch_transport::read_pkt_payload_raw(r)? else {
            break;
        };
        if payload.is_empty() {
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

/// Fetch packfile via HTTP protocol v2 into `local_git_dir`, using the same skipping
/// negotiation idea as local upload-pack (initial have window, then `done`).
///
/// Returns `(heads, tags, all_advertised)` where `all_advertised` is every ref from `ls-refs`.
pub fn http_fetch_pack(
    local_git_dir: &Path,
    repo_url: &str,
    refspecs: &[String],
    filter_active: bool,
    client: &crate::http_client::HttpClientContext,
) -> Result<(Vec<LsRefEntry>, Vec<LsRefEntry>, Vec<LsRefEntry>)> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
    refs_url.push_str(&format!("service={SERVICE}"));

    let body = http_get(client, &refs_url)?;
    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;
    let discovery = discover_http_protocol(pkt_body)?;
    let (caps, object_format) = match discovery {
        HttpDiscovery::V2 {
            caps,
            object_format,
        } => (caps, object_format),
        HttpDiscovery::V0V1 { advertised, caps } => {
            return fetch_pack_v0_v1_stateless_http(
                local_git_dir,
                base,
                &advertised,
                refspecs,
                &caps,
                filter_active,
                client,
            )
        }
    };

    let advertised = {
        let mut req = Vec::new();
        pkt_line::write_line_to_vec(&mut req, "command=ls-refs")?;
        pkt_line::write_line_to_vec(&mut req, &format!("object-format={object_format}"))?;
        for line in cap_lines_for_client_request(&caps) {
            pkt_line::write_line_to_vec(&mut req, &line)?;
        }
        pkt_line::write_delim(&mut req)?;
        pkt_line::write_line_to_vec(&mut req, "peel")?;
        pkt_line::write_line_to_vec(&mut req, "symrefs")?;
        pkt_line::write_flush(&mut req)?;

        let post_url = format!("{base}/{SERVICE}");
        let resp = http_post(
            client,
            &post_url,
            &format!("application/x-{SERVICE}-request"),
            &format!("application/x-{SERVICE}-result"),
            &req,
        )?;
        parse_ls_refs_v2_response(&resp)?
    };

    let wants = collect_wants_from_advertised(&advertised, refspecs)?;
    if wants.is_empty() {
        bail!("nothing to fetch (empty want list)");
    }

    let want_set: HashSet<ObjectId> = wants.iter().copied().collect();
    let all_advertised = advertised.clone();
    let remote_heads: Vec<_> = advertised
        .iter()
        .filter(|e| e.name.starts_with("refs/heads/"))
        .cloned()
        .collect();
    let remote_tags: Vec<_> = advertised
        .iter()
        .filter(|e| e.name.starts_with("refs/tags/"))
        .cloned()
        .collect();

    let local_repo = Repository::open(local_git_dir, None)
        .with_context(|| format!("open {}", local_git_dir.display()))?;
    let mut negotiator = SkippingNegotiator::new(local_repo);

    if let Ok(entries) = refs::list_refs(local_git_dir, "refs/bundles/") {
        for (name, oid) in entries {
            let t = if let Ok(resolved) = resolve_revision(negotiator.repo(), &name) {
                resolved
            } else {
                oid
            };
            if negotiator.repo().odb.read(&t).is_ok() {
                negotiator.add_tip(t)?;
            }
        }
    }

    for w in &wants {
        if negotiator.repo().odb.read(w).is_ok() {
            negotiator.add_tip(*w)?;
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

    for e in &advertised {
        if want_set.contains(&e.oid) {
            continue;
        }
        if negotiator.repo().odb.read(&e.oid).is_ok() {
            negotiator.known_common(e.oid)?;
        }
    }

    let post_url = format!("{base}/{SERVICE}");
    let cap_send = cap_lines_for_client_request(&caps);
    let fetch_caps = "thin-pack ofs-delta side-band-64k no-progress wait-for-done";

    let mut pending_haves: Vec<ObjectId> = Vec::new();
    while let Some(oid) = negotiator.next_have()? {
        pending_haves.push(oid);
    }

    let write_fetch_request = |include_done: bool| -> Result<Vec<u8>> {
        let mut req = Vec::new();
        pkt_line::write_line_to_vec(&mut req, "command=fetch")?;
        pkt_line::write_line_to_vec(&mut req, &format!("object-format={object_format}"))?;
        for line in &cap_send {
            pkt_line::write_line_to_vec(&mut req, line)?;
        }
        pkt_line::write_delim(&mut req)?;
        for w in &wants {
            pkt_line::write_line_to_vec(&mut req, &format!("want {} {}", w.to_hex(), fetch_caps))?;
        }
        for h in &pending_haves {
            let trace = format!("clone> have {}", h.to_hex());
            trace_clone_negotiation_line(&trace);
            pkt_line::write_line_to_vec(&mut req, &format!("have {}", h.to_hex()))?;
        }
        if include_done {
            pkt_line::write_line_to_vec(&mut req, "done")?;
            trace_clone_negotiation_line("clone> done");
        }
        pkt_line::write_flush(&mut req)?;
        Ok(req)
    };

    let unpack_packfile = |pack_buf: &[u8]| -> Result<()> {
        if pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK" {
            bail!("did not receive a pack file from HTTP fetch");
        }
        crate::fetch_transport::unpack_upload_pack_bytes(local_git_dir, pack_buf, filter_active)?;
        Ok(())
    };

    if !pending_haves.is_empty() {
        let req = write_fetch_request(false)?;
        let resp = http_post(
            client,
            &post_url,
            &format!("application/x-{SERVICE}-request"),
            &format!("application/x-{SERVICE}-result"),
            &req,
        )?;
        let mut cur = Cursor::new(resp.as_slice());
        let first = match pkt_line::read_packet(&mut cur)? {
            Some(pkt_line::Packet::Data(s)) => s,
            _ => String::new(),
        };
        if first == "packfile" {
            let mut pack_buf = Vec::new();
            read_sideband_pack_until_done(&mut cur, &mut pack_buf)?;
            unpack_packfile(&pack_buf)?;
            crate::trace_packet::trace_packet_line(b"clone> packfile negotiation complete");
            return Ok((remote_heads, remote_tags, all_advertised));
        }
        if first == "acknowledgments" {
            skip_to_flush(&mut cur)?;
        }
    }

    let req = write_fetch_request(true)?;
    let resp = http_post(
        client,
        &post_url,
        &format!("application/x-{SERVICE}-request"),
        &format!("application/x-{SERVICE}-result"),
        &req,
    )?;

    let mut cur = Cursor::new(resp.as_slice());
    loop {
        let pkt = match pkt_line::read_packet(&mut cur)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(s)) => s,
            Some(other) => bail!("unexpected fetch response: {other:?}"),
        };
        if pkt == "acknowledgments" {
            skip_to_flush(&mut cur)?;
        } else if pkt == "packfile" {
            let mut pack_buf = Vec::new();
            read_sideband_pack_until_done(&mut cur, &mut pack_buf)?;
            unpack_packfile(&pack_buf)?;
            break;
        }
    }

    crate::trace_packet::trace_packet_line(b"clone> packfile negotiation complete");
    Ok((remote_heads, remote_tags, all_advertised))
}

/// Best-effort default branch from `ls-refs` (`HEAD` symref target or first `refs/heads/*`).
pub fn remote_default_branch_from_advertised(adv: &[LsRefEntry]) -> Option<String> {
    for e in adv {
        if e.name == "HEAD" {
            // v2 ls-refs includes symref-target in line - we only store name+oid here;
            // HEAD oid often points at a branch tip; find matching branch.
            for h in adv {
                if h.name.starts_with("refs/heads/") && h.oid == e.oid {
                    return h.name.strip_prefix("refs/heads/").map(str::to_owned);
                }
            }
        }
    }
    adv.iter()
        .find(|e| e.name.starts_with("refs/heads/"))
        .and_then(|e| e.name.strip_prefix("refs/heads/"))
        .map(str::to_owned)
}
