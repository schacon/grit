//! `grit bugreport` — generate a bug report with system information.
//!
//! Collects system info and writes a template report that mirrors upstream
//! `git bugreport` behavior closely enough for the test suite.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::hooks::resolve_hooks_dir;
use grit_lib::repo::Repository;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

/// Arguments for `grit bugreport`.
#[derive(Debug, ClapArgs)]
#[command(about = "Generate a bug report")]
pub struct Args {
    /// Output directory (default: current directory).
    #[arg(short = 'o', long = "output-directory")]
    pub output_directory: Option<String>,

    /// Suffix for the generated report filename.
    #[arg(short = 's', long = "suffix")]
    pub suffix: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    let mut report = String::new();

    // Intro/template before first section must match upstream wording.
    report.push_str("Thank you for filling out a Git bug report!\n");
    report.push_str("Please answer the following questions to help us understand your issue.\n\n");
    report.push_str("What did you do before the bug happened? (Steps to reproduce your issue)\n\n");
    report.push_str("What did you expect to happen? (Expected behavior)\n\n");
    report.push_str("What happened instead? (Actual behavior)\n\n");
    report.push_str("What's different between what you expected and what actually happened?\n\n");
    report.push_str("Anything else you want to add:\n\n");
    report.push_str("Please review the rest of the bug report below.\n");
    report.push_str("You can delete any lines you don't wish to share.\n\n\n");

    // System info section.
    report.push_str("[System Info]\n");
    report.push_str("grit version: git version 2.47.0.grit\n");
    report.push_str(&format!("shell-path: {}\n", shell_path()));
    report.push_str(&format!("uname: {}\n", collect_uname()));
    report.push_str(&format!("compiler info: {}\n", collect_compiler_info()));
    report.push_str("zlib: 1.2.x\n");
    report.push('\n');

    // Git config (repo-level if available).
    report.push_str("[Git Config]\n");
    match Repository::discover(None) {
        Ok(repo) => {
            match ConfigSet::load(Some(&repo.git_dir), true) {
                Ok(config) => {
                    for entry in config.entries() {
                        // Redact potentially sensitive values
                        let key = &entry.key;
                        let raw_value = entry.value.as_deref().unwrap_or("true");
                        let value = if key.contains("password")
                            || key.contains("token")
                            || key.contains("secret")
                            || key.contains("credential")
                        {
                            "***REDACTED***"
                        } else {
                            raw_value
                        };
                        report.push_str(&format!("  {key} = {value}\n"));
                    }
                }
                Err(e) => {
                    report.push_str(&format!("  (failed to load config: {e})\n"));
                }
            }
        }
        Err(_) => {
            report.push_str("  (not inside a git repository)\n");
        }
    }

    report.push('\n');

    // Enabled hooks section (known hooks only).
    // Keep this as the final section so sed range extraction in tests
    // does not capture a trailing blank separator line.
    report.push_str("[Enabled Hooks]\n");
    if let Ok(repo) = Repository::discover(None) {
        for hook in collect_enabled_hooks(&repo) {
            report.push_str(&hook);
            report.push('\n');
        }
    }

    // Determine output path.
    let file_name = if let Some(suffix) = args.suffix {
        format!("git-bugreport-{suffix}.txt")
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("git-bugreport-{now}.txt")
    };

    let out_path = if let Some(dir) = args.output_directory {
        let dir_path = PathBuf::from(dir);
        fs::create_dir_all(&dir_path).with_context(|| {
            format!("failed to create output directory '{}'", dir_path.display())
        })?;
        dir_path.join(file_name)
    } else {
        PathBuf::from(file_name)
    };

    if out_path.exists() {
        bail!("fatal: file '{}' already exists", out_path.display());
    }

    fs::write(&out_path, &report)
        .with_context(|| format!("failed to write bug report to {}", out_path.display()))?;

    println!("Created bug report at '{}'", out_path.display());
    Ok(())
}

fn shell_path() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
}

fn collect_uname() -> String {
    if let Ok(output) = Command::new("uname").arg("-a").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn collect_compiler_info() -> String {
    if let Ok(output) = Command::new("rustc").arg("--version").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    "rustc (unknown version)".to_owned()
}

fn collect_enabled_hooks(repo: &Repository) -> Vec<String> {
    // Keep this list to known hooks so random files in hooks/ are ignored.
    let known_hooks = [
        "applypatch-msg",
        "pre-applypatch",
        "post-applypatch",
        "pre-commit",
        "pre-merge-commit",
        "prepare-commit-msg",
        "commit-msg",
        "post-commit",
        "pre-rebase",
        "post-checkout",
        "post-merge",
        "pre-push",
        "pre-receive",
        "update",
        "proc-receive",
        "post-receive",
        "post-update",
        "reference-transaction",
        "push-to-checkout",
        "pre-auto-gc",
        "post-rewrite",
        "sendemail-validate",
        "fsmonitor-watchman",
        "p4-pre-submit",
        "post-index-change",
    ];

    let hooks_dir = resolve_hooks_dir(repo);
    let mut enabled = Vec::new();
    for hook in known_hooks {
        let path = hooks_dir.join(hook);
        if let Ok(meta) = fs::metadata(path) {
            if meta.permissions().mode() & 0o111 != 0 {
                enabled.push(hook.to_owned());
            }
        }
    }
    enabled.sort();
    enabled
}
