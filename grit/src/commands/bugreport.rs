//! `grit bugreport` — generate a bug report with system information.
//!
//! Collects system info (grit version, OS, shell, config) and writes
//! it to a timestamped file in the current directory.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Arguments for `grit bugreport`.
#[derive(Debug, ClapArgs)]
#[command(about = "Generate a bug report")]
pub struct Args {
    /// Directory to place the generated report in.
    #[arg(short = 'o', long = "output-directory", value_name = "PATH")]
    pub output_directory: Option<String>,

    /// Suffix used in the generated filename: git-bugreport-<suffix>.txt
    #[arg(short = 's', long = "suffix", value_name = "FORMAT")]
    pub suffix: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    let mut report = String::new();

    // Intro template (must match upstream wording used by tests).
    report.push_str("Thank you for filling out a Git bug report!\n");
    report.push_str("Please answer the following questions to help us understand your issue.\n\n");
    report.push_str("What did you do before the bug happened? (Steps to reproduce your issue)\n\n");
    report.push_str("What did you expect to happen? (Expected behavior)\n\n");
    report.push_str("What happened instead? (Actual behavior)\n\n");
    report.push_str("What's different between what you expected and what actually happened?\n\n");
    report.push_str("Anything else you want to add:\n\n");
    report.push_str("Please review the rest of the bug report below.\n");
    report.push_str("You can delete any lines you don't wish to share.\n\n\n");

    report.push_str("[System Info]\n");
    report.push_str("git version 2.47.0.grit\n");
    report.push_str(&format!("shell-path: {}\n", shell_path()));
    report.push_str(&format!("uname: {}\n", collect_uname()));
    report.push_str(&format!("compiler info: {}\n", compiler_info()));
    report.push_str("zlib: present\n\n");

    if let Ok(repo) = Repository::discover(None) {
        let hooks = collect_enabled_hooks(&repo);
        if !hooks.is_empty() {
            report.push_str("[Enabled Hooks]\n");
            for hook in hooks {
                report.push_str(&hook);
                report.push('\n');
            }
        }
    }

    let suffix = if let Some(s) = args.suffix {
        s
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.to_string()
    };
    let filename = format!("git-bugreport-{suffix}.txt");

    let out_path = if let Some(dir) = args.output_directory {
        let dir_path = PathBuf::from(dir);
        fs::create_dir_all(&dir_path)
            .with_context(|| format!("failed to create output directory {}", dir_path.display()))?;
        dir_path.join(filename)
    } else {
        PathBuf::from(filename)
    };

    let path = out_path.as_path();
    if path.exists() {
        bail!("fatal: file '{}' already exists", path.display());
    }

    fs::write(path, &report)
        .with_context(|| format!("failed to write bug report to {}", path.display()))?;

    println!("Created bug report at '{}'", path.display());
    Ok(())
}

fn shell_path() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn collect_uname() -> String {
    if let Ok(output) = Command::new("uname").arg("-a").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn compiler_info() -> String {
    if let Ok(output) = Command::new("rustc").arg("--version").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    "rustc".to_string()
}

fn collect_enabled_hooks(repo: &Repository) -> Vec<String> {
    let known_hooks: BTreeSet<&'static str> = [
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
        "reference-transaction",
        "sendemail-validate",
        "update",
    ]
    .into_iter()
    .collect();

    let hooks_dir = repo.git_dir.join("hooks");
    let mut enabled = Vec::new();
    let Ok(entries) = fs::read_dir(hooks_dir) else {
        return enabled;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if !known_hooks.contains(name.as_str()) {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        enabled.push(name);
    }
    enabled.sort();
    enabled

}
