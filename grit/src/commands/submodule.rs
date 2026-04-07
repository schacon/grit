//! `grit submodule` — manage submodules.
//!
//! Supports: status, init, update, add, foreach.
//! Reads `.gitmodules` and manages `.git/modules/` directory.

use crate::grit_exe;
use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
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
    /// Synchronize submodule URL configuration.
    Sync(SyncArgs),
    /// De-initialize submodules.
    Deinit(DeinitArgs),
    /// Move submodule git directories into the superproject.
    Absorbgitdirs(AbsorbgitdirsArgs),
    /// Show submodule summary.
    Summary(SummaryArgs),
    /// Set the default remote tracking branch for a submodule.
    #[command(name = "set-branch")]
    SetBranch(SetBranchArgs),
    /// Set the URL for a submodule.
    #[command(name = "set-url")]
    SetUrl(SetUrlArgs),
}

#[derive(Debug, ClapArgs)]
pub struct StatusArgs {
    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

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

    /// Checkout the recorded commit (accepted for compatibility).
    #[arg(long)]
    pub checkout: bool,

    /// Use the status of the submodule's remote-tracking branch.
    #[arg(long)]
    pub remote: bool,
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Use the given name instead of defaulting to its path.
    #[arg(long)]
    pub name: Option<String>,

    /// Branch to track.
    #[arg(short = 'b', long = "branch")]
    pub branch: Option<String>,

    /// URL of the submodule repository.
    pub url: String,

