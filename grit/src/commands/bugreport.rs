//! `grit bugreport` — generate a bug report with system information.
//!
//! Collects system info (grit version, OS, shell, config) and writes
//! it to a timestamped file in the current directory.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;
use std::fs;
use std::process::Command;

/// Arguments for `grit bugreport`.
#[derive(Debug, ClapArgs)]
#[command(about = "Generate a bug report")]
pub struct Args {
    /// Output file path (default: auto-generated timestamped name).
    #[arg(short = 'o', long = "output-path")]
    pub output_path: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    let mut report = String::new();

    // Header
    report.push_str("Thank you for filling out a grit bug report!\n");
    report.push_str(
        "Please answer the following questions and provide as much detail as possible.\n\n",
    );

    // Version info
    report.push_str("[System Info]\n");
    report.push_str(&format!("grit version: git version 2.47.0.grit\n"));

    // OS info
    let os_info = collect_os_info();
    report.push_str(&format!("os: {os_info}\n"));

    // Shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    report.push_str(&format!("shell: {shell}\n"));

    // CPU architecture
    report.push_str(&format!("arch: {}\n", std::env::consts::ARCH));

    // Compiler info
    report.push_str(&format!(
        "built with: rustc (target: {})\n",
        std::env::consts::OS
    ));

    report.push('\n');

    // Git config (repo-level if available)
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

    // Placeholders for user to fill in
    report.push_str("[What happened]\n");
    report.push_str("(please describe what happened)\n\n");

    report.push_str("[What did you expect to happen]\n");
    report.push_str("(please describe what you expected)\n\n");

    report.push_str("[Steps to reproduce]\n");
    report.push_str("(please provide steps to reproduce the issue)\n\n");

    report.push_str("[Anything else]\n");
    report.push_str("(any additional context)\n");

    // Determine output filename
    let filename = if let Some(ref path) = args.output_path {
        path.clone()
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("git-bugreport-{now}.txt")
    };

    fs::write(&filename, &report)
        .with_context(|| format!("failed to write bug report to {filename}"))?;

    println!("Created bug report at '{filename}'");
    Ok(())
}

fn collect_os_info() -> String {
    // Try to read /etc/os-release for Linux
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(pretty) = line.strip_prefix("PRETTY_NAME=") {
                return pretty.trim_matches('"').to_string();
            }
        }
    }

    // Fallback to uname
    if let Ok(output) = Command::new("uname").arg("-srm").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }

    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}
