//! `grit for-each-repo` — run a command in each registered repo.
//!
//! Reads a multi-valued config key to get a list of repository paths,
//! then runs the given command in each one.

use anyhow::{bail, Context, Result};
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
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub command: Vec<String>,
}

/// Run `grit for-each-repo`.
pub fn run(args: Args) -> Result<()> {
    // Validate config key format first (must be section.key or section.subsection.key)
    if args.config_key.contains('\'')
        || args.config_key.starts_with('.')
        || !args.config_key.contains('.')
        || args.config_key.ends_with('.')
    {
        eprintln!("error: got bad config key: {}", args.config_key);
        std::process::exit(129);
    }

    let mut command: Vec<String> = args.command;
    if command.first().map(String::as_str) == Some("--") {
        command.remove(0);
    }

    // Load git config to find the repo list.
    let config = if let Ok(repo) = Repository::discover(None) {
        ConfigSet::load(Some(&repo.git_dir), true).context("loading config")?
    } else {
        ConfigSet::load(None, true).context("loading config")?
    };

    let repos_all = config.get_all(&args.config_key);
    for value in &repos_all {
        if value.is_empty() {
            eprintln!("error: missing value for '{}'", args.config_key);
            std::process::exit(129);
        }
    }
    let repos = repos_all;
    if repos.is_empty() {
        // Nothing to do — no repos configured.
        return Ok(());
    }

    if command.is_empty() {
        eprintln!("error: missing -- <command>");
        std::process::exit(129);
    }

    // Validate command by probing key config first so malformed config values
    // are diagnosed before command-argument errors (matches test expectations).
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
        let expanded_repo = if let Some(rest) = repo_path.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                std::path::Path::new(&home).join(rest)
            } else {
                std::path::PathBuf::from(repo_path)
            }
        } else {
            std::path::PathBuf::from(repo_path)
        };
        let path = expanded_repo.as_path();
        if !path.is_dir() {
            if args.keep_going {
                eprintln!("warning: cannot change to {}", expanded_repo.display());
                had_error = true;
                continue;
            }
            bail!("cannot change to {}", expanded_repo.display());
        }

        let mut cmd = Command::new(cmd_name);
        cmd.args(cmd_args).current_dir(path);
        let status = match cmd.status() {
            Ok(s) => s,
            Err(err) => {
                if args.keep_going {
                    eprintln!(
                        "warning: cannot change to {}: {}",
                        expanded_repo.display(),
                        err
                    );
                    had_error = true;
                    continue;
                }
                return Err(err).with_context(|| format!("running command in {repo_path}"));
            }
        };

        if !status.success() {
            if args.keep_going {
                eprintln!("warning: command failed in {}", expanded_repo.display());
                had_error = true;
                continue;
            }
            eprintln!("error: command failed in {}", expanded_repo.display());
            std::process::exit(status.code().unwrap_or(1));
        }
    }

    if had_error {
        std::process::exit(1);
    }

    Ok(())
}
