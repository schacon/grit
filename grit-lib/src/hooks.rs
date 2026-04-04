//! Hook execution utilities.
//!
//! Provides a reusable function for running Git hooks from `.git/hooks/`
//! or from the directory configured via `core.hooksPath`.

use crate::config::ConfigSet;
use crate::repo::Repository;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

/// Result of running a hook.
#[derive(Debug)]
pub enum HookResult {
    /// Hook ran successfully (exit code 0).
    Success,
    /// Hook does not exist or is not executable — treated as success.
    NotFound,
    /// Hook ran but returned a non-zero exit code.
    Failed(i32),
}

impl HookResult {
    /// Returns true if the hook was successful or not found.
    pub fn is_ok(&self) -> bool {
        matches!(self, HookResult::Success | HookResult::NotFound)
    }

    /// Returns true if the hook existed and ran (regardless of exit code).
    pub fn was_executed(&self) -> bool {
        matches!(self, HookResult::Success | HookResult::Failed(_))
    }
}

/// Resolve the hooks directory from config or fall back to `$GIT_DIR/hooks`.
pub fn resolve_hooks_dir(repo: &Repository) -> PathBuf {
    let config = ConfigSet::load(Some(&repo.git_dir), true).ok();

    if let Some(ref config) = config {
        if let Some(hooks_path) = config.get("core.hooksPath") {
            let expanded = crate::config::parse_path(&hooks_path);
            let p = PathBuf::from(expanded);
            if p.is_absolute() {
                return p;
            }
            // Relative to the working directory (git behaviour).
            if let Ok(cwd) = std::env::current_dir() {
                return cwd.join(p);
            }
        }
    }

    repo.git_dir.join("hooks")
}

/// Run a hook by name with the given arguments.
///
/// The hook is looked up in the hooks directory (respecting `core.hooksPath`).
/// If the hook file doesn't exist or isn't executable, returns `HookResult::NotFound`.
///
/// `stdin_data` can optionally provide data to write to the hook's stdin.
pub fn run_hook(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    stdin_data: Option<&[u8]>,
) -> HookResult {
    let hooks_dir = resolve_hooks_dir(repo);
    let hook_path = hooks_dir.join(hook_name);

    // If the hook doesn't exist, silently succeed (git behaviour).
    if !hook_path.exists() {
        return HookResult::NotFound;
    }

    // Check if executable.
    let meta = match fs::metadata(&hook_path) {
        Ok(m) => m,
        Err(_) => return HookResult::NotFound,
    };
    if meta.permissions().mode() & 0o111 == 0 {
        return HookResult::NotFound;
    }

    let work_dir = repo
        .work_tree
        .as_deref()
        .unwrap_or(&repo.git_dir);

    let mut cmd = Command::new(&hook_path);
    cmd.args(args)
        .current_dir(work_dir)
        .env("GIT_DIR", &repo.git_dir);

    if stdin_data.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return HookResult::Failed(1),
    };

    if let Some(data) = stdin_data {
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            let _ = stdin.write_all(data);
        }
        // Drop stdin to signal EOF
        drop(child.stdin.take());
    }

    match child.wait() {
        Ok(status) => {
            if status.success() {
                HookResult::Success
            } else {
                HookResult::Failed(status.code().unwrap_or(1))
            }
        }
        Err(_) => HookResult::Failed(1),
    }
}

/// Like `run_hook` but with additional environment variables and optional cwd override.
pub fn run_hook_with_env(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    stdin_data: Option<&[u8]>,
    env_vars: &[(&str, &str)],
) -> HookResult {
    run_hook_with_env_cwd(repo, hook_name, args, stdin_data, env_vars, None)
}

/// Like `run_hook_with_env` but allows overriding the working directory.
pub fn run_hook_with_env_cwd(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    stdin_data: Option<&[u8]>,
    env_vars: &[(&str, &str)],
    cwd: Option<&std::path::Path>,
) -> HookResult {
    let hooks_dir = resolve_hooks_dir(repo);
    let hook_path = hooks_dir.join(hook_name);

    if !hook_path.exists() {
        return HookResult::NotFound;
    }

    let meta = match fs::metadata(&hook_path) {
        Ok(m) => m,
        Err(_) => return HookResult::NotFound,
    };
    if meta.permissions().mode() & 0o111 == 0 {
        return HookResult::NotFound;
    }

    let default_dir = repo
        .work_tree
        .as_deref()
        .unwrap_or(&repo.git_dir);
    let work_dir = cwd.unwrap_or(default_dir);

    let mut cmd = Command::new(&hook_path);
    cmd.args(args)
        .current_dir(work_dir)
        .env("GIT_DIR", &repo.git_dir);

    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    if stdin_data.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return HookResult::Failed(1),
    };

    if let Some(data) = stdin_data {
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            let _ = stdin.write_all(data);
        }
        drop(child.stdin.take());
    }

    match child.wait() {
        Ok(status) => {
            if status.success() {
                HookResult::Success
            } else {
                HookResult::Failed(status.code().unwrap_or(1))
            }
        }
        Err(_) => HookResult::Failed(1),
    }
}

/// Like `run_hook` but captures stdout and returns it alongside the result.
pub fn run_hook_capture(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    stdin_data: Option<&[u8]>,
) -> (HookResult, Vec<u8>) {
    let hooks_dir = resolve_hooks_dir(repo);
    let hook_path = hooks_dir.join(hook_name);

    if !hook_path.exists() {
        return (HookResult::NotFound, Vec::new());
    }

    let meta = match fs::metadata(&hook_path) {
        Ok(m) => m,
        Err(_) => return (HookResult::NotFound, Vec::new()),
    };
    if meta.permissions().mode() & 0o111 == 0 {
        return (HookResult::NotFound, Vec::new());
    }

    let work_dir = repo
        .work_tree
        .as_deref()
        .unwrap_or(&repo.git_dir);

    let mut cmd = Command::new(&hook_path);
    cmd.args(args)
        .current_dir(work_dir)
        .env("GIT_DIR", &repo.git_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if stdin_data.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return (HookResult::Failed(1), Vec::new()),
    };

    if let Some(data) = stdin_data {
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            let _ = stdin.write_all(data);
        }
        drop(child.stdin.take());
    }

    match child.wait_with_output() {
        Ok(output) => {
            let mut combined = output.stdout;
            combined.extend_from_slice(&output.stderr);
            let result = if output.status.success() {
                HookResult::Success
            } else {
                HookResult::Failed(output.status.code().unwrap_or(1))
            };
            (result, combined)
        }
        Err(_) => (HookResult::Failed(1), Vec::new()),
    }
}
