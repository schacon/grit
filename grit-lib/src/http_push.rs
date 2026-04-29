//! Smart HTTP push support for embedding callers.
//!
//! This module implements a narrow library API for pushing one local branch to an
//! HTTP(S) remote URL without invoking the `grit` binary.

use std::collections::{BTreeSet, HashSet};
use std::io::{Cursor, Read, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use crate::config::ConfigSet;
use crate::error::{Error, Result};
use crate::merge_base::is_ancestor;
use crate::objects::{ObjectId, ObjectKind};
use crate::pkt_line;
use crate::repo::Repository;
use crate::rev_list::{rev_list, RevListOptions};
use crate::rev_parse;

const SERVICE: &str = "git-receive-pack";

/// Options for pushing a single branch over smart HTTP.
#[derive(Debug, Clone)]
pub struct HttpPushOptions {
    /// Local branch name or ref to push.
    pub branch: String,
    /// HTTP(S) repository URL.
    pub url: String,
    /// Remote branch name or ref to update. Defaults to the local branch.
    pub remote_branch: Option<String>,
    /// Allow non-fast-forward updates.
    pub force: bool,
}

/// Summary returned after a successful smart HTTP push.
#[derive(Debug, Clone)]
pub struct HttpPushOutcome {
    /// Fully qualified remote ref updated by the push.
    pub remote_ref: String,
    /// Object ID sent as the new remote value.
    pub new_oid: ObjectId,
    /// Previous remote object ID, if the ref existed.
    pub old_oid: Option<ObjectId>,
    /// True when the remote already had the requested value.
    pub up_to_date: bool,
    /// Sideband progress and error output returned by the remote.
    pub sideband_stderr: Vec<u8>,
}

/// Push one local branch to an HTTP(S) URL using smart `git-receive-pack`.
///
/// # Parameters
///
/// - `repo` - local repository containing the branch and objects to send.
/// - `options` - branch, URL, destination, and force settings.
///
/// # Errors
///
/// Returns an error if the URL is not HTTP(S), the branch cannot be resolved,
/// the remote rejects the update, or network/protocol handling fails.
pub fn push_branch(repo: &Repository, options: &HttpPushOptions) -> Result<HttpPushOutcome> {
    if !options.url.starts_with("http://") && !options.url.starts_with("https://") {
        return Err(Error::Message(
            "http push requires an http:// or https:// URL".to_owned(),
        ));
    }

    let local_ref = normalize_branch_ref(&options.branch);
    let remote_ref =
        normalize_branch_ref(options.remote_branch.as_deref().unwrap_or(&options.branch));
    let local_oid = rev_parse::resolve_revision(repo, &local_ref)?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let client = HttpClient::from_config(&config)?;
    let advertised = discover_receive_pack(&options.url, &client)?;
    if advertised.object_format != "sha1" {
        return Err(Error::Message(format!(
            "unsupported remote object format '{}' for push over HTTP",
            advertised.object_format
        )));
    }
    if advertised.protocol_version == 2 {
        return Err(Error::Message(
            "smart HTTP push over protocol v2 is not implemented yet".to_owned(),
        ));
    }

    let old_oid = advertised.advertised_oid(&remote_ref);
    if old_oid == Some(local_oid) {
        return Ok(HttpPushOutcome {
            remote_ref,
            new_oid: local_oid,
            old_oid,
            up_to_date: true,
            sideband_stderr: Vec::new(),
        });
    }
    if let Some(old) = old_oid {
        if !options.force && !is_ancestor(repo, old, local_oid)? {
            return Err(Error::Message(
                "remote contains work that you do not have locally; use force to update it"
                    .to_owned(),
            ));
        }
    }

    let remote_have: Vec<ObjectId> = advertised.refs.iter().map(|r| r.oid).collect();
    let pack_data = build_push_pack(repo, local_oid, &remote_have)?;
    let command = PushCommand {
        old_oid,
        new_oid: Some(local_oid),
        refname: remote_ref.clone(),
    };
    let status = send_receive_pack(&client, &advertised, &[command], &pack_data)?;
    if !status.unpack_ok {
        return Err(Error::Message(format!(
            "remote unpack failed: {}",
            status.unpack_message
        )));
    }
    if let Some(rejected) = status
        .statuses
        .iter()
        .find(|s| s.refname == remote_ref && !s.ok)
    {
        let message = rejected
            .message
            .clone()
            .unwrap_or_else(|| "remote rejected".to_owned());
        return Err(Error::Message(format!(
            "remote rejected {}: {message}",
            rejected.refname
        )));
    }

    Ok(HttpPushOutcome {
        remote_ref,
        new_oid: local_oid,
        old_oid,
        up_to_date: false,
        sideband_stderr: status.sideband_stderr,
    })
}

fn normalize_branch_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

fn build_push_pack(
    repo: &Repository,
    local_tip: ObjectId,
    remote_have: &[ObjectId],
) -> Result<Vec<u8>> {
    let mut opts = RevListOptions::default();
    opts.objects = true;
    opts.no_object_names = true;
    opts.quiet = true;
    let positives = vec![local_tip.to_hex()];
    let negatives = remote_have.iter().map(ObjectId::to_hex).collect::<Vec<_>>();
    let result = rev_list(repo, &positives, &negatives, &opts)?;

    let mut oids = BTreeSet::new();
    for oid in result.commits {
        oids.insert(oid);
    }
    for (oid, _) in result.objects {
        oids.insert(oid);
    }
    if oids.is_empty() {
        return Ok(empty_packfile_v2_bytes());
    }

    let mut entries = Vec::with_capacity(oids.len());
    for oid in oids {
        let obj = repo.odb.read(&oid)?;
        entries.push(PackEntry {
            kind: obj.kind,
            data: obj.data,
        });
    }
    build_pack(&entries)
}

struct PackEntry {
    kind: ObjectKind,
    data: Vec<u8>,
}

fn pack_type(kind: ObjectKind) -> u8 {
    match kind {
        ObjectKind::Commit => 1,
        ObjectKind::Tree => 2,
        ObjectKind::Blob => 3,
        ObjectKind::Tag => 4,
    }
}

fn encode_pack_header(kind: ObjectKind, size: usize) -> Vec<u8> {
    let mut size = size as u64;
    let mut first = (pack_type(kind) << 4) | ((size as u8) & 0x0f);
    size >>= 4;
    let mut out = Vec::new();
    if size != 0 {
        first |= 0x80;
    }
    out.push(first);
    while size != 0 {
        let mut byte = (size as u8) & 0x7f;
        size >>= 7;
        if size != 0 {
            byte |= 0x80;
        }
        out.push(byte);
    }
    out
}

fn build_pack(entries: &[PackEntry]) -> Result<Vec<u8>> {
    let mut pack = Vec::new();
    pack.extend_from_slice(b"PACK");
    pack.extend_from_slice(&2u32.to_be_bytes());
    pack.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for entry in entries {
        pack.extend_from_slice(&encode_pack_header(entry.kind, entry.data.len()));
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&entry.data).map_err(Error::Io)?;
        let compressed = encoder
            .finish()
            .map_err(|err| Error::Zlib(err.to_string()))?;
        pack.extend_from_slice(&compressed);
    }
    let digest = Sha1::digest(&pack);
    pack.extend_from_slice(&digest);
    Ok(pack)
}

