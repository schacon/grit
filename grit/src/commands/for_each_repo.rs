//! `grit for-each-repo` — run a command in each registered repo.
//!
//! Reads a multi-valued config key to get a list of repository paths,
//! then runs the given command in each one.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::process::Command;

use grit_lib::config::canonical_key;
use grit_lib::config::ConfigSet;
use grit_lib::config::parse_path;

/// Arguments for `grit for-each-repo`.
#[derive(Debug, ClapArgs)]
#[command(trailing_var_arg = true)]
pub struct Args {
    /// Config key containing the list of repos.
    #[arg(long = "config")]
    pub config_key: String,

    /// Keep going even if one repository command fails.
    #[arg(long = "keep-going")]
    pub keep_going: bool,

    /// Command and arguments to run in each repo.
    #[arg(allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Run `grit for-each-repo`.
pub fn run(args: Args) -> Result<()> {
    // Validate config key format first.
    if canonical_key(&args.config_key).is_err() {
        eprintln!("error: got bad config --config={}", args.config_key);
        std::process::exit(129);
    }

    // Load git config to find the repo list.
    // for-each-repo is expected to work outside repositories too.
    let config = ConfigSet::load(None, true).context("loading config")?;

    let repos = config.get_all(&args.config_key);
    if repos.iter().any(|v| v.is_empty()) {
        eprintln!("error: missing value for '{}'", args.config_key);
        std::process::exit(129);
    }
    if repos.is_empty() {
        // Nothing to do — no repos configured.
        return Ok(());
    }

    let command = if args.command.first().is_some_and(|s| s == "--") {
        args.command[1..].to_vec()
    } else {
        args.command.clone()
    };
    if command.is_empty() {
        eprintln!("error: missing -- <command>");
        std::process::exit(129);
    }

    // git for-each-repo runs `git <command>` in each repo.
    // Find our own binary path to use as the git executable.
    let git_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("git"));
    let cmd_name = git_exe.as_os_str();
    let cmd_args = &command[..];

    let mut result = 0;

    for repo_path in &repos {
        let expanded = parse_path(repo_path);
        if !std::path::Path::new(&expanded).is_dir() {
            eprintln!("fatal: cannot change to '{}': No such file or directory", expanded);
            if !args.keep_going {
                std::process::exit(1);
            }
            result = 1;
            continue;
        }

        let status = Command::new(cmd_name)
            .arg("-C")
            .arg(&expanded)
            .args(cmd_args)
            .status()?;

        if !status.success() {
            if !args.keep_going {
                std::process::exit(status.code().unwrap_or(1));
            }
            result = 1;
        }
    }

    if result != 0 {
        std::process::exit(result);
    }

    Ok(())
}
