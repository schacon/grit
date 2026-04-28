//! Transport-neutral smart Git protocol helpers.
//!
//! These helpers build request bodies and parse response bodies. HTTP, SSH,
//! browser `fetch`, and other transports remain responsible for moving bytes.

use std::collections::HashSet;
use std::io::Cursor;

use crate::error::{Error, Result};
use crate::objects::ObjectId;
use crate::pkt_line::{self, Packet};

/// A single advertised reference from a smart protocol response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvertisedRef {
    /// Fully qualified ref name, or `HEAD`.
    pub name: String,
    /// Object ID currently stored at the reference.
    pub oid: ObjectId,
    /// Symbolic ref target advertised by protocol-v2 `ls-refs`, if present.
    pub symref_target: Option<String>,
}

/// Parsed upload-pack discovery response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadPackAdvertisement {
    /// Protocol v2 capability advertisement.
    V2 {
        /// Capability lines, including `version 2`.
        caps: Vec<String>,
        /// Negotiated object format, usually `sha1`.
        object_format: String,
    },
    /// Protocol v0/v1 ref advertisement.
    V0V1 {
        /// Protocol version observed (`0` or `1`).
        protocol_version: u8,
        /// Advertised refs.
        refs: Vec<AdvertisedRef>,
        /// Capability strings from the first advertised ref.
        capabilities: HashSet<String>,
        /// Negotiated object format, usually `sha1`.
        object_format: String,
    },
}

/// Parsed receive-pack discovery response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivePackAdvertisement {
    /// Protocol version observed (`0`, `1`, or `2`).
    pub protocol_version: u8,
    /// Advertised refs. Empty for protocol-v2 capability advertisements.
    pub refs: Vec<AdvertisedRef>,
    /// Capability strings.
    pub capabilities: HashSet<String>,
    /// Negotiated object format, usually `sha1`.
    pub object_format: String,
}

impl ReceivePackAdvertisement {
    /// Return true when an exact capability or key-value capability is advertised.
    #[must_use]
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c == capability || c.starts_with(&format!("{capability}=")))
    }

    /// Return the advertised object ID for `refname`, if present.
    #[must_use]
    pub fn advertised_oid(&self, refname: &str) -> Option<ObjectId> {
        self.refs
            .iter()
            .find(|advertised| advertised.name == refname)
            .map(|advertised| advertised.oid)
    }
}

/// Strip a smart-HTTP `# service=...` advertisement prefix when present.
///
/// Smart HTTP v0/v1 discovery may start with a service header followed by a
/// flush packet. Protocol v2 responses start directly with `version 2`.
pub fn strip_http_service_advertisement_if_present(body: &[u8]) -> Result<&[u8]> {
    let mut cur = Cursor::new(body);
    let first = match pkt_line::read_packet(&mut cur)? {
        None => return Ok(body),
        Some(Packet::Data(line)) => line,
        Some(Packet::Flush) => return Ok(body),
        Some(other) => {
            return Err(Error::CorruptObject(format!(
                "unexpected first smart-http packet: {other:?}"
            )));
        }
    };
    if first.starts_with("# service=") {
        loop {
            match pkt_line::read_packet(&mut cur)? {
                None => {
                    return Err(Error::CorruptObject(
                        "unexpected EOF in smart-http service advertisement".to_owned(),
                    ));
                }
                Some(Packet::Flush) => {
                    let pos = cur.position() as usize;
                    return Ok(&body[pos..]);
                }
                Some(Packet::Data(_)) => {}
                Some(other) => {
                    return Err(Error::CorruptObject(format!(
                        "unexpected packet in smart-http service block: {other:?}"
                    )));
                }
            }
        }
    }
    Ok(body)
}

/// Parse an upload-pack smart protocol discovery body after any HTTP service
/// advertisement prefix has been stripped.
pub fn parse_upload_pack_advertisement(body: &[u8]) -> Result<UploadPackAdvertisement> {
    let mut cur = Cursor::new(body);
    let first = match pkt_line::read_packet(&mut cur)? {
        None => {
            return Err(Error::CorruptObject(
                "empty smart protocol advertisement".to_owned(),
            ));
        }
        Some(Packet::Data(line)) => line,
        Some(other) => {
            return Err(Error::CorruptObject(format!(
                "unexpected first advertisement packet: {other:?}"
            )));
        }
    };
    if first == "version 2" {
        let caps = read_v2_caps_from_first(first, &mut cur)?;
        let object_format = object_format_from_caps(caps.iter().map(String::as_str));
        return Ok(UploadPackAdvertisement::V2 {
            caps,
            object_format,
        });
    }

    let (refs, capabilities) = parse_v0_v1_advertisement_from_first(first.clone(), &mut cur)?;
    let protocol_version = if first == "version 1" { 1 } else { 0 };
    let object_format = object_format_from_caps(capabilities.iter().map(String::as_str));
    Ok(UploadPackAdvertisement::V0V1 {
        protocol_version,
        refs,
        capabilities,
        object_format,
    })
}

