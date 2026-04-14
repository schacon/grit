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
