//! Smart HTTP helpers for `git-receive-pack` discovery and status parsing.
//!
//! This module provides the client-side HTTP helpers needed by native push over
//! `http://` and `https://` remotes.

use std::collections::HashSet;
use std::io::Cursor;

use anyhow::{bail, Context, Result};
use grit_lib::objects::ObjectId;

use crate::http_bundle_uri::strip_v0_service_advertisement_if_present;
use crate::pkt_line;

const SERVICE: &str = "git-receive-pack";

/// A single reference advertised by `git-receive-pack`.
#[derive(Clone, Debug)]
pub(crate) struct ReceivePackAdvertisedRef {
    /// Fully qualified reference name (for example `refs/heads/main`).
    pub(crate) name: String,
    /// Object id currently stored at the reference.
    pub(crate) oid: ObjectId,
}

/// Parsed smart-HTTP advertisement for `git-receive-pack`.
#[derive(Clone, Debug)]
pub(crate) struct ReceivePackAdvertisement {
    /// Protocol version observed in the advertisement (`0`, `1`, or `2`).
    pub(crate) protocol_version: u8,
    /// Advertised refs (empty for protocol-v2 capability advertisements).
    pub(crate) refs: Vec<ReceivePackAdvertisedRef>,
    /// Capability strings from advertisement.
    pub(crate) capabilities: HashSet<String>,
    /// Negotiated object format (currently expected to be `sha1`).
    pub(crate) object_format: String,
    /// RPC endpoint URL (`<base>/git-receive-pack`).
    pub(crate) service_url: String,
}

impl ReceivePackAdvertisement {
    /// Return true when an exact capability or key-value capability is advertised.
    pub(crate) fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c == capability || c.starts_with(&format!("{capability}=")))
    }

    /// Return the advertised object id for a ref name, if present.
    pub(crate) fn advertised_oid(&self, refname: &str) -> Option<ObjectId> {
        self.refs.iter().find(|r| r.name == refname).map(|r| r.oid)
    }
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
            Some(other) => bail!("unexpected packet in v2 capabilities: {other:?}"),
        }
    }
    Ok(caps)
}

fn parse_v0_v1_advertisement(
    body: &[u8],
) -> Result<(Vec<ReceivePackAdvertisedRef>, HashSet<String>)> {
    let mut cur = Cursor::new(body);
    let mut refs = Vec::new();
    let mut caps = HashSet::new();
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
                    .with_context(|| format!("bad oid in receive-pack advertisement: {oid_hex}"))?;
                let refname = refname.trim();
                if first_ref_line {
                    if let Some(raw_caps) = cap_part {
                        for cap in raw_caps.split_whitespace() {
                            caps.insert(cap.to_string());
                        }
                    }
                    first_ref_line = false;
                }
                refs.push(ReceivePackAdvertisedRef {
                    name: refname.to_string(),
                    oid,
                });
            }
            Some(other) => bail!("unexpected packet in v0/v1 advertisement: {other:?}"),
        }
    }
    Ok((refs, caps))
}

/// Discover `git-receive-pack` refs/capabilities for an HTTP(S) remote URL.
pub(crate) fn discover_receive_pack(
    repo_url: &str,
    client: &crate::http_client::HttpClientContext,
) -> Result<ReceivePackAdvertisement> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
    refs_url.push_str(&format!("service={SERVICE}"));

    let body = client.get(&refs_url)?;
    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;

    let mut probe = Cursor::new(pkt_body);
    let first = match pkt_line::read_packet(&mut probe)? {
        None => bail!("empty smart-http receive-pack advertisement"),
        Some(pkt_line::Packet::Data(s)) => s,
        Some(other) => bail!("unexpected first receive-pack advertisement packet: {other:?}"),
    };

    let service_url = format!("{base}/{SERVICE}");
    if first == "version 2" {
        let caps = read_v2_caps(pkt_body)?;
        let object_format = caps
            .iter()
            .find_map(|c| c.strip_prefix("object-format="))
            .unwrap_or("sha1")
            .to_string();
        return Ok(ReceivePackAdvertisement {
            protocol_version: 2,
            refs: Vec::new(),
            capabilities: caps.into_iter().collect(),
            object_format,
            service_url,
        });
    }

    let (refs, caps) = parse_v0_v1_advertisement(pkt_body)?;
    let protocol_version = if first == "version 1" { 1 } else { 0 };
    let object_format = caps
        .iter()
        .find_map(|c| c.strip_prefix("object-format="))
        .unwrap_or("sha1")
        .to_string();
    Ok(ReceivePackAdvertisement {
        protocol_version,
        refs,
        capabilities: caps,
        object_format,
        service_url,
    })
}

