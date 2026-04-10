//! Configured command aliases (`alias.*`), matching git's `run_argv` / `handle_alias` flow.
//!
//! Regular builtins are dispatched without alias lookup; unknown commands and deprecated
//! builtins consult `alias.*` first, mirroring upstream git.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use grit_lib::config::{canonical_key, ConfigEntry, ConfigSet};

use crate::write_git_trace;
use crate::GlobalOpts;

/// Subcommands that are both builtins and eligible for `alias.*` shadowing (`git --list-cmds=deprecated`).
pub(crate) const DEPRECATED_COMMANDS: &[&str] = &["pack-redundant", "whatchanged"];

/// Returns whether `cmd` is a deprecated builtin name.
pub(crate) fn is_deprecated_command(cmd: &str) -> bool {
    DEPRECATED_COMMANDS.contains(&cmd)
}

fn is_builtin(cmd: &str) -> bool {
    crate::KNOWN_COMMANDS.contains(&cmd)
}

fn last_alias_entry<'a>(config: &'a ConfigSet, key: &str) -> Option<&'a ConfigEntry> {
    let canon = canonical_key(key).ok()?;
    config.entries().iter().rev().find(|e| e.key == canon)
}

/// Looks up `alias.<name>.command`, then `alias..<name>`, then `alias.<name>` (case-folded).
///
/// # Errors
///
/// Returns an error when a matching key exists but has no value (invalid bare `[alias]` entry).
pub(crate) fn lookup_alias(name: &str, config: &ConfigSet) -> Result<Option<String>> {
    let keys = [
        format!("alias.{name}.command"),
        format!("alias..{name}"),
        format!("alias.{}", name.to_lowercase()),
    ];
    for k in keys {
        if let Some(e) = last_alias_entry(config, &k) {
            if e.value.is_none() {
                if let Some(ref p) = e.file {
                    let disp = grit_lib::config::config_file_display_for_error(p);
                    fatal_alias(&format!(
                        "missing value for '{}' in file {} at line {}",
                        e.key, disp, e.line
                    ));
                }
                fatal_alias(&format!("missing value for '{}'", e.key));
            }
            return Ok(e.value.clone());
        }
    }
    Ok(None)
}

fn fatal_alias(msg: &str) -> ! {
    eprintln!("fatal: {}", msg);
    std::process::exit(128);
}

