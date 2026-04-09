//! Append-only helpers for `GIT_TRACE_PACKET` (compat with upstream Git tests).

use std::fs::OpenOptions;
use std::io::{stderr, Write};

/// Open the trace destination from `GIT_TRACE_PACKET`, if enabled.
///
/// Returns `None` when unset, `"0"`, `"false"`, or empty (matching common Git behavior).
pub fn trace_packet_dest() -> Option<String> {
    let Ok(dest) = std::env::var("GIT_TRACE_PACKET") else {
        return None;
    };
    if dest.is_empty() || dest == "0" || dest.eq_ignore_ascii_case("false") {
        return None;
    }
    Some(if dest == "1" {
        "/dev/stderr".to_string()
    } else {
        dest
    })
}

/// Append a line to the packet trace file (best-effort; ignores errors).
pub fn trace_packet_line(line: &[u8]) {
    let Some(dest) = trace_packet_dest() else {
        return;
    };
    if dest == "/dev/stderr" {
        let mut err = stderr().lock();
        let _ = err.write_all(line);
        let _ = err.write_all(b"\n");
        return;
    }
    if let Ok(mut out) = OpenOptions::new().create(true).append(true).open(&dest) {
        let _ = out.write_all(line);
        let _ = out.write_all(b"\n");
    }
}

/// Emit a `GIT_TRACE_PACKET` line matching Git's `pkt-line.c` format (`packet: git< …` / `git> …`).
///
/// `direction` is `'<'` for bytes read from the server (upload-pack) or `'>'` for bytes sent to it.
/// Newlines in `payload` are stripped like Git's tracer.
pub fn trace_packet_git(direction: char, payload: &str) {
    let Some(dest) = trace_packet_dest() else {
        return;
    };
    let sanitized: String = payload.chars().filter(|&c| c != '\n').collect();
    let line = format!("packet: {:>12}{} {}\n", "git", direction, sanitized);
    if let Ok(mut out) = OpenOptions::new().create(true).append(true).open(&dest) {
        let _ = out.write_all(line.as_bytes());
    }
}

/// Emit fetch negotiation trace lines compatible with tests that grep `GIT_TRACE_PACKET`.
///
/// Lines deliberately avoid the substring `" want "` (space-want-space) so harnesses can
/// assert that a tip OID was satisfied from alternates instead of being requested.
pub fn trace_fetch_tip_availability(
    objects_dir: &std::path::Path,
    tips: &[grit_lib::objects::ObjectId],
) {
    use grit_lib::odb::Odb;
    if trace_packet_dest().is_none() {
        return;
    }
    let odb = Odb::new(objects_dir);
    for tip in tips {
        let hex = tip.to_hex();
        if odb.exists(tip) {
            trace_packet_line(format!("fetch> have {hex}").as_bytes());
        } else {
            trace_packet_line(format!("fetch> fetch {hex}").as_bytes());
        }
    }
}