/// Parse a receive-pack smart protocol discovery body after any HTTP service
/// advertisement prefix has been stripped.
pub fn parse_receive_pack_advertisement(body: &[u8]) -> Result<ReceivePackAdvertisement> {
    let mut cur = Cursor::new(body);
    let first = match pkt_line::read_packet(&mut cur)? {
        None => {
            return Err(Error::CorruptObject(
                "empty receive-pack advertisement".to_owned(),
            ));
        }
        Some(Packet::Data(line)) => line,
        Some(other) => {
            return Err(Error::CorruptObject(format!(
                "unexpected first receive-pack advertisement packet: {other:?}"
            )));
        }
    };
    if first == "version 2" {
        let caps = read_v2_caps_from_first(first, &mut cur)?;
        let object_format = object_format_from_caps(caps.iter().map(String::as_str));
        return Ok(ReceivePackAdvertisement {
            protocol_version: 2,
            refs: Vec::new(),
            capabilities: caps.into_iter().collect(),
            object_format,
        });
    }

    let (refs, capabilities) = parse_v0_v1_advertisement_from_first(first.clone(), &mut cur)?;
    let protocol_version = if first == "version 1" { 1 } else { 0 };
    let object_format = object_format_from_caps(capabilities.iter().map(String::as_str));
    Ok(ReceivePackAdvertisement {
        protocol_version,
        refs,
        capabilities,
        object_format,
    })
}

/// Parse a protocol-v2 `ls-refs` response body.
pub fn parse_ls_refs_v2_response(data: &[u8]) -> Result<Vec<AdvertisedRef>> {
    let mut cur = Cursor::new(data);
    let mut out = Vec::new();
    loop {
        let pkt = match pkt_line::read_packet(&mut cur)? {
            None | Some(Packet::Flush) => break,
            Some(Packet::Data(line)) => line,
            Some(other) => {
                return Err(Error::CorruptObject(format!(
                    "unexpected ls-refs packet: {other:?}"
                )));
            }
        };
        let (oid_hex, rest) = pkt
            .split_once(' ')
            .ok_or_else(|| Error::CorruptObject(format!("bad ls-refs line: {pkt}")))?;
        let oid = ObjectId::from_hex(oid_hex.trim())?;
        let name = rest.split_whitespace().next().unwrap_or(rest).to_string();
        if !name.is_empty() {
            let symref_target = rest
                .split_whitespace()
                .skip(1)
                .find_map(|attr| attr.strip_prefix("symref-target:"))
                .map(ToOwned::to_owned);
            out.push(AdvertisedRef {
                name,
                oid,
                symref_target,
            });
        }
    }
    Ok(out)
}

fn read_v2_caps_from_first(first: String, cur: &mut Cursor<&[u8]>) -> Result<Vec<String>> {
    if first != "version 2" {
        return Err(Error::CorruptObject(format!(
            "expected 'version 2', got {first:?}"
        )));
    }
    let mut caps = vec![first];
    loop {
        match pkt_line::read_packet(cur)? {
            None => {
                return Err(Error::CorruptObject(
                    "unexpected EOF in v2 capabilities".to_owned(),
                ));
            }
            Some(Packet::Flush) => break,
            Some(Packet::Data(line)) => caps.push(line),
            Some(other) => {
                return Err(Error::CorruptObject(format!(
                    "unexpected packet in v2 capabilities: {other:?}"
                )));
            }
        }
    }
    Ok(caps)
}