fn empty_packfile_v2_bytes() -> Vec<u8> {
    let mut pack = Vec::new();
    pack.extend_from_slice(b"PACK");
    pack.extend_from_slice(&2u32.to_be_bytes());
    pack.extend_from_slice(&0u32.to_be_bytes());
    let digest = Sha1::digest(&pack);
    pack.extend_from_slice(&digest);
    pack
}

struct HttpClient {
    agent: ureq::Agent,
}

impl HttpClient {
    fn from_config(config: &ConfigSet) -> Result<Self> {
        let ssl_verify = std::env::var("GIT_SSL_NO_VERIFY").ok().is_none_or(|value| {
            value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false")
        });
        let mut builder = ureq::AgentBuilder::new();
        if let Some(proxy) = config
            .get("http.proxy")
            .filter(|value| !value.trim().is_empty())
        {
            let proxy = ureq::Proxy::new(&proxy).map_err(|err| Error::Message(err.to_string()))?;
            builder = builder.proxy(proxy);
        }
        if !ssl_verify {
            return Err(Error::Message(
                "GIT_SSL_NO_VERIFY is not supported by grit-lib http_push".to_owned(),
            ));
        }
        Ok(Self {
            agent: builder.build(),
        })
    }

    fn get(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .agent
            .get(url)
            .set("User-Agent", "git/2.0 (grit-lib)")
            .call()
            .map_err(|err| Error::Message(format!("GET {}: {err}", scrub_url_credentials(url))))?;
        read_response_body(response.into_reader(), "read GET body")
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>> {
        let response = self
            .agent
            .post(url)
            .set("Content-Type", "application/x-git-receive-pack-request")
            .set("Accept", "application/x-git-receive-pack-result")
            .set("User-Agent", "git/2.0 (grit-lib)")
            .send_bytes(body)
            .map_err(|err| Error::Message(format!("POST {}: {err}", scrub_url_credentials(url))))?;
        read_response_body(response.into_reader(), "read POST body")
    }
}

fn read_response_body(mut reader: impl Read, context: &'static str) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    reader
        .read_to_end(&mut body)
        .map_err(|err| Error::Message(format!("{context}: {err}")))?;
    Ok(body)
}

fn scrub_url_credentials(url: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(url) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        return parsed.to_string();
    }
    url.to_owned()
}

