//! Smart HTTP client for protocol v2 `bundle-uri` (test-tool / harness).

use anyhow::{bail, Context, Result};
use std::io::{Cursor, Read};

use crate::pkt_line;

const SERVICE: &str = "git-upload-pack";

/// Skip the v0 smart-HTTP `# service=git-upload-pack` advertisement (pkt-lines until flush).
/// Protocol v2 responses start with `version 2` and must be returned in full (no leading
/// service block).
fn strip_v0_service_advertisement_if_present(body: &[u8]) -> Result<&[u8]> {
    let mut cur = Cursor::new(body);
    let first = match pkt_line::read_packet(&mut cur).context("read first smart-http pkt-line")? {
        None => return Ok(body),
        Some(pkt_line::Packet::Data(s)) => s,
        Some(pkt_line::Packet::Flush) => return Ok(body),
        Some(other) => bail!("unexpected first smart-http packet: {other:?}"),
    };
    if first.starts_with("# service=") {
        loop {
            match pkt_line::read_packet(&mut cur).context("read smart-http service pkt-line")? {
                None => bail!("unexpected EOF in smart-http service advertisement"),
                Some(pkt_line::Packet::Flush) => {
                    let pos = cur.position() as usize;
                    return Ok(&body[pos..]);
                }
                Some(pkt_line::Packet::Data(_)) => {}
                Some(other) => bail!("unexpected packet in smart-http service block: {other:?}"),
            }
        }
    }
    Ok(body)
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

fn cap_lines_for_bundle_request(caps: &[String]) -> Vec<String> {
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

/// Fetch `bundle.*` key/value lines from a smart HTTP remote (protocol v2).
///
/// `repo_url` is the repository URL (e.g. `http://host/smart/repo`).
pub fn fetch_bundle_uri_lines_http(repo_url: &str) -> Result<Vec<(String, String)>> {
    let base = repo_url.trim_end_matches('/');
    let mut refs_url = format!("{base}/info/refs");
    if base.starts_with("http://") || base.starts_with("https://") {
        refs_url.push_str(if refs_url.contains('?') { "&" } else { "?" });
        refs_url.push_str(&format!("service={SERVICE}"));
    }

    let agent = format!("grit/{}", crate::version_string());
    let resp = ureq::get(&refs_url)
        .set("Git-Protocol", "version=2")
        .set("User-Agent", &agent)
        .call()
        .with_context(|| format!("GET {refs_url}"))?;

    if resp.status() >= 400 {
        bail!(
            "info/refs request failed: HTTP {} {}",
            resp.status(),
            resp.status_text()
        );
    }

    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .context("read info/refs body")?;

    let pkt_body = strip_v0_service_advertisement_if_present(&body)?;
    let caps = read_v2_caps(pkt_body)?;
    if !caps
        .iter()
        .any(|c| c == "bundle-uri" || c.starts_with("bundle-uri="))
    {
        bail!("server does not advertise bundle-uri");
    }

    let cap_send = cap_lines_for_bundle_request(&caps);
    let mut request = Vec::new();
    pkt_line::write_line_to_vec(&mut request, "command=bundle-uri")?;
    for line in &cap_send {
        pkt_line::write_line_to_vec(&mut request, line)?;
    }
    pkt_line::write_delim(&mut request)?;
    pkt_line::write_flush(&mut request)?;

    let post_url = format!("{base}/{SERVICE}");
    let post = ureq::post(&post_url)
        .set("Content-Type", &format!("application/x-{SERVICE}-request"))
        .set("Accept", &format!("application/x-{SERVICE}-result"))
        .set("Git-Protocol", "version=2")
        .set("User-Agent", &agent)
        .send_bytes(&request)
        .with_context(|| format!("POST {post_url}"))?;

    if post.status() >= 400 {
        bail!(
            "upload-pack POST failed: HTTP {} {}",
            post.status(),
            post.status_text()
        );
    }

    let mut out_body = Vec::new();
    post.into_reader()
        .read_to_end(&mut out_body)
        .context("read bundle-uri response")?;

    let mut pairs = Vec::new();
    let mut cur = Cursor::new(&out_body);
    loop {
        match pkt_line::read_packet(&mut cur)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let (k, v) = line
                    .split_once('=')
                    .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("malformed bundle-uri line: {line}"))?;
                pairs.push((k.to_string(), v.to_string()));
            }
            Some(other) => bail!("unexpected bundle-uri response packet: {other:?}"),
        }
    }
    Ok(pairs)
}

/// Print a bundle list in the format expected by `test_cmp_config_output`.
pub fn print_bundle_list_from_pairs(pairs: &[(String, String)]) {
    println!("[bundle]");
    println!("\tversion = 1");
    println!("\tmode = all");
    for (k, v) in pairs {
        if let Some(rest) = k.strip_prefix("bundle.") {
            if let Some((id, subkey)) = rest.rsplit_once('.') {
                if subkey == "uri" {
                    println!("[bundle \"{id}\"]");
                    println!("\turi = {v}");
                }
            }
        }
    }
}
