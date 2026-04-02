//! `grit shell` — restricted login shell for Git-only SSH access.
//!
//! Only allows execution of git-receive-pack, git-upload-pack, and
//! git-upload-archive commands, rejecting everything else.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use std::process::Command;

/// Arguments for `grit shell`.
#[derive(Debug, ClapArgs)]
#[command(about = "Restricted login shell for Git-only SSH access")]
pub struct Args {
    /// Must be "-c" to execute a command.
    #[arg(value_name = "FLAG")]
    pub flag: Option<String>,

    /// The command string to execute.
    #[arg(value_name = "COMMAND")]
    pub command: Option<String>,
}

/// Allowed commands that can be executed via git shell.
const ALLOWED_COMMANDS: &[&str] = &[
    "git-receive-pack",
    "git-upload-pack",
    "git-upload-archive",
    "git receive-pack",
    "git upload-pack",
    "git upload-archive",
];

pub fn run(args: Args) -> Result<()> {
    let flag = match &args.flag {
        Some(f) => f.as_str(),
        None => {
            eprintln!("fatal: Interactive git shell is not enabled.");
            eprintln!("hint: ~/{}/allowed-commands should exist and list allowed commands.", "git-shell-commands");
            std::process::exit(128);
        }
    };

    if flag != "-c" {
        bail!("unrecognized flag '{}'; only -c is supported", flag);
    }

    let cmd_str = args
        .command
        .as_deref()
        .unwrap_or_else(|| {
            eprintln!("fatal: no command specified");
            std::process::exit(128);
        });

    // Parse the command to extract the git command name and the directory argument
    let (git_cmd, directory) = parse_git_command(cmd_str)?;

    // Verify it's an allowed command
    if !ALLOWED_COMMANDS.iter().any(|allowed| git_cmd == *allowed) {
        bail!(
            "fatal: unrecognized command '{}'. Only git commands are allowed.",
            git_cmd
        );
    }

    // Map to the grit subcommand
    let subcommand = match git_cmd.as_str() {
        "git-receive-pack" | "git receive-pack" => "receive-pack",
        "git-upload-pack" | "git upload-pack" => "upload-pack",
        "git-upload-archive" | "git upload-archive" => "upload-archive",
        _ => bail!("unrecognized command: {}", git_cmd),
    };

    // Execute via the current grit binary
    let grit_bin = std::env::current_exe().unwrap_or_else(|_| "grit".into());
    let status = Command::new(&grit_bin)
        .arg(subcommand)
        .arg(&directory)
        .status()?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Parse a git shell command string into (command_name, directory).
///
/// Accepts formats like:
///   "git-receive-pack '/path/to/repo.git'"
///   "git-upload-pack /path/to/repo"
///   "git receive-pack '/path/to/repo.git'"
fn parse_git_command(cmd_str: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = cmd_str.splitn(2, |c: char| c.is_whitespace()).collect();

    if parts.is_empty() {
        bail!("empty command");
    }

    let (cmd_name, rest) = if parts[0] == "git" && parts.len() > 1 {
        // "git receive-pack '/path'"
        let sub_parts: Vec<&str> = parts[1].splitn(2, |c: char| c.is_whitespace()).collect();
        if sub_parts.len() < 2 {
            bail!("missing directory argument");
        }
        (format!("git {}", sub_parts[0]), sub_parts[1].to_string())
    } else if parts.len() > 1 {
        // "git-receive-pack '/path'"
        (parts[0].to_string(), parts[1].to_string())
    } else {
        bail!("missing directory argument");
    };

    // Strip surrounding quotes from the directory
    let directory = rest
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string();

    Ok((cmd_name, directory))
}