#[derive(Clone, Debug)]
struct ReceivePackAdvertisedRef {
    name: String,
    oid: ObjectId,
}

#[derive(Clone, Debug)]
struct ReceivePackAdvertisement {
    protocol_version: u8,
    refs: Vec<ReceivePackAdvertisedRef>,
    capabilities: HashSet<String>,
    object_format: String,
    service_url: String,
}

impl ReceivePackAdvertisement {
    fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c == capability || c.starts_with(&format!("{capability}=")))
    }

    fn advertised_oid(&self, refname: &str) -> Option<ObjectId> {
        self.refs.iter().find(|r| r.name == refname).map(|r| r.oid)
    }
}

fn strip_v0_service_advertisement_if_present(body: &[u8]) -> Result<&[u8]> {
    let mut cur = Cursor::new(body);
    let first = match pkt_line::read_packet(&mut cur).map_err(Error::Io)? {
        Some(pkt_line::Packet::Data(line)) => line,
        _ => return Ok(body),
    };
    if first.trim_end_matches('\n') != "# service=git-receive-pack" {
        return Ok(body);
    }
    match pkt_line::read_packet(&mut cur).map_err(Error::Io)? {
        Some(pkt_line::Packet::Flush) => Ok(&body[cur.position() as usize..]),
        _ => Ok(body),
    }
}

fn discover_receive_pack(repo_url: &str, client: &HttpClient) -> Result<ReceivePackAdvertisement> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
    refs_url.push_str(&format!("service={SERVICE}"));

    let body = client.get(&refs_url)?;
    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;
    let mut probe = Cursor::new(pkt_body);
    let first = match pkt_line::read_packet(&mut probe).map_err(Error::Io)? {
        Some(pkt_line::Packet::Data(line)) => line,
        Some(other) => {
            return Err(Error::Message(format!(
                "unexpected first receive-pack advertisement packet: {other:?}"
            )))
        }
        None => {
            return Err(Error::Message(
                "empty smart-http receive-pack advertisement".to_owned(),
            ))
        }
    };

    let service_url = format!("{base}/{SERVICE}");
    if first == "version 2" {
        return Err(Error::Message(
            "smart HTTP push over protocol v2 is not implemented yet".to_owned(),
        ));
    }

    let (refs, capabilities) = parse_v0_v1_advertisement(pkt_body)?;
    let object_format = capabilities
        .iter()
        .find_map(|c| c.strip_prefix("object-format="))
        .unwrap_or("sha1")
        .to_string();
    Ok(ReceivePackAdvertisement {
        protocol_version: if first == "version 1" { 1 } else { 0 },
        refs,
        capabilities,
        object_format,
        service_url,
    })
}

