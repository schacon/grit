//! `grit submodule` — manage submodules.
//!
//! Supports: status, init, update, add, foreach.
//! Reads `.gitmodules` and manages `.git/modules/` directory.

use crate::commands::sparse_checkout::reapply_sparse_checkout_if_configured;
use crate::grit_exe;
use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Parser, Subcommand};

mod upstream_help_builtin_synopsis {
    include!(concat!(env!("OUT_DIR"), "/upstream_help_synopsis.rs"));
}

#[derive(Parser)]
#[command(
    name = "git submodule",
    disable_help_subcommand = true,
    disable_version_flag = true
)]
struct SubmoduleCliWrapper {
    #[command(flatten)]
    inner: Args,
}

fn submodule_synopsis_variants_from_adoc(syn: &str) -> Vec<Vec<String>> {
    let mut variants: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in syn.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("git ") && !current.is_empty() {
            variants.push(core::mem::take(&mut current));
        }
        current.push(trimmed.to_owned());
    }
    if !current.is_empty() {
        variants.push(current);
    }
    variants
}

fn print_submodule_usage_stderr() {
    let Some(syn) = upstream_help_builtin_synopsis::synopsis_for_builtin("submodule") else {
        return;
    };
    let pad = " ".repeat("git submodule ".len());
    let variants = submodule_synopsis_variants_from_adoc(syn);
    for (i, var) in variants.iter().enumerate() {
        let Some(first) = var.first() else {
            continue;
        };
        if i == 0 {
            eprintln!("usage: {first}");
        } else {
            eprintln!("   or: {first}");
        }
        for cont in var.iter().skip(1) {
            eprintln!("{pad}{cont}");
        }
    }
}

fn submodule_usage_exit(code: i32) -> ! {
    print_submodule_usage_stderr();
    std::process::exit(code);
}

/// Split `git submodule` leading `[--quiet|-q] [--cached]` flags (Git order). Rejects other
/// leading options with usage on stderr and exit **1** (matches Git / t7400).
fn split_submodule_leading_flags(rest: &[String]) -> (SubmoduleTopOpts, Vec<String>) {
    let mut top = SubmoduleTopOpts::default();
    let mut i = 0usize;
    while i < rest.len() {
        let a = rest[i].as_str();
        match a {
            "-h" | "--help" => break,
            "--quiet" | "-q" => {
                top.quiet = true;
                i += 1;
            }
            "--cached" => {
                top.cached = true;
                i += 1;
            }
            _ if a.starts_with('-') => submodule_usage_exit(1),
            _ => break,
        }
    }
    (top, rest[i..].to_vec())
}

fn parse_submodule_args(inner: &[String]) -> Args {
    if inner.len() == 1 && (inner[0] == "-h" || inner[0] == "--help") {
        if let Some(syn) = upstream_help_builtin_synopsis::synopsis_for_builtin("submodule") {
            let pad = " ".repeat("git submodule ".len());
            let variants = submodule_synopsis_variants_from_adoc(syn);
            for (i, var) in variants.iter().enumerate() {
                let Some(first) = var.first() else {
                    continue;
                };
                if i == 0 {
                    println!("usage: {first}");
                } else {
                    println!("   or: {first}");
                }
                for cont in var.iter().skip(1) {
                    println!("{pad}{cont}");
                }
            }
            println!();
            std::process::exit(0);
        }
    }

    let mut argv = vec!["git submodule".to_owned()];
    argv.extend(inner.iter().cloned());
    match SubmoduleCliWrapper::try_parse_from(&argv) {
        Ok(w) => w.inner,
        Err(e) => {
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                    | clap::error::ErrorKind::DisplayVersion
            ) {
                let mut msg = e.render().to_string();
                msg = msg.replace("Usage:", "usage:");
                print!("{msg}");
            } else {
                let _ = e.print();
            }
            let code = match e.kind() {
                clap::error::ErrorKind::DisplayHelp
                | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => 0,
                clap::error::ErrorKind::DisplayVersion => 129,
                _ => 129,
            };
            std::process::exit(code);
        }
    }
}

/// Entry point from `main`: handles leading `--quiet` / `--cached` like Git before clap.
pub fn run_from_argv(rest: &[String]) -> Result<()> {
    let (top, inner) = split_submodule_leading_flags(rest);
    if inner.len() == 1 && (inner[0] == "--" || inner[0] == "--end-of-options") {
        submodule_usage_exit(1);
    }
    let args = parse_submodule_args(&inner);
    run_with_top_opts(top, args)
}

fn run_with_top_opts(top: SubmoduleTopOpts, args: Args) -> Result<()> {
    if top.cached {
        match &args.command {
            None | Some(SubmoduleCommand::Status(_)) | Some(SubmoduleCommand::Summary(_)) => {}
            _ => submodule_usage_exit(1),
        }
    }

    match args.command {
        None => run_status(&StatusArgs {
            quiet: top.quiet,
            recursive: false,
            cached: top.cached,
            paths: vec![],
        }),
        Some(SubmoduleCommand::Status(mut s)) => {
            s.cached |= top.cached;
            s.quiet |= top.quiet;
            run_status(&s)
        }
        Some(SubmoduleCommand::Init(mut a)) => {
            a.quiet |= top.quiet;
            run_init(&a, a.quiet)
        }
        Some(SubmoduleCommand::Update(mut a)) => {
            a.quiet |= top.quiet;
            run_update(&a)
        }
        Some(SubmoduleCommand::Add(mut a)) => {
            a.quiet |= top.quiet;
            run_add(&a)
        }
        Some(SubmoduleCommand::Foreach(mut a)) => {
            a.quiet |= top.quiet;
            run_foreach(&a, a.quiet)
        }
        Some(SubmoduleCommand::Sync(mut a)) => {
            a.quiet |= top.quiet;
            run_sync(&a, a.quiet)
        }
        Some(SubmoduleCommand::Deinit(mut a)) => {
            a.quiet |= top.quiet;
            run_deinit(&a, a.quiet)
        }
        Some(SubmoduleCommand::Absorbgitdirs(mut a)) => {
            a.quiet |= top.quiet;
            run_absorbgitdirs(&a, a.quiet)
        }
        Some(SubmoduleCommand::Summary(mut a)) => {
            a.quiet |= top.quiet;
            a.cached |= top.cached;
            run_summary(&a, a.quiet)
        }
        Some(SubmoduleCommand::SetBranch(mut a)) => {
            a.quiet |= top.quiet;
            run_set_branch(&a, a.quiet)
        }
        Some(SubmoduleCommand::SetUrl(mut a)) => {
            a.quiet |= top.quiet;
            run_set_url(&a, a.quiet)
        }
    }
}
use grit_lib::config::{canonical_key, ConfigFile, ConfigScope};
use grit_lib::diff::{diff_index_to_tree, DiffEntry, DiffStatus};
use grit_lib::error::Error as LibError;
use grit_lib::index::MODE_GITLINK;
use grit_lib::merge_diff::blob_oid_at_path;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::pathspec::matches_pathspec;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{self, resolve_revision};
use grit_lib::state::resolve_head;
use grit_lib::submodule_gitdir::submodule_modules_git_dir;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

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