fn parse_v0_v1_advertisement_from_first(
    first: String,
    cur: &mut Cursor<&[u8]>,
) -> Result<(Vec<AdvertisedRef>, HashSet<String>)> {
    let mut refs = Vec::new();
    let mut capabilities = HashSet::new();
    let mut first_ref_line = true;
    let mut pending = Some(first);
    loop {
        let line = match pending.take() {
            Some(line) => line,
            None => match pkt_line::read_packet(cur)? {
                None | Some(Packet::Flush) => break,
                Some(Packet::Data(line)) => line,
                Some(other) => {
                    return Err(Error::CorruptObject(format!(
                        "unexpected packet in v0/v1 advertisement: {other:?}"
                    )));
                }
            },
        };
        let line = line.trim_end_matches('\n');
        if line.starts_with("version ") {
            continue;
        }
        let (payload, cap_part) = match line.split_once('\0') {
            Some((payload, caps)) => (payload.trim(), Some(caps)),
            None => (line.trim(), None),
        };
        let (oid_hex, refname) = payload
            .split_once('\t')
            .or_else(|| payload.split_once(' '))
            .ok_or_else(|| {
                Error::CorruptObject(format!("malformed v0/v1 advertisement: {line}"))
            })?;
        let oid = ObjectId::from_hex(oid_hex.trim())?;
        let refname = refname.trim();
        if first_ref_line {
            if let Some(raw_caps) = cap_part {
                for cap in raw_caps.split_whitespace() {
                    capabilities.insert(cap.to_string());
                }
            }
            first_ref_line = false;
        }
        if !refname.is_empty() {
            refs.push(AdvertisedRef {
                name: refname.to_string(),
                oid,
                symref_target: None,
            });
        }
    }
    Ok((refs, capabilities))
}

fn object_format_from_caps<'a>(caps: impl Iterator<Item = &'a str>) -> String {
    caps.filter_map(|cap| cap.strip_prefix("object-format="))
        .next()
        .unwrap_or("sha1")
        .to_string()
}

