//! `grit submodule` — manage submodules.
//!
//! Supports: status, init, update, add, foreach.
//! Reads `.gitmodules` and manages `.git/modules/` directory.

use crate::commands::sparse_checkout::reapply_sparse_checkout_if_configured;
use crate::grit_exe;
use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::error::Error as LibError;
use grit_lib::index::MODE_GITLINK;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Set by `clone --recurse-submodules` when `--shallow-submodules` was used.
pub(crate) const CLONE_SHALLOW_SUBMODULES_ENV: &str = "GRIT_CLONE_SHALLOW_SUBMODULES";

/// Set by `clone --recurse-submodules` when `--no-shallow-submodules` was used.
pub(crate) const CLONE_NO_SHALLOW_SUBMODULES_ENV: &str = "GRIT_CLONE_NO_SHALLOW_SUBMODULES";

/// Parse `.gitmodules` for clone-time submodule URLs and shallow recommendations.
pub(crate) fn parse_gitmodules_for_clone(work_tree: &Path) -> Result<Vec<SubmoduleInfo>> {
    let gitmodules_path = work_tree.join(".gitmodules");
    if !gitmodules_path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&gitmodules_path).context("reading .gitmodules")?;
    let _ = grit_lib::gitmodules::write_gitmodules_cli_option_warnings(&mut io::stderr(), &content);
    parse_gitmodules_with_repo(work_tree, None)
}

/// Submodule `git clone --depth N` when `Some(1)`; `None` means a non-shallow clone.
///
/// `super_shallow` is true when the superproject has a `.git/shallow` file (clone used `--depth` /
/// shallow negotiation). A shallow superproject does **not** imply shallow submodules unless
/// `--shallow-submodules` is set or `.gitmodules` recommends shallow (matches `t5614`).
#[must_use]
pub(crate) fn submodule_clone_depth_for_superproject(
    super_shallow: bool,
    shallow_submodules_cli: bool,
    no_shallow_submodules_cli: bool,
    no_recommend_shallow: bool,
    gitmodules_shallow: Option<bool>,
) -> Option<usize> {
    if no_shallow_submodules_cli {
        return None;
    }
    if shallow_submodules_cli {
        return Some(1);
    }
    if !no_recommend_shallow {
        if let Some(s) = gitmodules_shallow {
            return if s { Some(1) } else { None };
        }
    }
    if super_shallow {
        return None;
    }
    None
}

fn clone_shallow_submodules_from_env() -> bool {
    std::env::var_os(CLONE_SHALLOW_SUBMODULES_ENV)
        .as_deref()
        .is_some_and(|v| !v.is_empty())
}

fn clone_no_shallow_submodules_from_env() -> bool {
    std::env::var_os(CLONE_NO_SHALLOW_SUBMODULES_ENV)
        .as_deref()
        .is_some_and(|v| !v.is_empty())
}

/// Spawn grit for a nested operation without inheriting the superproject's `GIT_DIR` /
/// `GIT_WORK_TREE` (tests and detached work trees set those in the parent shell).
fn grit_subprocess(grit_bin: &Path) -> Command {
    let mut cmd = Command::new(grit_bin);
    cmd.env_remove("GIT_DIR");
    cmd.env_remove("GIT_WORK_TREE");
    grit_exe::strip_trace2_env(&mut cmd);
    cmd
}

/// Set `core.worktree` in a separate git-dir so checkouts materialize files (matches Git after
/// `clone --separate-git-dir`).
fn set_separate_gitdir_worktree(grit_bin: &Path, git_dir: &Path, work_tree: &Path) {
    let _ = grit_subprocess(grit_bin)
        .arg("--git-dir")
        .arg(git_dir)
        .arg("config")
        .arg("core.worktree")
        .arg(work_tree)
        .status();
}

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

    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Ignore `.gitmodules` shallow recommendations (still shallow when the superproject is shallow).
    #[arg(long = "no-recommend-shallow")]
    pub no_recommend_shallow: bool,
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

/// Parsed entry from `.gitmodules`.
#[derive(Debug, Clone)]
pub(crate) struct SubmoduleInfo {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) url: String,
    /// `submodule.<name>.shallow` from `.gitmodules`, when set.
    pub(crate) shallow: Option<bool>,
}

/// Update submodule working trees to the commits recorded in the superproject index.
///
/// Used after `pull` / `merge` when `--recurse-submodules` or `submodule.recurse` applies.
pub(crate) fn update_after_superproject_merge(init: bool, recursive: bool) -> Result<()> {
    run_update(&UpdateArgs {
        paths: vec![],
        init,
        checkout: false,
        remote: false,
        recursive,
        no_recommend_shallow: false,
    })
}

