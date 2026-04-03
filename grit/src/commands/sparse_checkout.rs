//! `grit sparse-checkout` — manage sparse checkout patterns.
//!
//! Sparse checkout allows only a subset of files to be checked out
//! in the working tree. Patterns are stored in `.git/info/sparse-checkout`
//! and controlled by `core.sparseCheckout` config.

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Write};

/// Arguments for `grit sparse-checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage sparse checkout patterns")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: SparseCheckoutSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SparseCheckoutSubcommand {
    /// Initialize sparse checkout.
    Init,
    /// Set sparse checkout patterns.
    Set(SetArgs),
    /// List current sparse checkout patterns.
    List,
    /// Disable sparse checkout.
    Disable,
}

#[derive(Debug, ClapArgs)]
pub struct SetArgs {
    /// Patterns to include in sparse checkout.
    #[arg(required = true)]
    pub patterns: Vec<String>,
}

/// Run `grit sparse-checkout`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.subcommand {
        SparseCheckoutSubcommand::Init => cmd_init(&repo),
        SparseCheckoutSubcommand::Set(set_args) => cmd_set(&repo, &set_args.patterns),
        SparseCheckoutSubcommand::List => cmd_list(&repo),
        SparseCheckoutSubcommand::Disable => cmd_disable(&repo),
    }
}

/// Enable sparse checkout by setting `core.sparseCheckout = true`
/// and creating the sparse-checkout file with a default `/*` pattern.
fn cmd_init(repo: &Repository) -> Result<()> {
    set_sparse_config(repo, true)?;

    let sc_path = sparse_checkout_path(repo);
    // Create parent directory if needed
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    // Only write defaults if the file doesn't exist yet
    if !sc_path.exists() {
        fs::write(&sc_path, "/*\n").context("writing sparse-checkout file")?;
    }

    Ok(())
}

/// Set the sparse checkout patterns, replacing any existing ones.
fn cmd_set(repo: &Repository, patterns: &[String]) -> Result<()> {
    // Ensure sparse checkout is enabled
    set_sparse_config(repo, true)?;

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    let mut content = String::new();
    for pat in patterns {
        content.push_str(pat);
        content.push('\n');
    }
    fs::write(&sc_path, &content).context("writing sparse-checkout file")?;

    Ok(())
}

/// List the current sparse checkout patterns.
fn cmd_list(repo: &Repository) -> Result<()> {
    // Check if sparse checkout is actually enabled
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        anyhow::bail!("this worktree is not sparse");
    }

    let sc_path = sparse_checkout_path(repo);
    if !sc_path.exists() {
        // No sparse checkout file — nothing to list
        return Ok(());
    }

    let content = fs::read_to_string(&sc_path).context("reading sparse-checkout file")?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            writeln!(out, "{trimmed}")?;
        }
    }
    Ok(())
}

/// Disable sparse checkout by setting `core.sparseCheckout = false`.
fn cmd_disable(repo: &Repository) -> Result<()> {
    set_sparse_config(repo, false)?;
    Ok(())
}

fn sparse_checkout_path(repo: &Repository) -> std::path::PathBuf {
    repo.git_dir.join("info").join("sparse-checkout")
}

/// Set `core.sparseCheckout` in the local repo config.
fn set_sparse_config(repo: &Repository, enable: bool) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content =
            fs::read_to_string(&config_path).context("reading repo config")?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    let value = if enable { "true" } else { "false" };
    config.set("core.sparseCheckout", value)?;
    config.write().context("writing repo config")?;
    Ok(())
}