/// Return capability lines that a client should echo before the command delimiter.
#[must_use]
pub fn capability_lines_for_client_request(caps: &[String]) -> Vec<String> {
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

/// Extract feature names from a protocol-v2 `fetch=` capability.
#[must_use]
pub fn fetch_features_from_caps(caps: &[String]) -> HashSet<String> {
    let mut features = HashSet::new();
    for line in caps {
        if let Some(rest) = line.strip_prefix("fetch=") {
            for feature in rest.split_whitespace() {
                features.insert(feature.to_string());
            }
        }
    }
    features
}

/// Options for protocol-v2 upload-pack `fetch` requests.
#[derive(Clone, Debug, Default)]
pub struct FetchRequestOptions {
    /// Object format to request, usually `sha1`.
    pub object_format: String,
    /// Capabilities to echo before the delimiter, such as `agent=...`.
    pub capability_lines: Vec<String>,
    /// Object IDs requested with `want`.
    pub wants: Vec<ObjectId>,
    /// Local object IDs sent as `have`.
    pub haves: Vec<ObjectId>,
    /// Whether to include `done`.
    pub include_done: bool,
    /// Absolute depth requested by `deepen`.
    pub depth: Option<usize>,
    /// Date boundary requested by `deepen-since`.
    pub shallow_since: Option<String>,
    /// Exclusion revisions requested by `deepen-not`.
    pub shallow_exclude: Vec<String>,
    /// Partial clone filter specification, such as `blob:none`.
    pub filter_spec: Option<String>,
}

/// Build a protocol-v2 `ls-refs` request body.
///
/// `capability_lines` are written before the delimiter. `object_format`
/// defaults to `sha1` when empty.
pub fn build_ls_refs_v2_request(
    object_format: &str,
    capability_lines: &[String],
    peel: bool,
    symrefs: bool,
) -> Result<Vec<u8>> {
    let object_format = if object_format.trim().is_empty() {
        "sha1"
    } else {
        object_format.trim()
    };
    let mut req = Vec::new();
    pkt_line::write_line_to_vec(&mut req, "command=ls-refs")?;
    pkt_line::write_line_to_vec(&mut req, &format!("object-format={object_format}"))?;
    for line in capability_lines {
        if !line.trim().is_empty() {
            pkt_line::write_line_to_vec(&mut req, line)?;
        }
    }
    pkt_line::write_delim(&mut req)?;
    if peel {
        pkt_line::write_line_to_vec(&mut req, "peel")?;
    }
    if symrefs {
        pkt_line::write_line_to_vec(&mut req, "symrefs")?;
    }
    pkt_line::write_flush(&mut req)?;
    Ok(req)
}

/// Build a protocol-v2 upload-pack `fetch` request body.
///
/// `server_fetch_features` should contain feature names advertised in the
/// server's `fetch=` capability, for example `filter` or `shallow`.
pub fn build_fetch_v2_request(
    server_fetch_features: &HashSet<String>,
    options: &FetchRequestOptions,
) -> Result<Vec<u8>> {
    if options.wants.is_empty() {
        return Err(Error::Message(
            "cannot build fetch request without wants".to_owned(),
        ));
    }
    let object_format = if options.object_format.trim().is_empty() {
        "sha1"
    } else {
        options.object_format.trim()
    };
    let mut req = Vec::new();
    pkt_line::write_line_to_vec(&mut req, "command=fetch")?;
    pkt_line::write_line_to_vec(&mut req, &format!("object-format={object_format}"))?;
    for line in &options.capability_lines {
        if !line.trim().is_empty() {
            pkt_line::write_line_to_vec(&mut req, line)?;
        }
    }
    pkt_line::write_delim(&mut req)?;
    for want in &options.wants {
        pkt_line::write_line_to_vec(&mut req, &format!("want {}", want.to_hex()))?;
    }
    append_fetch_extensions(&mut req, server_fetch_features, options)?;
    for have in &options.haves {
        pkt_line::write_line_to_vec(&mut req, &format!("have {}", have.to_hex()))?;
    }
    if options.include_done {
        pkt_line::write_line_to_vec(&mut req, "done")?;
    }
    pkt_line::write_flush(&mut req)?;
    Ok(req)
}

/// Extract pack bytes from a protocol-v2 upload-pack `fetch` response.
///
/// The response may include acknowledgement sections before the `packfile`
/// marker. Once `packfile` is found, the remainder is decoded as sideband and
/// channel 1 is returned.
pub fn extract_packfile_from_fetch_response(data: &[u8]) -> Result<Vec<u8>> {
    let mut cur = Cursor::new(data);
    loop {
        let pkt = match pkt_line::read_packet(&mut cur)? {
            None | Some(Packet::Flush) => break,
            Some(Packet::Delim | Packet::ResponseEnd) => continue,
            Some(Packet::Data(line)) => line,
        };
        if pkt == "packfile" {
            let pos = cur.position() as usize;
            let pack = pkt_line::decode_sideband_primary(&data[pos..])?;
            if pack.is_empty() {
                return Err(Error::CorruptObject(
                    "fetch response did not contain pack data".to_string(),
                ));
            }
            return Ok(pack);
        }
        skip_section(&mut cur)?;
    }
    Err(Error::CorruptObject(
        "fetch response did not contain a packfile section".to_string(),
    ))
}

fn skip_section(cur: &mut Cursor<&[u8]>) -> Result<()> {
    loop {
        match pkt_line::read_packet(cur)? {
            None | Some(Packet::Flush) => return Ok(()),
            Some(Packet::Data(_)) | Some(Packet::Delim | Packet::ResponseEnd) => {}
        }
    }
}

fn append_fetch_extensions(
    req: &mut Vec<u8>,
    server_fetch_features: &HashSet<String>,
    options: &FetchRequestOptions,
) -> Result<()> {
    if let Some(depth) = options.depth.filter(|depth| *depth > 0) {
        pkt_line::write_line_to_vec(req, &format!("deepen {depth}"))?;
    }
    if let Some(since) = options.shallow_since.as_deref() {
        if server_fetch_features.contains("deepen-since")
            || server_fetch_features.contains("shallow")
        {
            pkt_line::write_line_to_vec(req, &format!("deepen-since {since}"))?;
        }
    }
    if server_fetch_features.contains("deepen-not") || server_fetch_features.contains("shallow") {
        for excl in &options.shallow_exclude {
            let excl = excl.trim();
            if !excl.is_empty() {
                pkt_line::write_line_to_vec(req, &format!("deepen-not {excl}"))?;
            }
        }
    }
    if server_fetch_features.contains("filter") {
        if let Some(filter_spec) = options.filter_spec.as_deref() {
            let filter_spec = filter_spec.trim();
            if !filter_spec.is_empty() {
                pkt_line::write_line_to_vec(req, &format!("filter {filter_spec}"))?;
            }
        }
    }
    Ok(())
}

/// One reference update command sent to `git-receive-pack`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushCommand {
    /// Current old value expected on the remote (`None` means all-zero object id).
    pub old_oid: Option<ObjectId>,
    /// New value to update (`None` means delete).
    pub new_oid: Option<ObjectId>,
    /// Fully qualified destination reference name.
    pub refname: String,
}

