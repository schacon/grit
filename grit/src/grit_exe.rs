//! Path to the running `grit` executable for spawning subprocesses.
//!
//! Used instead of `REAL_GIT` or `/usr/bin/git` so maintenance, scalar, clone,
//! and submodule helpers invoke this implementation.

use std::path::PathBuf;

/// Returns the path to the current `grit` binary (`std::env::current_exe`),
/// or `"grit"` on the `PATH` if unavailable.
#[must_use]
pub fn grit_executable() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit"))
}

/// Removes `GIT_TRACE2*` variables from a child command so nested `grit` runs
/// do not append to the parent's trace file (e.g. `t2080` counts
/// `child_start[..] git checkout--worker` lines only from the top-level checkout).
pub(crate) fn strip_trace2_env(cmd: &mut std::process::Command) {
    for key in [
        "GIT_TRACE2",
        "GIT_TRACE2_EVENT",
        "GIT_TRACE2_PERF",
        "GIT_TRACE2_SETUP",
    ] {
        cmd.env_remove(key);
    }
}
