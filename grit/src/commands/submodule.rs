//! `grit submodule` — manage submodules.
//!
//! Supports: status, init, update, add, foreach.
//! Reads `.gitmodules` and manages `.git/modules/` directory.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::repo::Repository;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Arguments for `grit submodule`.
#[derive(Debug, ClapArgs)]
#[command(about = "Initialize, update, or inspect submodules")]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<SubmoduleCommand>,
}

/// Subcommands for `grit submodule`.
#[derive(Debug, Subcommand)]
pub enum SubmoduleCommand {
    /// Show the status of submodules.
    Status(StatusArgs),
    /// Initialize submodule configuration from .gitmodules.
    Init(InitArgs),
    /// Checkout the recorded submodule commits.
    Update(UpdateArgs),
    /// Add a new submodule.
    Add(AddArgs),
    /// Run a command in each submodule.
    Foreach(ForeachArgs),
}

#[derive(Debug, ClapArgs)]
pub struct StatusArgs {
    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct InitArgs {
    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct UpdateArgs {
    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,

    /// Initialize uninitialized submodules before updating.
    #[arg(long)]
    pub init: bool,
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// URL of the submodule repository.
    pub url: String,

    /// Path where the submodule should be placed.
    pub path: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ForeachArgs {
    /// Command to run in each submodule.
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Parsed entry from .gitmodules.
#[derive(Debug, Clone)]
struct SubmoduleInfo {
    name: String,
    path: String,
    url: String,
}

/// Run the `submodule` command.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        None => run_status(&StatusArgs { paths: vec![] }),
        Some(SubmoduleCommand::Status(a)) => run_status(&a),
        Some(SubmoduleCommand::Init(a)) => run_init(&a),
        Some(SubmoduleCommand::Update(a)) => run_update(&a),
        Some(SubmoduleCommand::Add(a)) => run_add(&a),
        Some(SubmoduleCommand::Foreach(a)) => run_foreach(&a),
    }
}

// ── .gitmodules parsing ──────────────────────────────────────────────

/// Parse `.gitmodules` into a list of submodule entries.
fn parse_gitmodules(work_tree: &Path) -> Result<Vec<SubmoduleInfo>> {
    let gitmodules_path = work_tree.join(".gitmodules");
    if !gitmodules_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&gitmodules_path)
        .context("failed to read .gitmodules")?;

    let config = ConfigFile::parse(&gitmodules_path, &content, ConfigScope::Local)
        .context("failed to parse .gitmodules")?;

    // Collect entries by submodule name.
    let mut modules: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();

    for entry in &config.entries {
        // Keys look like: submodule.<name>.path, submodule.<name>.url
        let key = &entry.key;
        if !key.starts_with("submodule.") {
            continue;
        }
        // Strip "submodule." prefix and split on last dot.
        let rest = &key["submodule.".len()..];
        if let Some(last_dot) = rest.rfind('.') {
            let name = &rest[..last_dot];
            let var = &rest[last_dot + 1..];
            let entry_val = modules.entry(name.to_string()).or_insert((None, None));
            match var {
                "path" => entry_val.0 = entry.value.clone(),
                "url" => entry_val.1 = entry.value.clone(),
                _ => {}
            }
        }
    }

    let mut result = Vec::new();
    for (name, (path, url)) in modules {
        if let (Some(path), Some(url)) = (path, url) {
            result.push(SubmoduleInfo { name, path, url });
        }
    }

    Ok(result)
}

/// Filter submodules by path args (empty = all).
fn filter_submodules<'a>(
    modules: &'a [SubmoduleInfo],
    paths: &[String],
) -> Vec<&'a SubmoduleInfo> {
    if paths.is_empty() {
        modules.iter().collect()
    } else {
        modules
            .iter()
            .filter(|m| paths.iter().any(|p| p == &m.path || p == &m.name))
            .collect()
    }
}

// ── Read recorded commit from the index ──────────────────────────────