/// One reference update command sent to `git-receive-pack`.
#[derive(Clone, Debug)]
pub(crate) struct PushCommand {
    /// Current old value expected on the remote (`None` means all-zero object id).
    pub(crate) old_oid: Option<ObjectId>,
    /// New value to update (`None` means delete).
    pub(crate) new_oid: Option<ObjectId>,
    /// Fully qualified destination reference name.
    pub(crate) refname: String,
}

/// One per-ref status line returned by the remote.
#[derive(Clone, Debug)]
pub(crate) struct PushStatusEntry {
    /// Updated reference.
    pub(crate) refname: String,
    /// Whether the update succeeded.
    pub(crate) ok: bool,
    /// Optional error text for rejected updates.
    pub(crate) message: Option<String>,
}

/// Parsed `report-status` response for a push request.
#[derive(Clone, Debug)]
pub(crate) struct PushStatusReport {
    /// Whether the remote unpack phase succeeded.
    pub(crate) unpack_ok: bool,
    /// Unpack status message returned by remote.
    pub(crate) unpack_message: String,
    /// Per-reference status entries.
    pub(crate) statuses: Vec<PushStatusEntry>,
    /// Sideband progress/error bytes from remote (channels 2 and 3).
    pub(crate) sideband_stderr: Vec<u8>,
}

fn format_push_old_new(oid: Option<ObjectId>) -> String {
    oid.map(|o| o.to_hex()).unwrap_or_else(|| "0".repeat(40))
}

fn client_push_capabilities(
    advertised: &ReceivePackAdvertisement,
    atomic: bool,
    push_options: &[String],
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if advertised.supports("report-status-v2") {
        out.push("report-status-v2".to_string());
    } else if advertised.supports("report-status") {
        out.push("report-status".to_string());
    } else {
        bail!("remote does not support report-status");
    }
    if advertised.supports("ofs-delta") {
        out.push("ofs-delta".to_string());
    }
    if advertised.supports("side-band-64k") {
        out.push("side-band-64k".to_string());
    } else if advertised.supports("side-band") {
        out.push("side-band".to_string());
    }
    if atomic {
        if !advertised.supports("atomic") {
            bail!("the receiving end does not support --atomic push");
        }
        out.push("atomic".to_string());
    }
    if !push_options.is_empty() {
        if !advertised.supports("push-options") {
            bail!("the receiving end does not support push options");
        }
        out.push("push-options".to_string());
    }
    if advertised.supports("agent") {
        out.push(format!("agent={}", crate::http_smart::agent_header()));
    }
    if advertised.supports("object-format") {
        out.push("object-format=sha1".to_string());
    }
    if advertised.supports("session-id") {
        out.push(format!(
            "session-id={}",
            crate::trace2_transfer::trace2_session_id_wire_once()
        ));
    }
    Ok(out)
}

fn decode_sideband_stream(body: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut i = 0usize;
    let mut primary = Vec::new();
    let mut stderr = Vec::new();
    while i + 4 <= body.len() {
        let len_str = std::str::from_utf8(&body[i..i + 4])
            .with_context(|| format!("invalid sideband length header at offset {i}"))?;
        let pkt_len = usize::from_str_radix(len_str, 16)
            .with_context(|| format!("invalid sideband length value '{len_str}'"))?;
        i += 4;
        if pkt_len == 0 {
            break;
        }
        if pkt_len < 5 || i + (pkt_len - 4) > body.len() {
            bail!("truncated sideband packet in push response");
        }
        let payload = &body[i..i + (pkt_len - 4)];
        i += pkt_len - 4;
        let (band, data) = (payload[0], &payload[1..]);
        match band {
            1 => primary.extend_from_slice(data),
            2 | 3 => stderr.extend_from_slice(data),
            _ => {}
        }
    }
    Ok((primary, stderr))
}