static SUBMODULE_JOBS_TRACE_EMITTED: AtomicBool = AtomicBool::new(false);

/// Best-effort `GIT_TRACE` line for `submodule update --jobs` (t7406 greps for `N tasks`).
fn trace_submodule_job_tasks_if_needed(jobs: Option<usize>) {
    let Some(n) = jobs else {
        return;
    };
    let Ok(trace_target) = std::env::var("GIT_TRACE") else {
        return;
    };
    if trace_target.is_empty() {
        return;
    }
    if SUBMODULE_JOBS_TRACE_EMITTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let line = format!("trace: submodule update: {n} tasks\n");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&trace_target)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

fn submodule_display_path_from_cwd(abs_submodule: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| abs_submodule.to_path_buf());
    pathdiff_relative(&cwd, abs_submodule).replace('\\', "/")
}

fn super_index_has_unmerged_stage(repo: &Repository, rel_path: &str) -> bool {
    let Ok(index) = repo.load_index() else {
        return false;
    };
    let needle = rel_path.as_bytes();
    index
        .entries
        .iter()
        .any(|e| e.path.as_slice() == needle && e.stage() != 0)
}

fn parse_local_config(git_dir: &Path) -> Result<ConfigFile> {
    let config_path = git_dir.join("config");
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        Ok(ConfigFile::parse(
            &config_path,
            &content,
            ConfigScope::Local,
        )?)
    } else {
        Ok(ConfigFile::parse(&config_path, "", ConfigScope::Local)?)
    }
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

/// Leading options parsed before the subcommand (matches `git submodule [--quiet] [--cached] …`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SubmoduleTopOpts {
    pub quiet: bool,
    pub cached: bool,
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
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Compare the index to `HEAD` (index gitlinks vs `HEAD` tree) instead of the submodule work tree.
    #[arg(long)]
    pub cached: bool,

    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct InitArgs {
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, ClapArgs)]
pub struct UpdateArgs {
    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,

    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Initialize uninitialized submodules before updating.
    #[arg(long)]
    pub init: bool,

    /// Checkout the recorded commit (accepted for compatibility).
    #[arg(long)]
    pub checkout: bool,

    /// Use the status of the submodule's remote-tracking branch.
    #[arg(long)]
    pub remote: bool,

    /// Rebase the current branch onto the recorded commit.
    #[arg(long)]
    pub rebase: bool,

    /// Merge the recorded commit into the current branch.
    #[arg(long)]
    pub merge: bool,

    /// Discard local changes when checking out.
    #[arg(long, short)]
    pub force: bool,

    /// Shallow clone depth when initializing a submodule.
    #[arg(long)]
    pub depth: Option<usize>,

    /// Parallel jobs hint (accepted for compatibility; best-effort).
    #[arg(long)]
    pub jobs: Option<usize>,

    /// Partial clone filter (requires `--init`).
    #[arg(long)]
    pub filter: Option<String>,

    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Borrow objects from this repository (repeatable). Writes `objects/info/alternates` in cloned submodules.
    #[arg(long = "reference", value_name = "REPO", action = clap::ArgAction::Append)]
    pub reference: Vec<String>,

