//! Protocol allow/deny checking.
//!
//! Implements the `protocol.<name>.allow` config and `GIT_ALLOW_PROTOCOL`
//! environment variable to restrict which transports may be used.
//!
//! See git-config(1) for the upstream semantics.

use anyhow::{bail, Result};
use std::path::Path;

/// Check whether a given protocol (e.g. "file", "git", "ssh", "https") is
/// allowed in the current configuration context.
///
/// Rules (matching git):
/// 1. `GIT_ALLOW_PROTOCOL` env var: comma-separated whitelist.  If set, only
///    protocols listed there are allowed.
/// 2. `protocol.<name>.allow` config key: "always", "never", or "user" (default
///    varies by protocol).
/// 3. `protocol.allow` config key: blanket default.
/// 4. Built-in defaults: file/ssh/ext → "user", everything else → "user".
///
/// `protocol.<name>.allow=user` matches Git: allowed only when
/// `GIT_PROTOCOL_FROM_USER` is truthy (default: allowed when unset).
pub fn check_protocol_allowed(protocol: &str, git_dir: Option<&Path>) -> Result<()> {
    // 1. GIT_ALLOW_PROTOCOL overrides everything
    if let Ok(val) = std::env::var("GIT_ALLOW_PROTOCOL") {
        let allowed: Vec<&str> = val.split(':').collect();
        if allowed.contains(&protocol) {
            return Ok(());
        }
        bail!(
            "protocol '{}' is not allowed by GIT_ALLOW_PROTOCOL",
            protocol
        );
    }

    // 2. Check protocol.<name>.allow in config
    let specific = read_config_value(&format!("protocol.{}.allow", protocol), git_dir);
    if let Some(ref val) = specific {
        return check_allow_value(protocol, val);
    }

    // 3. Check protocol.allow blanket
    let blanket = read_config_value("protocol.allow", git_dir);
    if let Some(ref val) = blanket {
        return check_allow_value(protocol, val);
    }

    // 4. Default: allow (user context)
    Ok(())
}

fn check_allow_value(protocol: &str, value: &str) -> Result<()> {
    match value.to_lowercase().as_str() {
        "always" => Ok(()),
        "never" => bail!("protocol '{}' is not allowed", protocol),
        "user" => {
            if protocol_from_user_allowed() {
                Ok(())
            } else {
                bail!("protocol '{}' is not allowed", protocol)
            }
        }
        other => bail!(
            "unknown protocol.allow value '{}' for protocol '{}'",
            other,
            protocol
        ),
    }
}

fn protocol_from_user_allowed() -> bool {
    match std::env::var("GIT_PROTOCOL_FROM_USER") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            if v.is_empty() {
                return true;
            }
            !(matches!(v.as_str(), "0" | "false" | "no" | "off"))
        }
        Err(_) => true,
    }
}

/// Read a git config value. Tries `-c` overrides from process env first,
/// then reads from config file.
fn read_config_value(key: &str, git_dir: Option<&Path>) -> Option<String> {
    // Check GIT_CONFIG_PARAMETERS / GIT_CONFIG_COUNT style overrides
    // These are set by `git -c key=value` and propagated via env.
    if let Some(val) = check_git_config_env(key) {
        return Some(val);
    }

    // Try to read from actual config files
    if let Some(dir) = git_dir {
        let config_path = dir.join("config");
        if let Ok(contents) = std::fs::read_to_string(&config_path) {
            if let Some(val) = parse_config_for_key(&contents, key) {
                return Some(val);
            }
        }
    }

    // Try global config
    if let Ok(home) = std::env::var("HOME") {
        let global = std::path::PathBuf::from(home).join(".gitconfig");
        if let Ok(contents) = std::fs::read_to_string(&global) {
            if let Some(val) = parse_config_for_key(&contents, key) {
                return Some(val);
            }
        }
    }

    None
}

/// Public helper: check GIT_CONFIG_PARAMETERS for a specific key.
pub fn check_config_param(key: &str) -> Option<String> {
    check_git_config_env(key)
}

/// Check GIT_CONFIG_COUNT / GIT_CONFIG_KEY_N / GIT_CONFIG_VALUE_N env vars,
/// and also GIT_CONFIG_PARAMETERS (the format used by `git -c key=value`).
fn check_git_config_env(key: &str) -> Option<String> {
    // Check GIT_CONFIG_COUNT style first
    if let Ok(count_str) = std::env::var("GIT_CONFIG_COUNT") {
        if let Ok(count) = count_str.parse::<usize>() {
            for i in 0..count {
                if let (Ok(k), Ok(v)) = (
                    std::env::var(format!("GIT_CONFIG_KEY_{}", i)),
                    std::env::var(format!("GIT_CONFIG_VALUE_{}", i)),
                ) {
                    if k.eq_ignore_ascii_case(key) {
                        return Some(v);
                    }
                }
            }
        }
    }

    // Check GIT_CONFIG_PARAMETERS (format: 'key=value' 'key=value' ...)
    if let Ok(params) = std::env::var("GIT_CONFIG_PARAMETERS") {
        // Parse entries like 'protocol.file.allow=never'
        let mut result = None;
        for entry in params.split('\'') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if let Some(eq_pos) = entry.find('=') {
                let k = &entry[..eq_pos];
                let v = &entry[eq_pos + 1..];
                if k.eq_ignore_ascii_case(key) {
                    result = Some(v.to_string());
                }
            }
        }
        if result.is_some() {
            return result;
        }
    }

    None
}

/// Very simple INI-style config parser for a specific key like "protocol.file.allow".
fn parse_config_for_key(contents: &str, key: &str) -> Option<String> {
    // Split key into section parts: "protocol.file.allow" -> section="protocol", subsection="file", name="allow"
    // or "protocol.allow" -> section="protocol", subsection=None, name="allow"
    let parts: Vec<&str> = key.splitn(3, '.').collect();
    let (section, subsection, name) = match parts.len() {
        2 => (parts[0], None, parts[1]),
        3 => (parts[0], Some(parts[1]), parts[2]),
        _ => return None,
    };

    let mut current_section = String::new();
    let mut current_subsection: Option<String> = None;
    let mut result = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with(';') || trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') {
            // Parse section header
            if let Some(end) = trimmed.find(']') {
                let header = &trimmed[1..end];
                if let Some(space_pos) = header.find(' ') {
                    current_section = header[..space_pos].to_lowercase();
                    let sub = header[space_pos..].trim().trim_matches('"');
                    current_subsection = Some(sub.to_string());
                } else {
                    current_section = header.to_lowercase();
                    current_subsection = None;
                }
            }
            continue;
        }
        // key = value line
        if current_section.eq_ignore_ascii_case(section) {
            let subsection_matches = match (subsection, &current_subsection) {
                (None, None) => true,
                (Some(s), Some(cs)) => s.eq_ignore_ascii_case(cs),
                _ => false,
            };
            if subsection_matches {
                if let Some(eq_pos) = trimmed.find('=') {
                    let k = trimmed[..eq_pos].trim();
                    if k.eq_ignore_ascii_case(name) {
                        result = Some(trimmed[eq_pos + 1..].trim().to_string());
                    }
                }
            }
        }
    }

    result
}
