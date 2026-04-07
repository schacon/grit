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
