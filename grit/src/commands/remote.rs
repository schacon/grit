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
    /// Configure the set of tracked branches for a remote.
    #[command(name = "set-branches")]
    SetBranches(SetBranchesArgs),
    /// Remove stale remote-tracking branches.
    Prune(PruneArgs),
    /// Fetch updates from remote(s).
    Update(UpdateArgs),
}

/// Arguments for `grit remote add`.
#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Name of the remote.
    pub name: String,
    /// URL of the remote.
    pub url: String,
    /// Fetch immediately after adding.
    #[arg(short = 'f')]
    pub fetch: bool,
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

/// Arguments for `grit remote set-branches`.
#[derive(Debug, ClapArgs)]
pub struct SetBranchesArgs {
    /// Replace (instead of add to) the list of currently tracked branches.
    #[arg(long)]
    pub add: bool,
    /// Name of the remote.
    pub name: String,
    /// Branch patterns to track.
    pub branches: Vec<String>,
}

/// Arguments for `grit remote prune`.
#[derive(Debug, ClapArgs)]
pub struct PruneArgs {
    /// Name of the remote.
    pub name: String,
}

/// Arguments for `grit remote update`.
#[derive(Debug, ClapArgs)]
pub struct UpdateArgs {
    /// Remote or group names to update (default: all configured remotes).
    pub groups: Vec<String>,
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
        Some(RemoteSubcommand::SetBranches(sb_args)) => cmd_set_branches(sb_args),
        Some(RemoteSubcommand::Prune(prune_args)) => cmd_prune(prune_args),
        Some(RemoteSubcommand::Update(update_args)) => cmd_update(update_args),
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
            println!(
                "{}\t{} (push)",
                name,
                info.push_url.as_deref().unwrap_or(&info.url)
            );
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

    // If -f was given, fetch immediately.
    if args.fetch {
        let self_exe = std::env::current_exe().context("cannot determine own executable")?;
        let status = std::process::Command::new(&self_exe)
            .arg("fetch")
            .arg(&args.name)
            .status()
            .context("failed to fetch")?;
        if !status.success() {
            bail!("fetch from {} failed", args.name);
        }
    }

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

    // Remove remote tracking refs (refs/remotes/<name>/*).
    let remotes_dir = git_dir.join("refs").join("remotes").join(&args.name);
    if remotes_dir.is_dir() {
        std::fs::remove_dir_all(&remotes_dir)
            .with_context(|| format!("removing refs/remotes/{}", args.name))?;
    }