fn parse_v0_v1_advertisement(
    body: &[u8],
) -> Result<(Vec<ReceivePackAdvertisedRef>, HashSet<String>)> {
    let mut cur = Cursor::new(body);
    let mut refs = Vec::new();
    let mut caps = HashSet::new();
    let mut first_ref_line = true;
    loop {
        match pkt_line::read_packet(&mut cur).map_err(Error::Io)? {
            None | Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                if line.starts_with("version ") || line.starts_with("shallow ") {
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
                        Error::Message(format!("malformed receive-pack advertisement: {line}"))
                    })?;
                let oid = ObjectId::from_hex(oid_hex.trim())?;
                if first_ref_line {
                    if let Some(raw_caps) = cap_part {
                        for cap in raw_caps.split_whitespace() {
                            caps.insert(cap.to_string());
                        }
                    }
                    first_ref_line = false;
                }
                let refname = refname.trim();
                if !(oid.is_zero() && refname == "capabilities^{}") {
                    refs.push(ReceivePackAdvertisedRef {
                        name: refname.to_string(),
                        oid,
                    });
                }
            }
            Some(other) => {
                return Err(Error::Message(format!(
                    "unexpected packet in receive-pack advertisement: {other:?}"
                )))
            }
        }
    }
    Ok((refs, caps))
}

struct PushCommand {
    old_oid: Option<ObjectId>,
    new_oid: Option<ObjectId>,
    refname: String,
}

struct PushStatusEntry {
    refname: String,
    ok: bool,
    message: Option<String>,
}

struct PushStatusReport {
    unpack_ok: bool,
    unpack_message: String,
    statuses: Vec<PushStatusEntry>,
    sideband_stderr: Vec<u8>,
}

fn format_push_old_new(oid: Option<ObjectId>) -> String {
    oid.map(|oid| oid.to_hex())
        .unwrap_or_else(|| "0".repeat(40))
}

fn client_push_capabilities(advertised: &ReceivePackAdvertisement) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if advertised.supports("report-status-v2") {
        out.push("report-status-v2".to_owned());
    } else if advertised.supports("report-status") {
        out.push("report-status".to_owned());
    } else {
        return Err(Error::Message(
            "remote does not support report-status".to_owned(),
        ));
    }
    if advertised.supports("ofs-delta") {
        out.push("ofs-delta".to_owned());
    }
    if advertised.supports("side-band-64k") {
        out.push("side-band-64k".to_owned());
    } else if advertised.supports("side-band") {
        out.push("side-band".to_owned());
    }
    if advertised.supports("agent") {
        out.push("agent=git/2.0 (grit-lib)".to_owned());
    }
    if advertised.supports("object-format") {
        out.push("object-format=sha1".to_owned());
    }
    Ok(out)
}

fn send_receive_pack(
    client: &HttpClient,
    advertised: &ReceivePackAdvertisement,
    commands: &[PushCommand],
    pack_data: &[u8],
) -> Result<PushStatusReport> {
    let (request, use_sideband) = build_receive_pack_request(advertised, commands, pack_data)?;
    let response = client.post(&advertised.service_url, &request)?;
    parse_receive_pack_response(response, use_sideband)
}

fn build_receive_pack_request(
    advertised: &ReceivePackAdvertisement,
    commands: &[PushCommand],
    pack_data: &[u8],
) -> Result<(Vec<u8>, bool)> {
    let caps = client_push_capabilities(advertised)?;
    let mut request = Vec::new();
    for (idx, command) in commands.iter().enumerate() {
        let old_hex = format_push_old_new(command.old_oid);
        let new_hex = format_push_old_new(command.new_oid);
        let mut payload = format!("{old_hex} {new_hex} {}", command.refname);
        if idx == 0 && !caps.is_empty() {
            payload.push('\0');
            payload.push_str(&caps.join(" "));
        }
        payload.push('\n');
        pkt_line::write_packet_raw(&mut request, payload.as_bytes()).map_err(Error::Io)?;
    }
    pkt_line::write_flush(&mut request).map_err(Error::Io)?;

    if commands.iter().any(|command| command.new_oid.is_some()) {
        request.extend_from_slice(pack_data);
    }
    let use_sideband = caps
        .iter()
        .any(|cap| cap == "side-band-64k" || cap == "side-band");
    Ok((request, use_sideband))
}