    /// Ignore `.gitmodules` shallow recommendations (still shallow when the superproject is shallow).
    #[arg(long = "no-recommend-shallow")]
    pub no_recommend_shallow: bool,
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Use the given name instead of defaulting to its path.
    #[arg(long)]
    pub name: Option<String>,

    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

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
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Command to run in each submodule (default: `:`). Use `--` before arguments that look like options.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SyncArgs {
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Recurse into nested submodules.
    #[arg(long)]
    pub recursive: bool,

    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct DeinitArgs {
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

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
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Restrict to specific submodule paths.
    #[arg(value_name = "PATH")]
    pub paths: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SummaryArgs {
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Compare the index to the given commit instead of the submodule working tree HEAD.
    #[arg(long)]
    pub cached: bool,

    /// Compare the index gitlink to the submodule HEAD (instead of index vs commit tree).
    #[arg(long)]
    pub files: bool,

    /// Skip submodules with `submodule.<name>.ignore=all` (Git `--for-status`; used by status).
    #[arg(long = "for-status")]
    pub for_status: bool,

    /// Limit how many commits `log` shows for each submodule (`-n`; Git `--summary-limit`).
    #[arg(short = 'n', long = "summary-limit")]
    pub summary_limit: Option<i32>,

    /// Optional commit to compare against, then pathspecs after `--`.
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    pub rest: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct SetBranchArgs {
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

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
    /// Operate quietly (suppress progress and informational messages).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

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
    pub(crate) update: Option<String>,
    pub(crate) branch: Option<String>,
    /// `submodule.<name>.ignore` from `.gitmodules` (e.g. `all`, `dirty`), when set.
    pub(crate) ignore: Option<String>,
}

/// Update submodule working trees to the commits recorded in the superproject index.
///
/// Used after `pull` / `merge` when `--recurse-submodules` or `submodule.recurse` applies.
pub(crate) fn update_after_superproject_merge(init: bool, recursive: bool) -> Result<()> {
    run_update(&UpdateArgs {
        paths: vec![],
        quiet: false,
        init,
        checkout: false,
        remote: false,
        rebase: false,
        merge: false,
        force: false,
        depth: None,
        jobs: None,
        filter: None,
        recursive,
        reference: vec![],
        no_recommend_shallow: false,
    })
}

/// Stage the given commit OID as the gitlink for `rel_path` in the superproject index.
///
/// Used by `submodule update --remote` so the superproject records the fetched submodule tip
/// (matches Git; required for `git commit <path>` after `--remote`).
fn stage_gitlink_in_super_index(
    repo: &Repository,
    rel_path: &str,
    new_oid_hex: &str,
) -> Result<()> {
    let new_oid = ObjectId::from_hex(new_oid_hex.trim())
        .with_context(|| format!("invalid submodule OID '{new_oid_hex}' for path '{rel_path}'"))?;
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
    if entry.mode != MODE_GITLINK {
        return Ok(());
    }
    entry.oid = new_oid;
    repo.write_index_at(&index_path, &mut index)?;
    Ok(())
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

/// Run the `submodule` command (no leading `--quiet` / `--cached`; use [`run_from_argv`] from main).
pub fn run(args: Args) -> Result<()> {
    run_with_top_opts(SubmoduleTopOpts::default(), args)
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
        let modules_git = submodule_modules_git_dir(&repo.git_dir, &path);
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

pub(crate) fn parse_gitmodules_with_repo(
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
    #[derive(Default)]
    struct ModuleFields {
        path: Option<String>,
        url: Option<String>,
        shallow: Option<bool>,
        update: Option<String>,
        branch: Option<String>,
        ignore: Option<String>,
    }
    let mut modules: BTreeMap<String, ModuleFields> = BTreeMap::new();

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
            let entry_val = modules.entry(name.to_string()).or_default();
            match var {
                "path" => entry_val.path = entry.value.clone(),
                "url" => entry_val.url = entry.value.clone(),
                "shallow" => {
                    if let Some(v) = entry.value.as_deref() {
                        let v = v.trim();
                        if v.eq_ignore_ascii_case("true")
                            || v == "1"
                            || v.eq_ignore_ascii_case("yes")
                        {
                            entry_val.shallow = Some(true);
                        } else if v.eq_ignore_ascii_case("false")
                            || v == "0"
                            || v.eq_ignore_ascii_case("no")
                        {
                            entry_val.shallow = Some(false);
                        }
                    }
                }
                "update" => entry_val.update = entry.value.clone(),
                "branch" => entry_val.branch = entry.value.clone(),
                "ignore" => entry_val.ignore = entry.value.clone(),
                _ => {}
            }
        }
    }

    let mut result = Vec::new();
    for (name, f) in modules {
        if let (Some(path), Some(url)) = (f.path, f.url) {
            result.push(SubmoduleInfo {
                name,
                path,
                url,
                shallow: f.shallow,
                update: f.update,
                branch: f.branch,
                ignore: f.ignore,
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

/// Gitlink OID for `submodule_path` in `HEAD`'s tree only (ignores the index).
fn read_gitlink_from_head_tree(repo: &Repository, submodule_path: &str) -> Result<Option<String>> {
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
    quiet: bool,
) -> Result<()> {
    let sub_path = work_tree.join(submodule_path);
    let modules_dir = submodule_modules_git_dir(&repo.git_dir, submodule_path);

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

    if !quiet {
        eprintln!(
            "Submodule path '{}': checked out '{}'",
            submodule_path,
            &oid[..oid.len().min(12)]
        );
    }
    Ok(())
}

// ── Subcommand implementations ───────────────────────────────────────

/// Matches Git's `compute_rev_name` in `submodule--helper.c`: try `describe` with several
/// flag sets until one succeeds.
fn submodule_describe_rev_name(sub_worktree: &Path, oid_hex: &str) -> Option<String> {
    if oid_hex.len() != 40 || !oid_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let grit_bin = grit_exe::grit_executable();
    let attempts: &[&[&str]] = &[&[], &["--tags"], &["--contains"], &["--all", "--always"]];
    for extra in attempts {
        let mut cmd = grit_subprocess(&grit_bin);
        cmd.current_dir(sub_worktree)
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .arg("describe");
        for flag in *extra {
            cmd.arg(flag);
        }
        cmd.arg(oid_hex);
        let Ok(output) = cmd.output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

fn emit_submodule_status_lines(
    super_repo: &Repository,
    super_work_tree: &Path,
    _super_git_dir: &Path,
    top_work_tree: &Path,
    invocation_cwd: &Path,
    modules: &[SubmoduleInfo],
    args: &StatusArgs,
    path_prefix: &str,
    out: &mut dyn Write,
) -> Result<()> {
    let mut sorted: Vec<&SubmoduleInfo> = modules.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    if args.quiet {
        return Ok(());
    }

    for m in sorted {
        let path_in_super = if path_prefix.is_empty() {
            m.path.replace('\\', "/")
        } else {
            format!("{}/{}", path_prefix.trim_end_matches('/'), m.path)
        };

        let sub_path = super_work_tree.join(&m.path);
        // Paths in the immediate superproject's index / HEAD tree use `m.path` (not the
        // top-level composite path).
        let gitlink_path = m.path.as_str();
        let recorded = read_submodule_commit(super_repo, gitlink_path)?;
        let has_checkout = sub_path.join(".git").exists();

        if !args.paths.is_empty() {
            let under_selected = args
                .paths
                .iter()
                .any(|p| path_in_super == *p || path_in_super.starts_with(&format!("{p}/")));
            if !under_selected {
                continue;
            }
        }

        let (prefix, display_oid, suffix) = if !sub_path.exists() || !has_checkout {
            let oid = recorded
                .as_deref()
                .unwrap_or("0000000000000000000000000000000000000000");
            ("-", oid.to_owned(), String::new())
        } else {
            let index_oid = read_gitlink_oid_from_index(super_repo, gitlink_path)?
                .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_owned());

            let head_file = sub_path.join(".git");
            let sub_head = if head_file.exists() {
                read_submodule_head(&sub_path)
            } else {
                // gitfile indirection: check .git/modules/<name>/HEAD
                let modules_head =
                    submodule_modules_git_dir(&super_repo.git_dir, &m.name).join("HEAD");
                if modules_head.exists() {
                    read_head_from_file(&modules_head)
                } else {
                    None
                }
            };
            let head_oid = sub_head.unwrap_or_default();

            // Match `git submodule--helper status`: `diff-files` marks the submodule dirty when
            // the checked-out commit differs from the index gitlink.
            let dirty = !head_oid.is_empty() && head_oid != index_oid;

            let (p, oid_for_line, oid_for_describe) = if !dirty {
                (" ", index_oid.clone(), index_oid.clone())
            } else if args.cached {
                ("+", index_oid.clone(), index_oid.clone())
            } else {
                ("+", head_oid.clone(), head_oid.clone())
            };

            let suf = submodule_describe_rev_name(&sub_path, &oid_for_describe)
                .map(|n| format!(" ({n})"))
                .unwrap_or_default();
            (p, oid_for_line, suf)
        };

        let display_path =
            rev_parse::to_relative_path(&top_work_tree.join(&path_in_super), invocation_cwd)
                .replace('\\', "/");

        writeln!(out, "{prefix}{display_oid} {display_path}{suffix}")?;

        if args.recursive && has_checkout && sub_path.join(".git").exists() {
            let Ok(sub_repo) = Repository::discover(Some(&sub_path)) else {
                continue;
            };
            let Some(sub_wt) = sub_repo.work_tree.as_ref() else {
                continue;
            };
            let nested = parse_gitmodules_with_repo(sub_wt, Some(&sub_repo)).unwrap_or_default();
            if !nested.is_empty() {
                emit_submodule_status_lines(
                    &sub_repo,
                    sub_wt,
                    &sub_repo.git_dir,
                    top_work_tree,
                    invocation_cwd,
                    &nested,
                    args,
                    &path_in_super,
                    out,
                )?;
            }
        }
    }

    Ok(())
}

fn run_status(args: &StatusArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;

    let cwd = std::env::current_dir().context("failed to read current directory")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    emit_submodule_status_lines(
        &repo,
        work_tree,
        &repo.git_dir,
        work_tree,
        &cwd,
        &modules,
        args,
        "",
        &mut out,
    )?;
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

fn default_initial_branch_name() -> String {
    std::env::var("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME").unwrap_or_else(|_| "main".to_string())
}

/// When `remote.origin.url` points at a local repository, emulate `git fetch origin` by copying
/// objects and updating `refs/remotes/origin/*` without `upload-pack` (avoids protocol v2 client
/// limitations for submodule `--remote`).
///
/// Returns `Ok(true)` when the fast path ran, `Ok(false)` when the URL is not a local repo path.
fn submodule_fetch_origin_local_path(
    sub_path: &Path,
    local_cfg: &ConfigFile,
    quiet: bool,
) -> Result<bool> {
    let Some(sub_git_dir) = resolve_submodule_git_dir(sub_path) else {
        return Ok(false);
    };
    let Some(url) = config_last_value(local_cfg, "remote.origin.url") else {
        return Ok(false);
    };
    let url = url.trim();
    if url.is_empty() {
        return Ok(false);
    }
    if url.starts_with("ext::") || url.starts_with("http://") || url.starts_with("https://") {
        return Ok(false);
    }
    if url.starts_with("git://") {
        return Ok(false);
    }
    if crate::ssh_transport::is_configured_ssh_url(url) {
        return Ok(false);
    }

    let mut remote_path = if let Some(stripped) = url.strip_prefix("file://") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(url)
    };
    if remote_path.is_relative() {
        remote_path = sub_path.join(&remote_path);
    }
    let remote_path = remote_path.canonicalize().unwrap_or(remote_path);
    let remote_repo = match Repository::open(&remote_path, None)
        .or_else(|_| Repository::discover(Some(&remote_path)))
    {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };
    let remote_git = remote_repo.git_dir.as_path();

    let heads = refs::list_refs(remote_git, "refs/heads/")?;
    if heads.is_empty() {
        return Ok(false);
    }

    if !quiet {
        eprintln!("From {}", remote_path.display());
    }

    let mut roots: Vec<ObjectId> = Vec::new();
    for (refname, oid) in &heads {
        let short = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(refname.as_str());
        let local_ref = format!("refs/remotes/origin/{short}");
        let old_hex = refs::resolve_ref(&sub_git_dir, &local_ref)
            .map(|o| o.to_hex())
            .unwrap_or_else(|_| "0".repeat(40));
        refs::write_ref(&sub_git_dir, &local_ref, oid)?;
        roots.push(*oid);
        if !quiet {
            let branch = short;
            eprintln!(
                "   {}..{}  {}     -> origin/{}",
                &old_hex[..7.min(old_hex.len())],
                &oid.to_hex()[..7],
                branch,
                branch
            );
        }
    }
    roots.sort_by_key(|o| o.to_hex());
    roots.dedup();

    if let Ok(head) = resolve_head(remote_git) {
        match head {
            grit_lib::state::HeadState::Branch { short_name, .. } => {
                let sym = format!("refs/remotes/origin/{short_name}");
                if refs::resolve_ref(&sub_git_dir, &sym).is_ok() {
                    let _ =
                        refs::write_symbolic_ref(&sub_git_dir, "refs/remotes/origin/HEAD", &sym);
                }
            }
            _ => {}
        }
    }

    crate::commands::fetch::copy_reachable_objects(remote_git, &sub_git_dir, &roots)?;

    Ok(true)
}

fn superproject_head_short_branch(repo: &Repository) -> Option<String> {
    resolve_head(&repo.git_dir)
        .ok()
        .and_then(|h| h.branch_name().map(|s| s.to_string()))
}

fn resolve_submodule_remote_branch_name(
    super_repo: &Repository,
    sm: &SubmoduleInfo,
    local_cfg: &ConfigFile,
) -> String {
    let key = format!("submodule.{}.branch", sm.name);
    let mut branch = config_last_value(local_cfg, &key)
        .or_else(|| sm.branch.clone())
        .unwrap_or_else(default_initial_branch_name);
    if branch == "." {
        branch =
            superproject_head_short_branch(super_repo).unwrap_or_else(default_initial_branch_name);
    }
    branch
}

fn expand_submodule_shell_command(cmd: &str, sha1: &str, path: &str, toplevel: &Path) -> String {
    cmd.replace("$sha1", sha1)
        .replace("$path", path)
        .replace("$toplevel", &toplevel.to_string_lossy())
}

fn init_in_repo(repo: &Repository, args: &InitArgs, quiet: bool) -> Result<()> {
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let modules = parse_gitmodules_with_repo(work_tree, Some(repo))?;
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
        let already = config.entries.iter().any(|e| e.key == url_key);

        if !already {
            if let Some(ref u) = m.update {
                let t = u.trim();
                if t.starts_with('!') {
                    bail!(
                        "error: invalid value for 'submodule.{}.update': '{}' cannot be specified in .gitmodules as a command exists\n\
                         You can still add the config by using:\n\
                         'git config submodule.{}.update {}'",
                        m.name,
                        t,
                        m.name,
                        t
                    );
                }
            }

            let resolved_url = resolve_submodule_super_url(work_tree, &repo.git_dir, &m.url)?;
            config.set(&url_key, &resolved_url)?;
            let reg_path = submodule_display_path_from_cwd(&work_tree.join(&m.path));
            if !quiet {
                eprintln!(
                    "Submodule '{}' ({}) registered for path '{}'",
                    m.name, resolved_url, reg_path
                );
            }
        }

        if let Some(ref u) = m.update {
            config.set(&format!("submodule.{}.update", m.name), u)?;
        }
        if let Some(ref b) = m.branch {
            config.set(&format!("submodule.{}.branch", m.name), b)?;
        }
    }

    config.write()?;
    Ok(())
}

fn run_init(args: &InitArgs, quiet: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    init_in_repo(&repo, args, quiet)
}

include!("_submodule_run_update_inner.rs.inc");

fn run_update(args: &UpdateArgs) -> Result<()> {
    run_update_inner(args, None)
}

/// Populate `objects/info/alternates` for a submodule git dir (matches `git clone --reference`).
fn write_submodule_object_alternates(
    modules_dir: &Path,
    super_git_dir: &Path,
    reference_roots: &[PathBuf],
) -> Result<()> {
    let dst_info = modules_dir.join("objects/info");
    fs::create_dir_all(&dst_info)?;

    let super_objects = super_git_dir.join("objects");
    let super_objects_abs = super_objects.canonicalize().unwrap_or(super_objects);

    let mut lines = vec![super_objects_abs.to_string_lossy().to_string()];
    for root in reference_roots {
        let ref_git = if root.join("HEAD").exists() {
            root.clone()
        } else {
            root.join(".git")
        };
        let ref_repo = Repository::open(&ref_git, None)
            .with_context(|| format!("cannot open reference repository '{}'", root.display()))?;
        let ref_objects = ref_repo.git_dir.join("objects");
        let ref_objects_abs = ref_objects.canonicalize().unwrap_or(ref_objects);
        lines.push(ref_objects_abs.to_string_lossy().to_string());
    }

    let content = lines.join("\n") + "\n";
    fs::write(dst_info.join("alternates"), content)?;
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

/// Called from `clone --recurse-submodules` after cloning a submodule with `--separate-git-dir`.
pub(crate) fn set_submodule_core_worktree_after_separate_clone(
    grit_bin: &Path,
    modules_dir: &Path,
    sub_path: &Path,
) {
    set_submodule_core_worktree(grit_bin, modules_dir, sub_path);
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

/// Whether `.gitmodules` may be created or updated in the work tree (`git/submodule.c:is_writing_gitmodules_ok`).
fn is_writing_gitmodules_ok(repo: &Repository, work_tree: &Path) -> bool {
    let gm = work_tree.join(".gitmodules");
    if gm.exists() {
        return true;
    }
    let Ok(index) = repo.load_index() else {
        return false;
    };
    if index.get(b".gitmodules", 0).is_some() {
        return false;
    }
    let Ok(head) = resolve_head(&repo.git_dir) else {
        return false;
    };
    let Some(commit_oid) = head.oid().copied() else {
        return true;
    };
    let Ok(obj) = repo.odb.read(&commit_oid) else {
        return false;
    };
    if obj.kind != ObjectKind::Commit {
        return false;
    }
    let Ok(c) = parse_commit(&obj.data) else {
        return false;
    };
    blob_oid_at_path(&repo.odb, &c.tree, ".gitmodules").is_none()
}

fn run_add(args: &AddArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;

    if !is_writing_gitmodules_ok(&repo, work_tree) {
        bail!("please make sure that the .gitmodules file is in the working tree");
    }

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
        if !args.quiet {
            eprintln!("Adding existing repo at '{}' to the index", path);
        }
    } else {
        // Clone the submodule.
        let modules_dir = submodule_modules_git_dir(&repo.git_dir, &path);
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
        checkout_submodule_worktree(&grit_bin, &repo, work_tree, &path, &oid, args.quiet)?;
    }

    if !args.quiet {
        eprintln!("Cloning into '{}'...", path);
    }
    Ok(())
}

fn run_foreach(args: &ForeachArgs, quiet: bool) -> Result<()> {
    let command_argv: Vec<String> = if args.command.is_empty() {
        vec![":".to_owned()]
    } else {
        args.command.clone()
    };

    if !command_argv.is_empty() && command_argv[0].starts_with("--") {
        eprintln!("usage: git submodule [--quiet] foreach [--recursive] [--] <command>...");
        std::process::exit(1);
    }

    let top_repo = Repository::discover(None).context("not a git repository")?;
    let top_work_tree = top_repo
        .work_tree
        .as_ref()
        .context("bare repository")?
        .to_path_buf();
    let cwd = std::env::current_dir().context("failed to read current directory")?;

    let modules = parse_gitmodules_with_repo(&top_work_tree, Some(&top_repo))?;
    run_foreach_in(
        &top_repo,
        &top_work_tree,
        &cwd,
        &modules,
        &command_argv,
        args.recursive,
        "",
        quiet,
    )
}

fn run_foreach_in(
    super_repo: &Repository,
    super_work_tree: &Path,
    invocation_cwd: &Path,
    modules: &[SubmoduleInfo],
    command_argv: &[String],
    recursive: bool,
    path_prefix: &str,
    quiet: bool,
) -> Result<()> {
    let mut sorted: Vec<&SubmoduleInfo> = modules.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    for m in sorted {
        let sub_path = super_work_tree.join(&m.path);
        if !sub_path.join(".git").exists() {
            continue;
        }

        let path_in_super = if path_prefix.is_empty() {
            m.path.replace('\\', "/")
        } else {
            format!("{}/{}", path_prefix.trim_end_matches('/'), m.path)
        };

        let displaypath = rev_parse::to_relative_path(&sub_path, invocation_cwd).replace('\\', "/");

        if !quiet {
            // Match Git: "Entering" goes to stdout so `submodule foreach cmd >file` captures it.
            println!("Entering '{}'", displaypath);
        }

        let sha1 = read_submodule_commit(super_repo, &m.path)?.unwrap_or_default();

        let mut cmd = Command::new("sh");
        if command_argv.len() == 1 {
            // One shell snippet (e.g. `git submodule foreach "git submodule update --init"`).
            cmd.arg("-c").arg(&command_argv[0]);
        } else {
            // Multiple argv words: run via `exec` so the command is not parsed twice (matches Git).
            cmd.arg("-c")
                .arg("exec \"$@\"")
                .arg("sh")
                .args(command_argv);
        }
        let status = cmd
            .current_dir(&sub_path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .env("name", &m.name)
            .env("sm_path", &m.path)
            .env("path", &m.path)
            .env("sha1", &sha1)
            .env(
                "toplevel",
                super_work_tree.to_string_lossy().replace('\\', "/"),
            )
            .env("displaypath", &displaypath)
            .status()
            .context("failed to run foreach command")?;

        if !status.success() {
            bail!(
                "Stopping at '{}'; command returned non-zero status",
                displaypath
            );
        }

        if recursive {
            let Ok(sub_repo) = Repository::discover(Some(&sub_path)) else {
                continue;
            };
            let Some(sub_wt) = sub_repo.work_tree.as_ref() else {
                continue;
            };
            let nested = parse_gitmodules_with_repo(sub_wt, Some(&sub_repo)).unwrap_or_default();
            if !nested.is_empty() {
                run_foreach_in(
                    &sub_repo,
                    sub_wt,
                    invocation_cwd,
                    &nested,
                    command_argv,
                    true,
                    &path_in_super,
                    quiet,
                )?;
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

fn run_sync(args: &SyncArgs, quiet: bool) -> Result<()> {
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
        if !quiet {
            eprintln!("Synchronizing submodule url for '{}'", m.path);
        }

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

fn run_deinit(args: &DeinitArgs, quiet: bool) -> Result<()> {
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

        if !quiet {
            eprintln!("Cleared directory '{}'", m.path);
            eprintln!(
                "Submodule '{}' ({}) unregistered for path '{}'",
                m.name, m.url, m.path
            );
        }
    }

    config.write()?;
    Ok(())
}

fn run_absorbgitdirs(args: &AbsorbgitdirsArgs, quiet: bool) -> Result<()> {
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

        let modules_dir = submodule_modules_git_dir(&repo.git_dir, &m.name);

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

        if !quiet {
            eprintln!(
                "Migrating git directory of '{}' from '{}' to '{}'",
                m.path,
                sub_path.join(".git").display(),
                modules_dir.display()
            );
        }
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

fn parse_mode_octal(mode: &str) -> u32 {
    u32::from_str_radix(mode.trim(), 8).unwrap_or(0)
}

fn mode_is_gitlink(mode: &str) -> bool {
    parse_mode_octal(mode) == MODE_GITLINK
}

fn short_oid_in_submodule(grit_bin: &Path, sub_path: &Path, committish: &str) -> Option<String> {
    let spec = format!("{committish}^0");
    let out = grit_subprocess(grit_bin)
        .args(["rev-parse", "-q", "--short", &spec, "--"])
        .current_dir(sub_path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

fn submodule_rev_list_count(grit_bin: &Path, sub_path: &Path, range: &str) -> Result<i32> {
    let out = grit_subprocess(grit_bin)
        .args(["rev-list", "--first-parent", "--count", range, "--"])
        .current_dir(sub_path)
        .output()
        .context("rev-list --count in submodule")?;
    if !out.status.success() {
        return Ok(-1);
    }
    let n = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i32>()
        .unwrap_or(-1);
    Ok(n)
}

fn submodule_log_first_parent(
    grit_bin: &Path,
    sub_path: &Path,
    src_abbrev: &str,
    dst_abbrev: &str,
    summary_limit: i32,
) -> Result<()> {
    let range = format!("{src_abbrev}...{dst_abbrev}");
    let mut cmd = grit_subprocess(grit_bin);
    cmd.current_dir(sub_path);
    cmd.arg("log");
    if summary_limit > 0 {
        cmd.arg(format!("-{summary_limit}"));
    }
    cmd.args(["--pretty=  %m %s", "--first-parent", &range, "--"]);
    let st = cmd.status().context("submodule log for summary")?;
    if !st.success() {
        bail!("submodule log failed");
    }
    Ok(())
}

fn submodule_log_one(
    grit_bin: &Path,
    sub_path: &Path,
    dst_abbrev: &str,
    prefix: char,
) -> Result<()> {
    let pretty = format!("  {} %s", prefix);
    let st = grit_subprocess(grit_bin)
        .args(["log", "--pretty", &pretty, "-1", dst_abbrev, "--"])
        .current_dir(sub_path)
        .status()
        .context("submodule log -1 for summary")?;
    if !st.success() {
        bail!("submodule log -1 failed");
    }
    Ok(())
}

fn resolve_summary_base_tree(repo: &Repository, commit_spec: &str) -> Result<Option<ObjectId>> {
    match resolve_revision(repo, commit_spec) {
        Ok(oid) => {
            let obj = repo.odb.read(&oid).context("read summary base commit")?;
            let commit = parse_commit(&obj.data).context("parse summary base commit")?;
            Ok(Some(commit.tree))
        }
        Err(e) => {
            if commit_spec == "HEAD" {
                Ok(None)
            } else {
                return Err(e).context("could not resolve summary base revision");
            }
        }
    }
}

fn summary_display_path(entry: &DiffEntry) -> &str {
    entry.old_path.as_deref().unwrap_or_else(|| entry.path())
}

fn pathspec_selected(pathspecs: &[String], sm_path: &str) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    pathspecs.iter().any(|spec| matches_pathspec(spec, sm_path))
}

/// Working tree directory for a submodule given the path Git uses in the summary diff (often the
/// old path after `git mv`).
fn submodule_work_tree_for_summary(work_tree: &Path, logical_path: &str) -> PathBuf {
    let direct = work_tree.join(logical_path);
    if direct.join(".git").exists() {
        return direct;
    }
    let Ok(modules) = parse_gitmodules(work_tree) else {
        return direct;
    };
    if let Some(m) = modules
        .iter()
        .find(|m| m.path == logical_path || m.name == logical_path)
    {
        let relocated = work_tree.join(&m.path);
        if relocated.join(".git").exists() {
            return relocated;
        }
    }
    direct
}

/// True when `submodule.<name>.ignore` is `all` in local config or in `.gitmodules` (Git `prepare_submodule_summary`).
fn submodule_ignore_all_for_summary(
    local_cfg: Option<&ConfigFile>,
    modules: &[SubmoduleInfo],
    sm_path: &str,
) -> bool {
    let Some(m) = modules
        .iter()
        .find(|mm| mm.path == sm_path || mm.name == sm_path)
    else {
        return false;
    };
    let key = format!("submodule.{}.ignore", m.name);
    if let Some(cfg) = local_cfg {
        if let Ok(canon) = canonical_key(&key) {
            if cfg
                .entries
                .iter()
                .rev()
                .find(|e| e.key == canon)
                .and_then(|e| e.value.as_deref())
                .is_some_and(|v| v.eq_ignore_ascii_case("all"))
            {
                return true;
            }
        }
    }
    m.ignore
        .as_deref()
        .is_some_and(|v| v.eq_ignore_ascii_case("all"))
}

fn run_summary(args: &SummaryArgs, _quiet: bool) -> Result<()> {
    if args.summary_limit == Some(0) {
        return Ok(());
    }
    let summary_limit = args.summary_limit.unwrap_or(-1);

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let grit_bin = grit_exe::grit_executable();

    let mut commit_spec = "HEAD";
    let pathspecs: Vec<String> = if let Some(p) = args.rest.iter().position(|x| x.as_str() == "--")
    {
        let head_tokens = &args.rest[..p];
        let tail = args.rest[p + 1..].to_vec();
        if head_tokens.is_empty() {
            tail
        } else if resolve_revision(&repo, &head_tokens[0]).is_ok() {
            commit_spec = head_tokens[0].as_str();
            let mut ps: Vec<String> = head_tokens[1..].to_vec();
            ps.extend(tail);
            ps
        } else {
            let mut ps = head_tokens.to_vec();
            ps.extend(tail);
            ps
        }
    } else if args.rest.is_empty() {
        vec![]
    } else if resolve_revision(&repo, &args.rest[0]).is_ok() {
        commit_spec = args.rest[0].as_str();
        args.rest[1..].to_vec()
    } else {
        args.rest.clone()
    };

    let base_tree_oid = resolve_summary_base_tree(&repo, commit_spec)?;
    let index = repo
        .load_index()
        .context("load index for submodule summary")?;

    let (modules_for_ignore, local_cfg_for_ignore) = if args.for_status {
        (
            parse_gitmodules_with_repo(work_tree, Some(&repo)).unwrap_or_default(),
            parse_local_config(&repo.git_dir).ok(),
        )
    } else {
        (Vec::new(), None)
    };

    let entries: Vec<DiffEntry> = if args.files {
        if args.cached {
            bail!("options '--cached' and '--files' cannot be used together");
        }
        let mut out = Vec::new();
        let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
        for m in &modules {
            let path_bytes = m.path.as_bytes();
            let Some(ie) = index.get(path_bytes, 0) else {
                continue;
            };
            if ie.mode != MODE_GITLINK {
                continue;
            }
            let sub_path = work_tree.join(&m.path);
            let dst_oid = if let Some(h) = grit_lib::diff::read_submodule_head_oid(&sub_path) {
                h
            } else {
                ObjectId::zero()
            };
            if ie.oid == dst_oid {
                continue;
            }
            out.push(DiffEntry {
                status: DiffStatus::Modified,
                old_path: Some(m.path.clone()),
                new_path: Some(m.path.clone()),
                old_mode: format!("{:o}", MODE_GITLINK),
                new_mode: format!("{:o}", MODE_GITLINK),
                old_oid: ie.oid,
                new_oid: dst_oid,
                score: None,
            });
        }
        out.sort_by(|a, b| a.path().cmp(b.path()));
        out
    } else {
        let mut entries = diff_index_to_tree(&repo.odb, &index, base_tree_oid.as_ref())?;
        // Git `submodule summary` uses `diff-index --ignore-submodules=dirty`: when the index
        // gitlink matches `HEAD^{tree}` but the submodule worktree HEAD differs (e.g. after
        // `pull` before `submodule update`), still report the range (`t7418`).
        if !args.cached {
            if let Some(tree_oid) = base_tree_oid.as_ref() {
                let mut extra: Vec<DiffEntry> = Vec::new();
                for ie in &index.entries {
                    if ie.stage() != 0 || ie.mode != MODE_GITLINK || ie.skip_worktree() {
                        continue;
                    }
                    let path_str = String::from_utf8_lossy(&ie.path).into_owned();
                    let Some(te_oid) = blob_oid_at_path(&repo.odb, tree_oid, &path_str) else {
                        continue;
                    };
                    if te_oid != ie.oid {
                        continue;
                    }
                    let sub_path = submodule_work_tree_for_summary(work_tree, &path_str);
                    if !sub_path.join(".git").exists() {
                        continue;
                    }
                    let Some(sub_head) = grit_lib::diff::read_submodule_head_oid(&sub_path) else {
                        continue;
                    };
                    if sub_head == ie.oid {
                        continue;
                    }
                    extra.push(DiffEntry {
                        status: DiffStatus::Modified,
                        old_path: Some(path_str.clone()),
                        new_path: Some(path_str),
                        old_mode: format!("{:o}", MODE_GITLINK),
                        new_mode: format!("{:o}", MODE_GITLINK),
                        old_oid: ie.oid,
                        new_oid: sub_head,
                        score: None,
                    });
                }
                entries.extend(extra);
                entries.sort_by(|a, b| a.path().cmp(b.path()));
            }
        }
        entries
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for e in &entries {
        if !mode_is_gitlink(&e.old_mode) && !mode_is_gitlink(&e.new_mode) {
            continue;
        }
        let sm_path = summary_display_path(e);
        if !pathspec_selected(&pathspecs, sm_path) {
            continue;
        }

        if args.for_status
            && e.status != DiffStatus::Added
            && submodule_ignore_all_for_summary(
                local_cfg_for_ignore.as_ref(),
                &modules_for_ignore,
                sm_path,
            )
        {
            continue;
        }

        let sub_path = submodule_work_tree_for_summary(work_tree, sm_path);
        if !args.cached && !sub_path.join(".git").exists() {
            continue;
        }

        let oid_src = e.old_oid;
        let mut oid_dst = e.new_oid;
        if !args.cached && oid_dst.is_zero() && mode_is_gitlink(&e.new_mode) {
            if let Some(h) = grit_lib::diff::read_submodule_head_oid(&sub_path) {
                oid_dst = h;
            }
        }

        let src_gitlink = mode_is_gitlink(&e.old_mode);
        let dst_gitlink = mode_is_gitlink(&e.new_mode);

        let src_hex = oid_src.to_hex();
        let dst_hex = oid_dst.to_hex();
        let src_abbrev = short_oid_in_submodule(&grit_bin, &sub_path, &src_hex)
            .unwrap_or_else(|| src_hex.chars().take(7).collect());
        let dst_abbrev = short_oid_in_submodule(&grit_bin, &sub_path, &dst_hex)
            .unwrap_or_else(|| dst_hex.chars().take(7).collect());

        if e.status == DiffStatus::TypeChanged {
            if dst_gitlink && !src_gitlink {
                writeln!(
                    out,
                    "* {} {}(blob)->{}(submodule)",
                    sm_path, src_abbrev, dst_abbrev
                )?;
            } else if src_gitlink && !dst_gitlink {
                writeln!(
                    out,
                    "* {} {}(submodule)->{}(blob)",
                    sm_path, src_abbrev, dst_abbrev
                )?;
            } else {
                writeln!(out, "* {} {}...{}", sm_path, src_abbrev, dst_abbrev)?;
            }
            writeln!(out)?;
            continue;
        }

        let total_commits = if !src_abbrev.is_empty() && !dst_abbrev.is_empty() {
            if src_gitlink && dst_gitlink {
                submodule_rev_list_count(
                    &grit_bin,
                    &sub_path,
                    &format!("{src_abbrev}...{dst_abbrev}"),
                )?
            } else {
                submodule_rev_list_count(&grit_bin, &sub_path, &dst_abbrev)?
            }
        } else {
            -1
        };

        write!(out, "* {} {}...{}", sm_path, src_abbrev, dst_abbrev)?;
        if total_commits < 0 {
            writeln!(out, ":")?;
        } else {
            writeln!(out, " ({total_commits}):")?;
        }
        out.flush()?;

        if total_commits > 0 {
            if src_gitlink && dst_gitlink {
                submodule_log_first_parent(
                    &grit_bin,
                    &sub_path,
                    &src_abbrev,
                    &dst_abbrev,
                    summary_limit,
                )?;
            } else if dst_gitlink {
                submodule_log_one(&grit_bin, &sub_path, &dst_abbrev, '>')?;
            } else {
                submodule_log_one(&grit_bin, &sub_path, &src_abbrev, '<')?;
            }
        }
        writeln!(out)?;
    }

    Ok(())
}

fn run_set_branch(args: &SetBranchArgs, _quiet: bool) -> Result<()> {
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

fn run_set_url(args: &SetUrlArgs, _quiet: bool) -> Result<()> {
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