    // Also remove from packed-refs if present.
    let packed_refs_path = git_dir.join("packed-refs");
    if packed_refs_path.is_file() {
        let prefix = format!("refs/remotes/{}/", args.name);
        let content = std::fs::read_to_string(&packed_refs_path).context("reading packed-refs")?;
        let filtered: String = content
            .lines()
            .filter(|line| {
                // Keep comment/header lines and lines that don't reference this remote
                if line.starts_with('#') || line.starts_with('^') {
                    return true;
                }
                // Each line is "<hash> <refname>" — check if refname matches
                if let Some(refname) = line.split_whitespace().nth(1) {
                    !refname.starts_with(&prefix)
                } else {
                    true
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let filtered = if filtered.is_empty() || filtered.ends_with('\n') {
            filtered
        } else {
            format!("{filtered}\n")
        };
        std::fs::write(&packed_refs_path, filtered).context("writing packed-refs")?;
    }

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
        std::fs::rename(&old_refs_dir, &new_refs_dir).with_context(|| {
            format!(
                "renaming refs/remotes/{} to refs/remotes/{}",
                args.old, args.new
            )
        })?;
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

// ── Set Branches ────────────────────────────────────────────────────

fn cmd_set_branches(args: SetBranchesArgs) -> Result<()> {
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
    let fetch_key = format!("remote.{}.fetch", args.name);

    if !args.add {
        // Remove all existing fetch refspecs first
        config_file.unset(&fetch_key)?;
    }

    // Add a fetch refspec for each branch pattern
    for branch in &args.branches {
        let refspec = format!("+refs/heads/{branch}:refs/remotes/{}/{branch}", args.name);
        config_file.add_value(&fetch_key, &refspec)?;
    }

    config_file.write().context("writing config")?;
    Ok(())
}

// ── Prune ───────────────────────────────────────────────────────────

fn cmd_prune(args: PruneArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config = load_local_config(&git_dir)?;
    let remotes = collect_remotes(&config);

    if !remotes.contains_key(&args.name) {
        bail!("No such remote '{}'", args.name);
    }

    let info = &remotes[&args.name];
    let url = &info.url;

    // Open the remote repo to see which branches still exist
    let remote_path = if let Some(stripped) = url.strip_prefix("file://") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(url)
    };

    let remote_git_dir = find_git_dir(&remote_path)?;
    let remote_heads = grit_lib::refs::list_refs(&remote_git_dir, "refs/heads/")?;

    // List our local remote-tracking refs for this remote
    let prefix = format!("refs/remotes/{}/", args.name);
    let local_tracking = grit_lib::refs::list_refs(&git_dir, &prefix)?;

    let mut pruned = false;
    for (local_ref, _oid) in &local_tracking {
        // Reconstruct what remote ref this corresponds to
        let branch = local_ref.strip_prefix(&prefix).unwrap_or(local_ref);
        let remote_ref = format!("refs/heads/{branch}");
        if !remote_heads.iter().any(|(r, _)| r == &remote_ref) {
            grit_lib::refs::delete_ref(&git_dir, local_ref)
                .with_context(|| format!("pruning {local_ref}"))?;
            eprintln!(" * [pruned] {}", local_ref);
            pruned = true;
        }
    }

    if !pruned {
        eprintln!("No stale remote-tracking branches for '{}'", args.name);
    }

    Ok(())
}

// ── Update ──────────────────────────────────────────────────────────

fn cmd_update(args: UpdateArgs) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config = load_local_config(&git_dir)?;
    let all_remotes = collect_remotes(&config);

    // Determine which remotes to fetch
    let remote_names: Vec<String> = if args.groups.is_empty() {
        // Fetch all remotes
        all_remotes.keys().cloned().collect()
    } else {
        // Each arg can be a remote name or a remotes group
        let mut names = Vec::new();
        for group in &args.groups {
            if all_remotes.contains_key(group) {
                names.push(group.clone());
            } else {
                // Check remotes.<group> config for group members
                let group_key = format!("remotes.{group}");
                if let Some(members) = config.get(&group_key) {
                    for member in members.split_whitespace() {
                        if !names.contains(&member.to_string()) {
                            names.push(member.to_string());
                        }
                    }
                } else {
                    bail!("No such remote or remote group: '{}'", group);
                }
            }
        }
        names
    };

    // Fetch from each remote using the fetch command
    for name in &remote_names {
        eprintln!("Fetching {name}");
        let fetch_args = super::fetch::Args {
            remote: Some(name.clone()),
            refspecs: Vec::new(),
            filter: None,
            all: false,
            tags: false,
            no_tags: false,
            prune: false,
            prune_tags: false,
            deepen: None,
            depth: None,
            shallow_since: None,
            shallow_exclude: None,
            refetch: false,
            output: None,
            quiet: false,
            jobs: None,
            porcelain: false,
            no_show_forced_updates: false,
            show_forced_updates: false,
            negotiate_only: false,
            update_head_ok: false,
            update_refs: false,
        };
        super::fetch::run(fetch_args)?;
    }

    Ok(())
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

/// Find the .git directory for a given path (bare or non-bare repo).
fn find_git_dir(path: &Path) -> Result<PathBuf> {
    // Bare repo: path itself has objects/ and HEAD
    if path.join("objects").is_dir() && path.join("HEAD").is_file() {
        return Ok(path.to_path_buf());
    }
    // Non-bare: path/.git
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return Ok(dot_git);
    }
    bail!("not a git repository: '{}'", path.display())
}