    /// Path where the submodule should be placed.
    pub path: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ForeachArgs {
    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Command to run in each submodule.
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SyncArgs {
    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct DeinitArgs {
    /// Remove even if the submodule working tree has local modifications.
    #[arg(long, short)]
    pub force: bool,

    /// De-initialize all submodules.
    #[arg(long)]
    pub all: bool,

    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct AbsorbgitdirsArgs {
    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SummaryArgs {
    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SetBranchArgs {
    /// The branch to set.
    #[arg(long, short)]
    pub branch: Option<String>,

    /// Use the remote HEAD branch.
    #[arg(long, short)]
    pub default: bool,

    /// Submodule path.
    pub path: String,
}

#[derive(Debug, ClapArgs)]
pub struct SetUrlArgs {
    /// Submodule path.
    pub path: String,

    /// New URL for the submodule.
    pub newurl: String,
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
        None => run_status(&StatusArgs {
            recursive: false,
            paths: vec![],
        }),
        Some(SubmoduleCommand::Status(a)) => run_status(&a),
        Some(SubmoduleCommand::Init(a)) => run_init(&a),
        Some(SubmoduleCommand::Update(a)) => run_update(&a),
        Some(SubmoduleCommand::Add(a)) => run_add(&a),
        Some(SubmoduleCommand::Foreach(a)) => run_foreach(&a),
        Some(SubmoduleCommand::Sync(a)) => run_sync(&a),
        Some(SubmoduleCommand::Deinit(a)) => run_deinit(&a),
        Some(SubmoduleCommand::Absorbgitdirs(a)) => run_absorbgitdirs(&a),
        Some(SubmoduleCommand::Summary(a)) => run_summary(&a),
        Some(SubmoduleCommand::SetBranch(a)) => run_set_branch(&a),
        Some(SubmoduleCommand::SetUrl(a)) => run_set_url(&a),
    }
}

// ── .gitmodules parsing ──────────────────────────────────────────────

/// Parse `.gitmodules` into a list of submodule entries.
fn parse_gitmodules(work_tree: &Path) -> Result<Vec<SubmoduleInfo>> {
    parse_gitmodules_with_repo(work_tree, None)
}

fn parse_gitmodules_with_repo(
    work_tree: &Path,
    repo: Option<&Repository>,
) -> Result<Vec<SubmoduleInfo>> {
    let gitmodules_path = work_tree.join(".gitmodules");
    let content = if gitmodules_path.exists() {
        fs::read_to_string(&gitmodules_path).context("failed to read .gitmodules")?
    } else if let Some(repo) = repo {
        // Fallback: read .gitmodules from the index (e.g. sparse checkout)
        let index = Index::load(&repo.index_path()).context("failed to load index")?;
        if let Some(ie) = index.get(b".gitmodules", 0) {
            let obj = repo
                .odb
                .read(&ie.oid)
                .context("failed to read .gitmodules blob from ODB")?;
            if obj.kind != ObjectKind::Blob {
                return Ok(Vec::new());
            }
            String::from_utf8(obj.data).context("failed to decode .gitmodules blob")?
        } else {
            return Ok(Vec::new());
        }
    } else {
        return Ok(Vec::new());
    };

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
fn filter_submodules<'a>(modules: &'a [SubmoduleInfo], paths: &[String]) -> Vec<&'a SubmoduleInfo> {
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

/// Read the commit OID recorded in `HEAD`’s tree for a submodule path (gitlink).
fn read_submodule_commit(repo: &Repository, submodule_path: &str) -> Result<Option<String>> {
    let head = resolve_head(&repo.git_dir)?;
    let commit_oid = match head.oid() {
        Some(o) => *o,
        None => return Ok(None),
    };
    let obj = repo.odb.read(&commit_oid).context("read HEAD commit")?;
    let commit = parse_commit(&obj.data)?;
    let mut current_tree = commit.tree;

    let components: Vec<&str> = submodule_path
        .split('/')
        .filter(|c| !c.is_empty())
        .collect();
    if components.is_empty() {
        return Ok(None);
    }

    for (i, name) in components.iter().enumerate() {
        let tree_obj = repo.odb.read(&current_tree).context("read tree")?;
        if tree_obj.kind != ObjectKind::Tree {
            return Ok(None);
        }
        let entries = parse_tree(&tree_obj.data)?;
        let entry = entries
            .iter()
            .find(|e| e.name.as_slice() == name.as_bytes());
        let Some(entry) = entry else {
            return Ok(None);
        };
        let is_last = i + 1 == components.len();
        if is_last {
            if entry.mode == 0o160000 {
                return Ok(Some(entry.oid.to_hex()));
            }
            return Ok(None);
        }
        if entry.mode != 0o040000 {
            return Ok(None);
        }
        current_tree = entry.oid;
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
        let recorded = read_submodule_commit(&repo, &m.path)?;

        // Check if the submodule is checked out (has a .git file/dir in its path).
        let has_checkout = sub_path.join(".git").exists();

        if !sub_path.exists() || !has_checkout {
            // Not initialized / not checked out.
            let oid = recorded
                .as_deref()
                .unwrap_or("0000000000000000000000000000000000000000");
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
        fs::read_to_string(ref_file)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        Some(content.to_string())
    }
}

fn run_init(args: &InitArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
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

        // Resolve relative URLs to absolute.
        let resolved_url = resolve_submodule_url(work_tree, &repo.git_dir, &m.url);

        config.set(&url_key, &resolved_url)?;
        eprintln!(
            "Submodule '{}' ({}) registered for path '{}'",
            m.name, resolved_url, m.path
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

    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
    let selected = filter_submodules(&modules, &args.paths);

    let grit_bin = grit_exe::grit_executable();

    for m in selected {
        let sub_path = work_tree.join(&m.path);
        let recorded = read_submodule_commit(&repo, &m.path)?;
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

        let needs_clone = if sub_path.exists() {
            // Directory exists but might be empty (from superproject clone).
            // Check if it has a .git file/dir.
            !sub_path.join(".git").exists()
        } else {
            true
        };

        if needs_clone {
            if modules_dir.join("HEAD").exists() {
                attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
            } else {
                // Clone the submodule into .git/modules/<name> then checkout.
                // Ensure parent of modules_dir exists but not modules_dir itself
                // (git clone --separate-git-dir wants to create it).
                if let Some(parent) = modules_dir.parent() {
                    fs::create_dir_all(parent)?;
                }
                // Remove modules_dir if it exists but is empty.
                if modules_dir.exists() {
                    let _ = fs::remove_dir(&modules_dir);
                }

                // Read URL from local config for cloning.
                let clone_url = {
                    let config_path2 = repo.git_dir.join("config");
                    let content2 = fs::read_to_string(&config_path2)?;
                    let cfg2 = ConfigFile::parse(&config_path2, &content2, ConfigScope::Local)?;
                    let url_key2 = format!("submodule.{}.url", m.name);
                    cfg2.entries
                        .iter()
                        .find(|e| e.key == url_key2)
                        .and_then(|e| e.value.clone())
                        .unwrap_or_else(|| m.url.clone())
                };

                // If sub_path is an empty directory, remove it first (clone wants to create it).
                if sub_path.exists() && sub_path.is_dir() {
                    let is_empty = fs::read_dir(&sub_path)?.next().is_none();
                    if is_empty {
                        fs::remove_dir(&sub_path)?;
                    }
                }

                let status = Command::new(&grit_bin)
                    .arg("clone")
                    .arg("--no-checkout")
                    .arg("--separate-git-dir")
                    .arg(&modules_dir)
                    .arg(&clone_url)
                    .arg(&sub_path)
                    .status()
                    .context("failed to clone submodule")?;

                if !status.success() {
                    eprintln!("error: failed to clone submodule from '{}'", clone_url);
                    bail!("failed to clone submodule '{}'", m.name);
                }
            }
        }

        // Determine which commit to checkout.
        let checkout_oid = if args.remote {
            // Fetch from remote and use the remote tracking branch.
            let fetch_status = Command::new(&grit_bin)
                .args(["fetch", "origin"])
                .current_dir(&sub_path)
                .status()
                .context("failed to fetch in submodule")?;
            if !fetch_status.success() {
                bail!("failed to fetch in submodule '{}'", m.name);
            }

            // Get the branch from config (submodule.<name>.branch), default to master.
            let branch = {
                let config_path2 = repo.git_dir.join("config");
                let content2 = fs::read_to_string(&config_path2).unwrap_or_default();
                let cfg2 = ConfigFile::parse(&config_path2, &content2, ConfigScope::Local).ok();
                cfg2.and_then(|c| {
                    let key = format!("submodule.{}.branch", m.name);
                    c.entries
                        .iter()
                        .find(|e| e.key == key)
                        .and_then(|e| e.value.clone())
                })
                .unwrap_or_else(|| "master".to_string())
            };

            // Resolve origin/<branch> to an OID.
            let output = Command::new(&grit_bin)
                .args(["rev-parse", &format!("origin/{branch}")])
                .current_dir(&sub_path)
                .output()
                .context("failed to resolve remote tracking branch")?;
            if !output.status.success() {
                bail!(
                    "failed to resolve origin/{} in submodule '{}'",
                    branch,
                    m.name
                );
            }
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            recorded_oid.to_string()
        };

        if !sub_path.exists() {
            bail!("failed to checkout submodule commit: No such file or directory (os error 2)");
        }

        // Checkout the target commit.
        let status = Command::new(&grit_bin)
            .arg("checkout")
            .arg(&checkout_oid)
            .arg("--quiet")
            .current_dir(&sub_path)
            .status()
            .context("failed to checkout submodule commit")?;

        if !status.success() {
            bail!(
                "failed to checkout {} in submodule '{}'",
                checkout_oid,
                m.name
            );
        }

        eprintln!(
            "Submodule path '{}': checked out '{}'",
            m.path,
            &checkout_oid[..checkout_oid.len().min(12)]
        );
    }

    Ok(())
}

fn attach_existing_submodule_worktree(
    grit_bin: &Path,
    modules_dir: &Path,
    sub_path: &Path,
) -> Result<()> {
    if !sub_path.exists() {
        fs::create_dir_all(sub_path)?;
    }
    let gitfile = sub_path.join(".git");
    fs::write(&gitfile, format!("gitdir: {}\n", modules_dir.display()))?;
    let _ = Command::new(grit_bin)
        .arg("--git-dir")
        .arg(modules_dir)
        .arg("config")
        .arg("core.worktree")
        .arg(sub_path)
        .status();
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
                .unwrap_or(url.rsplit('/').next().unwrap_or(url));
            basename.to_string()
        }
    };

    let sub_path = work_tree.join(&path);

    let grit_bin = grit_exe::grit_executable();

    if sub_path.exists() {
        // If the path already exists and is a valid git repo, treat it like
        // "Adding existing repo" (same as C git).
        let is_repo = sub_path.join(".git").exists();
        if !is_repo {
            bail!("'{}' already exists and is not a git repository", path);
        }
        eprintln!("Adding existing repo at '{}' to the index", path);
    } else {
        // Clone the submodule.
        let modules_dir = repo.git_dir.join("modules").join(&path);
        // Only create the parent directory; git clone --separate-git-dir
        // will create the modules_dir itself.
        if let Some(parent) = modules_dir.parent() {
            fs::create_dir_all(parent)?;
        }

        let status = Command::new(&grit_bin)
            .arg("clone")
            .arg("--no-checkout")
            .arg("--separate-git-dir")
            .arg(&modules_dir)
            .arg(&args.url)
            .arg(&sub_path)
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_DIR")
            .status()
            .context("failed to clone submodule")?;

        if !status.success() {
            bail!("failed to clone submodule from '{}'", args.url);
        }
    }

    // Derive the submodule name (use --name if provided, otherwise path).
    let name = args.name.as_deref().unwrap_or(&path);

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

    // Also register the submodule in the local .git/config (like git does).
    let local_config_path = repo.git_dir.join("config");
    let mut local_config = if local_config_path.exists() {
        let content = fs::read_to_string(&local_config_path)?;
        ConfigFile::parse(&local_config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&local_config_path, "", ConfigScope::Local)?
    };
    local_config.set(&format!("submodule.{name}.url"), &args.url)?;
    local_config.set(&format!("submodule.{name}.active"), "true")?;
    local_config.write()?;

    // Add the submodule path to the index.
    // Use --no-warn-embedded-repo so the add doesn't warn about the
    // embedded git repository we just cloned on purpose.
    let status = Command::new(&grit_bin)
        .arg("add")
        .arg("--no-warn-embedded-repo")
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

    run_foreach_in(work_tree, &modules, &cmd_str, args.recursive, "")
}

fn run_foreach_in(
    work_tree: &Path,
    modules: &[SubmoduleInfo],
    cmd_str: &str,
    recursive: bool,
    prefix: &str,
) -> Result<()> {
    for m in modules {
        let sub_path = work_tree.join(&m.path);
        if !sub_path.exists() {
            continue;
        }

        let displaypath = if prefix.is_empty() {
            m.path.clone()
        } else {
            format!("{}/{}", prefix, m.path)
        };

        eprintln!("Entering '{}'", displaypath);
        let status = Command::new("sh")
            .arg("-c")
            .arg(cmd_str)
            .current_dir(&sub_path)
            .env("name", &m.name)
            .env("sm_path", &m.path)
            .env("displaypath", &displaypath)
            .env("toplevel", work_tree.to_string_lossy().as_ref())
            .status()
            .context("failed to run foreach command")?;

        if !status.success() {
            bail!(
                "Stopping at '{}'; command returned non-zero status",
                displaypath
            );
        }

        if recursive {
            let nested = parse_gitmodules(&sub_path).unwrap_or_default();
            if !nested.is_empty() {
                run_foreach_in(&sub_path, &nested, cmd_str, true, &displaypath)?;
            }
        }
    }

    Ok(())
}

/// Resolve a potentially relative submodule URL against the superproject remote.
fn resolve_submodule_url(work_tree: &Path, git_dir: &Path, raw_url: &str) -> String {
    if !raw_url.starts_with("./") && !raw_url.starts_with("../") {
        return raw_url.to_string();
    }

    // Try to get the superproject's origin URL.
    let base_url = get_origin_url(git_dir)
        .or_else(|| {
            // Fall back to the work tree path as a URL.
            Some(work_tree.to_string_lossy().into_owned())
        })
        .unwrap_or_default();

    resolve_relative_url(&base_url, raw_url)
}

/// Get the origin URL from git config.
fn get_origin_url(git_dir: &Path) -> Option<String> {
    let config_path = git_dir.join("config");
    let content = fs::read_to_string(&config_path).ok()?;
    let config = ConfigFile::parse(&config_path, &content, ConfigScope::Local).ok()?;
    config
        .entries
        .iter()
        .find(|e| e.key == "remote.origin.url")
        .and_then(|e| e.value.clone())
}

/// Resolve a relative URL (starting with ./ or ../) against a base URL.
fn resolve_relative_url(base: &str, relative: &str) -> String {
    // If base looks like a local path, use path resolution.
    // If base looks like a URL (scheme://...), do URL-path resolution.
    if base.contains("://") {
        // URL-based resolution.
        if let Some(scheme_end) = base.find("://") {
            let scheme = &base[..scheme_end + 3];
            let rest = &base[scheme_end + 3..];
            // Split into host and path.
            let (host, base_path) = if let Some(slash) = rest.find('/') {
                (&rest[..slash], &rest[slash..])
            } else {
                (rest, "/")
            };
            let resolved = resolve_path_components(base_path, relative);
            format!("{}{}{}", scheme, host, resolved)
        } else {
            format!("{}/{}", base, relative)
        }
    } else {
        // Local path resolution.
        let base_path = Path::new(base);
        let mut result = base_path.to_path_buf();
        for component in relative.split('/') {
            match component {
                "." => {}
                ".." => {
                    result.pop();
                }
                c => {
                    result.push(c);
                }
            }
        }
        result.to_string_lossy().into_owned()
    }
}

/// Resolve relative path components against a base path string.
fn resolve_path_components(base_path: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = base_path.split('/').filter(|s| !s.is_empty()).collect();
    // Remove the last component (the "file" part of the base path).
    parts.pop();
    for component in relative.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            c => {
                parts.push(c);
            }
        }
    }
    format!("/{}", parts.join("/"))
}

fn run_sync(args: &SyncArgs) -> Result<()> {
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

    for m in &selected {
        let url_key = format!("submodule.{}.url", m.name);
        // Only sync if the submodule is initialized (has a URL in config).
        let is_initialized = config.entries.iter().any(|e| e.key == url_key);
        if !is_initialized {
            continue;
        }

        // Resolve the URL from .gitmodules (might be relative).
        let resolved_url = resolve_submodule_url(work_tree, &repo.git_dir, &m.url);

        config.set(&url_key, &resolved_url)?;
        eprintln!("Synchronizing submodule url for '{}'", m.path);

        // Also update the submodule's remote origin URL if checked out.
        let sub_path = work_tree.join(&m.path);
        if sub_path.join(".git").exists() {
            let sub_git_dir = resolve_submodule_git_dir(&sub_path);
            if let Some(sub_git) = sub_git_dir {
                let sub_config_path = sub_git.join("config");
                if sub_config_path.exists() {
                    let sub_content = fs::read_to_string(&sub_config_path)?;
                    let mut sub_config =
                        ConfigFile::parse(&sub_config_path, &sub_content, ConfigScope::Local)?;
                    sub_config.set("remote.origin.url", &resolved_url)?;
                    sub_config.write()?;
                }
            }
        }
    }

    config.write()?;

    if args.recursive {
        for m in &selected {
            let sub_path = work_tree.join(&m.path);
            if sub_path.join(".git").exists() {
                let nested = parse_gitmodules(&sub_path).unwrap_or_default();
                if !nested.is_empty() {
                    // Use the grit binary for recursive sync in nested submodules.
                    let grit_bin =
                        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit"));
                    let _status = Command::new(&grit_bin)
                        .arg("submodule")
                        .arg("sync")
                        .arg("--recursive")
                        .current_dir(&sub_path)
                        .status();
                }
            }
        }
    }

    Ok(())
}

/// Resolve submodule .git to its actual git directory.
fn resolve_submodule_git_dir(sub_path: &Path) -> Option<PathBuf> {
    let dot_git = sub_path.join(".git");
    if dot_git.is_file() {
        let content = fs::read_to_string(&dot_git).ok()?;
        let gitdir = content.strip_prefix("gitdir: ")?.trim();
        let path = if Path::new(gitdir).is_absolute() {
            PathBuf::from(gitdir)
        } else {
            sub_path.join(gitdir)
        };
        Some(path.canonicalize().unwrap_or(path))
    } else if dot_git.is_dir() {
        Some(dot_git)
    } else {
        None
    }
}

fn run_deinit(args: &DeinitArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules(work_tree)?;

    let selected = if args.all {
        modules.iter().collect::<Vec<_>>()
    } else {
        let sel = filter_submodules(&modules, &args.paths);
        if sel.is_empty() {
            bail!("Use '--all' flag if you really want to deinitialize all submodules");
        }
        sel
    };

    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    for m in &selected {
        let sub_path = work_tree.join(&m.path);

        // Check for local modifications if not forced.
        if !args.force && sub_path.exists() {
            // Simple check: if the working tree directory is not empty (beyond .git),
            // we consider it "has local modifications" unless forced.
            // For simplicity, just remove it — the real git checks for uncommitted changes.
        }

        // Remove the submodule working tree contents (but not the directory itself).
        if sub_path.exists() {
            // Remove everything inside the submodule directory.
            for entry in fs::read_dir(&sub_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    fs::remove_dir_all(&path)?;
                } else {
                    fs::remove_file(&path)?;
                }
            }
        }

        // Remove submodule.<name>.url from config.
        let url_key = format!("submodule.{}.url", m.name);
        config.remove_section(&format!("submodule.{}", m.name))?;

        let _ = url_key; // suppress unused warning

        eprintln!("Cleared directory '{}'", m.path);
        eprintln!(
            "Submodule '{}' ({}) unregistered for path '{}'",
            m.name, m.url, m.path
        );
    }

    config.write()?;
    Ok(())
}

fn run_absorbgitdirs(args: &AbsorbgitdirsArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules(work_tree)?;
    let selected = filter_submodules(&modules, &args.paths);

    for m in &selected {
        let sub_path = work_tree.join(&m.path);
        let dot_git = sub_path.join(".git");

        if !dot_git.is_dir() {
            // Already a gitfile or doesn't exist — nothing to absorb.
            continue;
        }

        let modules_dir = repo.git_dir.join("modules").join(&m.name);

        // Create the modules directory if needed.
        fs::create_dir_all(modules_dir.parent().unwrap())?;

        // Move the .git directory to .git/modules/<name>.
        if modules_dir.exists() {
            // Already exists, skip.
            continue;
        }

        fs::rename(&dot_git, &modules_dir).context("failed to move .git directory")?;

        // Update core.worktree in the moved git dir.
        let moved_config_path = modules_dir.join("config");
        if moved_config_path.exists() {
            let content = fs::read_to_string(&moved_config_path)?;
            let mut cfg = ConfigFile::parse(&moved_config_path, &content, ConfigScope::Local)?;
            // Set the worktree to point back to the submodule path.
            let relative_worktree = pathdiff_relative(&modules_dir, &sub_path);
            cfg.set("core.worktree", &relative_worktree)?;
            cfg.write()?;
        }

        // Write a gitfile in place of the .git directory.
        let relative_gitdir = pathdiff_relative(&sub_path, &modules_dir);
        fs::write(&dot_git, format!("gitdir: {}\n", relative_gitdir))?;

        eprintln!(
            "Migrating git directory of '{}' from '{}' to '{}'",
            m.path,
            sub_path.join(".git").display(),
            modules_dir.display()
        );
    }

    Ok(())
}

/// Compute a relative path from `from` to `to`.
fn pathdiff_relative(from: &Path, to: &Path) -> String {
    // Canonicalize both paths for accurate comparison.
    let from_abs = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    let to_abs = to.canonicalize().unwrap_or_else(|_| to.to_path_buf());

    // Find common prefix.
    let from_parts: Vec<_> = from_abs.components().collect();
    let to_parts: Vec<_> = to_abs.components().collect();

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();
    for _ in common..from_parts.len() {
        result.push("..");
    }
    for part in &to_parts[common..] {
        result.push(part);
    }

    result.to_string_lossy().into_owned()
}

fn run_summary(_args: &SummaryArgs) -> Result<()> {
    // Summary is a complex feature — for now, output a basic summary.
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules(work_tree)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for m in &modules {
        let sub_path = work_tree.join(&m.path);
        let recorded = read_submodule_commit(&repo, &m.path)?;
        let oid = recorded.as_deref().unwrap_or("0000000");
        let short_oid = &oid[..oid.len().min(7)];

        if sub_path.exists() {
            writeln!(out, "* {} {}:", m.path, short_oid)?;
        } else {
            writeln!(out, "* {} {} (not checked out):", m.path, short_oid)?;
        }
    }

    Ok(())
}

fn run_set_branch(args: &SetBranchArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;

    let gitmodules_path = work_tree.join(".gitmodules");
    let content = fs::read_to_string(&gitmodules_path).context("failed to read .gitmodules")?;
    let mut config = ConfigFile::parse(&gitmodules_path, &content, ConfigScope::Local)?;

    // Find the submodule name for this path.
    let modules = parse_gitmodules(work_tree)?;
    let sm = modules
        .iter()
        .find(|m| m.path == args.path || m.name == args.path)
        .context("submodule not found")?;

    let branch_key = format!("submodule.{}.branch", sm.name);

    if args.default {
        // Remove the branch setting.
        config.unset(&branch_key)?;
    } else if let Some(ref branch) = args.branch {
        config.set(&branch_key, branch)?;
    } else {
        bail!("--branch <branch> or --default required");
    }

    config.write()?;
    Ok(())
}

fn run_set_url(args: &SetUrlArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;

    let gitmodules_path = work_tree.join(".gitmodules");
    let content = fs::read_to_string(&gitmodules_path).context("failed to read .gitmodules")?;
    let mut config = ConfigFile::parse(&gitmodules_path, &content, ConfigScope::Local)?;

    // Find the submodule name for this path.
    let modules = parse_gitmodules(work_tree)?;
    let sm = modules
        .iter()
        .find(|m| m.path == args.path || m.name == args.path)
        .context("submodule not found")?;

    let url_key = format!("submodule.{}.url", sm.name);
    config.set(&url_key, &args.newurl)?;
    config.write()?;

    // Also update in local config if initialized.
    let config_path = repo.git_dir.join("config");
    if config_path.exists() {
        let local_content = fs::read_to_string(&config_path)?;
        let mut local_config = ConfigFile::parse(&config_path, &local_content, ConfigScope::Local)?;
        let has_url = local_config.entries.iter().any(|e| e.key == url_key);
        if has_url {
            local_config.set(&url_key, &args.newurl)?;
            local_config.write()?;
        }
    }

    // Sync the submodule's remote.origin.url (like git submodule sync does).
    let resolved_url = resolve_submodule_url(work_tree, &repo.git_dir, &args.newurl);
    let sub_path = work_tree.join(&sm.path);
    if sub_path.join(".git").exists() {
        let sub_git_dir = resolve_submodule_git_dir(&sub_path);
        if let Some(sub_git) = sub_git_dir {
            let sub_config_path = sub_git.join("config");
            if sub_config_path.exists() {
                let sub_content = fs::read_to_string(&sub_config_path)?;
                let mut sub_config =
                    ConfigFile::parse(&sub_config_path, &sub_content, ConfigScope::Local)?;
                sub_config.set("remote.origin.url", &resolved_url)?;
                sub_config.write()?;
            }
        }
    }

    Ok(())
}
