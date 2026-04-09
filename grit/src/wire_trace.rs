//! `GIT_TRACE_PACKET` helpers with Git-compatible command identity (`clone`, `fetch`, `push`).

use std::fs::OpenOptions;
use std::io::{stderr, Write};

fn trace_enabled_path() -> Option<String> {
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

/// Emit one trace line: `packet: <identity><dir> <payload>` (matches Git's pkt-line trace).
pub fn trace_packet_line_ident(identity: &str, direction: char, payload: &str) {
    let Some(path) = trace_enabled_path() else {
        return;
    };
    let line = format!(
        "packet: {:>12}{} {}\n",
        identity,
        direction,
        payload.replace('\n', "").replace('\0', "\\0")
    );
    // Use the process stderr lock when tracing to stderr so lines do not interleave with
    // `eprintln!` / `println!` (opening `/dev/stderr` separately races and corrupts pkt traces).
    if path == "/dev/stderr" {
        let _ = stderr().lock().write_all(line.as_bytes());
        return;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn trace_packet_upload_pack(direction: char, payload: &str) {
    trace_packet_line_ident("upload-pack", direction, payload);
}

pub fn trace_packet_receive_pack(direction: char, payload: &str) {
    trace_packet_line_ident("receive-pack", direction, payload);
}

pub fn trace_packet_push(direction: char, payload: &str) {
    trace_packet_line_ident("push", direction, payload);
}
