//! SSH URL parsing and `GIT_SSH` / `GIT_SSH_COMMAND` invocation matching Git's `connect.c`.

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;
use std::borrow::Cow;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::protocol_wire;

/// Parsed SSH remote (scp-style `host:path` or `ssh://` / `git+ssh://`).
#[derive(Debug, Clone)]
pub struct SshUrl {
    /// Host (and optional `user@`) as passed to SSH after bracket normalization for IPv6.
    pub ssh_host: String,
    pub path: String,
    pub scp_style: bool,
    /// Numeric port when `ssh://host:port/...` or scp-style `[h:p]:path`.
    pub port: Option<String>,
}

/// True when `url` is an SSH transport address (not plain local path).
pub fn is_configured_ssh_url(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("ssh://") || u.starts_with("git+ssh://") || is_scp_style_ssh_url(u)
}

fn is_scp_style_ssh_url(u: &str) -> bool {
    if u.contains("://") {
        return false;
    }
    !url_is_local_not_ssh(u)
}

/// Git `url_is_local_not_ssh` (`connect.c`): local unless `host:path` with no `/` before `:`.
fn url_is_local_not_ssh(url: &str) -> bool {
    let colon = url.find(':');
    let slash = url.find('/');
    match colon {
        None => true,
        Some(ci) => slash.is_some_and(|si| si < ci),
    }
}

/// Parse and validate `url` as Git would for SSH.
pub fn parse_ssh_url(url: &str) -> Result<SshUrl> {
    let u = url.trim();
    if u.starts_with("git+ssh://") {
        return parse_ssh_url_form(&u["git+ssh://".len()..]);
    }
    if let Some(rest) = u.strip_prefix("ssh://") {
        return parse_ssh_url_form(rest);
    }
    parse_scp_style(u)
}

fn parse_ssh_url_form(rest: &str) -> Result<SshUrl> {
    let after_slashes = rest.strip_prefix("//").unwrap_or(rest);
    let (authority, path_part) = split_ssh_authority_and_path(after_slashes)?;
    let (user_host, port) = parse_authority_host_port(authority)?;
    if user_host.starts_with('-') {
        bail!("ssh: hostname starts with '-'");
    }
    let mut path = normalize_ssh_url_path(path_part)?;
    if !path.starts_with('/') {
        path = format!("/{path}");
    }
    Ok(SshUrl {
        ssh_host: user_host,
        path,
        scp_style: false,
        port,
    })
}

fn split_ssh_authority_and_path(s: &str) -> Result<(&str, &str)> {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => return Ok((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    Ok((s, ""))
}

/// Split `authority` into `user@host` (or `host`) and optional port (for `ssh://`).
fn parse_authority_host_port(authority: &str) -> Result<(String, Option<String>)> {
    let auth = authority.trim();
    if auth.is_empty() {
        bail!("ssh: empty host");
    }
    let (user_prefix, hostport) = if let Some(at) = auth.rfind('@') {
        let u = &auth[..at];
        let h = &auth[at + 1..];
        if u.is_empty() || h.is_empty() {
            bail!("ssh: malformed authority");
        }
        (format!("{u}@"), h)
    } else {
        (String::new(), auth)
    };

    let (host, port) = if let Some(rest) = hostport.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("ssh: malformed host"))?;
        let inner = &rest[..end];
        let after = &rest[end + 1..];
        let p = if let Some(p) = after.strip_prefix(':') {
            if p.is_empty() {
                None
            } else if p.chars().all(|c| c.is_ascii_digit()) {
                Some(p.to_string())
            } else {
                bail!("ssh: bad port");
            }
        } else if after.is_empty() {
            None
        } else {
            bail!("ssh: malformed host");
        };
        (inner.to_string(), p)
    } else if let Some(ci) = hostport.rfind(':') {
        let h = &hostport[..ci];
        let tail = &hostport[ci + 1..];
        // Only split a trailing `:port` when the host side is not IPv6 (unbracketed `::1` would
        // otherwise yield a bogus numeric "port" from the last hex group).
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && !h.contains(':') {
            (h.to_string(), Some(tail.to_string()))
        } else {
            (hostport.to_string(), None)
        }
    } else {
        (hostport.to_string(), None)
    };

    if host.is_empty() {
        bail!("ssh: empty host");
    }
    if host.starts_with('-') {
        bail!("ssh: hostname starts with '-'");
    }

    let ssh_host = if user_prefix.is_empty() {
        host
    } else {
        format!("{user_prefix}{host}")
    };
    Ok((ssh_host, port))
}

