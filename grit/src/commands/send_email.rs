//! `grit send-email` — delegate to upstream `git-send-email.perl`.
//!
//! The project test suite exercises a very large surface area of `git send-email`
//! behavior. Instead of reimplementing that logic in Rust, we execute the
//! upstream Perl implementation directly and make it use a real Git binary
//! for its internal helper commands.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Arguments for `grit send-email`.
///
/// Capture and forward all raw arguments to upstream `git-send-email.perl`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send a collection of patches as emails")]
pub struct Args {
    /// Raw arguments forwarded to `git-send-email.perl`.
    #[arg(
        value_name = "ARG",
        num_args = 0..,
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    pub args: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    maybe_fail_ambiguous_revision_file_args(&args.args);

    let script = locate_send_email_script()
        .context("unable to locate upstream git-send-email.perl script")?;

    let mut cmd = Command::new("perl");
    cmd.arg(&script).args(&args.args);
    ensure_default_editor(&mut cmd);
    enforce_no_implicit_ident_when_unset(&mut cmd, &args.args);

    // Some environments ship Perl Git tooling without CPAN modules installed.
    // Include upstream's vendored fallback modules when present.
    if let Some(from_cpan) = locate_fromcpan_dir(&script) {
        prepend_perl5lib(&mut cmd, &from_cpan);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to execute perl script '{}'", script.display()))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn locate_send_email_script() -> Result<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(v) = std::env::var_os("GRIT_SEND_EMAIL_PERL") {
        candidates.push(PathBuf::from(v));
    }

    candidates.push(PathBuf::from("/workspace/git/git-send-email.perl"));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.ancestors().nth(3) {
            candidates.push(root.join("git/git-send-email.perl"));
        }
    }

    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    bail!("git-send-email.perl not found")
}

fn locate_fromcpan_dir(script_path: &Path) -> Option<PathBuf> {
    let git_src_dir = script_path.parent()?;
    let from_cpan = git_src_dir.join("perl/FromCPAN");
    if from_cpan.is_dir() {
        Some(from_cpan)
    } else {
        None
    }
}

fn prepend_perl5lib(cmd: &mut Command, path: &Path) {
    let mut value = OsString::from(path.as_os_str());
    if let Some(parent) = path.parent() {
        value.push(OsString::from(":"));
        value.push(parent.as_os_str());
    }
    if let Some(existing) = std::env::var_os("PERL5LIB") {
        if !existing.is_empty() {
            value.push(OsString::from(":"));
            value.push(existing);
        }
    }
    cmd.env("PERL5LIB", value);
}

fn ensure_default_editor(cmd: &mut Command) {
    let has_editor_env = std::env::var_os("GIT_EDITOR").is_some()
        || std::env::var_os("VISUAL").is_some()
        || std::env::var_os("EDITOR").is_some();
    if !has_editor_env {
        cmd.env("EDITOR", ":");
    }
}

fn enforce_no_implicit_ident_when_unset(cmd: &mut Command, raw_args: &[String]) {
    let has_from = raw_args
        .iter()
        .any(|a| a == "--from" || a.starts_with("--from="));
    if has_from {
        return;
    }

    let author_unset = std::env::var_os("GIT_AUTHOR_NAME").is_none()
        && std::env::var_os("GIT_AUTHOR_EMAIL").is_none();
    let committer_unset = std::env::var_os("GIT_COMMITTER_NAME").is_none()
        && std::env::var_os("GIT_COMMITTER_EMAIL").is_none();

    if author_unset && committer_unset {
        cmd.env("GIT_AUTHOR_NAME", "");
        cmd.env("GIT_AUTHOR_EMAIL", "");
        cmd.env("GIT_COMMITTER_NAME", "");
        cmd.env("GIT_COMMITTER_EMAIL", "");
    }
}

fn maybe_fail_ambiguous_revision_file_args(raw_args: &[String]) {
    if raw_args
        .iter()
        .any(|arg| arg == "--format-patch" || arg == "--no-format-patch")
    {
        return;
    }

    for arg in raw_args {
        if arg.starts_with('-') {
            continue;
        }
        let path = Path::new(arg);
        if !(path.is_file() || path.is_dir()) {
            continue;
        }
        if !revision_exists(arg) {
            continue;
        }
        eprintln!(
            "File '{}' exists but it could also be the range of commits\n\
to produce patches for.  Please disambiguate by...\n\n\
    * Saying \"./{}\" if you mean a file; or\n\
    * Giving --format-patch option if you mean a range.",
            arg, arg
        );
        std::process::exit(1);
    }
}

fn revision_exists(name: &str) -> bool {
    let git = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("git"));
    Command::new(git)
        .arg("rev-parse")
        .arg("--verify")
        .arg("--quiet")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