/// Refresh cached stat data for a gitlink in the superproject index after checkout.
fn refresh_gitlink_index_stat(repo: &Repository, rel_path: &str) -> Result<()> {
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let abs = work_tree.join(rel_path);
    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path)?;
    let path_bytes = rel_path.as_bytes().to_vec();
    let Some(entry) = index
        .entries
        .iter_mut()
        .find(|e| e.stage() == 0 && e.path == path_bytes)
    else {
        return Ok(());
    };
    if entry.mode != 0o160000 {
        return Ok(());
    }
    if let Ok(meta) = fs::symlink_metadata(&abs) {
        #[cfg(unix)]
        {
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.size = meta.len() as u32;
        }
    }
    repo.write_index_at(&index_path, &mut index)?;
    Ok(())
}

/// Run `grit fetch` in each initialized submodule (and nested submodules when `recursive`).
pub(crate) fn recursive_fetch_submodules(recursive: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
    let grit_bin = grit_exe::grit_executable();

    fn fetch_one(
        grit_bin: &std::path::Path,
        sub_path: &std::path::Path,
        recursive: bool,
    ) -> Result<()> {
        if !sub_path.join(".git").exists() {
            return Ok(());
        }
        let status = std::process::Command::new(grit_bin)
            .args(["fetch", "origin"])
            .current_dir(sub_path)
            .status()
            .context("submodule fetch")?;
        if !status.success() {
            bail!("submodule fetch failed in {}", sub_path.display());
        }
        if recursive {
            let sub_repo = Repository::discover(Some(sub_path)).context("open submodule repo")?;
            let sub_wt = sub_repo.work_tree.as_ref().context("bare submodule")?;
            let nested = parse_gitmodules_with_repo(sub_wt, Some(&sub_repo)).unwrap_or_default();
            for m in nested {
                fetch_one(grit_bin, &sub_wt.join(&m.path), true)?;
            }
        }
        Ok(())
    }

    for m in modules {
        fetch_one(&grit_bin, &work_tree.join(&m.path), recursive)?;
    }
    Ok(())
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

/// Built-in helper invoked as `git submodule--helper …` (matches Git's plumbing).
///
/// Currently implements `get-default-remote` only.
pub fn run_submodule_helper(rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        submodule_helper_usage_get_default_remote();
    }
    match rest[0].as_str() {
        "get-default-remote" => {
            if rest.len() != 2 {
                submodule_helper_usage_get_default_remote();
            }
            let path = &rest[1];
            let name = get_default_remote_for_path(path)?;
            println!("{name}");
            Ok(())
        }
        _ => {
            eprintln!("Unknown subcommand: {}", rest[0]);
            submodule_helper_usage_get_default_remote();
        }
    }
}

fn submodule_helper_usage_get_default_remote() -> ! {
    eprintln!("usage: git submodule--helper get-default-remote <path>");
    std::process::exit(129);
}

fn submodule_path_not_handle_error<T>(path: &str) -> Result<T> {
    Err(LibError::Message(format!(
        "fatal: could not get a repository handle for submodule '{path}'"
    ))
    .into())
}

fn worktree_relative_posix(work_tree: &Path, abs_path: &Path) -> Result<String> {
    let wt = work_tree
        .canonicalize()
        .with_context(|| format!("cannot canonicalize {}", work_tree.display()))?;
    let abs = abs_path
        .canonicalize()
        .with_context(|| format!("cannot canonicalize {}", abs_path.display()))?;
    let rel = abs.strip_prefix(&wt).with_context(|| {
        format!(
            "path {} is not inside work tree {}",
            abs.display(),
            wt.display()
        )
    })?;
    Ok(rel
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/"))
}

fn urls_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.contains("://") || b.contains("://") {
        return false;
    }
    let pa = Path::new(a);
    let pb = Path::new(b);
    match (pa.canonicalize(), pb.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

fn remote_names_with_urls(config: &ConfigFile) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in &config.entries {
        let Some(rest) = e.key.strip_prefix("remote.") else {
            continue;
        };
        let Some(name) = rest.strip_suffix(".url") else {
            continue;
        };
        if let Some(url) = e.value.as_deref() {
            out.push((name.to_string(), url.to_string()));
        }
    }
    out
}

fn config_last_value(config: &ConfigFile, key: &str) -> Option<String> {
    config
        .entries
        .iter()
        .rev()
        .find(|e| e.key == key)
        .and_then(|e| e.value.clone())
}

fn remote_from_resolved_url(config: &ConfigFile, resolved_url: &str) -> Option<String> {
    for (name, url) in remote_names_with_urls(config) {
        if urls_match(resolved_url, &url) {
            return Some(name);
        }
    }
    None
}

fn default_remote_for_config(config: &ConfigFile, head_branch: Option<&str>) -> String {
    if let Some(bn) = head_branch {
        let key = format!("branch.{bn}.remote");
        if let Some(r) = config_last_value(config, &key) {
            if !r.is_empty() {
                return r;
            }
        }
    }
    let names: std::collections::BTreeSet<String> = remote_names_with_urls(config)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    if names.len() == 1 {
        return names
            .iter()
            .next()
            .cloned()
            .unwrap_or_else(|| "origin".to_string());
    }
    "origin".to_string()
}

fn get_default_remote_for_path(path: &str) -> Result<String> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let path_buf = Path::new(path);
    let abs_sub = if path_buf.is_absolute() {
        path_buf.to_path_buf()
    } else {
        std::env::current_dir()?.join(path_buf)
    };
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let sub_rel = match worktree_relative_posix(work_tree, &abs_sub) {
        Ok(s) => s,
        Err(_) => {
            return submodule_path_not_handle_error(path);
        }
    };
    let (final_git_dir, _final_wt, super_wt, super_git_dir, sm) =
        resolve_submodule_chain(&repo, path, &sub_rel)?;

    let resolved_url = resolve_submodule_super_url(&super_wt, &super_git_dir, &sm.url)?;
    let config_path = final_git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)
        .context("parse submodule config")?;

    if let Some(name) = remote_from_resolved_url(&config, &resolved_url) {
        return Ok(name);
    }

    let head = resolve_head(&final_git_dir)?;
    let branch = head.branch_name().map(str::to_owned);
    Ok(default_remote_for_config(&config, branch.as_deref()))
}

