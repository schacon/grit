//! `grit credential` — retrieve and store user credentials.
//!
//! Implements the Git credential helper protocol:
//! - `fill`    — read credential spec from stdin, output filled credentials
//! - `approve` — mark credentials as good (`store` in helpers)
//! - `reject`  — mark credentials as bad (`erase` in helpers)
//!
//! Reads key=value pairs (protocol, host, username, password, path) from
//! stdin and passes them through the configured credential helpers.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use url::Url;

/// Arguments for `grit credential`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub action: CredentialAction,
}

#[derive(Debug, Subcommand)]
pub enum CredentialAction {
    /// Read credential spec from stdin, output filled credentials.
    Fill,
    /// Mark credentials as good.
    Approve,
    /// Mark credentials as bad.
    Reject,
}

/// Parse credential key=value pairs from stdin until a blank line or EOF.
fn read_credential_input() -> Result<BTreeMap<String, String>> {
    let stdin = io::stdin();
    let mut map = BTreeMap::new();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Ok(map)
}

fn host_header_value(url: &Url) -> String {
    let host = url.host_str().unwrap_or("localhost");
    match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    }
}

/// Normalize `url=<scheme>://...` into protocol/host/path/username/password fields.
///
/// Git helpers commonly receive either split fields or a single `url=...` input.
fn normalize_url_field(creds: &mut BTreeMap<String, String>) -> Result<()> {
    let Some(raw_url) = creds.get("url").cloned() else {
        return Ok(());
    };
    let parsed =
        Url::parse(&raw_url).map_err(|e| anyhow::anyhow!("invalid credential url: {e}"))?;
    if !creds.contains_key("protocol") {
        creds.insert("protocol".to_string(), parsed.scheme().to_string());
    }
    if !creds.contains_key("host") {
        creds.insert("host".to_string(), host_header_value(&parsed));
    }
    if !creds.contains_key("path") {
        let path = parsed.path().trim_start_matches('/');
        if !path.is_empty() {
            creds.insert("path".to_string(), path.to_string());
        }
    }
    if !creds.contains_key("username") && !parsed.username().is_empty() {
        creds.insert("username".to_string(), parsed.username().to_string());
    }
    if !creds.contains_key("password") {
        if let Some(password) = parsed.password() {
            creds.insert("password".to_string(), password.to_string());
        }
    }
    Ok(())
}

