//! Hook execution utilities.
//!
//! Provides a reusable function for running Git hooks from `.git/hooks/`
//! or from the directory configured via `core.hooksPath`.

use crate::config::ConfigSet;
use crate::repo::Repository;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(unix)]
const ENOEXEC: i32 = 8;

#[cfg(unix)]
fn is_enoexec(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(ENOEXEC)
}

fn stdio_piped(piped: bool) -> Stdio {
    if piped {
        Stdio::piped()
    } else {
        Stdio::inherit()
    }
}

/// Spawn a hook script. If the kernel rejects direct execution (e.g. no shebang, ENOEXEC), run it
/// with `/bin/sh` like Git does.
fn spawn_hook_child(
    hook_path: &Path,
    hook_args: &[&str],
    cwd: &Path,
    git_dir: &Path,
    extra_env: &[(&str, &str)],
    stdin_piped: bool,
    stdout_piped: bool,
    stderr_piped: bool,
    use_shell: bool,
) -> std::io::Result<std::process::Child> {
    let mut cmd = if use_shell {
        let mut sh = Command::new("/bin/sh");
        sh.arg(hook_path);
        sh
    } else {
        Command::new(hook_path)
    };
    cmd.args(hook_args)
        .current_dir(cwd)
        .env("GIT_DIR", git_dir)
        .stdin(stdio_piped(stdin_piped))
        .stdout(stdio_piped(stdout_piped))
        .stderr(stdio_piped(stderr_piped));
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    match cmd.spawn() {
        Ok(c) => Ok(c),
        Err(e) => {
            #[cfg(unix)]
            {
                if !use_shell && is_enoexec(&e) {
                    return spawn_hook_child(
                        hook_path,
                        hook_args,
                        cwd,
                        git_dir,
                        extra_env,
                        stdin_piped,
                        stdout_piped,
                        stderr_piped,
                        true,
                    );
                }
            }
            Err(e)
        }
    }
}

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

fn hook_command_path(repo: &Repository, hooks_dir: &Path, hook_name: &str, cwd: &Path) -> PathBuf {
    let default_hooks_dir = repo.git_dir.join("hooks");
    if hooks_dir == default_hooks_dir {
        if cwd == repo.git_dir {
            return PathBuf::from("hooks").join(hook_name);
        }
        if let Some(work_tree) = repo.work_tree.as_deref() {
            if cwd == work_tree {
                return PathBuf::from(".git").join("hooks").join(hook_name);
            }
        }
    }
    hooks_dir.join(hook_name)
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
        // Warn that the hook exists but is not executable (like git does)
        let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
        let show_warning = config
            .as_ref()
            .and_then(|c| c.get("advice.ignoredHook"))
            .map(|v| !matches!(v.to_lowercase().as_str(), "false" | "no" | "off" | "0"))
            .unwrap_or(true);
        if show_warning {
            eprintln!(
                "hint: The '{}' hook was ignored because it's not set as executable.",
                hook_name
            );
            eprintln!(
                "hint: You can disable this warning with `git config set advice.ignoredHook false`."
            );
        }
        return HookResult::NotFound;
    }

    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let command_path = hook_command_path(repo, &hooks_dir, hook_name, work_dir);

    let stdin_piped = stdin_data.is_some();

    let mut child = match spawn_hook_child(
        &command_path,
        args,
        work_dir,
        &repo.git_dir,
        &[],
        stdin_piped,
        false,
        false,
        false,
    ) {
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

/// Like `run_hook` but captures stdout and returns it alongside the result.
/// Run a hook with extra env vars, setting cwd to GIT_DIR (for receive-side hooks).
pub fn run_hook_in_git_dir(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    stdin_data: Option<&[u8]>,
    env_vars: &[(&str, &str)],
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

    let command_path = hook_command_path(repo, &hooks_dir, hook_name, &repo.git_dir);
    let stdin_piped = stdin_data.is_some();

    let mut child = match spawn_hook_child(
        &command_path,
        args,
        &repo.git_dir,
        &repo.git_dir,
        env_vars,
        stdin_piped,
        true,
        true,
        false,
    ) {
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

/// Like `run_hook` but with extra environment variables and captures output.
pub fn run_hook_with_env(
    repo: &Repository,
    hook_name: &str,
    args: &[&str],
    stdin_data: Option<&[u8]>,
    env_vars: &[(&str, &str)],
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

    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let command_path = hook_command_path(repo, &hooks_dir, hook_name, work_dir);

    let stdin_piped = stdin_data.is_some();

    let mut child = match spawn_hook_child(
        &command_path,
        args,
        work_dir,
        &repo.git_dir,
        env_vars,
        stdin_piped,
        true,
        true,
        false,
    ) {
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

    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let command_path = hook_command_path(repo, &hooks_dir, hook_name, work_dir);

    let stdin_piped = stdin_data.is_some();

    let mut child = match spawn_hook_child(
        &command_path,
        args,
        work_dir,
        &repo.git_dir,
        &[],
        stdin_piped,
        true,
        true,
        false,
    ) {
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