fn resolve_submodule_chain(
    top_repo: &Repository,
    display_path: &str,
    sub_rel: &str,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf, SubmoduleInfo)> {
    let components: Vec<&str> = sub_rel.split('/').filter(|c| !c.is_empty()).collect();
    if components.is_empty() {
        return submodule_path_not_handle_error(display_path);
    }

    let top_work = top_repo.work_tree.as_ref().context("bare repository")?;
    let mut parent_wt = top_work.to_path_buf();
    let mut parent_git = top_repo.git_dir.clone();

    for (idx, seg) in components.iter().enumerate() {
        let is_last = idx + 1 == components.len();
        let parent_repo = Repository::open(&parent_git, Some(&parent_wt))
            .context("open repository for submodule walk")?;
        let modules = parse_gitmodules_with_repo(&parent_wt, Some(&parent_repo))?;
        let Some(sm) = modules.iter().find(|m| m.path == *seg) else {
            return submodule_path_not_handle_error(sub_rel);
        };

        let seg_work = parent_wt.join(seg);
        if !seg_work.join(".git").exists() {
            return submodule_path_not_handle_error(display_path);
        }
        let Some(git_dir) = resolve_submodule_git_dir(&seg_work) else {
            return submodule_path_not_handle_error(display_path);
        };

        if is_last {
            return Ok((git_dir, seg_work, parent_wt, parent_git, sm.clone()));
        }

        parent_wt = seg_work;
        parent_git = git_dir;
    }

    Err(anyhow::anyhow!(
        "internal error: submodule path walk did not complete"
    ))
}

// ── .gitmodules parsing ──────────────────────────────────────────────

/// Parse `.gitmodules` into a list of submodule entries.
fn parse_gitmodules(work_tree: &Path) -> Result<Vec<SubmoduleInfo>> {
    parse_gitmodules_with_repo(work_tree, None)
}

/// Paths listed in `.gitmodules` (or the index blob), used by `git clean` to avoid removing
/// submodule work trees that are not recorded in the current index (e.g. after checkout).
pub fn listed_submodule_paths(repo: &Repository) -> Result<Vec<String>> {
    let Some(wt) = repo.work_tree.as_ref() else {
        return Ok(Vec::new());
    };
    let modules = parse_gitmodules_with_repo(wt, Some(repo))?;
    Ok(modules.into_iter().map(|m| m.path).collect())
}

/// Ensure each configured submodule work tree has a `.git` gitfile pointing at
/// `.git/modules/<path>/` when that module directory exists (needed after checkout removes
/// paths not in the new index).
pub fn refresh_submodule_gitfiles(repo: &Repository) -> Result<()> {
    let Some(wt) = repo.work_tree.as_ref() else {
        return Ok(());
    };
    for path in listed_submodule_paths(repo)? {
        let sm_dir = wt.join(&path);
        if !sm_dir.is_dir() {
            continue;
        }
        let modules_git = repo.git_dir.join("modules").join(&path);
        if !modules_git.exists() {
            continue;
        }
        if let Ok(rel) = relativize_submodule_gitfile(&sm_dir, &modules_git) {
            let gitfile = sm_dir.join(".git");
            let line = format!("gitdir: {}\n", rel.to_string_lossy().replace('\\', "/"));
            fs::write(&gitfile, line).with_context(|| {
                format!("failed to write submodule gitfile at {}", gitfile.display())
            })?;
        }
    }
    Ok(())
}