/// Read the commit OID recorded in the index for a submodule path.
fn read_submodule_commit(git_dir: &Path, submodule_path: &str) -> Result<Option<String>> {
    // Use git ls-tree HEAD to get the recorded commit for the submodule path.
    let git_bin = std::env::var("REAL_GIT").unwrap_or_else(|_| "/usr/bin/git".to_string());
    let output = Command::new(&git_bin)
        .arg("ls-tree")
        .arg("HEAD")
        .arg(submodule_path)
        .current_dir(
            git_dir
                .parent()
                .unwrap_or(git_dir),
        )
        .output()
        .context("failed to run git ls-tree")?;

    if !output.status.success() {
        return Ok(None);
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // Format: <mode> <type> <oid>\t<path>
    for line in text.lines() {
        let parts: Vec<&str> = line.splitn(4, |c: char| c == ' ' || c == '\t').collect();
        if parts.len() >= 3 && parts[1] == "commit" {
            return Ok(Some(parts[2].to_string()));
        }
    }

    Ok(None)
}

// ── Subcommand implementations ───────────────────────────────────────

fn run_status(args: &StatusArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules(work_tree)?;
    let selected = filter_submodules(&modules, &args.paths);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for m in selected {
        let sub_path = work_tree.join(&m.path);
        let recorded = read_submodule_commit(&repo.git_dir, &m.path)?;

        // Check if the submodule is checked out (has a .git file/dir in its path).
        let has_checkout = sub_path.join(".git").exists();

        if !sub_path.exists() || !has_checkout {
            // Not initialized / not checked out.
            let oid = recorded.as_deref().unwrap_or("0000000000000000000000000000000000000000");
            writeln!(out, "-{oid} {}", m.path)?;
        } else {
            // Check current HEAD of the submodule.
            let head_file = sub_path.join(".git");
            let sub_head = if head_file.exists() {
                read_submodule_head(&sub_path)
            } else {
                // gitfile indirection: check .git/modules/<name>/HEAD
                let modules_head = repo.git_dir.join("modules").join(&m.name).join("HEAD");
                if modules_head.exists() {
                    read_head_from_file(&modules_head)
                } else {
                    None
                }
            };

            let recorded_oid = recorded.as_deref().unwrap_or("");
            let current_oid = sub_head.as_deref().unwrap_or("");

            let prefix = if current_oid == recorded_oid {
                " "
            } else {
                "+"
            };

            let display_oid = if current_oid.is_empty() {
                recorded_oid
            } else {
                current_oid
            };

            writeln!(out, "{prefix}{display_oid} {}", m.path)?;
        }
    }

    Ok(())
}

/// Read HEAD of a submodule working directory.
fn read_submodule_head(sub_path: &Path) -> Option<String> {
    // If .git is a file (gitfile), follow it.
    let dot_git = sub_path.join(".git");
    let git_dir = if dot_git.is_file() {
        let content = fs::read_to_string(&dot_git).ok()?;
        let gitdir = content.strip_prefix("gitdir: ")?.trim();
        if Path::new(gitdir).is_absolute() {
            PathBuf::from(gitdir)
        } else {
            sub_path.join(gitdir)
        }
    } else if dot_git.is_dir() {
        dot_git
    } else {
        return None;
    };

    read_head_from_dir(&git_dir)
}

/// Read the HEAD OID from a git directory.
fn read_head_from_dir(git_dir: &Path) -> Option<String> {
    read_head_from_file(&git_dir.join("HEAD"))
}

/// Read HEAD from a specific file, resolving symbolic refs.
fn read_head_from_file(head_file: &Path) -> Option<String> {
    let content = fs::read_to_string(head_file).ok()?;
    let content = content.trim();
    if let Some(refname) = content.strip_prefix("ref: ") {
        // Resolve the ref.
        let git_dir = head_file.parent()?;
        let ref_file = git_dir.join(refname);
        fs::read_to_string(ref_file).ok().map(|s| s.trim().to_string())
    } else {
        Some(content.to_string())
    }
}

fn run_init(args: &InitArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules(work_tree)?;
    let selected = filter_submodules(&modules, &args.paths);

    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    for m in selected {
        let url_key = format!("submodule.{}.url", m.name);
        // Check if already initialized.
        let already = config.entries.iter().any(|e| e.key == url_key);
        if already {
            continue;
        }

        config.set(&url_key, &m.url)?;
        eprintln!(
            "Submodule '{}' ({}) registered for path '{}'",
            m.name, m.url, m.path
        );
    }

    config.write()?;
    Ok(())
}

fn run_update(args: &UpdateArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;

    if args.init {
        run_init(&InitArgs {
            paths: args.paths.clone(),
        })?;
    }

    let modules = parse_gitmodules(work_tree)?;
    let selected = filter_submodules(&modules, &args.paths);

    let git_bin = std::env::var("REAL_GIT").unwrap_or_else(|_| "/usr/bin/git".to_string());

    for m in selected {
        let sub_path = work_tree.join(&m.path);
        let recorded = read_submodule_commit(&repo.git_dir, &m.path)?;
        let recorded_oid = match &recorded {
            Some(oid) => oid.as_str(),
            None => {
                eprintln!("Skipping submodule '{}': not in index", m.path);
                continue;
            }
        };

        // Read URL from local config (must be initialized).
        let config_path = repo.git_dir.join("config");
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;
            let url_key = format!("submodule.{}.url", m.name);
            let has_url = config.entries.iter().any(|e| e.key == url_key);
            if !has_url {
                eprintln!(
                    "Skipping submodule '{}': not initialized (run 'grit submodule init')",
                    m.path
                );
                continue;
            }
        }

        let modules_dir = repo.git_dir.join("modules").join(&m.name);

        if !sub_path.exists() {
            // Clone the submodule into .git/modules/<name> then checkout.
            fs::create_dir_all(&modules_dir)?;

            let status = Command::new(&git_bin)
                .arg("clone")
                .arg("--no-checkout")
                .arg("--separate-git-dir")
                .arg(&modules_dir)
                .arg(&m.url)
                .arg(&sub_path)
                .status()
                .context("failed to clone submodule")?;

            if !status.success() {
                bail!("failed to clone submodule '{}'", m.name);
            }
        }

        // Checkout the recorded commit.
        let status = Command::new(&git_bin)
            .arg("checkout")
            .arg(recorded_oid)
            .arg("--quiet")
            .current_dir(&sub_path)
            .status()
            .context("failed to checkout submodule commit")?;

        if !status.success() {
            bail!(
                "failed to checkout {} in submodule '{}'",
                recorded_oid,
                m.name
            );
        }

        eprintln!(
            "Submodule path '{}': checked out '{}'",
            m.path,
            &recorded_oid[..recorded_oid.len().min(12)]
        );
    }

    Ok(())
}

