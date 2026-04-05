//! `grit for-each-repo` — run a command in each registered repo.
//!
//! Reads a multi-valued config key to get a list of repository paths,
//! then runs the given command in each one.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::process::Command;

use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;

/// Arguments for `grit for-each-repo`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Config key containing the list of repos.
    #[arg(long = "config")]
    pub config_key: String,

    /// Continue processing repos even if one fails.
    #[arg(long = "keep-going")]
    pub keep_going: bool,

    /// Command and arguments to run in each repo.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Run `grit for-each-repo`.
pub fn run(args: Args) -> Result<()> {
    // Validate config key format first.
    if !valid_config_key(&args.config_key) {
        eprintln!("error: got bad config key: {}", args.config_key);
        std::process::exit(129);
    }
    let canon_key = args.config_key.to_ascii_lowercase();

    // Load git config to find the repo list.
    let config = if let Ok(repo) = Repository::discover(None) {
        ConfigSet::load(Some(&repo.git_dir), true).context("loading config")?
    } else {
        ConfigSet::load(None, true).context("loading config")?
    };

    // Existing key with no value is an error.
    if config
        .entries()
        .iter()
        .any(|entry| entry.key == canon_key && entry.value.is_none())
    {
        eprintln!("error: missing value for '{}'", args.config_key);
        std::process::exit(129);
    }

    let repos = config
        .get_all(&args.config_key)
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .collect::<Vec<_>>();
    if repos.is_empty() {
        // Nothing to do — no repos configured.
        return Ok(());
    }

    let mut command = args.command;
    if command.first().map(String::as_str) == Some("--") {
        command.remove(0);
    }
    if command.is_empty() {
        eprintln!("error: missing -- <command>");
        std::process::exit(129);
    }

    // git for-each-repo runs `git <command>` in each repo.
    // Find our own binary path to use as the git executable.
    let git_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("git"));
    let cmd_name = git_exe.as_os_str();
    let cmd_args = &command[..];

    let mut had_error = false;

    for repo_path in &repos {
        let expanded = grit_lib::config::parse_path(repo_path);
        let path = std::path::Path::new(&expanded);
        if !path.is_dir() {
            eprintln!("error: cannot change to '{}'", expanded);
            had_error = true;
            if !args.keep_going {
                std::process::exit(1);
            }
            continue;
        }

        let status = Command::new(cmd_name)
            .args(cmd_args)
            .current_dir(path)
            .status();
        let status = match status {
            Ok(s) => s,
            Err(_) => {
                eprintln!("error: cannot change to '{}'", expanded);
                had_error = true;
                if !args.keep_going {
                    std::process::exit(1);
                }
                continue;
            }
        };

        if !status.success() {
            had_error = true;
            if !args.keep_going {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }

    if had_error {
        std::process::exit(1);
    }

    Ok(())
}

fn valid_config_key(key: &str) -> bool {
    // Require at least section.key with no empty components.
    if key.is_empty() || !key.contains('.') || key.starts_with('.') || key.ends_with('.') {
        return false;
    }
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    parts.iter().all(|part| {
        part.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    })
}