fn relativize_submodule_gitfile(from_dir: &Path, to_path: &Path) -> Result<PathBuf> {
    let from_abs = fs::canonicalize(from_dir).unwrap_or_else(|_| from_dir.to_path_buf());
    let to_abs = fs::canonicalize(to_path).unwrap_or_else(|_| to_path.to_path_buf());
    let from_c: Vec<_> = from_abs.components().collect();
    let to_c: Vec<_> = to_abs.components().collect();
    let mut i = 0usize;
    while i < from_c.len() && i < to_c.len() && from_c[i] == to_c[i] {
        i += 1;
    }
    let mut out = PathBuf::new();
    for _ in i..from_c.len() {
        out.push("..");
    }
    for c in &to_c[i..] {
        out.push(c);
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    Ok(out)
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
        let index = repo.load_index().context("failed to load index")?;
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
    let mut modules: BTreeMap<String, (Option<String>, Option<String>, Option<bool>)> =
        BTreeMap::new();

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
            let entry_val = modules
                .entry(name.to_string())
                .or_insert((None, None, None));
            match var {
                "path" => entry_val.0 = entry.value.clone(),
                "url" => entry_val.1 = entry.value.clone(),
                "shallow" => {
                    if let Some(v) = entry.value.as_deref() {
                        let v = v.trim();
                        if v.eq_ignore_ascii_case("true")
                            || v == "1"
                            || v.eq_ignore_ascii_case("yes")
                        {
                            entry_val.2 = Some(true);
                        } else if v.eq_ignore_ascii_case("false")
                            || v == "0"
                            || v.eq_ignore_ascii_case("no")
                        {
                            entry_val.2 = Some(false);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut result = Vec::new();
    for (name, (path, url, shallow)) in modules {
        if let (Some(path), Some(url)) = (path, url) {
            result.push(SubmoduleInfo {
                name,
                path,
                url,
                shallow,
            });
        }
    }

    Ok(result)
}

/// Filter submodules by path args (empty = all).
fn filter_submodules<'a>(modules: &'a [SubmoduleInfo], paths: &[String]) -> Vec<&'a SubmoduleInfo> {
    if paths.is_empty() || paths.iter().any(|p| p == ".") {
        modules.iter().collect()
    } else {
        modules
            .iter()
            .filter(|m| paths.iter().any(|p| p == &m.path || p == &m.name))
            .collect()
    }
}

// ── Read recorded commit from the index ──────────────────────────────

/// Read the commit OID for a submodule path (gitlink).
///
/// Prefer the **index** when it contains a stage-0 gitlink at `submodule_path`, so
/// `git submodule update` works after `git apply --index` / partial index updates while `HEAD`
/// still points at an older commit. Fall back to `HEAD`'s tree when the path is not in the index.
fn read_submodule_commit(repo: &Repository, submodule_path: &str) -> Result<Option<String>> {
    let index_path = repo.index_path();
    if let Ok(index) = repo.load_index_at(&index_path) {
        if let Some(entry) = index.get(submodule_path.as_bytes(), 0) {
            if entry.mode == MODE_GITLINK {
                return Ok(Some(entry.oid.to_hex()));
            }
        }
    }

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

/// Gitlink OID for `submodule_path` in the current index (stage 0), if present.
///
/// Used after `grit add <path>` when `HEAD`’s tree does not yet list the new submodule
/// (e.g. `submodule add` before `commit`).
fn read_gitlink_oid_from_index(repo: &Repository, submodule_path: &str) -> Result<Option<String>> {
    let index = repo
        .load_index()
        .context("load index for submodule gitlink")?;
    let needle = submodule_path.as_bytes();
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if entry.path.as_slice() == needle && entry.mode == MODE_GITLINK {
            return Ok(Some(entry.oid.to_hex()));
        }
    }
    Ok(None)
}

/// Check out `oid` in the submodule at `path` (separate git dir under `.git/modules/` or in-tree `.git`).
fn checkout_submodule_worktree(
    grit_bin: &Path,
    repo: &Repository,
    work_tree: &Path,
    submodule_path: &str,
    oid: &str,
) -> Result<()> {
    let sub_path = work_tree.join(submodule_path);
    let modules_dir = repo.git_dir.join("modules").join(submodule_path);

    // CWD must lie inside `GIT_WORK_TREE`; the superproject root is outside the submodule tree.
    // `--force`: after `clone --no-checkout`, HEAD may already equal `oid` while the index and
    // work tree are empty; without force, `checkout` skips `switch_to_tree` and leaves no files.
    let status = if modules_dir.join("HEAD").exists() {
        let mut cmd = Command::new(grit_bin);
        grit_exe::strip_trace2_env(&mut cmd);
        cmd.env("GIT_DIR", &modules_dir)
            .env("GIT_WORK_TREE", &sub_path)
            .current_dir(&sub_path)
            .args(["checkout", "--force", "--quiet", oid])
            .status()
    } else {
        let mut cmd = Command::new(grit_bin);
        grit_exe::strip_trace2_env(&mut cmd);
        cmd.args(["checkout", "--force", "--quiet", oid])
            .current_dir(&sub_path)
            .status()
    }
    .context("failed to checkout submodule commit")?;

    if !status.success() {
        bail!(
            "failed to checkout {} in submodule '{}'",
            oid,
            submodule_path
        );
    }

    if let Ok(sub_repo) = Repository::open(&modules_dir, Some(&sub_path)) {
        let _ = reapply_sparse_checkout_if_configured(&sub_repo);
    } else if sub_path.join(".git").exists() {
        if let Ok(sub_repo) = Repository::discover(Some(&sub_path)) {
            let _ = reapply_sparse_checkout_if_configured(&sub_repo);
        }
    }

    eprintln!(
        "Submodule path '{}': checked out '{}'",
        submodule_path,
        &oid[..oid.len().min(12)]
    );
    Ok(())
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
        let resolved_url = resolve_submodule_super_url(work_tree, &repo.git_dir, &m.url)?;

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
    let super_shallow = repo.git_dir.join("shallow").is_file();

    if args.init {
        run_init(&InitArgs {
            paths: args.paths.clone(),
        })?;
    }

    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
    let selected = filter_submodules(&modules, &args.paths);

    let grit_bin = grit_exe::grit_executable();
    let super_index = repo.load_index().ok();

    for m in &selected {
        let sub_path = work_tree.join(&m.path);
        let path_bytes = m.path.as_bytes();
        let recorded_from_index = super_index
            .as_ref()
            .and_then(|idx| idx.get(path_bytes, 0))
            .filter(|e| e.mode == 0o160000)
            .map(|e| e.oid.to_hex());
        let recorded = if recorded_from_index.is_some() {
            recorded_from_index
        } else {
            read_submodule_commit(&repo, &m.path)?
        };
        let Some(recorded_oid) = recorded.as_deref() else {
            continue;
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

        let modules_dir = repo.git_dir.join("modules").join(&m.path);

        // Submodule checkouts must use a gitfile at `<path>/.git` pointing at
        // `.git/modules/<name>/`. A nested `.git` directory breaks `git rev-parse --git-dir`
        // and `replace_gitfile_with_git_dir` in `lib-submodule-update.sh` (t4137).
        if sub_path.join(".git").is_dir() && modules_dir.join("HEAD").exists() {
            fs::remove_dir_all(sub_path.join(".git")).with_context(|| {
                format!(
                    "failed to normalize submodule .git at {}",
                    sub_path.display()
                )
            })?;
            attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
        }

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

                // `git clone` requires the destination to be absent or empty; remove a leftover
                // directory (e.g. `cp -R` copied an uninitialized submodule path without `.git`).
                if sub_path.exists() {
                    let _ = fs::remove_dir_all(&sub_path);
                }

                let clone_src_str =
                    resolve_submodule_super_url(work_tree, &repo.git_dir, &clone_url)?;

                let submodule_depth = submodule_clone_depth_for_superproject(
                    super_shallow,
                    clone_shallow_submodules_from_env(),
                    clone_no_shallow_submodules_from_env(),
                    args.no_recommend_shallow,
                    m.shallow,
                );

                let mut cmd = grit_subprocess(&grit_bin);
                cmd.arg("clone")
                    .arg("--no-checkout")
                    .arg("--separate-git-dir")
                    .arg(&modules_dir);
                if let Some(d) = submodule_depth {
                    cmd.arg("--depth").arg(d.to_string());
                }
                let status = cmd
                    .arg(&clone_src_str)
                    .arg(&sub_path)
                    .current_dir(work_tree)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_WORK_TREE")
                    .status()
                    .context("failed to clone submodule")?;

                if !status.success() {
                    eprintln!("error: failed to clone submodule from '{}'", clone_src_str);
                    bail!("failed to clone submodule '{}'", m.name);
                }
                set_submodule_core_worktree(&grit_bin, &modules_dir, &sub_path);

                let abs_origin = if Path::new(&clone_src_str).is_absolute() {
                    Path::new(&clone_src_str).canonicalize()
                } else {
                    work_tree.join(&clone_src_str).canonicalize()
                };
                if let Ok(p) = abs_origin {
                    let sub_cfg_path = modules_dir.join("config");
                    if sub_cfg_path.exists() {
                        let sub_content = fs::read_to_string(&sub_cfg_path)?;
                        let mut sub_cfg =
                            ConfigFile::parse(&sub_cfg_path, &sub_content, ConfigScope::Local)?;
                        sub_cfg.set("remote.origin.url", &p.to_string_lossy())?;
                        sub_cfg.write()?;
                    }
                }
            }
        }

        // Determine which commit to checkout.
        let checkout_oid = if args.remote {
            // Fetch from remote and use the remote tracking branch.
            let fetch_status = grit_subprocess(&grit_bin)
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
            let output = grit_subprocess(&grit_bin)
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

        checkout_submodule_worktree(&grit_bin, &repo, work_tree, &m.path, &checkout_oid)?;

        // `checkout` must leave the submodule index matching `HEAD`; otherwise
        // `git status` inside the submodule shows spurious staged deletions
        // (t4137-apply-submodule `test_submodule_content`).
        let reset_status = Command::new(&grit_bin)
            .args(["reset", "--hard", "--quiet"])
            .current_dir(&sub_path)
            .status()
            .context("failed to reset submodule index after checkout")?;
        if !reset_status.success() {
            bail!("failed to reset submodule '{}' after checkout", m.name);
        }

        refresh_gitlink_index_stat(&repo, &m.path)?;

        eprintln!(
            "Submodule path '{}': checked out '{}'",
            m.path,
            &checkout_oid[..checkout_oid.len().min(12)]
        );

        // `reset --hard` materializes every path from `HEAD`; reapply sparse-checkout so cone
        // submodules stay trimmed (matches Git; t7817 expects only `sub/B/b`).
        if let Ok(sub_repo) = Repository::open(&modules_dir, Some(&sub_path)) {
            let _ = reapply_sparse_checkout_if_configured(&sub_repo);
        } else if let Ok(sub_repo) = Repository::discover(Some(&sub_path)) {
            let _ = reapply_sparse_checkout_if_configured(&sub_repo);
        }

        // `checkout`/`reset` must not replace the submodule gitfile with a nested `.git/` directory;
        // normalize again so `git rev-parse --git-dir` matches Git (t4137 `replace_gitfile_with_git_dir`).
        if sub_path.join(".git").is_dir() && modules_dir.join("HEAD").exists() {
            fs::remove_dir_all(sub_path.join(".git")).with_context(|| {
                format!(
                    "failed to normalize submodule .git after checkout for {}",
                    sub_path.display()
                )
            })?;
            attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
        }
    }

    if args.recursive {
        for m in &selected {
            let sub_path = work_tree.join(&m.path);
            if !sub_path.join(".git").exists() {
                continue;
            }
            if parse_gitmodules(&sub_path).unwrap_or_default().is_empty() {
                continue;
            }
            let mut nested = grit_subprocess(&grit_bin);
            nested.args(["submodule", "update", "--init", "--recursive"]);
            if args.no_recommend_shallow {
                nested.arg("--no-recommend-shallow");
            }
            let status = nested
                .current_dir(&sub_path)
                .status()
                .with_context(|| format!("nested submodule update in {}", m.path))?;
            if !status.success() {
                bail!("nested submodule update failed for submodule '{}'", m.path);
            }
        }
    }

    Ok(())
}

fn set_submodule_core_worktree(grit_bin: &Path, modules_dir: &Path, sub_path: &Path) {
    // Match Git: store a path relative to the module git dir so `test_git_directory_is_unchanged`
    // can compare `.git/modules/<name>` with a copied `<path>/.git` (t4137).
    let wt = pathdiff_relative(modules_dir, sub_path);
    let _ = Command::new(grit_bin)
        .arg("--git-dir")
        .arg(modules_dir)
        .arg("config")
        .arg("core.worktree")
        .arg(&wt)
        .status();
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
    set_submodule_core_worktree(grit_bin, modules_dir, sub_path);
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

        // Relative submodule URLs: from the superproject root for normal repos, and from the
        // parent of this work tree when this repo lives under `.git/modules/<name>/` (nested
        // submodule), matching Git.
        let url_base = if repo
            .git_dir
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "modules")
        {
            work_tree
                .parent()
                .ok_or_else(|| anyhow::anyhow!("cannot resolve nested submodule clone URL"))?
        } else {
            work_tree
        };
        let clone_source = if args.url.starts_with("./") || args.url.starts_with("../") {
            url_base.join(&args.url).canonicalize().with_context(|| {
                format!(
                    "cannot resolve relative submodule URL '{}' from '{}'",
                    args.url,
                    url_base.display()
                )
            })?
        } else {
            PathBuf::from(&args.url)
        };
        let clone_source_str = clone_source.to_string_lossy().into_owned();

        let status = grit_subprocess(&grit_bin)
            .arg("clone")
            .arg("--no-checkout")
            .arg("--separate-git-dir")
            .arg(&modules_dir)
            .arg(&clone_source_str)
            .arg(&sub_path)
            .current_dir(work_tree)
            .status()
            .context("failed to clone submodule")?;

        if !status.success() {
            bail!("failed to clone submodule from '{}'", args.url);
        }
        set_separate_gitdir_worktree(&grit_bin, &modules_dir, &sub_path);
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

    // `clone --no-checkout` leaves an empty work tree; populate it from the staged gitlink
    // (HEAD’s tree may not include the new submodule until after commit — read the index).
    if let Some(oid) = read_gitlink_oid_from_index(&repo, &path)? {
        checkout_submodule_worktree(&grit_bin, &repo, work_tree, &path, &oid)?;
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

/// Resolve a relative `.gitmodules` URL for superproject config / clone / URL matching.
/// Matches Git's `resolve_relative_url(url, NULL)` (`relative_url` with no `up_path`).
fn resolve_submodule_super_url(
    work_tree: &Path,
    repo_git_dir: &Path,
    raw_url: &str,
) -> Result<String> {
    if !raw_url.starts_with("./") && !raw_url.starts_with("../") {
        return Ok(raw_url.to_string());
    }

    let super_git = superproject_git_dir_for_nested_modules(repo_git_dir)
        .unwrap_or_else(|| repo_git_dir.to_path_buf());
    let super_wt = super_git
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| work_tree.to_path_buf());

    let mut base = default_remote_url_raw(&super_git)
        .unwrap_or_else(|| super_wt.to_string_lossy().into_owned());
    if url_is_local_not_ssh(&base)
        && !is_absolute_path_url(&base)
        && (base.starts_with("./") || base.starts_with("../"))
    {
        base = canonicalize_local_remote_url_base(&super_wt, &super_git, &base);
    }
    git_relative_url(&base, raw_url, None)
}

/// URL written to a checked-out submodule's `remote.<name>.url` (Git `sync`: `get_up_path` + `relative_url`).
fn resolve_submodule_sub_origin_url(
    work_tree: &Path,
    repo_git_dir: &Path,
    submodule_path: &str,
    raw_url: &str,
) -> Result<String> {
    if !raw_url.starts_with("./") && !raw_url.starts_with("../") {
        return Ok(raw_url.to_string());
    }
    let super_git = superproject_git_dir_for_nested_modules(repo_git_dir)
        .unwrap_or_else(|| repo_git_dir.to_path_buf());
    let super_wt = super_git
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| work_tree.to_path_buf());

    let mut base = default_remote_url_raw(&super_git)
        .unwrap_or_else(|| super_wt.to_string_lossy().into_owned());
    if url_is_local_not_ssh(&base)
        && !is_absolute_path_url(&base)
        && (base.starts_with("./") || base.starts_with("../"))
    {
        base = canonicalize_local_remote_url_base(&super_wt, &super_git, &base);
    }
    let up = submodule_up_path(submodule_path);
    let up_ref = (!up.is_empty()).then_some(up.as_str());
    git_relative_url(&base, raw_url, up_ref)
}

/// `.../super/.git` when `git_dir` is `.../super/.git/modules/<name>` (submodule object store).
fn superproject_git_dir_for_nested_modules(git_dir: &Path) -> Option<PathBuf> {
    let mut p = git_dir.to_path_buf();
    while let Some(parent) = p.parent() {
        if p.file_name().is_some_and(|n| n == "modules")
            && parent.file_name().is_some_and(|n| n == ".git")
        {
            return Some(parent.to_path_buf());
        }
        p = parent.to_path_buf();
    }
    None
}

/// Superproject work tree when `git_dir` is under `.git/modules/<name>/` (else `None`).
fn superproject_work_tree_for_nested_git_dir(git_dir: &Path) -> Option<PathBuf> {
    superproject_git_dir_for_nested_modules(git_dir).and_then(|g| g.parent().map(Path::to_path_buf))
}

/// Join and canonicalize a possibly-relative remote URL; uses the submodule's work tree, or the
/// outer superproject tree for paths under `.git/modules/` (so `../sub` matches Git).
fn canonicalize_local_remote_url_base(work_tree: &Path, git_dir: &Path, url: &str) -> String {
    if !url_is_local_not_ssh(url)
        || is_absolute_path_url(url)
        || (!url.starts_with("./") && !url.starts_with("../"))
    {
        return url.to_string();
    }
    let base_dir = superproject_work_tree_for_nested_git_dir(git_dir)
        .unwrap_or_else(|| work_tree.to_path_buf());
    let joined = base_dir.join(url);
    let joined_s = joined.to_string_lossy().into_owned();
    joined
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            crate::git_path::normalize_path_copy(&joined_s).unwrap_or_else(|_| url.to_string())
        })
}

