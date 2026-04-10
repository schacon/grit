//! Trace2 `transfer` category lines for protocol negotiation and session IDs (`t5705`).

use std::path::Path;
use std::sync::OnceLock;

use grit_lib::config::ConfigSet;

/// Emit `negotiated-version` from `GIT_PROTOCOL` (`version=N`), matching Git's
/// `determine_protocol_version_server` (default **0** when unset).
pub(crate) fn emit_negotiated_version_from_git_protocol_env() {
    let version = crate::protocol_wire::server_protocol_version_from_git_protocol_env();
    let Ok(path) = std::env::var("GIT_TRACE2_EVENT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _ = crate::trace2_write_json_data_line(
        &path,
        "transfer",
        "negotiated-version",
        &format!("{version}"),
    );
}

/// Emit `negotiated-version` for the fetch client after reading the server's first pkt-line
/// (`version 1` → **1**, else **0** for v0 ref advertisement).
pub(crate) fn emit_negotiated_version_client_fetch(first_line_was_version_1: bool) {
    let v: u8 = if first_line_was_version_1 { 1 } else { 0 };
    let Ok(path) = std::env::var("GIT_TRACE2_EVENT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _ = crate::trace2_write_json_data_line(
        &path,
        "transfer",
        "negotiated-version",
        &format!("{v}"),
    );
}

/// Emit `negotiated-version` for protocol v2 fetch client (**2**).
pub(crate) fn emit_negotiated_version_client_fetch_v2() {
    let Ok(path) = std::env::var("GIT_TRACE2_EVENT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _ = crate::trace2_write_json_data_line(&path, "transfer", "negotiated-version", "2");
}

/// Whether `transfer.advertiseSID` / `transfer.advertisesid` is enabled in repo config (`-c` aware).
pub(crate) fn transfer_advertise_sid_enabled(git_dir: &Path) -> bool {
    let Ok(set) = ConfigSet::load(Some(git_dir), true) else {
        return false;
    };
    for key in [
        "transfer.advertiseSID",
        "transfer.advertisesid",
        "transfer.advertiseSid",
    ] {
        if let Some(Ok(true)) = set.get_bool(key) {
            return true;
        }
    }
    false
}

/// Lazily generated wire session id for this process (matches one stable id per `grit` invocation).
pub(crate) fn trace2_session_id_wire_once() -> String {
    static SID: OnceLock<String> = OnceLock::new();
    SID.get_or_init(trace2_session_id_wire_value).clone()
}

/// Stable-enough session id for `session-id=` capability (tests only check presence).
fn trace2_session_id_wire_value() -> String {
    use sha1::{Digest, Sha1};

    let mut noise = Vec::new();
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let mut b = [0u8; 12];
            if f.read_exact(&mut b).is_ok() {
                noise.extend_from_slice(&b);
            }
        }
    }
    if noise.is_empty() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        noise.extend_from_slice(&now.to_le_bytes());
        noise.extend_from_slice(&std::process::id().to_le_bytes());
    }
    let digest = Sha1::digest(&noise);
    format!("grit-{}", hex::encode(digest))
}

fn emit_transfer_string(key: &str, value: &str) {
    let Ok(path) = std::env::var("GIT_TRACE2_EVENT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _ = crate::trace2_write_json_data_line(&path, "transfer", key, value);
}

pub(crate) fn emit_server_sid(value: &str) {
    emit_transfer_string("server-sid", value);
}

pub(crate) fn emit_client_sid(value: &str) {
    emit_transfer_string("client-sid", value);
}

/// JSON-escape a string for a trace2 `data` line value field.
pub(crate) fn json_escape_trace_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Parse `session-id=<value>` from a capability string (NUL or whitespace separated).
pub(crate) fn extract_session_id_feature(caps: &str) -> Option<&str> {
    for cap in caps
        .split(|c: char| c.is_whitespace() || c == '\0')
        .filter(|s| !s.is_empty())
    {
        if let Some(v) = cap.strip_prefix("session-id=") {
            return Some(v);
        }
    }
    None
}
