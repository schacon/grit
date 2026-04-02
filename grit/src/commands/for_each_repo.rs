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
    #[arg(last = true, required = true)]
    pub command: Vec<String>,
}

/// Run `grit for-each-repo`.
pub fn run(args: Args) -> Result<()> {
    if args.command.is_empty() {
        bail!("no command specified");
    }

    // Load git config to find the repo list.
    let config = if let Ok(repo) = Repository::discover(None) {
        ConfigSet::load(Some(&repo.git_dir), true)
            .context("loading config")?
    } else {
        ConfigSet::load(None, true)
            .context("loading config")?
    };

    let repos = config.get_all(&args.config_key);
    if repos.is_empty() {
        // Nothing to do — no repos configured.
        return Ok(());
    }

    let cmd_name = &args.command[0];
    let cmd_args = &args.command[1..];

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