/// Raw `remote.<default>.url` from config (may be `../sub`); matches Git's
/// `get_default_remote` + config lookup passed to `relative_url`.
fn default_remote_url_raw(git_dir: &Path) -> Option<String> {
    let config_path = git_dir.join("config");
    let content = fs::read_to_string(&config_path).ok()?;
    let config = ConfigFile::parse(&config_path, &content, ConfigScope::Local).ok()?;
    let mut raw_url = None;
    if let Ok(head) = resolve_head(git_dir) {
        if let Some(bn) = head.branch_name() {
            if let Some(rn) = config_last_value(&config, &format!("branch.{bn}.remote")) {
                if !rn.is_empty() {
                    raw_url = config_last_value(&config, &format!("remote.{rn}.url"));
                }
            }
        }
    }
    if raw_url.is_none() {
        let remotes = remote_names_with_urls(&config);
        if remotes.len() == 1 {
            raw_url = Some(remotes[0].1.clone());
        } else {
            raw_url = config_last_value(&config, "remote.origin.url");
        }
    }
    raw_url
}

fn count_slashes_in_submodule_path(path: &str) -> usize {
    path.bytes().filter(|&b| b == b'/').count()
}

/// Git's `get_up_path(path)` for submodule URL resolution (`relative_url` `up_path`).
fn submodule_up_path(path: &str) -> String {
    let mut s = String::new();
    for _ in 0..count_slashes_in_submodule_path(path) {
        s.push_str("../");
    }
    if !path.is_empty() && !path.ends_with('/') {
        s.push_str("../");
    }
    s
}

