//! Git wire-protocol version negotiation (`protocol.version`, `GIT_PROTOCOL`).

use std::process::Command;

use crate::protocol;
use grit_lib::protocol::ClientProtocolVersionInputs;

/// Client-side `protocol.version` from `-c` / env / config (default **2**, matching Git).
///
/// Returns `0`, `1`, or `2`. Unknown values are treated as `2`.
pub fn effective_client_protocol_version() -> u8 {
    // Match `git/protocol.c` `get_protocol_version_config`: repo config (including `-c`) wins over
    // `GIT_TEST_PROTOCOL_VERSION`, so `git -c protocol.version=1` still uses v1 when the test
    // harness pins the default to v0 via env.
    let config_param_version = protocol::check_config_param("protocol.version");
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });
    let repo_config_version = if let Some(ref dir) = git_dir {
        if let Ok(set) = grit_lib::config::ConfigSet::load(Some(dir.as_path()), true) {
            set.get("protocol.version")
        } else {
            None
        }
    } else {
        None
    };
    let inputs = ClientProtocolVersionInputs {
        config_param_version,
        repo_config_version,
        git_test_protocol_version: std::env::var("GIT_TEST_PROTOCOL_VERSION").ok(),
    };
    grit_lib::protocol::effective_client_protocol_version_from_inputs(&inputs)
}

/// Server: highest `version=N` from `GIT_PROTOCOL` (`version=0|1|2`), or **0** if unset.
pub fn server_protocol_version_from_git_protocol_env() -> u8 {
    grit_lib::protocol::server_protocol_version_from_git_protocol(
        std::env::var("GIT_PROTOCOL").ok().as_deref(),
    )
}

/// When spawning `upload-pack` / `receive-pack`, merge `GIT_PROTOCOL` so the server negotiates v1/v2.
///
/// Any existing `version=N` entries are removed before appending `version={client_wants}` so a
/// parent process cannot pin v2 when the child explicitly uses `protocol.version=1` (e.g.
/// `submodule update --remote` local fetch).
pub fn merge_git_protocol_env_for_child(cmd: &mut Command, client_wants: u8) {
    let existing = std::env::var("GIT_PROTOCOL").ok();
    if let Some(merged) =
        grit_lib::protocol::merged_git_protocol_value(client_wants, existing.as_deref())
    {
        cmd.env("GIT_PROTOCOL", merged);
    }
}
