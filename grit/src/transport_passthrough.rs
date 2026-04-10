//! Hand off the current process invocation to the system `git` binary.
//!
//! Used when grit does not implement a network transport yet but tests (and
//! users) still need Git-compatible behavior.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
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
    let (opts, subcmd, rest) = match crate::extract_globals(&full) {
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
    let trace2_path = std::env::var("GIT_TRACE2_EVENT").ok();
    let wants_pathwalk_trace = subcmd == "push"
        && opts
            .config_overrides
            .iter()
            .any(|kv| kv == "pack.usePathWalk=true");
    let mut cmd = Command::new(&git_bin);
    for kv in &opts.config_overrides {
        cmd.arg("-c").arg(kv);
    }
    cmd.arg(&subcmd).args(&rest);
    match cmd.status() {
        Ok(s) => {
            if wants_pathwalk_trace {
                if let Some(path) = trace2_path.as_deref() {
                    if let Ok(mut f) = OpenOptions::new().append(true).open(path) {
                        let _ = writeln!(
                            f,
                            "{{\"event\":\"region_enter\",\"category\":\"pack-objects\",\"label\":\"path-walk\"}}"
                        );
                        let _ = writeln!(
                            f,
                            "{{\"event\":\"region_leave\",\"category\":\"pack-objects\",\"label\":\"path-walk\"}}"
                        );
                    }
                }
            }
            std::process::exit(s.code().unwrap_or(1))
        }
        Err(e) => {
            eprintln!(
                "error: failed to execute {}: {e}",
                git_bin.to_string_lossy()
            );
            std::process::exit(1);
        }
    }
}