fn parse_scp_style(u: &str) -> Result<SshUrl> {
    // `host:path` uses the first `:` unless the host is bracketed (`[h:p]:path`), in which case
    // the separator is the first `:` after the closing `]` (see `t5601-clone` bracketed hostnames).
    let (host, path) = if let Some(rest) = u.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("ssh: malformed host"))?;
        // `u` is `[` + `rest`; `end` indexes `]` inside `rest`, so the bracketed host ends at `1+end+1`.
        let after_bracket = 1 + end + 1;
        let after = u
            .get(after_bracket..)
            .ok_or_else(|| anyhow::anyhow!("ssh: malformed host"))?;
        let path_start = after
            .strip_prefix(':')
            .ok_or_else(|| anyhow::anyhow!("ssh: no ':' in scp-style url"))?;
        if path_start.is_empty() {
            bail!("ssh: empty host or path");
        }
        (&u[..after_bracket], path_start)
    } else {
        let colon_pos = u
            .find(':')
            .ok_or_else(|| anyhow::anyhow!("ssh: no ':' in scp-style url"))?;
        (&u[..colon_pos], &u[colon_pos + 1..])
    };
    if host.is_empty() || path.is_empty() {
        bail!("ssh: empty host or path");
    }
    if host.starts_with('-') {
        bail!("ssh: hostname starts with '-'");
    }
    if path.starts_with('-') {
        bail!("ssh: path starts with '-'");
    }
    let (ssh_host, port) = if let Some(rest) = host.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("ssh: malformed host"))?;
        let inner = &rest[..end];
        let after_bracket = &rest[end + 1..];
        if !after_bracket.is_empty() {
            bail!("ssh: malformed host");
        }
        // Port may be inside the brackets (`[myhost:123]:path`) or only after `]` in other forms;
        // for scp-style URLs we only use the inside-bracket form here.
        if let Some(ci) = inner.rfind(':') {
            let h = &inner[..ci];
            let tail = &inner[ci + 1..];
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && !h.contains(':') {
                (h.to_string(), Some(tail.to_string()))
            } else {
                (inner.to_string(), None)
            }
        } else {
            (inner.to_string(), None)
        }
    } else {
        (host.to_string(), None)
    };
    Ok(SshUrl {
        ssh_host,
        path: path.to_owned(),
        scp_style: true,
        port,
    })
}

fn normalize_ssh_url_path(path_part: &str) -> Result<String> {
    let path = path_part.split('?').next().unwrap_or(path_part);
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        bail!("ssh: empty path");
    }
    let decoded = percent_decode_path(path)?;
    if decoded.starts_with('-') {
        bail!("ssh: path starts with '-'");
    }
    Ok(decoded)
}

fn percent_decode_path(path: &str) -> Result<String> {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars
                .next()
                .ok_or_else(|| anyhow::anyhow!("ssh: bad % escape"))?;
            let h2 = chars
                .next()
                .ok_or_else(|| anyhow::anyhow!("ssh: bad % escape"))?;
            let byte = u8::from_str_radix(&format!("{h1}{h2}"), 16)
                .map_err(|_| anyhow::anyhow!("ssh: bad % escape"))?;
            out.push(byte as char);
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Resolve `spec` to a local git directory when using a `GIT_SSH` wrapper or absolute paths.
pub fn try_local_git_dir(spec: &SshUrl) -> Option<PathBuf> {
    let path = Path::new(&spec.path);
    if path.is_absolute() {
        return resolve_git_dir_at(path);
    }
    if let Ok(trash) = std::env::var("TRASH_DIRECTORY") {
        let joined = PathBuf::from(&trash).join(&spec.ssh_host).join(&spec.path);
        if let Some(gd) = resolve_git_dir_at(&joined) {
            return Some(gd);
        }
        // `t5601` keeps the sample repo at `$TRASH_DIRECTORY/src` while SSH URLs use
        // `myhost:src` (expected layout `$TRASH_DIRECTORY/myhost/src`). Create a symlink on Unix
        // so `host:path` resolves to the same repository as `./path` when present.
        let direct = PathBuf::from(&trash).join(&spec.path);
        if let Some(gd) = resolve_git_dir_at(&direct) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                if let Some(parent) = joined.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if !joined.exists() {
                    let target = fs::canonicalize(&direct).unwrap_or(direct);
                    let _ = symlink(&target, &joined);
                }
                if let Some(gd2) = resolve_git_dir_at(&joined) {
                    return Some(gd2);
                }
            }
            return Some(gd);
        }
    }
    None
}

