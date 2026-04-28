//! Smart HTTP helpers for `git-receive-pack` discovery and status parsing.
//!
//! This module provides the client-side HTTP helpers needed by native push over
//! `http://` and `https://` remotes.

use std::collections::HashSet;
use std::io::{Read, Write};

use anyhow::{bail, Context, Result};
use grit_lib::objects::ObjectId;
use grit_lib::pkt_line;
use grit_lib::smart_protocol;

use crate::http_bundle_uri::strip_v0_service_advertisement_if_present;

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

    let service_url = format!("{base}/{SERVICE}");
    let parsed = smart_protocol::parse_receive_pack_advertisement(pkt_body)?;
    Ok(ReceivePackAdvertisement {
        protocol_version: parsed.protocol_version,
        refs: parsed
            .refs
            .into_iter()
            .map(|entry| ReceivePackAdvertisedRef {
                name: entry.name,
                oid: entry.oid,
            })
            .collect(),
        capabilities: parsed.capabilities,
        object_format: parsed.object_format,
        service_url,
    })
}

/// Read a `git-receive-pack` advertisement from an already-open smart transport stream.
pub(crate) fn read_receive_pack_advertisement<R: Read>(
    reader: &mut R,
    service_url: String,
) -> Result<ReceivePackAdvertisement> {
    let mut refs = Vec::new();
    let mut caps = HashSet::new();
    let mut first_ref_line = true;
    let mut protocol_version = 0;
    let mut saw_first = false;

    loop {
        match pkt_line::read_packet(reader)? {
            None => bail!("empty receive-pack advertisement"),
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Delim | pkt_line::Packet::ResponseEnd) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                if !saw_first {
                    saw_first = true;
                    if line == "version 2" {
                        protocol_version = 2;
                        caps.insert(line.to_string());
                        loop {
                            match pkt_line::read_packet(reader)? {
                                Some(pkt_line::Packet::Flush) => break,
                                Some(pkt_line::Packet::Data(cap)) => {
                                    caps.insert(cap.trim_end_matches('\n').to_string());
                                }
                                Some(other) => {
                                    bail!("unexpected packet in v2 receive-pack advertisement: {other:?}");
                                }
                                None => bail!("unexpected EOF in v2 receive-pack advertisement"),
                            }
                        }
                        let object_format = caps
                            .iter()
                            .find_map(|c| c.strip_prefix("object-format="))
                            .unwrap_or("sha1")
                            .to_string();
                        return Ok(ReceivePackAdvertisement {
                            protocol_version,
                            refs,
                            capabilities: caps,
                            object_format,
                            service_url,
                        });
                    }
                    if line == "version 1" {
                        protocol_version = 1;
                        continue;
                    }
                }

                let (payload, cap_part) = match line.split_once('\0') {
                    Some((p, c)) => (p.trim(), Some(c)),
                    None => (line.trim(), None),
                };
                let Some((oid_hex, refname)) =
                    payload.split_once('\t').or_else(|| payload.split_once(' '))
                else {
                    continue;
                };
                let oid = ObjectId::from_hex(oid_hex.trim())
                    .with_context(|| format!("bad oid in receive-pack advertisement: {oid_hex}"))?;
                if first_ref_line {
                    if let Some(raw_caps) = cap_part {
                        for cap in raw_caps.split_whitespace() {
                            caps.insert(cap.to_string());
                        }
                    }
                    first_ref_line = false;
                }
                refs.push(ReceivePackAdvertisedRef {
                    name: refname.trim().to_string(),
                    oid,
                });
            }
        }
    }

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

    let (request, use_sideband) =
        build_receive_pack_request(advertised, commands, push_options, pack_data, atomic)?;
    let response = client.post_with_git_protocol(
        &advertised.service_url,
        "application/x-git-receive-pack-request",
        "application/x-git-receive-pack-result",
        &request,
        None,
    )?;

    parse_receive_pack_response(response, use_sideband)
}

/// Send a `git-receive-pack` request over a bidirectional stream (SSH/local smart transport).
pub(crate) fn send_receive_pack_stream<W: Write, R: Read>(
    advertised: &ReceivePackAdvertisement,
    commands: &[PushCommand],
    push_options: &[String],
    pack_data: &[u8],
    atomic: bool,
    mut writer: W,
    mut reader: R,
) -> Result<PushStatusReport> {
    if commands.is_empty() {
        bail!("cannot push without update commands");
    }

    let (request, use_sideband) =
        build_receive_pack_request(advertised, commands, push_options, pack_data, atomic)?;
    writer.write_all(&request)?;
    writer.flush()?;
    drop(writer);

    let mut response = Vec::new();
    reader.read_to_end(&mut response)?;
    parse_receive_pack_response(response, use_sideband)
}

fn build_receive_pack_request(
    advertised: &ReceivePackAdvertisement,
    commands: &[PushCommand],
    push_options: &[String],
    pack_data: &[u8],
    atomic: bool,
) -> Result<(Vec<u8>, bool)> {
    let capabilities = smart_protocol::ReceivePackCapabilities {
        advertised: advertised.capabilities.clone(),
        agent: Some(crate::http_smart::agent_header()),
        session_id: Some(crate::trace2_transfer::trace2_session_id_wire_once()),
    };
    let commands = commands
        .iter()
        .map(|cmd| smart_protocol::PushCommand {
            old_oid: cmd.old_oid,
            new_oid: cmd.new_oid,
            refname: cmd.refname.clone(),
        })
        .collect::<Vec<_>>();
    Ok(smart_protocol::build_receive_pack_request(
        &capabilities,
        &commands,
        push_options,
        pack_data,
        atomic,
    )?)
}

fn parse_receive_pack_response(response: Vec<u8>, use_sideband: bool) -> Result<PushStatusReport> {
    let status = smart_protocol::parse_receive_pack_response(&response, use_sideband)?;
    Ok(PushStatusReport {
        unpack_ok: status.unpack_ok,
        unpack_message: status.unpack_message,
        statuses: status
            .statuses
            .into_iter()
            .map(|entry| PushStatusEntry {
                refname: entry.refname,
                ok: entry.ok,
                message: entry.message,
            })
            .collect(),
        sideband_stderr: status.sideband_stderr,
    })
}
