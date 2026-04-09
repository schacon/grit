//! Hand off the current process invocation to the system `git` binary.
//!
//! Used when grit does not implement a network transport yet but tests (and
//! users) still need Git-compatible behavior.

use std::ffi::OsString;
use std::process::Command;

/// Re-exec as `REAL_GIT` (default `/usr/bin/git`) with **subcommand + args only**.
///
/// Global options (`-C`, `--git-dir`, `-c`, …) are omitted because this crate
/// applies them before dispatch (e.g. `chdir`); passing them again would make
/// upstream git apply them twice.
///
/// Exits the process with the child's status; does not return on success.
pub fn delegate_current_invocation_to_real_git() -> ! {
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let full: Vec<String> = std::env::args().collect();
    let (_opts, subcmd, rest) = match crate::extract_globals(&full) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    };
    let Some(subcmd) = subcmd else {
        eprintln!("error: missing subcommand");
        std::process::exit(1);
    };
    match Command::new(&git_bin).arg(&subcmd).args(&rest).status() {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!(
                "error: failed to execute {}: {e}",
                git_bin.to_string_lossy()
            );
            std::process::exit(1);
        }
    }
}
