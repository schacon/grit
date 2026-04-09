//! Git wire-protocol version negotiation (`protocol.version`, `GIT_PROTOCOL`).

use std::process::Command;

use crate::protocol;

/// Client-side `protocol.version` from `-c` / env / config (default **2**, matching Git).
///
/// Returns `0`, `1`, or `2`. Unknown values are treated as `2`.
pub fn effective_client_protocol_version() -> u8 {
    // Match `git/protocol.c` `get_protocol_version_config`: repo config (including `-c`) wins over
    // `GIT_TEST_PROTOCOL_VERSION`, so `git -c protocol.version=1` still uses v1 when the test
    // harness pins the default to v0 via env.
    if let Some(v) = protocol::check_config_param("protocol.version") {
        return parse_protocol_version_digit(&v).unwrap_or(2);
    }
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });
    if let Some(ref dir) = git_dir {
        if let Ok(set) = grit_lib::config::ConfigSet::load(Some(dir.as_path()), true) {
            if let Some(v) = set.get("protocol.version") {
                return parse_protocol_version_digit(&v).unwrap_or(2);
            }
        }
    }
    if let Some(raw) = std::env::var("GIT_TEST_PROTOCOL_VERSION")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return parse_protocol_version_digit(&raw).unwrap_or(2);
    }
    2
}

fn parse_protocol_version_digit(s: &str) -> Option<u8> {
    match s.trim() {
        "0" => Some(0),
        "1" => Some(1),
        "2" => Some(2),
        _ => None,
    }
}

/// Server: highest `version=N` from `GIT_PROTOCOL` (`version=0|1|2`), or **0** if unset.
pub fn server_protocol_version_from_git_protocol_env() -> u8 {
    let Ok(raw) = std::env::var("GIT_PROTOCOL") else {
        return 0;
    };
    let mut best = 0u8;
    for part in raw.split(':') {
        let Some(rest) = part.strip_prefix("version=") else {
            continue;
        };
        let v = rest.parse::<u8>().unwrap_or(0);
        if v > best {
            best = v;
        }
    }
    best
}

/// When spawning `upload-pack` / `receive-pack`, merge `GIT_PROTOCOL` so the server negotiates v1/v2.
pub fn merge_git_protocol_env_for_child(cmd: &mut Command, client_wants: u8) {
    if client_wants == 0 {
        return;
    }
    let entry = format!("version={client_wants}");
    let merged = match std::env::var("GIT_PROTOCOL") {
        Ok(existing) if !existing.is_empty() => {
            if existing.split(':').any(|p| p == entry.as_str()) {
                existing
            } else {
                format!("{existing}:{entry}")
            }
        }
        _ => entry,
    };
    cmd.env("GIT_PROTOCOL", merged);
}