fn parse_report_status_body(body: &[u8]) -> Result<PushStatusReport> {
    let mut cur = Cursor::new(body);
    let unpack_line = match pkt_line::read_packet(&mut cur)? {
        Some(pkt_line::Packet::Data(line)) => line,
        Some(other) => bail!("unexpected first report-status packet: {other:?}"),
        None => bail!("empty report-status response"),
    };
    let unpack_line = unpack_line.trim_end_matches('\n').to_string();
    let unpack_ok = unpack_line == "unpack ok";
    let unpack_message = unpack_line
        .strip_prefix("unpack ")
        .unwrap_or(unpack_line.as_str())
        .to_string();

    let mut statuses = Vec::new();
    loop {
        match pkt_line::read_packet(&mut cur)? {
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                if let Some(rest) = line.strip_prefix("ok ") {
                    statuses.push(PushStatusEntry {
                        refname: rest.trim().to_string(),
                        ok: true,
                        message: None,
                    });
                    continue;
                }
                if let Some(rest) = line.strip_prefix("ng ") {
                    let (refname, message) = rest
                        .split_once(' ')
                        .map(|(r, m)| (r.trim(), Some(m.trim().to_string())))
                        .unwrap_or((rest.trim(), None));
                    statuses.push(PushStatusEntry {
                        refname: refname.to_string(),
                        ok: false,
                        message,
                    });
                    continue;
                }
            }
            Some(pkt_line::Packet::Flush) | None => break,
            Some(pkt_line::Packet::Delim | pkt_line::Packet::ResponseEnd) => {}
        }
    }

    Ok(PushStatusReport {
        unpack_ok,
        unpack_message,
        statuses,
        sideband_stderr: Vec::new(),
    })
}

/// Send a smart-HTTP `git-receive-pack` request and parse `report-status`.
pub(crate) fn send_receive_pack(
    client: &crate::http_client::HttpClientContext,
    advertised: &ReceivePackAdvertisement,
    commands: &[PushCommand],
    push_options: &[String],
    pack_data: &[u8],
    atomic: bool,
) -> Result<PushStatusReport> {
    if commands.is_empty() {
        bail!("cannot push without update commands");
    }

    let caps = client_push_capabilities(advertised, atomic, push_options)?;
    let mut request = Vec::new();
    for (idx, cmd) in commands.iter().enumerate() {
        let old_hex = format_push_old_new(cmd.old_oid);
        let new_hex = format_push_old_new(cmd.new_oid);
        let mut payload = format!("{old_hex} {new_hex} {}", cmd.refname);
        if idx == 0 && !caps.is_empty() {
            payload.push('\0');
            payload.push_str(&caps.join(" "));
        }
        payload.push('\n');
        pkt_line::write_packet_raw(&mut request, payload.as_bytes())?;
    }
    pkt_line::write_flush(&mut request)?;

    if !push_options.is_empty() {
        for opt in push_options {
            pkt_line::write_line_to_vec(&mut request, opt)?;
        }
        pkt_line::write_flush(&mut request)?;
    }

    let delete_only = commands.iter().all(|cmd| cmd.new_oid.is_none());
    if !delete_only {
        request.extend_from_slice(pack_data);
    }

    let response = client.post_with_git_protocol(
        &advertised.service_url,
        "application/x-git-receive-pack-request",
        "application/x-git-receive-pack-result",
        &request,
        None,
    )?;

    let (primary, sideband_stderr) =
        if caps.iter().any(|c| c == "side-band-64k" || c == "side-band") {
            decode_sideband_stream(&response)?
        } else {
            (response, Vec::new())
        };

    let mut status = parse_report_status_body(&primary)?;
    status.sideband_stderr = sideband_stderr;
    Ok(status)
}
