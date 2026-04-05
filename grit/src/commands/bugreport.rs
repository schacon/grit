//! `grit bugreport` — generate a bug report with system information.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Arguments for `grit bugreport`.
#[derive(Debug, ClapArgs)]
#[command(about = "Generate a bug report")]
pub struct Args {
    /// Output directory path.
    #[arg(short = 'o', long = "output-directory")]
    pub output_directory: Option<String>,

    /// Filename suffix (`git-bugreport-<suffix>.txt`).
    #[arg(short = 's', long = "suffix")]
    pub suffix: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    const PREAMBLE: &str = "\
Thank you for filling out a Git bug report!
Please answer the following questions to help us understand your issue.

What did you do before the bug happened? (Steps to reproduce your issue)

What did you expect to happen? (Expected behavior)

What happened instead? (Actual behavior)

What's different between what you expected and what actually happened?

Anything else you want to add:

Please review the rest of the bug report below.
You can delete any lines you don't wish to share.


";

    let mut report = String::new();
    report.push_str(PREAMBLE);

    // System info section
    report.push_str("[System Info]\n");
    report.push_str("git version 2.47.0.grit\n");
    let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    report.push_str(&format!("shell-path: {shell_path}\n"));
    report.push_str(&format!("uname: {}\n", collect_uname()));
    report.push_str("compiler info: rustc\n");
    report.push_str("zlib: available\n");
    report.push('\n');

    // Enabled hooks section (known hooks only)
    match Repository::discover(None) {
        Ok(repo) => {
            let hooks = list_enabled_hooks(&repo.git_dir.join("hooks"));
            if !hooks.is_empty() {
                report.push_str("[Enabled Hooks]\n");
                for hook in hooks {
                    report.push_str(&hook);
                    report.push('\n');
                }
            }
        }
        Err(_) => {}
    }

    let filename = if let Some(suffix) = args.suffix {
        format!("git-bugreport-{suffix}.txt")
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("git-bugreport-{now}.txt")
    };

    let output_dir = args.output_directory.unwrap_or_else(|| ".".to_string());
    let output_dir_path = PathBuf::from(output_dir);
    fs::create_dir_all(&output_dir_path).with_context(|| {
        format!(
            "failed to create output directory '{}'",
            output_dir_path.display()
        )
    })?;

    let output_path = output_dir_path.join(filename);
    if output_path.exists() {
        bail!("fatal: file '{}' already exists", output_path.display());
    }

    fs::write(&output_path, &report)
        .with_context(|| format!("failed to write bug report to {}", output_path.display()))?;

    println!("Created bug report at '{}'", output_path.display());
    Ok(())
}

fn collect_uname() -> String {
    if let Ok(output) = Command::new("uname").arg("-a").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn list_enabled_hooks(hooks_dir: &Path) -> Vec<String> {
    const KNOWN_HOOKS: &[&str] = &[
        "applypatch-msg",
        "commit-msg",
        "fsmonitor-watchman",
        "post-applypatch",
        "post-checkout",
        "post-commit",
        "post-merge",
        "post-receive",
        "post-rewrite",
        "post-update",
        "pre-applypatch",
        "pre-auto-gc",
        "pre-commit",
        "pre-merge-commit",
        "pre-push",
        "pre-rebase",
        "pre-receive",
        "prepare-commit-msg",
        "push-to-checkout",
        "sendemail-validate",
        "update",
    ];

    let mut enabled = Vec::new();
    for hook in KNOWN_HOOKS {
        let path = hooks_dir.join(hook);
        if let Ok(meta) = fs::metadata(&path) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 != 0 {
                    enabled.push((*hook).to_string());
                }
            }
            #[cfg(not(unix))]
            {
                if meta.is_file() {
                    enabled.push((*hook).to_string());
                }
            }
        }
    }
    enabled.sort();
    enabled
}
