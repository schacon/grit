//! `grit http-backend` — CGI program for smart HTTP transport.
//!
//! Implements a minimal server side of the Git smart HTTP protocol as a CGI
//! program. It supports the content-length focused POST paths exercised by
//! `t5562-http-backend-content-length.sh`, including optional gzip request
//! decoding and CGI-style status/header output.
//!
//!     grit http-backend

use anyhow::{anyhow, Context, Result};
use clap::Args as ClapArgs;
use flate2::read::GzDecoder;
use std::env;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::grit_exe;
use grit_lib::pkt_line;

/// HTTP smart service endpoint kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Service {
    UploadPack,
    ReceivePack,
}

impl Service {
    fn command_name(self) -> &'static str {
        match self {
            Self::UploadPack => "upload-pack",
            Self::ReceivePack => "receive-pack",
        }
    }

    fn request_content_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-request",
            Self::ReceivePack => "application/x-git-receive-pack-request",
        }
    }

    fn result_content_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-result",
            Self::ReceivePack => "application/x-git-receive-pack-result",
        }
    }

    fn advertisement_content_type(self) -> &'static str {
        match self {
            Self::UploadPack => "application/x-git-upload-pack-advertisement",
            Self::ReceivePack => "application/x-git-receive-pack-advertisement",
        }
    }
}

#[derive(Debug)]
struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

/// Arguments for `grit http-backend`.
#[derive(Debug, ClapArgs)]
#[command(about = "Server side implementation of Git over HTTP")]
pub struct Args {
    /// Stateless RPC mode (for smart HTTP).
    #[arg(long = "stateless-rpc")]
    pub stateless_rpc: bool,
}

/// Run `grit http-backend`.
pub fn run(_args: Args) -> Result<()> {
    let response = match run_inner() {
        Ok(response) => response,
        Err(err) => {
            eprintln!("fatal: {err}");
            HttpResponse {
                status: "500 Internal Server Error",
                content_type: "text/plain",
                body: format!("fatal: {err}\n").into_bytes(),
            }
        }
    };
    write_cgi_response(&response)?;
    Ok(())
}

fn run_inner() -> Result<HttpResponse> {
    let method = env::var("REQUEST_METHOD")
        .unwrap_or_else(|_| "GET".to_owned())
        .trim()
        .to_ascii_uppercase();

    let query = env::var("QUERY_STRING").unwrap_or_default();
    let path_translated = env::var("PATH_TRANSLATED")
        .ok()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("missing PATH_TRANSLATED"))?;
    let service = detect_service()?;

    match method.as_str() {
        "POST" => handle_post(service, &path_translated),
        "GET" => handle_get(service, &query, &path_translated),
        _ => Err(anyhow!("unsupported REQUEST_METHOD: {method}")),
    }
}

fn handle_post(service: Service, path_translated: &Path) -> Result<HttpResponse> {
    let content_type = env::var("CONTENT_TYPE").unwrap_or_default();
    if !content_type
        .to_ascii_lowercase()
        .starts_with(service.request_content_type())
    {
        return Err(anyhow!(
            "unexpected CONTENT_TYPE for {}: {content_type}",
            service.command_name()
        ));
    }

    let content_length = parse_content_length()?;
    let encoded_body = read_request_body(content_length)?;
    if encoded_body.is_empty() {
        return Err(anyhow!("request body is empty"));
    }
    let body = decode_request_body(encoded_body)?;
    validate_post_body(service, &body)?;

    let repo_path = derive_repo_path(path_translated)?;
    let output = run_service_command(service, &repo_path, &body)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            return Err(anyhow!(
                "{} failed with status {}",
                service.command_name(),
                output.status
            ));
        }
        return Err(anyhow!("{stderr}"));
    }

    Ok(HttpResponse {
        status: "200 OK",
        content_type: service.result_content_type(),
        body: output.stdout,
    })
}

fn handle_get(service: Service, query: &str, path_translated: &Path) -> Result<HttpResponse> {
    if !query.contains("service=") {
        return Ok(HttpResponse {
            status: "200 OK",
            content_type: "text/plain",
            body: Vec::new(),
        });
    }

    if service == Service::UploadPack {
        let repo_path = derive_repo_path(path_translated)?;
        let output = run_service_advertisement(service, &repo_path)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                return Err(anyhow!(
                    "{} --advertise-refs failed with status {}",
                    service.command_name(),
                    output.status
                ));
            }
            return Err(anyhow!("{stderr}"));
        }
        return Ok(HttpResponse {
            status: "200 OK",
            content_type: service.advertisement_content_type(),
            body: output.stdout,
        });
    }

    Ok(HttpResponse {
        status: "200 OK",
        content_type: service.advertisement_content_type(),
        body: Vec::new(),
    })
}

fn detect_service() -> Result<Service> {
    let content_type = env::var("CONTENT_TYPE").unwrap_or_default();
    if content_type.contains("git-upload-pack") {
        return Ok(Service::UploadPack);
    }
    if content_type.contains("git-receive-pack") {
        return Ok(Service::ReceivePack);
    }

    let query = env::var("QUERY_STRING").unwrap_or_default();
    if query.contains("git-upload-pack") {
        return Ok(Service::UploadPack);
    }
    if query.contains("git-receive-pack") {
        return Ok(Service::ReceivePack);
    }

    let path = env::var("PATH_TRANSLATED").unwrap_or_default();
    if path.ends_with("/git-upload-pack") {
        return Ok(Service::UploadPack);
    }
    if path.ends_with("/git-receive-pack") {
        return Ok(Service::ReceivePack);
    }

    Err(anyhow!("unable to determine requested smart HTTP service"))
}

