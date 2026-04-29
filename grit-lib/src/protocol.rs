//! Protocol allow/deny policy.
//!
//! Implements the `protocol.<name>.allow` config and `GIT_ALLOW_PROTOCOL`
//! environment semantics without reading process-global state directly.

use thiserror::Error;

/// Errors returned when a transport protocol is not allowed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    /// Protocol is not included in `GIT_ALLOW_PROTOCOL`.
    #[error("protocol '{protocol}' is not allowed by GIT_ALLOW_PROTOCOL")]
    NotAllowedByEnvironment {
        /// Protocol name that was denied.
        protocol: String,
    },
    /// Protocol is denied by config or default policy.
    #[error("protocol '{protocol}' is not allowed")]
    NotAllowed {
        /// Protocol name that was denied.
        protocol: String,
    },
    /// Config contains an unknown allow value.
    #[error("unknown protocol.allow value '{value}' for protocol '{protocol}'")]
    UnknownAllowValue {
        /// Protocol name whose policy was evaluated.
        protocol: String,
        /// Unknown config value.
        value: String,
    },
}

/// Explicit inputs for protocol allow/deny evaluation.
#[derive(Clone, Debug, Default)]
pub struct ProtocolPolicyInputs {
    /// `GIT_ALLOW_PROTOCOL` value, when set.
    pub git_allow_protocol: Option<String>,
    /// `GIT_PROTOCOL_FROM_USER` value, when set.
    pub git_protocol_from_user: Option<String>,
    /// `protocol.<name>.allow` config value.
    pub specific_allow: Option<String>,
    /// `protocol.allow` config value.
    pub blanket_allow: Option<String>,
}

/// Check whether a given protocol (e.g. `file`, `git`, `ssh`, `https`) is allowed.
///
/// Rules match Git's `transport.c` / `is_transport_allowed`:
/// 1. `GIT_ALLOW_PROTOCOL` is a colon- or comma-separated whitelist.
/// 2. `protocol.<name>.allow` overrides the blanket config.
/// 3. `protocol.allow` supplies a blanket default.
/// 4. Built-in defaults: `http`, `https`, `git`, and `ssh` are always allowed;
///    `ext` is never allowed; any other protocol is `user`.
pub fn check_protocol_allowed_with(
    protocol: &str,
    inputs: &ProtocolPolicyInputs,
) -> Result<(), ProtocolError> {
    if let Some(val) = inputs.git_allow_protocol.as_deref() {
        let allowed: Vec<&str> = val
            .split([':', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if allowed.contains(&protocol) {
            return Ok(());
        }
        return Err(ProtocolError::NotAllowedByEnvironment {
            protocol: protocol.to_owned(),
        });
    }

    if let Some(ref val) = inputs.specific_allow {
        return check_allow_value(protocol, val, inputs.git_protocol_from_user.as_deref());
    }

    if let Some(ref val) = inputs.blanket_allow {
        return check_allow_value(protocol, val, inputs.git_protocol_from_user.as_deref());
    }

    match protocol.to_ascii_lowercase().as_str() {
        "http" | "https" | "git" | "ssh" => Ok(()),
        "ext" => Err(ProtocolError::NotAllowed {
            protocol: protocol.to_owned(),
        }),
        _ => check_allow_value(protocol, "user", inputs.git_protocol_from_user.as_deref()),
    }
}

fn check_allow_value(
    protocol: &str,
    value: &str,
    git_protocol_from_user: Option<&str>,
) -> Result<(), ProtocolError> {
    match value.to_lowercase().as_str() {
        "always" => Ok(()),
        "never" => Err(ProtocolError::NotAllowed {
            protocol: protocol.to_owned(),
        }),
        "user" => {
            if protocol_from_user(git_protocol_from_user) {
                Ok(())
            } else {
                Err(ProtocolError::NotAllowed {
                    protocol: protocol.to_owned(),
                })
            }
        }
        other => Err(ProtocolError::UnknownAllowValue {
            protocol: protocol.to_owned(),
            value: other.to_owned(),
        }),
    }
}

/// Whether `protocol.<name>.allow=user` should be considered allowed.
#[must_use]
pub fn protocol_from_user(raw: Option<&str>) -> bool {
    match raw {
        None => true,
        Some(v) => {
            let v = v.trim().to_ascii_lowercase();
            v.is_empty() || !matches!(v.as_str(), "0" | "false" | "no" | "off")
        }
    }
}
