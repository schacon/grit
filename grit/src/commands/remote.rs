//! `grit remote` — manage remote repository connections.
//!
//! Porcelain command that manages `remote.<name>.url` and
//! `remote.<name>.fetch` entries in the local Git configuration.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Arguments for `grit remote`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage set of tracked repositories")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: Option<RemoteSubcommand>,

    /// Show remote URL after name.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

/// Subcommands for `grit remote`.
#[derive(Debug, Subcommand)]
pub enum RemoteSubcommand {
    /// Add a new remote.
    Add(AddArgs),
    /// Remove a remote.
    Remove(RemoveArgs),
    /// Remove a remote (alias for remove).
    Rm(RemoveArgs),
    /// Rename a remote.
    Rename(RenameArgs),
    /// Get the URL for a remote.
    #[command(name = "get-url")]
    GetUrl(GetUrlArgs),
    /// Set the URL for a remote.
    #[command(name = "set-url")]
    SetUrl(SetUrlArgs),
    /// Show information about a remote.
    Show(ShowArgs),
}

/// Arguments for `grit remote add`.
#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Name of the remote.
    pub name: String,
    /// URL of the remote.
    pub url: String,
}

/// Arguments for `grit remote remove` / `grit remote rm`.
#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    /// Name of the remote to remove.
    pub name: String,
}

/// Arguments for `grit remote rename`.
#[derive(Debug, ClapArgs)]
pub struct RenameArgs {
    /// Current name of the remote.
    pub old: String,
    /// New name for the remote.
    pub new: String,
}

/// Arguments for `grit remote get-url`.
#[derive(Debug, ClapArgs)]
pub struct GetUrlArgs {
    /// Name of the remote.
    pub name: String,
}

/// Arguments for `grit remote set-url`.
#[derive(Debug, ClapArgs)]
pub struct SetUrlArgs {
    /// Name of the remote.
    pub name: String,
    /// New URL for the remote.
    pub newurl: String,
}

/// Arguments for `grit remote show`.
#[derive(Debug, ClapArgs)]
pub struct ShowArgs {
    /// Name of the remote.
    pub name: String,
}

// ── Entrypoint ──────────────────────────────────────────────────────

pub fn run(args: Args) -> Result<()> {
    match args.subcommand {
        Some(RemoteSubcommand::Add(add_args)) => cmd_add(add_args),
        Some(RemoteSubcommand::Remove(rm_args)) | Some(RemoteSubcommand::Rm(rm_args)) => {
            cmd_remove(rm_args)
        }
        Some(RemoteSubcommand::Rename(rename_args)) => cmd_rename(rename_args),
        Some(RemoteSubcommand::GetUrl(get_url_args)) => cmd_get_url(get_url_args),
        Some(RemoteSubcommand::SetUrl(set_url_args)) => cmd_set_url(set_url_args),
        Some(RemoteSubcommand::Show(show_args)) => cmd_show(show_args),
        None => cmd_list(args.verbose),
    }
}

// ── List remotes ────────────────────────────────────────────────────

fn cmd_list(verbose: bool) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config = load_local_config(&git_dir)?;
    let remotes = collect_remotes(&config);

    for (name, info) in &remotes {
        if verbose {
            println!("{}\t{} (fetch)", name, info.url);
            println!("{}\t{} (push)", name, info.push_url.as_deref().unwrap_or(&info.url));
        } else {
            println!("{}", name);
        }
    }

    Ok(())
}

// ── Add ─────────────────────────────────────────────────────────────

fn cmd_add(args: AddArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config_path = git_dir.join("config");

    // Check that remote doesn't already exist
    {
        let config = load_local_config(&git_dir)?;
        let remotes = collect_remotes(&config);
        if remotes.contains_key(&args.name) {
            bail!("remote {} already exists.", args.name);
        }
    }

    let mut config_file = load_or_create_config_file(&config_path)?;
    let fetch_refspec = format!("+refs/heads/*:refs/remotes/{}/*", args.name);
    config_file.set(&format!("remote.{}.url", args.name), &args.url)?;
    config_file.set(&format!("remote.{}.fetch", args.name), &fetch_refspec)?;
    config_file.write().context("writing config")?;

    Ok(())
}

// ── Remove ──────────────────────────────────────────────────────────

fn cmd_remove(args: RemoveArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config_path = git_dir.join("config");

    // Verify the remote exists
    {
        let config = load_local_config(&git_dir)?;
        let remotes = collect_remotes(&config);
        if !remotes.contains_key(&args.name) {
            bail!("No such remote: '{}'", args.name);
        }
    }

    let mut config_file = load_or_create_config_file(&config_path)?;
    let section_name = format!("remote.{}", args.name);
    if !config_file.remove_section(&section_name)? {
        bail!("No such remote: '{}'", args.name);
    }
    config_file.write().context("writing config")?;

    Ok(())
}

// ── Rename ──────────────────────────────────────────────────────────

