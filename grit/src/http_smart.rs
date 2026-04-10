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
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::unpack_objects::{unpack_objects, UnpackOptions};

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

fn agent_header() -> String {
    format!("grit/{}", crate::version_string())
}

fn http_get(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .set("Git-Protocol", "version=2")
        .set("User-Agent", &agent_header())
        .call()
        .with_context(|| format!("GET {url}"))?;
    if resp.status() >= 400 {
        bail!(
            "GET request failed: HTTP {} {}",
            resp.status(),
            resp.status_text()
        );
    }
    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .context("read GET body")?;
    Ok(body)
}

fn http_post(url: &str, content_type: &str, accept: &str, body: &[u8]) -> Result<Vec<u8>> {
    let resp = ureq::post(url)
        .set("Content-Type", content_type)
        .set("Accept", accept)
        .set("Git-Protocol", "version=2")
        .set("User-Agent", &agent_header())
        .send_bytes(body)
        .with_context(|| format!("POST {url}"))?;
    if resp.status() >= 400 {
        bail!(
            "POST request failed: HTTP {} {}",
            resp.status(),
            resp.status_text()
        );
    }
    let mut out = Vec::new();
    resp.into_reader()
        .read_to_end(&mut out)
        .context("read POST body")?;
    Ok(out)
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
pub fn http_ls_refs(repo_url: &str) -> Result<Vec<LsRefEntry>> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
    refs_url.push_str(&format!("service={SERVICE}"));

    let body = http_get(&refs_url)?;
    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;
    let caps = read_v2_caps(pkt_body)?;

    let object_format = caps
        .iter()
        .find_map(|c| c.strip_prefix("object-format="))
        .unwrap_or("sha1");

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
) -> Result<(Vec<LsRefEntry>, Vec<LsRefEntry>, Vec<LsRefEntry>)> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
    refs_url.push_str(&format!("service={SERVICE}"));

    let body = http_get(&refs_url)?;
    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;
    let caps = read_v2_caps(pkt_body)?;

    let object_format = caps
        .iter()
        .find_map(|c| c.strip_prefix("object-format="))
        .unwrap_or("sha1");

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
        let odb = Odb::new(&local_git_dir.join("objects"));
        let mut reader: &[u8] = pack_buf;
        unpack_objects(&mut reader, &odb, &UnpackOptions::default())?;
        Ok(())
    };

    if !pending_haves.is_empty() {
        let req = write_fetch_request(false)?;
        let resp = http_post(
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