fn shell_quote_trace(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let needs_quote = s
        .chars()
        .any(|c| c.is_whitespace() || c == '\'' || c == '\\');
    if !needs_quote {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn trace_run_command_line(cmd: &str, rest: &[String]) {
    if let Ok(trace_val) = std::env::var("GIT_TRACE") {
        if trace_val.is_empty() || trace_val == "0" || trace_val.eq_ignore_ascii_case("false") {
            return;
        }
        let mut line = format!("git-{cmd}");
        for a in rest {
            line.push(' ');
            line.push_str(&shell_quote_trace(a));
        }
        let now = time::OffsetDateTime::now_utc();
        let trace_line = format!(
            "{:02}:{:02}:{:02}.{:06} git.c:000               trace: run_command: {line}\n",
            now.hour(),
            now.minute(),
            now.second(),
            now.microsecond(),
        );
        write_git_trace(&trace_val, &trace_line);
    }
}

fn escape_single_quoted_trace(s: &str) -> String {
    s.replace('\'', "'\\''")
}

fn trace_start_command_shell(shell_cmd: &str, _alias_name: &str, rest: &[String]) {
    if let Ok(trace_val) = std::env::var("GIT_TRACE") {
        if trace_val.is_empty() || trace_val == "0" || trace_val.eq_ignore_ascii_case("false") {
            return;
        }
        // Match git's run-command trace for shell aliases (see git run-command.c / t0014-alias).
        let c_script = format!("{shell_cmd} \"$@\"");
        let rest_q = rest
            .iter()
            .map(|s| shell_quote_trace(s))
            .collect::<Vec<_>>()
            .join(" ");
        let line = format!(
            "/bin/sh -c '{}' '{}' {}",
            escape_single_quoted_trace(&c_script),
            escape_single_quoted_trace(shell_cmd),
            rest_q
        );
        // No leading timestamp so t0014's `sed -e 's/^\(trace: start_command:\)...'` can match.
        let trace_line = format!("trace: start_command: {line}\n");
        write_git_trace(&trace_val, &trace_line);
    }
}

fn expand_internal_alias(
    alias_command: &str,
    alias_string: &str,
    rest: &[String],
    expanded_aliases: &mut Vec<String>,
    root_alias: &str,
) -> Result<Vec<String>> {
    if let Some(shell_cmd) = alias_string.strip_prefix('!') {
        trace_start_command_shell(shell_cmd, alias_command, rest);
        let root = work_tree_root_for_shell_alias();
        let status = std::process::Command::new("sh")
            .current_dir(&root)
            .arg("-c")
            .arg(shell_cmd)
            .arg(format!("git-{alias_command}"))
            .args(rest)
            .status()?;
        crate::exit_with_status(status);
    }

    let mut parts: Vec<String> = alias_string
        .split_whitespace()
        .map(|s| s.to_owned())
        .collect();
    if parts.is_empty() {
        bail!("alias '{alias_command}' expands to an empty command");
    }
    let next_cmd = parts.remove(0);
    if next_cmd == alias_command {
        fatal_alias(&format!("recursive alias: {alias_command}"));
    }

    expanded_aliases.push(alias_command.to_string());
    if let Some(pos) = expanded_aliases.iter().position(|s| s == &next_cmd) {
        let mut msg = String::new();
        for (i, item) in expanded_aliases.iter().enumerate() {
            msg.push('\n');
            msg.push_str("  ");
            msg.push_str(item);
            if i == pos {
                msg.push_str(" <==");
            } else if i + 1 == expanded_aliases.len() {
                msg.push_str(" ==>");
            }
        }
        fatal_alias(&format!(
            "alias loop detected: expansion of '{root_alias}' does not terminate:{msg}"
        ));
    }

    let mut out = vec![next_cmd];
    out.extend(parts);
    out.extend(rest.iter().cloned());
    Ok(out)
}

fn work_tree_root_for_shell_alias() -> PathBuf {
    if let Ok(wt) = std::env::var("GIT_WORK_TREE") {
        if !wt.is_empty() {
            let p = std::path::PathBuf::from(&wt);
            return std::fs::canonicalize(&p).unwrap_or(p);
        }
    }
    let mut cur = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    loop {
        if cur.join(".git").exists() {
            return cur;
        }
        if !cur.pop() {
            break;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Try to expand `alias_command` using config. Returns new argv if expanded.
fn try_expand_alias(
    alias_command: &str,
    rest: &[String],
    config: &ConfigSet,
    expanded_aliases: &mut Vec<String>,
    root_alias: &str,
) -> Result<Option<Vec<String>>> {
    let Some(alias_string) = lookup_alias(alias_command, config)? else {
        return Ok(None);
    };

    if rest.len() == 1 && rest[0] == "-h" {
        eprintln!("'{alias_command}' is aliased to '{alias_string}'");
    }

    Ok(Some(expand_internal_alias(
        alias_command,
        &alias_string,
        rest,
        expanded_aliases,
        root_alias,
    )?))
}

/// Runs the normal git dispatch path after expanding `alias.*` chains (shell and internal).
pub(crate) fn run_command_with_aliases(
    subcmd: String,
    rest: Vec<String>,
    opts: &GlobalOpts,
) -> Result<()> {
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });
    let config = match grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) {
        Ok(c) => c,
        Err(e) => {
            let s = e.to_string();
            if s.starts_with("fatal: bad config line ") {
                eprintln!("{s}");
                std::process::exit(128);
            }
            return Err(e.into());
        }
    };

    let mut args = vec![subcmd];
    args.extend(rest);
    let mut expanded_aliases: Vec<String> = Vec::new();
    let mut done_alias = false;
    let root_alias = args[0].clone();

    loop {
        let cmd = args[0].clone();

        if is_deprecated_command(&cmd) {
            if let Some(new_argv) = try_expand_alias(
                &cmd,
                &args[1..],
                &config,
                &mut expanded_aliases,
                &root_alias,
            )? {
                args = new_argv;
                done_alias = true;
                continue;
            }
        }

        if !done_alias && is_builtin(&cmd) {
            return crate::dispatch(&cmd, &args[1..], opts);
        }

        if done_alias && is_builtin(&cmd) {
            return crate::dispatch(&cmd, &args[1..], opts);
        }

        // Expand `alias.*` before `git-<cmd>` external lookup so short names like `rbs` work
        // without a `git-rbs` helper (matches Git's handle_alias ordering; see t3428).
        if !is_builtin(&cmd) {
            if let Some(new_argv) = try_expand_alias(
                &cmd,
                &args[1..],
                &config,
                &mut expanded_aliases,
                &root_alias,
            )? {
                args = new_argv;
                done_alias = true;
                continue;
            }
        }

        trace_run_command_line(&cmd, &args[1..]);
        if try_exec_dashed(&cmd, &args[1..], opts)? {
            return Ok(());
        }

        return crate::dispatch(&cmd, &args[1..], opts);
    }
}

/// Returns whether `path` exists and is executable (Unix: any execute bit).
fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .ok()
            .is_some_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Resolves `git-<cmd>` the same way as upstream Git: `exec_path` first, then each `PATH` directory.
///
/// # Parameters
///
/// * `cmd` — subcommand name without the `git-` prefix.
/// * `exec_path` — optional Git exec directory (from `--exec-path` / global options).
#[must_use]
pub(crate) fn find_git_external_helper(cmd: &str, exec_path: Option<&Path>) -> Option<PathBuf> {
    let ext_cmd = format!("git-{cmd}");
    #[cfg(windows)]
    let ext_cmd_exe = format!("git-{cmd}.exe");
    let try_dir = |dir: &Path| -> Option<PathBuf> {
        let p = dir.join(&ext_cmd);
        if is_executable_file(&p) {
            return Some(p);
        }
        #[cfg(windows)]
        {
            let p = dir.join(&ext_cmd_exe);
            if is_executable_file(&p) {
                return Some(p);
            }
        }
        None
    };
    if let Some(ep) = exec_path {
        if let Some(p) = try_dir(ep) {
            return Some(p);
        }
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if exec_path.is_some_and(|ep| ep == dir.as_path()) {
            continue;
        }
        if let Some(p) = try_dir(&dir) {
            return Some(p);
        }
    }
    None
}

fn try_exec_dashed(cmd: &str, rest: &[String], opts: &GlobalOpts) -> Result<bool> {
    let ext_cmd = format!("git-{cmd}");
    let exec_path = crate::git_exec_path_for_helpers(opts.exec_path.as_deref());
    if let Some(ext_path) = find_git_external_helper(cmd, exec_path.as_deref()) {
        let status = std::process::Command::new(&ext_path)
            .args(rest.iter())
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run {}: {}", ext_cmd, e))?;
        crate::exit_with_status(status);
    }
    Ok(false)
}

/// Collects alias names and expansion strings for the `git help -a` "Command aliases" listing.
pub(crate) fn list_aliases_from_config(config: &ConfigSet) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for e in config.entries() {
        let Some(val) = &e.value else {
            continue;
        };
        let key = &e.key;
        if !key.starts_with("alias.") {
            continue;
        }
        if key.ends_with(".command") {
            let name = key
                .strip_prefix("alias.")
                .unwrap()
                .strip_suffix(".command")
                .unwrap();
            out.push((name.to_string(), val.clone()));
        } else if key.starts_with("alias..") {
            let name = key.strip_prefix("alias..").unwrap();
            out.push((name.to_string(), val.clone()));
        } else {
            let rest = key.strip_prefix("alias.").unwrap();
            if !rest.contains('.') {
                out.push((rest.to_string(), val.clone()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