fn run_add(args: &AddArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;

    // Derive path from URL if not provided.
    let path = match &args.path {
        Some(p) => p.clone(),
        None => {
            let url = &args.url;
            let basename = url
                .rsplit('/')
                .next()
                .unwrap_or(url)
                .strip_suffix(".git")
                .unwrap_or(
                    url.rsplit('/').next().unwrap_or(url),
                );
            basename.to_string()
        }
    };

    let sub_path = work_tree.join(&path);

    if sub_path.exists() {
        bail!("'{}' already exists", path);
    }

    let git_bin = std::env::var("REAL_GIT").unwrap_or_else(|_| "/usr/bin/git".to_string());

    // Clone the submodule.
    let modules_dir = repo.git_dir.join("modules").join(&path);
    fs::create_dir_all(&modules_dir)?;

    let status = Command::new(&git_bin)
        .arg("clone")
        .arg("--separate-git-dir")
        .arg(&modules_dir)
        .arg(&args.url)
        .arg(&sub_path)
        .status()
        .context("failed to clone submodule")?;

    if !status.success() {
        bail!("failed to clone submodule from '{}'", args.url);
    }

    // Derive the submodule name (same as path for simplicity).
    let name = &path;

    // Update .gitmodules.
    let gitmodules_path = work_tree.join(".gitmodules");
    let mut config = if gitmodules_path.exists() {
        let content = fs::read_to_string(&gitmodules_path)?;
        ConfigFile::parse(&gitmodules_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&gitmodules_path, "", ConfigScope::Local)?
    };

    config.set(&format!("submodule.{name}.path"), &path)?;
    config.set(&format!("submodule.{name}.url"), &args.url)?;
    config.write()?;

    // Add the submodule path to the index.
    let status = Command::new(&git_bin)
        .arg("add")
        .arg(".gitmodules")
        .arg(&path)
        .current_dir(work_tree)
        .status()
        .context("failed to stage submodule")?;

    if !status.success() {
        bail!("failed to stage submodule");
    }

    eprintln!("Cloning into '{}'...", path);
    Ok(())
}

fn run_foreach(args: &ForeachArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules(work_tree)?;

    let cmd_str = args.command.join(" ");

    for m in &modules {
        let sub_path = work_tree.join(&m.path);
        if !sub_path.exists() {
            continue;
        }

        eprintln!("Entering '{}'", m.path);
        let status = Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .current_dir(&sub_path)
            .env("name", &m.name)
            .env("sm_path", &m.path)
            .env("displaypath", &m.path)
            .env("toplevel", work_tree.to_string_lossy().as_ref())
            .status()
            .context("failed to run foreach command")?;

        if !status.success() {
            bail!(
                "Stopping at '{}'; command returned non-zero status",
                m.path
            );
        }
    }

    Ok(())
}