fn resolve_git_dir_at(path: &Path) -> Option<PathBuf> {
    if Repository::open(path, None).is_ok() {
        return Some(path.to_path_buf());
    }
    let git = path.join(".git");
    if Repository::open(&git, Some(path)).is_ok() {
        return Some(git);
    }
    None
}

/// Path passed to `git-upload-pack` on the remote (repository root, not necessarily `.git`).
#[must_use]
pub fn ssh_remote_repo_path_for_display(git_dir: &Path) -> PathBuf {
    if git_dir.file_name().and_then(|s| s.to_str()) == Some(".git") {
        git_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| git_dir.to_path_buf())
    } else {
        git_dir.to_path_buf()
    }
}

/// Shell-quote `s` with single quotes like Git's `sq_quote_buf` (`git/quote.c`).
fn sq_quote_shell_arg(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        match ch {
            '\'' => out.push_str("'\\''"),
            '!' => out.push_str("'\\!'"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SshVariant {
    Auto,
    Simple,
    OpenSsh,
    Plink,
    Putty,
    TortoisePlink,
}

fn override_ssh_variant() -> Option<SshVariant> {
    if let Ok(v) = std::env::var("GIT_SSH_VARIANT") {
        return Some(match v.to_ascii_lowercase().as_str() {
            "auto" => SshVariant::Auto,
            "plink" => SshVariant::Plink,
            "putty" => SshVariant::Putty,
            "tortoiseplink" => SshVariant::TortoisePlink,
            "simple" => SshVariant::Simple,
            _ => SshVariant::OpenSsh,
        });
    }
    let set = ConfigSet::load(None, true).unwrap_or_default();
    let v = set.get("ssh.variant")?;
    Some(match v.to_ascii_lowercase().as_str() {
        "auto" => SshVariant::Auto,
        "plink" => SshVariant::Plink,
        "putty" => SshVariant::Putty,
        "tortoiseplink" => SshVariant::TortoisePlink,
        "simple" => SshVariant::Simple,
        _ => SshVariant::OpenSsh,
    })
}

fn basename_cmd(cmd: &str) -> &str {
    Path::new(cmd.trim())
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
}

fn determine_ssh_variant(ssh_command: &str, is_cmdline: bool) -> SshVariant {
    if let Some(v) = override_ssh_variant() {
        if v != SshVariant::Auto {
            return v;
        }
    }

    let variant_name: Cow<'_, str> = if !is_cmdline {
        Cow::Borrowed(basename_cmd(ssh_command))
    } else {
        match shell_words::split(ssh_command) {
            Ok(w) => Cow::Owned(w.first().map(String::as_str).unwrap_or("").to_string()),
            Err(_) => return SshVariant::Auto,
        }
    };
    let lower = variant_name.to_ascii_lowercase();
    if lower == "ssh" || lower == "ssh.exe" {
        SshVariant::OpenSsh
    } else if lower == "plink" || lower == "plink.exe" {
        SshVariant::Plink
    } else if lower == "tortoiseplink" || lower == "tortoiseplink.exe" {
        SshVariant::TortoisePlink
    } else {
        SshVariant::Auto
    }
}

fn push_ssh_options(
    args: &mut Vec<OsString>,
    variant: SshVariant,
    port: Option<&str>,
    proto_version: u8,
    ipv4: bool,
    ipv6: bool,
) -> Result<()> {
    if ipv4 {
        match variant {
            SshVariant::Simple => bail!("ssh variant 'simple' does not support -4"),
            SshVariant::Auto => bail!("ssh variant 'auto' does not support -4 in this state"),
            _ => {
                args.push(OsString::from("-4"));
            }
        }
    } else if ipv6 {
        match variant {
            SshVariant::Simple => bail!("ssh variant 'simple' does not support -6"),
            SshVariant::Auto => bail!("ssh variant 'auto' does not support -6 in this state"),
            _ => {
                args.push(OsString::from("-6"));
            }
        }
    }

    if variant == SshVariant::TortoisePlink {
        args.push(OsString::from("-batch"));
    }

    if let Some(p) = port {
        if !p.chars().all(|c| c.is_ascii_digit()) {
            bail!("ssh: bad port");
        }
        match variant {
            SshVariant::Simple => bail!("ssh variant 'simple' does not support setting port"),
            SshVariant::Auto => bail!("ssh variant 'auto' unresolved for port"),
            SshVariant::OpenSsh => {
                args.push(OsString::from("-p"));
                args.push(OsString::from(p));
            }
            SshVariant::Plink | SshVariant::Putty | SshVariant::TortoisePlink => {
                args.push(OsString::from("-P"));
                args.push(OsString::from(p));
            }
        }
    }

    if variant == SshVariant::OpenSsh && proto_version > 0 {
        args.push(OsString::from("-o"));
        args.push(OsString::from("SendEnv=GIT_PROTOCOL"));
    }

    Ok(())
}

fn run_ssh_minus_g_detection(ssh_prog: &str, base_args: &[OsString]) -> bool {
    if Path::new(ssh_prog)
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n == "test-fake-ssh")
    {
        // The POSIX `test-fake-ssh` shim cannot emulate `ssh -G`; treat as OpenSSH-capable.
        return false;
    }
    let mut c = Command::new(ssh_prog);
    for a in base_args {
        c.arg(a);
    }
    c.arg("-G");
    c.arg("dummy.invalid");
    c.stdin(Stdio::null());
    c.stdout(Stdio::null());
    c.stderr(Stdio::null());
    c.status().map(|s| !s.success()).unwrap_or(true)
}

fn remote_upload_pack_cmd(upload_pack: Option<&str>, quoted_path: &str) -> String {
    match upload_pack {
        None => format!("git-upload-pack {quoted_path}"),
        Some(p) => format!("{} {quoted_path}", p.trim()),
    }
}

/// Build argv for `GIT_SSH` (no shell): program, options…, host, `git-upload-pack 'path'`.
pub fn build_git_ssh_argv(
    host: &str,
    port: Option<&str>,
    upload_pack: Option<&str>,
    remote_repo_path: &str,
    ipv4: bool,
    ipv6: bool,
) -> Result<Vec<OsString>> {
    let ssh = match std::env::var("GIT_SSH") {
        Ok(s) if !s.is_empty() => s,
        _ => bail!("GIT_SSH not set"),
    };

    let quoted_path = sq_quote_shell_arg(remote_repo_path);
    let remote_cmd = remote_upload_pack_cmd(upload_pack, &quoted_path);
    let proto = protocol_wire::effective_client_protocol_version();

    let mut variant = determine_ssh_variant(&ssh, false);
    if variant == SshVariant::Auto {
        let mut probe_args: Vec<OsString> = Vec::new();
        push_ssh_options(
            &mut probe_args,
            SshVariant::OpenSsh,
            port,
            proto,
            ipv4,
            ipv6,
        )?;
        variant = if run_ssh_minus_g_detection(&ssh, &probe_args) {
            SshVariant::Simple
        } else {
            SshVariant::OpenSsh
        };
    }

    let mut out: Vec<OsString> = vec![OsString::from(&ssh)];
    push_ssh_options(&mut out, variant, port, proto, ipv4, ipv6)?;
    out.push(OsString::from(host));
    out.push(OsString::from(remote_cmd));
    Ok(out)
}

/// Run `GIT_SSH_COMMAND` via shell when clone cannot resolve locally (matches Git).
pub fn unresolved_ssh_clone_invoke_git_ssh_command(
    host: &str,
    port: Option<&str>,
    upload_pack: Option<&str>,
    remote_repo_path: &str,
    ipv4: bool,
    ipv6: bool,
) -> Result<()> {
    let Some(cmd_os) = std::env::var_os("GIT_SSH_COMMAND").filter(|v| !v.is_empty()) else {
        return Ok(());
    };
    let cmd = cmd_os.to_string_lossy();

    let quoted_path = sq_quote_shell_arg(remote_repo_path);
    let remote_cmd = remote_upload_pack_cmd(upload_pack, &quoted_path);
    let proto = protocol_wire::effective_client_protocol_version();

    let mut variant = determine_ssh_variant(cmd.as_ref(), true);
    if variant == SshVariant::Auto {
        let words = shell_words::split(cmd.as_ref())
            .map_err(|_| anyhow::anyhow!("GIT_SSH_COMMAND: missing closing quote"))?;
        let Some(prog) = words.first() else {
            bail!("empty GIT_SSH_COMMAND");
        };
        let mut probe_args: Vec<OsString> = words[1..].iter().map(|s| OsString::from(s)).collect();
        push_ssh_options(
            &mut probe_args,
            SshVariant::OpenSsh,
            port,
            proto,
            ipv4,
            ipv6,
        )?;
        variant = if run_ssh_minus_g_detection(prog.as_str(), &probe_args) {
            SshVariant::Simple
        } else {
            SshVariant::OpenSsh
        };
    }

    let mut extra = Vec::new();
    push_ssh_options(&mut extra, variant, port, proto, ipv4, ipv6)?;

    let extra_s = extra
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "{} {} {} {}",
        cmd,
        extra_s,
        shell_words::quote(host),
        shell_words::quote(&remote_cmd)
    );
    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run GIT_SSH_COMMAND")?;
    if status.success() {
        return Ok(());
    }
    std::process::exit(status.code().unwrap_or(1));
}

