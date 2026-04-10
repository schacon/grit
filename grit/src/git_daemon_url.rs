//! Parse `git://` URLs and connect to a Git daemon (upload-pack).

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::pkt_line;
use crate::protocol_wire;

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

/// Writes the git-daemon `git-upload-pack` request pkt-line (host + optional protocol version).
///
/// Returns the NUL-escaped trace string used for `GIT_TRACE_PACKET` output.
pub fn write_git_daemon_upload_pack_handshake(
    stream_w: &mut impl Write,
    parsed: &GitDaemonUrl,
) -> Result<String> {
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

    Ok(String::from_utf8_lossy(&inner)
        .replace('\0', "\\0")
        .replace('\n', ""))
}

/// Open a TCP connection to `git://` and complete the daemon request line so upload-pack speaks
/// pkt-line on the socket (duplex read/write halves).
///
/// The third return value is the request payload (NULs escaped) for `GIT_TRACE_PACKET` when matching
/// upstream `git fetch` / `git clone` traces.
pub fn connect_git_daemon_upload_pack(url: &str) -> Result<(TcpStream, TcpStream, String)> {
    let parsed = parse_git_url(url)?;
    let addr = format!("{}:{}", parsed.host, parsed.port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve git://{}:{}", parsed.host, parsed.port))?
        .next()
        .with_context(|| format!("no addresses for git://{}:{}", parsed.host, parsed.port))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(30))
        .with_context(|| format!("could not connect to git://{}:{}", parsed.host, parsed.port))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(600)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(600)));

    let mut stream_w = stream
        .try_clone()
        .context("dup git:// socket for simultaneous read/write")?;
    let trace_show = write_git_daemon_upload_pack_handshake(&mut stream_w, &parsed)?;
    Ok((stream_w, stream, trace_show))
}