/// Receive-pack capabilities used by the client request builder.
#[derive(Clone, Debug, Default)]
pub struct ReceivePackCapabilities {
    /// Raw advertised capability strings.
    pub advertised: HashSet<String>,
    /// Agent value to send when the remote advertises `agent`.
    pub agent: Option<String>,
    /// Session ID value to send when the remote advertises `session-id`.
    pub session_id: Option<String>,
}

impl ReceivePackCapabilities {
    /// Return whether an exact capability or key-value capability is advertised.
    #[must_use]
    pub fn supports(&self, capability: &str) -> bool {
        self.advertised
            .iter()
            .any(|c| c == capability || c.starts_with(&format!("{capability}=")))
    }
}

/// One per-ref status line returned by the remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushStatusEntry {
    /// Updated reference.
    pub refname: String,
    /// Whether the update succeeded.
    pub ok: bool,
    /// Optional error text for rejected updates.
    pub message: Option<String>,
}

/// Parsed `report-status` response for a push request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushStatusReport {
    /// Whether the remote unpack phase succeeded.
    pub unpack_ok: bool,
    /// Unpack status message returned by remote.
    pub unpack_message: String,
    /// Per-reference status entries.
    pub statuses: Vec<PushStatusEntry>,
    /// Sideband progress/error bytes from remote channels 2 and 3.
    pub sideband_stderr: Vec<u8>,
}

/// Build a `git-receive-pack` request and return `(request_bytes, use_sideband)`.
pub fn build_receive_pack_request(
    capabilities: &ReceivePackCapabilities,
    commands: &[PushCommand],
    push_options: &[String],
    pack_data: &[u8],
    atomic: bool,
) -> Result<(Vec<u8>, bool)> {
    if commands.is_empty() {
        return Err(Error::Message(
            "cannot push without update commands".to_owned(),
        ));
    }
    let caps = client_push_capabilities(capabilities, atomic, push_options)?;
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

    let use_sideband = caps
        .iter()
        .any(|cap| cap == "side-band-64k" || cap == "side-band");
    Ok((request, use_sideband))
}

/// Parse a receive-pack response body into a status report.
pub fn parse_receive_pack_response(
    response: &[u8],
    use_sideband: bool,
) -> Result<PushStatusReport> {
    let (primary, sideband_stderr) = if use_sideband {
        decode_sideband_stream(response)?
    } else {
        (response.to_vec(), Vec::new())
    };
    let mut status = parse_report_status_body(&primary)?;
    status.sideband_stderr = sideband_stderr;
    Ok(status)
}

fn client_push_capabilities(
    capabilities: &ReceivePackCapabilities,
    atomic: bool,
    push_options: &[String],
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if capabilities.supports("report-status-v2") {
        out.push("report-status-v2".to_string());
    } else if capabilities.supports("report-status") {
        out.push("report-status".to_string());
    } else {
        return Err(Error::Message(
            "remote does not support report-status".to_owned(),
        ));
    }
    if capabilities.supports("ofs-delta") {
        out.push("ofs-delta".to_string());
    }
    if capabilities.supports("side-band-64k") {
        out.push("side-band-64k".to_string());
    } else if capabilities.supports("side-band") {
        out.push("side-band".to_string());
    }
    if atomic {
        if !capabilities.supports("atomic") {
            return Err(Error::Message(
                "the receiving end does not support --atomic push".to_owned(),
            ));
        }
        out.push("atomic".to_string());
    }
    if !push_options.is_empty() {
        if !capabilities.supports("push-options") {
            return Err(Error::Message(
                "the receiving end does not support push options".to_owned(),
            ));
        }
        out.push("push-options".to_string());
    }
    if capabilities.supports("agent") {
        if let Some(agent) = capabilities.agent.as_deref() {
            out.push(format!("agent={agent}"));
        }
    }
    if capabilities.supports("object-format") {
        out.push("object-format=sha1".to_string());
    }
    if capabilities.supports("session-id") {
        if let Some(session_id) = capabilities.session_id.as_deref() {
            out.push(format!("session-id={session_id}"));
        }
    }
    Ok(out)
}

fn format_push_old_new(oid: Option<ObjectId>) -> String {
    oid.map(|oid| oid.to_hex())
        .unwrap_or_else(|| "0".repeat(40))
}