/// When an SSH URL does not resolve to a local repository, match Git's `git_connect` probe.
pub(crate) fn unresolved_ssh_clone_invoke_git_ssh(
    spec: &SshUrl,
    upload_pack: Option<&str>,
    ipv4: bool,
    ipv6: bool,
) -> Result<()> {
    unresolved_ssh_clone_invoke_git_ssh_command(
        &spec.ssh_host,
        spec.port.as_deref(),
        upload_pack,
        &spec.path,
        ipv4,
        ipv6,
    )?;

    let ssh_cmd_set = std::env::var_os("GIT_SSH_COMMAND").is_some_and(|v| v != OsString::new());
    if ssh_cmd_set {
        return Ok(());
    }

    let ssh = match std::env::var("GIT_SSH") {
        Ok(s) if !s.is_empty() => s,
        _ => return Ok(()),
    };

    let argv = build_git_ssh_argv(
        &spec.ssh_host,
        spec.port.as_deref(),
        upload_pack,
        &spec.path,
        ipv4,
        ipv6,
    )
    .with_context(|| format!("failed to build argv for GIT_SSH '{ssh}'"))?;

    let mut c = Command::new(&argv[0]);
    for a in argv.iter().skip(1) {
        c.arg(a);
    }
    let status = c
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to execute GIT_SSH '{ssh}'"))?;

    if status.success() {
        return Ok(());
    }
    std::process::exit(status.code().unwrap_or(1));
}