/// Discover the `.git` directory by walking up from the current directory.
fn find_git_dir() -> Option<std::path::PathBuf> {
    // Check GIT_DIR env var first
    if let Ok(d) = std::env::var("GIT_DIR") {
        let p = std::path::PathBuf::from(&d);
        if p.is_dir() {
            return Some(p);
        }
    }
    // Walk up from cwd looking for .git
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            let dot_git = dir.join(".git");
            if dot_git.is_dir() {
                return Some(dot_git);
            }
            // Bare repo check
            if dir.join("HEAD").is_file() && dir.join("objects").is_dir() {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

/// Build the effective `credential.helper` list in Git order.
///
/// Git walks every `credential.helper` and `credential.<URL>.helper` config entry in
/// load order. URL-scoped entries only apply when the subsection pattern matches
/// `target_url` (per Git's URL-match rules). For every applicable entry, a non-empty
/// value is appended to the helper list and an empty value resets it (Git's
/// `string_list_clear` semantics in `credential_apply_config_cb`).
///
/// `target_url` is the URL we're authenticating against (e.g.
/// `https://github.com/owner/repo.git`). When `None`, only unscoped
/// `credential.helper` entries contribute.
fn credential_helpers(
    config: &grit_lib::config::ConfigSet,
    target_url: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    for entry in config.entries() {
        let key = &entry.key;
        let Some(first_dot) = key.find('.') else {
            continue;
        };
        let Some(last_dot) = key.rfind('.') else {
            continue;
        };
        let section = &key[..first_dot];
        let variable = &key[last_dot + 1..];
        if !section.eq_ignore_ascii_case("credential") || !variable.eq_ignore_ascii_case("helper") {
            continue;
        }
        if first_dot != last_dot {
            let subsection = &key[first_dot + 1..last_dot];
            let Some(target) = target_url else {
                continue;
            };
            if !grit_lib::config::url_matches(subsection, target) {
                continue;
            }
        }
        let value = entry.value.as_deref().unwrap_or("");
        if value.trim().is_empty() {
            out.clear();
        } else {
            out.push(value.to_string());
        }
    }
    out
}

/// Reconstruct the target URL from credential input fields.
///
/// Git helpers are scoped per-URL. The credential protocol either provides
/// `url=<full url>` directly or split fields (`protocol`, `host`, `path`); this
/// helper produces a single URL string for [`grit_lib::config::url_matches`]. Returns
/// `None` when there is not enough information to build a URL.
fn target_url_from_credentials(creds: &BTreeMap<String, String>) -> Option<String> {
    if let Some(u) = creds.get("url") {
        if !u.trim().is_empty() {
            return Some(u.clone());
        }
    }
    let protocol = creds.get("protocol")?;
    let host = creds.get("host")?;
    let mut url = format!("{protocol}://{host}");
    if let Some(path) = creds.get("path").filter(|p| !p.is_empty()) {
        if !path.starts_with('/') {
            url.push('/');
        }
        url.push_str(path);
    }
    Some(url)
}

/// Directories to search for `git-credential-*` the same way Git does (`exec_path` before `PATH`).
///
/// Git installs helpers under `/usr/libexec/git-core` on macOS; they are not on `PATH`, so a bare
/// [`Command::new`] lookup fails while `git credential` still works.
fn credential_helper_exec_path_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(ep) = std::env::var("GIT_EXEC_PATH") {
        let p = PathBuf::from(ep.trim());
        if p.is_dir() {
            v.push(p);
        }
    }
    for candidate in [
        "/usr/libexec/git-core",
        "/Library/Developer/CommandLineTools/usr/libexec/git-core",
        "/opt/homebrew/opt/git/libexec/git-core",
        "/opt/homebrew/libexec/git-core",
        "/usr/lib/git-core",
        "/usr/local/libexec/git-core",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_dir() {
            v.push(p);
        }
    }
    if let Some(p) = crate::git_exec_path_for_helpers(None) {
        v.push(p);
    }
    v
}

fn resolve_credential_helper_executable(helper_program: &str) -> PathBuf {
    if helper_program.contains('/') {
        return PathBuf::from(helper_program);
    }
    if helper_program.starts_with("git-credential-") {
        let cmd = helper_program
            .strip_prefix("git-")
            .unwrap_or(helper_program);
        for ep in credential_helper_exec_path_candidates() {
            if let Some(p) = crate::alias::find_git_external_helper(cmd, Some(&ep)) {
                return p;
            }
        }
        if let Some(p) = crate::alias::find_git_external_helper(cmd, None) {
            return p;
        }
    }
    PathBuf::from(helper_program)
}

/// Invoke an external credential helper program.
///
/// The helper may be:
/// - shell form: `!command ...` (executed by `sh -c`)
/// - absolute/relative path containing `/`
/// - bare helper name (expanded to `git-credential-<name>`)
/// - already-expanded binary (`git-credential-...`)
///
/// The helper is invoked with one action argument (`get`, `store`, `erase`).
/// Credential fields are written to stdin as `key=value` lines followed by a
/// blank line; stdout is parsed back into key/value pairs.
fn invoke_helper(
    helper: &str,
    action: &str,
    creds: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let helper_words = shell_words::split(helper)
        .map_err(|e| anyhow::anyhow!("invalid credential.helper '{helper}': {e}"))?;
    let (first_word, extra_args) = if let Some((first, rest)) = helper_words.split_first() {
        (first.as_str(), rest)
    } else {
        ("", &[][..])
    };

    let mut child = if let Some(shell_cmd) = helper.strip_prefix('!') {
        Command::new("sh")
            .arg("-c")
            .arg(format!("{shell_cmd} {action}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to run credential helper shell '{helper}': {e}"))?
    } else if matches!(
        first_word,
        "store" | "cache" | "git-credential-store" | "git-credential-cache"
    ) {
        let subcmd = if first_word.ends_with("store") {
            "credential-store"
        } else {
            "credential-cache"
        };
        let exe = std::env::current_exe().context("resolve current executable")?;
        let mut cmd = Command::new(exe);
        cmd.arg(subcmd).arg(action);
        for arg in extra_args {
            cmd.arg(arg);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("failed to run built-in credential helper '{subcmd}': {e}")
            })?
    } else {
        let helper_program = if first_word.contains('/') {
            first_word.to_string()
        } else if first_word.starts_with("git-credential-") {
            first_word.to_string()
        } else {
            format!("git-credential-{first_word}")
        };
        let resolved = resolve_credential_helper_executable(&helper_program);
        let mut cmd = Command::new(&resolved);
        cmd.arg(action);
        for arg in extra_args {
            cmd.arg(arg);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("failed to run credential helper '{helper_program}': {e}")
            })?
    };

    // Write credential fields to helper's stdin, followed by blank line.
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for (key, value) in creds {
            writeln!(stdin, "{key}={value}")?;
        }
        writeln!(stdin)?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "credential helper '{}' exited with status {}",
            helper,
            output.status
        );
    }

    // Parse helper output — key=value lines until blank line or EOF.
    let mut result = creds.clone();
    for line in output.stdout.split(|&b| b == b'\n') {
        let line = std::str::from_utf8(line)
            .unwrap_or("")
            .trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            result.insert(key.to_string(), value.to_string());
        }
    }

    Ok(result)
}

/// Run `grit credential`.
pub fn run(args: Args) -> Result<()> {
    let mut creds = read_credential_input()?;
    normalize_url_field(&mut creds)?;

    let target_url = target_url_from_credentials(&creds);

    match args.action {
        CredentialAction::Fill => {
            // For fill, we need at minimum protocol and host.
            if !creds.contains_key("protocol") || !creds.contains_key("host") {
                bail!("credential input must include protocol and host");
            }

            // Try configured credential helpers in order (Git runs the full chain, not only the
            // last `credential.helper` value).
            let git_dir = find_git_dir();
            let config =
                grit_lib::config::ConfigSet::load(git_dir.as_deref(), true).unwrap_or_default();
            let mut filled = creds.clone();
            for helper in credential_helpers(&config, target_url.as_deref()) {
                filled = invoke_helper(&helper, "get", &filled)?;
            }

            // Output all known fields.
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for (key, value) in &filled {
                writeln!(out, "{key}={value}")?;
            }
            writeln!(out)?;
        }
        CredentialAction::Approve => {
            let git_dir = find_git_dir();
            let config =
                grit_lib::config::ConfigSet::load(git_dir.as_deref(), true).unwrap_or_default();
            for helper in credential_helpers(&config, target_url.as_deref()) {
                let _ = invoke_helper(&helper, "store", &creds)?;
            }
        }
        CredentialAction::Reject => {
            let git_dir = find_git_dir();
            let config =
                grit_lib::config::ConfigSet::load(git_dir.as_deref(), true).unwrap_or_default();
            for helper in credential_helpers(&config, target_url.as_deref()) {
                let _ = invoke_helper(&helper, "erase", &creds)?;
            }
        }
    }

    Ok(())
}