fn parse_receive_pack_response(response: Vec<u8>, use_sideband: bool) -> Result<PushStatusReport> {
    let (primary, sideband_stderr) = if use_sideband {
        decode_sideband_stream(&response)?
    } else {
        (response, Vec::new())
    };
    let mut status = parse_report_status_body(&primary)?;
    status.sideband_stderr = sideband_stderr;
    Ok(status)
}

fn decode_sideband_stream(body: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut pos = 0usize;
    let mut primary = Vec::new();
    let mut stderr = Vec::new();
    while pos + 4 <= body.len() {
        let len_str = std::str::from_utf8(&body[pos..pos + 4])
            .map_err(|err| Error::Message(format!("invalid sideband header: {err}")))?;
        let pkt_len = usize::from_str_radix(len_str, 16)
            .map_err(|err| Error::Message(format!("invalid sideband length: {err}")))?;
        pos += 4;
        if pkt_len == 0 {
            break;
        }
        if pkt_len < 5 || pos + (pkt_len - 4) > body.len() {
            return Err(Error::Message(
                "truncated sideband packet in push response".to_owned(),
            ));
        }
        let payload = &body[pos..pos + (pkt_len - 4)];
        pos += pkt_len - 4;
        match payload[0] {
            1 => primary.extend_from_slice(&payload[1..]),
            2 | 3 => stderr.extend_from_slice(&payload[1..]),
            _ => {}
        }
    }
    Ok((primary, stderr))
}