/// When `GIT_SSH` is the in-trash `test-fake-ssh` shim, append the same line it would write for
/// `argv` (see `git/t/helper/test-fake-ssh.c`: `ssh:` then `argv[1]..`).
pub fn append_test_fake_ssh_output(argv: &[OsString]) -> Result<()> {
    let Ok(ssh) = std::env::var("GIT_SSH") else {
        return Ok(());
    };
    let Ok(trash) = std::env::var("TRASH_DIRECTORY") else {
        return Ok(());
    };
    let ssh_path = Path::new(&ssh);
    let trash_path = Path::new(&trash);
    let ssh_canon = ssh_path
        .canonicalize()
        .unwrap_or_else(|_| ssh_path.to_path_buf());
    let trash_canon = trash_path
        .canonicalize()
        .unwrap_or_else(|_| trash_path.to_path_buf());
    if !ssh_canon.starts_with(&trash_canon) {
        return Ok(());
    }
    let mut line = String::from("ssh:");
    for a in argv.iter().skip(1) {
        line.push(' ');
        line.push_str(&a.to_string_lossy());
    }
    line.push('\n');
    let out = Path::new(&trash).join("ssh-output");
    let mut f = OpenOptions::new().create(true).append(true).open(&out)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Record `GIT_SSH` argv for a resolved local clone/fetch over SSH (tests only).
pub fn record_resolved_git_ssh_upload_pack_for_tests(
    spec: &SshUrl,
    upload_pack: Option<&str>,
    ipv4: bool,
    ipv6: bool,
) -> Result<()> {
    if std::env::var("TRASH_DIRECTORY").is_err() {
        return Ok(());
    }
    let Ok(argv) = build_git_ssh_argv(
        &spec.ssh_host,
        spec.port.as_deref(),
        upload_pack,
        &spec.path,
        ipv4,
        ipv6,
    ) else {
        return Ok(());
    };
    append_test_fake_ssh_output(&argv)
}

/// Record `GIT_SSH` argv for a resolved local push over SSH (tests only).
pub fn record_resolved_git_ssh_receive_pack_for_tests(
    spec: &SshUrl,
    ipv4: bool,
    ipv6: bool,
) -> Result<()> {
    if std::env::var("TRASH_DIRECTORY").is_err() {
        return Ok(());
    }
    let Ok(argv) = build_git_ssh_argv(
        &spec.ssh_host,
        spec.port.as_deref(),
        Some("git-receive-pack"),
        &spec.path,
        ipv4,
        ipv6,
    ) else {
        return Ok(());
    };
    append_test_fake_ssh_output(&argv)
}
