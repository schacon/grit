//! `grit daemon` — Git protocol daemon.
//!
//! Supports `--inetd` mode used by tests and `ext::` git:// bridging: read one daemon request
//! packet from stdin, resolve the repository path (`--base-path`, `--interpolated-path`), then
//! exec `grit upload-pack` with stdin/stdout inherited so the remaining wire bytes reach
//! upload-pack.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::grit_exe::grit_executable;
use crate::pkt_line;

/// Arguments for `grit daemon`.
#[derive(Debug, ClapArgs)]
#[command(about = "A really simple server for Git repositories")]
pub struct Args {
    /// Base path for all served repositories.
    #[arg(long = "base-path")]
    pub base_path: Option<PathBuf>,

    /// Template for virtual hosting (`%H` hostname, `%D` request path).
    #[arg(long = "interpolated-path")]
    pub interpolated_path: Option<String>,

    /// Listen on a specific port (default: 9418).
    #[arg(long)]
    pub port: Option<u16>,

    /// Export all repositories without needing git-daemon-export-ok.
    #[arg(long = "export-all")]
    pub export_all: bool,

    /// Run in inetd mode.
    #[arg(long)]
    pub inetd: bool,

    /// Enable verbose logging.
    #[arg(long)]
    pub verbose: bool,

    /// Initial timeout in seconds (0 means no timeout).
    #[arg(long = "init-timeout")]
    pub init_timeout: Option<String>,

    /// Idle timeout in seconds (0 means no timeout).
    #[arg(long)]
    pub timeout: Option<String>,

    /// Maximum number of simultaneous connections.
    #[arg(long = "max-connections")]
    pub max_connections: Option<String>,

    /// More detailed error messages over the wire (accepted; inetd uses stderr only).
    #[arg(long = "informative-errors")]
    pub informative_errors: bool,

    /// Directories to serve.
    #[arg(value_name = "DIRECTORY")]
    pub directories: Vec<PathBuf>,
}

/// Validate that a string is a non-negative integer; die with git-compatible
/// `fatal:` message on failure.
fn validate_non_negative_int(value: &str, name: &str) {
    match value.parse::<i64>() {
        Ok(n) if n >= 0 => {}
        _ => {
            eprintln!(
                "fatal: invalid {} '{}', expecting a non-negative integer",
                name, value
            );
            std::process::exit(128);
        }
    }
}

/// Validate that a string is an integer (may be negative); die with
/// git-compatible `fatal:` message on failure.
fn validate_int(value: &str, name: &str) {
    match value.parse::<i64>() {
        Ok(_) => {}
        _ => {
            eprintln!("fatal: invalid {} '{}', expecting an integer", name, value);
            std::process::exit(128);
        }
    }
}

/// Run `grit daemon`.
pub fn run(args: Args) -> Result<()> {
    if let Some(ref v) = args.init_timeout {
        validate_non_negative_int(v, "init-timeout");
    }
    if let Some(ref v) = args.timeout {
        validate_non_negative_int(v, "timeout");
    }
    if let Some(ref v) = args.max_connections {
        validate_int(v, "max-connections");
    }

    if args.inetd {
        return run_inetd(args);
    }

    bail!("daemon mode is not yet supported in grit")
}

fn expand_interpolated_path(template: &str, host_lc: &str, request_path: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('H') => out.push_str(host_lc),
                Some('D') => out.push_str(request_path),
                Some('%') => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn extract_host_from_request(full: &str) -> Option<String> {
    for seg in full.split('\0').skip(1) {
        if let Some(h) = seg.strip_prefix("host=") {
            let host = h.split(':').next().unwrap_or(h).trim();
            if !host.is_empty() {
                return Some(host.to_ascii_lowercase());
            }
        }
    }
    None
}

fn looks_like_git_repo(path: &Path) -> bool {
    path.join("objects").is_dir() && path.join("refs").is_dir()
}

fn validate_under_roots(resolved: &Path, roots: &[PathBuf]) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let canon = resolved
        .canonicalize()
        .with_context(|| format!("repository path {}", resolved.display()))?;
    for root in roots {
        let r = root
            .canonicalize()
            .with_context(|| format!("daemon root {}", root.display()))?;
        if canon.starts_with(&r) {
            return Ok(());
        }
    }
    bail!(
        "path '{}' is not under allowed daemon directories",
        resolved.display()
    );
}

fn run_inetd(args: Args) -> Result<()> {
    let mut stdin_lock = io::stdin().lock();
    let pkt = match pkt_line::read_packet(&mut stdin_lock)
        .map_err(|e| anyhow::anyhow!("read daemon request: {e}"))?
    {
        Some(p) => p,
        None => return Ok(()),
    };
    drop(stdin_lock);

    let line = match pkt {
        pkt_line::Packet::Data(s) => s,
        _ => {
            if args.informative_errors {
                eprintln!("fatal: protocol error");
            }
            std::process::exit(1);
        }
    };

    let first_line = line
        .split('\0')
        .next()
        .unwrap_or(&line)
        .trim_end_matches('\n')
        .trim();
    let mut parts = first_line.split_whitespace();
    let service = parts.next().context("empty git-daemon request")?;
    if service != "git-upload-pack" {
        bail!("unsupported git-daemon service: {service}");
    }
    let repo_arg = parts
        .next()
        .context("git-daemon request missing repository path")?;
    if !repo_arg.starts_with('/') {
        bail!("non-absolute repository path in git-daemon request: {repo_arg}");
    }

    let fs_path = if let Some(ref tpl) = args.interpolated_path {
        let host_lc = extract_host_from_request(&line).unwrap_or_default();
        PathBuf::from(expand_interpolated_path(tpl, &host_lc, repo_arg))
    } else if let Some(ref bp) = args.base_path {
        bp.join(repo_arg.trim_start_matches('/'))
    } else {
        PathBuf::from(repo_arg)
    };

    validate_under_roots(&fs_path, &args.directories)?;

    if !looks_like_git_repo(&fs_path) {
        bail!(
            "'{}' does not appear to be a git repository",
            fs_path.display()
        );
    }

    if !args.export_all {
        let export_ok = fs_path.join("git-daemon-export-ok");
        if !export_ok.is_file() {
            bail!(
                "repository '{}' is not exported (missing git-daemon-export-ok; use --export-all for tests)",
                fs_path.display()
            );
        }
    }

    let repo = fs_path
        .canonicalize()
        .with_context(|| format!("canonicalize {}", fs_path.display()))?;

    let status = Command::new(grit_executable())
        .arg("upload-pack")
        .arg(&repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env_remove("GIT_TRACE_PACKET")
        .status()
        .context("spawn grit upload-pack from git-daemon")?;

    std::process::exit(status.code().unwrap_or(1));
}