fn parse_content_length() -> Result<Option<usize>> {
    let Some(raw) = env::var("CONTENT_LENGTH").ok() else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed
        .parse::<u128>()
        .with_context(|| format!("invalid CONTENT_LENGTH: {trimmed}"))?;
    if parsed > isize::MAX as u128 || parsed > usize::MAX as u128 {
        return Err(anyhow!(
            "invalid CONTENT_LENGTH: {trimmed} does not fit in ssize_t"
        ));
    }
    Ok(Some(parsed as usize))
}

fn read_request_body(content_length: Option<usize>) -> Result<Vec<u8>> {
    let mut stdin = io::stdin().lock();
    match content_length {
        Some(expected) => {
            let mut body = vec![0_u8; expected];
            stdin
                .read_exact(&mut body)
                .with_context(|| format!("failed to read CONTENT_LENGTH bytes ({expected})"))?;
            Ok(body)
        }
        None => {
            let mut body = Vec::new();
            stdin
                .read_to_end(&mut body)
                .context("failed to read request body")?;
            Ok(body)
        }
    }
}

fn decode_request_body(encoded: Vec<u8>) -> Result<Vec<u8>> {
    let encoding = env::var("HTTP_CONTENT_ENCODING")
        .or_else(|_| env::var("CONTENT_ENCODING"))
        .unwrap_or_else(|_| "identity".to_owned())
        .trim()
        .to_ascii_lowercase();

    match encoding.as_str() {
        "" | "identity" => Ok(encoded),
        "gzip" => {
            let mut decoder = GzDecoder::new(encoded.as_slice());
            let mut decoded = Vec::new();
            decoder
                .read_to_end(&mut decoded)
                .context("failed to decode gzip request body")?;
            Ok(decoded)
        }
        other => Err(anyhow!("unsupported HTTP_CONTENT_ENCODING: {other}")),
    }
}

fn validate_post_body(service: Service, body: &[u8]) -> Result<()> {
    match service {
        Service::UploadPack => validate_upload_pack_request(body),
        Service::ReceivePack => validate_receive_pack_request(body),
    }
}

fn validate_upload_pack_request(body: &[u8]) -> Result<()> {
    let mut cursor = Cursor::new(body);
    let mut saw_data = false;
    let mut saw_flush = false;

    loop {
        match pkt_line::read_packet(&mut cursor).context("invalid upload-pack request")? {
            None => break,
            Some(pkt_line::Packet::Flush) => saw_flush = true,
            Some(pkt_line::Packet::Data(_)) => saw_data = true,
            Some(pkt_line::Packet::Delim | pkt_line::Packet::ResponseEnd) => {}
        }
    }

    if !saw_data {
        return Err(anyhow!("upload-pack request is missing pkt-line payload"));
    }
    if !saw_flush {
        return Err(anyhow!("upload-pack request is missing flush packet"));
    }
    Ok(())
}

fn validate_receive_pack_request(body: &[u8]) -> Result<()> {
    let mut cursor = Cursor::new(body);
    let mut saw_update_line = false;
    let mut saw_flush = false;

    loop {
        match pkt_line::read_packet(&mut cursor).context("invalid receive-pack request")? {
            None => break,
            Some(pkt_line::Packet::Data(_)) => saw_update_line = true,
            Some(pkt_line::Packet::Flush) => {
                saw_flush = true;
                break;
            }
            Some(pkt_line::Packet::Delim | pkt_line::Packet::ResponseEnd) => {}
        }
    }

    if !saw_update_line {
        return Err(anyhow!("receive-pack request is missing update line"));
    }
    if !saw_flush {
        return Err(anyhow!("receive-pack request is missing flush packet"));
    }

    let offset = cursor.position() as usize;
    let pack = &body[offset..];
    if pack.len() <= 12 || !pack.starts_with(b"PACK") {
        return Err(anyhow!("receive-pack request is missing PACK payload"));
    }
    Ok(())
}

fn derive_repo_path(path_translated: &Path) -> Result<PathBuf> {
    let Some(file_name) = path_translated.file_name().and_then(|s| s.to_str()) else {
        return Err(anyhow!(
            "unable to derive repository path from PATH_TRANSLATED"
        ));
    };

    if file_name == "git-upload-pack" || file_name == "git-receive-pack" {
        return path_translated
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("PATH_TRANSLATED does not include repository directory"));
    }
    if file_name == "refs" {
        return path_translated
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("PATH_TRANSLATED info/refs is malformed"));
    }

    Ok(path_translated.to_path_buf())
}

fn run_service_command(service: Service, repo_path: &Path, body: &[u8]) -> Result<Output> {
    let mut cmd = Command::new(grit_exe::grit_executable());
    cmd.arg(service.command_name())
        .arg(repo_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", service.command_name()))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(body)
            .with_context(|| format!("failed to write body to {}", service.command_name()))?;
    }
    child
        .wait_with_output()
        .with_context(|| format!("failed to wait for {}", service.command_name()))
}

fn run_service_advertisement(service: Service, repo_path: &Path) -> Result<Output> {
    let mut cmd = Command::new(grit_exe::grit_executable());
    cmd.arg(service.command_name())
        .arg(repo_path)
        .arg("--advertise-refs")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.output()
        .with_context(|| format!("failed to run {} --advertise-refs", service.command_name()))
}

fn write_cgi_response(response: &HttpResponse) -> Result<()> {
    let mut out = io::stdout().lock();
    if response.status != "200 OK" {
        write!(out, "Status: {}\r\n", response.status)?;
    }
    write!(out, "Content-Type: {}\r\n", response.content_type)?;
    write!(out, "Content-Length: {}\r\n", response.body.len())?;
    write!(out, "\r\n")?;
    out.write_all(&response.body)?;
    out.flush()?;
    Ok(())
}