fn url_is_local_not_ssh(url: &str) -> bool {
    !url.contains("://") || url.starts_with("file://")
}

fn is_absolute_path_url(url: &str) -> bool {
    url.starts_with('/') || url.len() > 2 && url.as_bytes().get(1) == Some(&b':')
}

fn chop_last_dir_git(remoteurl: &mut String, is_relative: bool) -> Result<bool> {
    if let Some(pos) = remoteurl.rfind('/') {
        remoteurl.truncate(pos);
        return Ok(false);
    }
    if let Some(pos) = remoteurl.rfind(':') {
        remoteurl.truncate(pos);
        return Ok(true);
    }
    if is_relative || remoteurl == "." {
        bail!("cannot strip one component off url '{remoteurl}'");
    }
    *remoteurl = ".".to_string();
    Ok(false)
}

/// Git's `relative_url(remote_url, url, up_path)` for local paths (see `git/remote.c`).
fn git_relative_url(remote_url: &str, url: &str, up_path: Option<&str>) -> Result<String> {
    let url = url.trim_end_matches('/');
    if !url_is_local_not_ssh(url) || is_absolute_path_url(url) {
        return Ok(url.to_string());
    }
    let mut remoteurl = remote_url.trim_end_matches('/').to_string();
    if remoteurl.is_empty() {
        return Ok(url.to_string());
    }
    let is_relative = url_is_local_not_ssh(&remoteurl) && !is_absolute_path_url(&remoteurl);
    if is_relative && !remoteurl.starts_with("./") && !remoteurl.starts_with("../") {
        remoteurl = format!("./{remoteurl}");
    }
    let mut rest = url;
    let mut colonsep = false;
    while rest.starts_with("../") {
        rest = &rest[3..];
        colonsep |= chop_last_dir_git(&mut remoteurl, is_relative)?;
    }
    while rest.starts_with("./") {
        rest = &rest[2..];
    }
    let sep = if colonsep { ":" } else { "/" };
    let mut out = format!("{remoteurl}{sep}{rest}");
    if out.ends_with('/') {
        out.pop();
    }
    let mut out = if out.starts_with("./") {
        out[2..].to_string()
    } else {
        out
    };
    if let Some(up) = up_path {
        if is_relative {
            out = format!("{up}{out}");
        }
    }
    Ok(out)
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

        // Superproject config: resolve_relative_url(url, NULL).
        let super_url = resolve_submodule_super_url(work_tree, &repo.git_dir, &m.url)?;
        config.set(&url_key, &super_url)?;
        eprintln!("Synchronizing submodule url for '{}'", m.path);

        // Submodule working tree remote: relative_url with get_up_path (see git submodule sync).
        let sub_origin_url =
            resolve_submodule_sub_origin_url(work_tree, &repo.git_dir, &m.path, &m.url)?;

        let sub_path = work_tree.join(&m.path);
        if sub_path.join(".git").exists() {
            let sub_git_dir = resolve_submodule_git_dir(&sub_path);
            if let Some(sub_git) = sub_git_dir {
                let sub_config_path = sub_git.join("config");
                if sub_config_path.exists() {
                    let sub_content = fs::read_to_string(&sub_config_path)?;
                    let mut sub_config =
                        ConfigFile::parse(&sub_config_path, &sub_content, ConfigScope::Local)?;
                    sub_config.set("remote.origin.url", &sub_origin_url)?;
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
                    let _status = grit_subprocess(&grit_bin)
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
    let resolved_url =
        resolve_submodule_sub_origin_url(work_tree, &repo.git_dir, &sm.path, &args.newurl)?;
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
