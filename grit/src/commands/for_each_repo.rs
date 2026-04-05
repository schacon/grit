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

    /// Command and arguments to run in each repo.
    #[arg(last = true)]
    pub command: Vec<String>,
}

/// Run `grit for-each-repo`.
pub fn run(args: Args) -> Result<()> {
    // Validate config key format first (must be section.key or section.subsection.key)
    if !args.config_key.contains('.') || args.config_key.ends_with('.') {
        eprintln!("error: got bad config key: {}", args.config_key);
        std::process::exit(129);
    }

    if args.command.is_empty() {
        bail!("missing -- <command>");
    }

    // Load git config to find the repo list.
    let config = if let Ok(repo) = Repository::discover(None) {
        ConfigSet::load(Some(&repo.git_dir), true).context("loading config")?
    } else {
        ConfigSet::load(None, true).context("loading config")?
    };

    let repos = config.get_all(&args.config_key);
    if repos.is_empty() {
        // Nothing to do — no repos configured.
        return Ok(());
    }

    // git for-each-repo runs `git <command>` in each repo.
    // Find our own binary path to use as the git executable.
    let git_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("git"));
    let cmd_name = git_exe.as_os_str();
    let cmd_args = &args.command[..];

    let mut had_error = false;

    for repo_path in &repos {
        let path = std::path::Path::new(repo_path);
        if !path.is_dir() {
            eprintln!("warning: skipping non-directory {repo_path}");
            continue;
        }

        let status = Command::new(cmd_name)
            .args(cmd_args)
            .current_dir(path)
            .status()
            .with_context(|| format!("running command in {repo_path}"))?;

        if !status.success() {
            eprintln!("error: command failed in {repo_path}");
            had_error = true;
        }
    }

    if had_error {
        std::process::exit(1);
    }

    Ok(())
}