fn decode_sideband_stream(body: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut i = 0usize;
    let mut primary = Vec::new();
    let mut stderr = Vec::new();
    while i + 4 <= body.len() {
        let len_str = std::str::from_utf8(&body[i..i + 4])
            .map_err(|e| Error::CorruptObject(format!("invalid sideband length: {e}")))?;
        let pkt_len = usize::from_str_radix(len_str, 16)
            .map_err(|e| Error::CorruptObject(format!("invalid sideband length: {e}")))?;
        i += 4;
        if pkt_len == 0 {
            break;
        }
        if pkt_len < 5 || i + (pkt_len - 4) > body.len() {
            return Err(Error::CorruptObject(
                "truncated sideband packet in push response".to_owned(),
            ));
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
        Some(Packet::Data(line)) => line,
        Some(other) => {
            return Err(Error::CorruptObject(format!(
                "unexpected first report-status packet: {other:?}"
            )));
        }
        None => {
            return Err(Error::CorruptObject(
                "empty report-status response".to_owned(),
            ))
        }
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
            Some(Packet::Data(line)) => {
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
                }
            }
            Some(Packet::Flush) | None => break,
            Some(Packet::Delim | Packet::ResponseEnd) => {}
        }
    }

    Ok(PushStatusReport {
        unpack_ok,
        unpack_message,
        statuses,
        sideband_stderr: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(hex: &str) -> ObjectId {
        ObjectId::from_hex(hex).unwrap()
    }

    #[test]
    fn builds_blobless_fetch_request() {
        let features = HashSet::from(["filter".to_string(), "shallow".to_string()]);
        let request = build_fetch_v2_request(
            &features,
            &FetchRequestOptions {
                object_format: "sha1".to_string(),
                wants: vec![oid("67bf698f3ab735e92fb011a99cff3497c44d30c1")],
                include_done: true,
                filter_spec: Some("blob:none".to_string()),
                ..FetchRequestOptions::default()
            },
        )
        .unwrap();
        let text = String::from_utf8_lossy(&request);

        assert!(text.contains("command=fetch"));
        assert!(text.contains("want 67bf698f3ab735e92fb011a99cff3497c44d30c1"));
        assert!(text.contains("filter blob:none"));
        assert!(text.contains("done"));
    }

    #[test]
    fn extracts_packfile_from_fetch_response() {
        let mut response = Vec::new();
        pkt_line::write_line_to_vec(&mut response, "packfile").unwrap();
        pkt_line::write_sideband_channel1_64k(&mut response, b"PACK bytes").unwrap();
        pkt_line::write_flush(&mut response).unwrap();

        let pack = extract_packfile_from_fetch_response(&response).unwrap();

        assert_eq!(pack, b"PACK bytes");
    }

    #[test]
    fn parses_upload_pack_v2_advertisement() {
        let mut body = Vec::new();
        pkt_line::write_line_to_vec(&mut body, "version 2").unwrap();
        pkt_line::write_line_to_vec(&mut body, "agent=git/test").unwrap();
        pkt_line::write_line_to_vec(&mut body, "object-format=sha1").unwrap();
        pkt_line::write_line_to_vec(&mut body, "fetch=shallow filter").unwrap();
        pkt_line::write_flush(&mut body).unwrap();

        let parsed = parse_upload_pack_advertisement(&body).unwrap();

        match parsed {
            UploadPackAdvertisement::V2 {
                caps,
                object_format,
            } => {
                assert_eq!(object_format, "sha1");
                assert!(caps.iter().any(|cap| cap == "fetch=shallow filter"));
            }
            UploadPackAdvertisement::V0V1 { .. } => panic!("expected v2 advertisement"),
        }
    }

    #[test]
    fn strips_http_service_advertisement() {
        let mut body = Vec::new();
        pkt_line::write_line_to_vec(&mut body, "# service=git-upload-pack").unwrap();
        pkt_line::write_flush(&mut body).unwrap();
        pkt_line::write_line_to_vec(&mut body, "version 2").unwrap();
        pkt_line::write_flush(&mut body).unwrap();

        let stripped = strip_http_service_advertisement_if_present(&body).unwrap();

        assert!(String::from_utf8_lossy(stripped).contains("version 2"));
    }

    #[test]
    fn parses_receive_pack_v0_advertisement() {
        let mut body = Vec::new();
        pkt_line::write_packet_raw(
            &mut body,
            b"67bf698f3ab735e92fb011a99cff3497c44d30c1 refs/heads/main\0report-status side-band-64k\n",
        )
        .unwrap();
        pkt_line::write_flush(&mut body).unwrap();

        let parsed = parse_receive_pack_advertisement(&body).unwrap();

        assert_eq!(parsed.protocol_version, 0);
        assert_eq!(
            parsed.advertised_oid("refs/heads/main"),
            Some(oid("67bf698f3ab735e92fb011a99cff3497c44d30c1"))
        );
        assert!(parsed.supports("report-status"));
        assert!(parsed.supports("side-band-64k"));
    }

    #[test]
    fn parses_ls_refs_response() {
        let mut body = Vec::new();
        pkt_line::write_line_to_vec(
            &mut body,
            "67bf698f3ab735e92fb011a99cff3497c44d30c1 HEAD symref-target:refs/heads/main",
        )
        .unwrap();
        pkt_line::write_flush(&mut body).unwrap();

        let refs = parse_ls_refs_v2_response(&body).unwrap();

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "HEAD");
        assert_eq!(refs[0].symref_target.as_deref(), Some("refs/heads/main"));
    }

    #[test]
    fn builds_receive_pack_request_and_parses_status() {
        let caps = ReceivePackCapabilities {
            advertised: HashSet::from([
                "report-status".to_string(),
                "side-band-64k".to_string(),
                "agent".to_string(),
            ]),
            agent: Some("grit-test".to_string()),
            session_id: None,
        };
        let new_oid = oid("67bf698f3ab735e92fb011a99cff3497c44d30c1");
        let (request, use_sideband) = build_receive_pack_request(
            &caps,
            &[PushCommand {
                old_oid: None,
                new_oid: Some(new_oid),
                refname: "refs/heads/main".to_string(),
            }],
            &[],
            b"PACK",
            false,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&request);

        assert!(use_sideband);
        assert!(text.contains("refs/heads/main"));
        assert!(text.contains("report-status side-band-64k agent=grit-test"));
        assert!(request.ends_with(b"PACK"));

        let mut status_body = Vec::new();
        pkt_line::write_line_to_vec(&mut status_body, "unpack ok").unwrap();
        pkt_line::write_line_to_vec(&mut status_body, "ok refs/heads/main").unwrap();
        pkt_line::write_flush(&mut status_body).unwrap();
        let parsed = parse_receive_pack_response(&status_body, false).unwrap();

        assert!(parsed.unpack_ok);
        assert_eq!(parsed.statuses.len(), 1);
        assert!(parsed.statuses[0].ok);
    }

    #[test]
    fn parses_service_wrapped_upload_pack_v2_fixture() {
        let fixture = concat!(
            "001e# service=git-upload-pack\n",
            "0000",
            "000eversion 2\n",
            "0013agent=git/2.44\n",
            "0017object-format=sha1\n",
            "0019fetch=shallow filter\n",
            "0000",
        )
        .as_bytes();

        let stripped = strip_http_service_advertisement_if_present(fixture).unwrap();
        let parsed = parse_upload_pack_advertisement(stripped).unwrap();

        match parsed {
            UploadPackAdvertisement::V2 {
                caps,
                object_format,
            } => {
                assert_eq!(object_format, "sha1");
                assert_eq!(caps[0], "version 2");
                assert!(caps.iter().any(|cap| cap == "fetch=shallow filter"));
            }
            UploadPackAdvertisement::V0V1 { .. } => panic!("expected protocol v2 fixture"),
        }
    }

    #[test]
    fn parses_sidebanded_receive_pack_status_fixture() {
        let mut primary = Vec::new();
        pkt_line::write_line_to_vec(&mut primary, "unpack ok").unwrap();
        pkt_line::write_line_to_vec(&mut primary, "ok refs/heads/main").unwrap();
        pkt_line::write_flush(&mut primary).unwrap();
        let mut sideband = Vec::new();
        pkt_line::write_sideband_channel1_64k(&mut sideband, &primary).unwrap();
        pkt_line::write_flush(&mut sideband).unwrap();

        let parsed = parse_receive_pack_response(&sideband, true).unwrap();

        assert!(parsed.unpack_ok);
        assert_eq!(parsed.statuses.len(), 1);
        assert_eq!(parsed.statuses[0].refname, "refs/heads/main");
        assert!(parsed.statuses[0].ok);
    }
}