fn parse_report_status_body(body: &[u8]) -> Result<PushStatusReport> {
    let mut cur = Cursor::new(body);
    let unpack_line = match pkt_line::read_packet(&mut cur).map_err(Error::Io)? {
        Some(pkt_line::Packet::Data(line)) => line.trim_end_matches('\n').to_string(),
        Some(other) => {
            return Err(Error::Message(format!(
                "unexpected first report-status packet: {other:?}"
            )))
        }
        None => return Err(Error::Message("empty report-status response".to_owned())),
    };
    let unpack_ok = unpack_line == "unpack ok";
    let unpack_message = unpack_line
        .strip_prefix("unpack ")
        .unwrap_or(&unpack_line)
        .to_string();

    let mut statuses = Vec::new();
    loop {
        match pkt_line::read_packet(&mut cur).map_err(Error::Io)? {
            Some(pkt_line::Packet::Data(line)) => {
                let line = line.trim_end_matches('\n');
                if let Some(rest) = line.strip_prefix("ok ") {
                    statuses.push(PushStatusEntry {
                        refname: rest.trim().to_string(),
                        ok: true,
                        message: None,
                    });
                } else if let Some(rest) = line.strip_prefix("ng ") {
                    let (refname, message) = rest
                        .split_once(' ')
                        .map(|(refname, message)| {
                            (refname.trim(), Some(message.trim().to_string()))
                        })
                        .unwrap_or((rest.trim(), None));
                    statuses.push(PushStatusEntry {
                        refname: refname.to_string(),
                        ok: false,
                        message,
                    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::{serialize_commit, CommitData};
    use crate::refs;
    use crate::repo::init_repository;
    use crate::unpack_objects::{unpack_objects, UnpackOptions};
    use crate::write_tree::write_tree_from_index;

    fn make_commit(repo: &Repository) -> Result<ObjectId> {
        use crate::index::{Index, IndexEntry, MODE_REGULAR};

        let blob_oid = repo.odb.write(ObjectKind::Blob, b"push me\n")?;
        let path = b"file.txt".to_vec();
        let entry = IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode: MODE_REGULAR,
            uid: 0,
            gid: 0,
            size: 0,
            oid: blob_oid,
            flags: path.len().min(0xfff) as u16,
            flags_extended: None,
            path,
            base_index_pos: 0,
        };
        let mut index = Index::new();
        index.add_or_replace(entry);
        repo.write_index(&mut index)?;
        let index = repo.load_index()?;
        let tree = write_tree_from_index(&repo.odb, &index, "")?;
        let commit = CommitData {
            tree,
            parents: Vec::new(),
            author: "Example <example@example.com> 1700000000 +0000".to_owned(),
            committer: "Example <example@example.com> 1700000000 +0000".to_owned(),
            author_raw: Vec::new(),
            committer_raw: Vec::new(),
            encoding: None,
            message: "pushable commit\n".to_owned(),
            raw_message: None,
        };
        let oid = repo
            .odb
            .write(ObjectKind::Commit, &serialize_commit(&commit))?;
        refs::write_ref(&repo.git_dir, "refs/heads/main", &oid)?;
        Ok(oid)
    }

    #[test]
    fn build_receive_pack_request_formats_update_and_pack() -> Result<()> {
        let advertised = ReceivePackAdvertisement {
            protocol_version: 0,
            refs: Vec::new(),
            capabilities: ["report-status".to_owned(), "side-band-64k".to_owned()]
                .into_iter()
                .collect(),
            object_format: "sha1".to_owned(),
            service_url: "https://example.test/repo.git/git-receive-pack".to_owned(),
        };
        let new_oid = ObjectId::from_hex("1111111111111111111111111111111111111111")?;
        let pack_data = empty_packfile_v2_bytes();
        let command = PushCommand {
            old_oid: None,
            new_oid: Some(new_oid),
            refname: "refs/heads/main".to_owned(),
        };

        let (request, sideband) = build_receive_pack_request(&advertised, &[command], &pack_data)?;

        assert!(sideband);
        let first_packet_len = usize::from_str_radix(
            std::str::from_utf8(&request[..4]).map_err(|err| Error::Message(err.to_string()))?,
            16,
        )
        .map_err(|err| Error::Message(err.to_string()))?;
        let first_packet = std::str::from_utf8(&request[4..first_packet_len])
            .map_err(|err| Error::Message(err.to_string()))?;
        assert!(first_packet.contains("0000000000000000000000000000000000000000"));
        assert!(first_packet.contains("1111111111111111111111111111111111111111"));
        assert!(first_packet.contains(" refs/heads/main\0"));
        assert!(first_packet.contains("report-status"));
        assert!(first_packet.contains("side-band-64k"));
        assert!(request.ends_with(&pack_data));
        Ok(())
    }

    #[test]
    fn build_push_pack_round_trips_with_unpack_objects() -> Result<()> {
        let root = tempfile::tempdir()?;
        let repo = init_repository(root.path(), false, "main", None, "files")?;
        let tip = make_commit(&repo)?;

        let pack = build_push_pack(&repo, tip, &[])?;

        let remote_root = tempfile::tempdir()?;
        let remote = init_repository(remote_root.path(), true, "main", None, "files")?;
        let mut pack_reader = pack.as_slice();
        let count = unpack_objects(&mut pack_reader, &remote.odb, &UnpackOptions::default())?;

        assert!(count >= 3);
        assert!(remote.odb.exists(&tip));
        Ok(())
    }

    #[test]
    fn parse_report_status_reads_remote_results() -> Result<()> {
        let mut body = Vec::new();
        pkt_line::write_line_to_vec(&mut body, "unpack ok")?;
        pkt_line::write_line_to_vec(&mut body, "ok refs/heads/main")?;
        pkt_line::write_flush(&mut body)?;

        let report = parse_report_status_body(&body)?;

        assert!(report.unpack_ok);
        assert_eq!(report.statuses.len(), 1);
        assert!(report.statuses[0].ok);
        assert_eq!(report.statuses[0].refname, "refs/heads/main");
        Ok(())
    }
}