fn cmd_rename(args: RenameArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config_path = git_dir.join("config");

    // Verify old remote exists and new doesn't
    {
        let config = load_local_config(&git_dir)?;
        let remotes = collect_remotes(&config);
        if !remotes.contains_key(&args.old) {
            bail!("No such remote: '{}'", args.old);
        }
        if remotes.contains_key(&args.new) {
            bail!("remote {} already exists.", args.new);
        }
    }

    let mut config_file = load_or_create_config_file(&config_path)?;

    // Rename the section
    let old_section = format!("remote.{}", args.old);
    let new_section = format!("remote.{}", args.new);
    if !config_file.rename_section(&old_section, &new_section)? {
        bail!("No such remote: '{}'", args.old);
    }

    // Update the fetch refspec to reflect the new name
    let old_refspec_target = format!("refs/remotes/{}/*", args.old);
    let new_fetch = format!("+refs/heads/*:refs/remotes/{}/*", args.new);

    // Re-parse after rename to get updated entries
    let fetch_key = format!("remote.{}.fetch", args.new);
    // Check if the fetch refspec contains the old remote name and update it
    let current_fetch: Vec<String> = config_file
        .entries
        .iter()
        .filter(|e| e.key == fetch_key)
        .filter_map(|e| e.value.clone())
        .collect();

    for val in &current_fetch {
        if val.contains(&old_refspec_target) {
            config_file.set(&fetch_key, &new_fetch)?;
            break;
        }
    }

    config_file.write().context("writing config")?;

    // Rename remote-tracking refs
    let old_refs_dir = git_dir.join("refs/remotes").join(&args.old);
    let new_refs_dir = git_dir.join("refs/remotes").join(&args.new);
    if old_refs_dir.is_dir() {
        std::fs::rename(&old_refs_dir, &new_refs_dir)
            .with_context(|| format!("renaming refs/remotes/{} to refs/remotes/{}", args.old, args.new))?;
    }

    Ok(())
}

// ── Get URL ─────────────────────────────────────────────────────────

fn cmd_get_url(args: GetUrlArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config = load_local_config(&git_dir)?;
    let remotes = collect_remotes(&config);

    match remotes.get(&args.name) {
        Some(info) => {
            println!("{}", info.url);
            Ok(())
        }
        None => bail!("No such remote '{}'", args.name),
    }
}

// ── Set URL ─────────────────────────────────────────────────────────

fn cmd_set_url(args: SetUrlArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config_path = git_dir.join("config");

    // Verify remote exists
    {
        let config = load_local_config(&git_dir)?;
        let remotes = collect_remotes(&config);
        if !remotes.contains_key(&args.name) {
            bail!("No such remote '{}'", args.name);
        }
    }

    let mut config_file = load_or_create_config_file(&config_path)?;
    config_file.set(&format!("remote.{}.url", args.name), &args.newurl)?;
    config_file.write().context("writing config")?;

    Ok(())
}

// ── Show ────────────────────────────────────────────────────────────

fn cmd_show(args: ShowArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config = load_local_config(&git_dir)?;
    let remotes = collect_remotes(&config);

    match remotes.get(&args.name) {
        Some(info) => {
            println!("* remote {}", args.name);
            println!("  Fetch URL: {}", info.url);
            println!(
                "  Push  URL: {}",
                info.push_url.as_deref().unwrap_or(&info.url)
            );
            if let Some(ref fetch) = info.fetch {
                println!("  HEAD branch: (not queried)");
                println!("  Remote branch:");
                println!("    {} tracked", fetch);
            }
            Ok(())
        }
        None => bail!("No such remote '{}'", args.name),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

struct RemoteInfo {
    url: String,
    push_url: Option<String>,
    fetch: Option<String>,
}

/// Collect all remotes from a config set into a sorted map.
fn collect_remotes(config: &ConfigSet) -> BTreeMap<String, RemoteInfo> {
    let mut remotes: BTreeMap<String, RemoteInfo> = BTreeMap::new();

    for entry in config.entries() {
        // Match keys like remote.<name>.url, remote.<name>.fetch, etc.
        let parts: Vec<&str> = entry.key.splitn(3, '.').collect();
        if parts.len() == 3 && parts[0] == "remote" {
            let name = parts[1].to_owned();
            let field = parts[2];
            let value = entry.value.clone().unwrap_or_default();

            let info = remotes.entry(name).or_insert_with(|| RemoteInfo {
                url: String::new(),
                push_url: None,
                fetch: None,
            });

            match field {
                "url" => info.url = value,
                "pushurl" => info.push_url = Some(value),
                "fetch" => info.fetch = Some(value),
                _ => {}
            }
        }
    }

    // Remove entries without a URL (shouldn't happen but be safe)
    remotes.retain(|_, info| !info.url.is_empty());
    remotes
}

/// Resolve the git directory, erroring if not in a repo.
fn resolve_git_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("GIT_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let mut cur = cwd.as_path();
    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() {
            return Ok(dot_git);
        }
        if dot_git.is_file() {
            if let Ok(content) = std::fs::read_to_string(&dot_git) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("gitdir:") {
                        let path = rest.trim();
                        let resolved = if Path::new(path).is_absolute() {
                            PathBuf::from(path)
                        } else {
                            cur.join(path)
                        };
                        return Ok(resolved);
                    }
                }
            }
        }
        if cur.join("objects").is_dir() && cur.join("HEAD").is_file() {
            return Ok(cur.to_path_buf());
        }
        cur = match cur.parent() {
            Some(p) => p,
            None => bail!("not a git repository (or any of the parent directories): .git"),
        };
    }
}

/// Load config from the local .git/config (full cascade).
fn load_local_config(git_dir: &Path) -> Result<ConfigSet> {
    Ok(ConfigSet::load(Some(git_dir), true)?)
}

/// Load or create the config file for writing.
fn load_or_create_config_file(config_path: &Path) -> Result<ConfigFile> {
    match ConfigFile::from_path(config_path, ConfigScope::Local)? {
        Some(cfg) => Ok(cfg),
        None => Ok(ConfigFile::parse(config_path, "", ConfigScope::Local)?),
    }
}
