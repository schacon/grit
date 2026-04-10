//! `grit checkout` — switch branches or restore working tree files.
//!
//! Supports:
//! - `checkout <branch>` — switch to a branch, updating HEAD, index, and working tree.
//! - `checkout -b <new-branch> [<start>]` — create and switch to a new branch.
//! - `checkout <commit>` — detach HEAD at a commit.
//! - `checkout [<tree-ish>] -- <paths>` — restore specific files.
//! - `-f` / `--force` — discard local changes when switching.

use crate::grit_exe;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use grit_lib::check_ref_format::{check_refname_format, RefNameOptions};
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{self, MergeAttr};
use grit_lib::diff::read_submodule_head_oid;
use grit_lib::diff::{diff_index_to_worktree, zero_oid};
use grit_lib::filter_process;
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_SYMLINK};
use grit_lib::merge_file::{self, ConflictStyle, MergeFavor, MergeInput};
use grit_lib::merge_trees::{
    merge_trees_three_way, TheirsConflictLabel, TreeMergeConflictPresentation,
    WhitespaceMergeOptions,
};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::refs::{self, append_reflog};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    abbreviate_object_id, resolve_revision, resolve_upstream_symbolic_name, upstream_suffix_info,
};
use grit_lib::sparse_checkout::apply_sparse_checkout_skip_worktree;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::submodule_gitdir::submodule_modules_git_dir;
use grit_lib::write_tree::write_tree_from_index;

use crate::branch_tracking::{format_tracking_info, AheadBehindMode};
use crate::commands::merge::execute_custom_merge_driver;

/// Count parallel checkout worker processes to spawn for trace2 tests (`t2080`).
///
/// Returns `0` when checkout stays sequential (one worker, below threshold, or no work).
pub(crate) fn checkout_parallel_worker_spawns(repo: &Repository, work_units: usize) -> usize {
    if work_units == 0 {
        return 0;
    }
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let workers: u32 = config
        .get("checkout.workers")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let threshold: usize = config
        .get("checkout.thresholdForParallelism")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if workers <= 1 || work_units <= threshold {
        return 0;
    }
    (workers as usize).min(work_units)
}

/// Append `child_start[..] git checkout--worker` lines to `GIT_TRACE2` when set.
pub(crate) fn trace2_emit_checkout_parallel_workers(count: usize) {
    if count == 0 {
        return;
    }
    let Ok(path) = std::env::var("GIT_TRACE2") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let now = chrono_now_for_trace2();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        for i in 0..count {
            let _ = writeln!(
                file,
                "{} grit:0                         child_start[{}] git checkout--worker",
                now, i
            );
        }
    }
}

fn chrono_now_for_trace2() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:02}:{:02}:{:02}.{:06}",
        now.hour(),
        now.minute(),
        now.second(),
        now.microsecond()
    )
}

/// Run `grit submodule update --init --recursive` after a superproject checkout.
fn recurse_submodules_after_checkout(repo: &Repository) -> Result<()> {
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let grit_bin = grit_exe::grit_executable();
    let mut cmd = Command::new(&grit_bin);
    grit_exe::strip_trace2_env(&mut cmd);
    let status = cmd
        .args(["submodule", "update", "--init", "--recursive"])
        .current_dir(work_tree)
        .status()
        .context("spawning submodule update")?;
    if !status.success() {
        bail!("submodule update failed");
    }
    Ok(())
}

/// Arguments for `grit checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Switch branches or restore working tree files")]
#[derive(Default)]
pub struct Args {
    /// Create a new branch and switch to it.
    #[arg(short = 'b')]
    pub new_branch: Option<String>,

    /// Create (or force-reset) a new branch and switch to it.
    #[arg(short = 'B', conflicts_with = "new_branch")]
    pub force_branch: Option<String>,

    /// Create a new orphan branch (no parent commit).
    #[arg(long = "orphan", conflicts_with_all = ["new_branch", "force_branch", "track"])]
    pub orphan: Option<String>,

    /// Force: discard local changes.
    #[arg(short = 'f', long = "force", hide = true)]
    pub force: bool,

    /// Overwrite ignored files (allow checkout to clobber ignored files).
    #[arg(long = "overwrite-ignore", hide = true)]
    pub overwrite_ignore: bool,

    /// Suppress feedback messages.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Create reflog for the new branch.
    #[arg(short = 'l')]
    pub create_reflog: bool,

    /// Detach HEAD at the named commit (even if it is a branch).
    #[arg(long = "detach", short = 'd', conflicts_with_all = ["new_branch", "force_branch", "orphan"])]
    pub detach: bool,

    /// Set up tracking (upstream) configuration for the new branch.
    /// Accepts optional value: direct (default), inherit.
    #[arg(long = "track", short = 't', value_name = "MODE", num_args = 0..=1,
          default_missing_value = "direct", require_equals = true)]
    pub track: Option<String>,

    /// Do not set up tracking configuration.
    #[arg(long = "no-track", hide = true)]
    pub no_track: bool,

    /// Do not keep files that are not in the source tree (path mode).
    #[arg(long = "no-overlay", hide = true)]
    pub no_overlay: bool,

    /// Keep overlay behaviour (default, for explicitness).
    #[arg(long = "overlay")]
    pub overlay: bool,

    /// Interactively select hunks to discard.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Merge local modifications when switching branches.
    #[arg(short = 'm', long = "merge")]
    pub merge: bool,

    /// Check out their version for unmerged files.
    #[arg(long = "ours")]
    pub ours: bool,

    /// Check out our version for unmerged files.
    #[arg(long = "theirs")]
    pub theirs: bool,

    /// Conflict style (merge or diff3).
    #[arg(long = "conflict")]
    pub conflict: Option<String>,

    /// Lines of context for --patch.
    #[arg(long = "unified", short = 'U')]
    pub unified: Option<usize>,

    /// Maximum number of context lines between diff hunks.
    #[arg(long = "inter-hunk-context")]
    pub inter_hunk_context: Option<usize>,

    /// Do not fail on entries with skip-worktree bit set.
    #[arg(long = "ignore-skip-worktree-bits")]
    pub ignore_skip_worktree_bits: bool,

    /// Do not check if another worktree has it checked out.
    #[arg(long = "ignore-other-worktrees")]
    pub ignore_other_worktrees: bool,

    /// Recurse into submodules.
    #[arg(long = "recurse-submodules")]
    pub recurse_submodules: bool,

    /// Auto-advance to next conflict.
    #[arg(long = "auto-advance")]
    pub auto_advance: bool,

    /// Display progress.
    #[arg(long = "progress")]
    pub progress: bool,

    /// Guess branch name from remote tracking branches (default).
    #[arg(long = "guess")]
    pub guess: bool,

    /// Do not guess branch name from remote tracking branches.
    #[arg(long = "no-guess")]
    pub no_guess: bool,

    /// NUL-terminated pathspec from file.
    #[arg(long = "pathspec-file-nul")]
    pub pathspec_file_nul: bool,

    /// Read pathspec from file.
    #[arg(long = "pathspec-from-file")]
    pub pathspec_from_file: Option<String>,

    /// Internal: set by `grit switch` so `-- <name>` is interpreted as a branch, not a path.
    #[arg(long = "__grit_switch_mode", hide = true, default_value_t = false)]
    pub switch_mode: bool,

    /// Remaining positional arguments: `[<branch|commit>] [--] [<paths>...]`
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

/// Run `grit checkout`.
use std::cell::Cell;

/// Parsed `-m` / `--merge` / `--no-merge` / `--conflict` / `--no-conflict` (matches Git tri-state `merge`).
#[derive(Clone, Debug, Default)]
struct CheckoutMergeCli {
    /// `-1` = unset, `0` = explicit `--no-merge`, `1` = explicit `-m`/`--merge`.
    merge_tri: i8,
    conflict_style: Option<ConflictStyle>,
    /// With explicit `--merge`, `--no-conflict` forces two-way markers (t7201).
    force_two_way_markers: bool,
}

fn parse_conflict_style_name(raw: &str) -> Result<ConflictStyle> {
    match raw.to_ascii_lowercase().as_str() {
        "merge" => Ok(ConflictStyle::Merge),
        "diff3" | "zdiff3" => Ok(ConflictStyle::Diff3),
        _ => bail!("error: unknown conflict style '{}'", raw),
    }
}

fn parse_checkout_merge_flags_from_raw(raw: &[String]) -> CheckoutMergeCli {
    let mut out = CheckoutMergeCli::default();
    let mut i = 0usize;
    while i < raw.len() {
        let a = raw[i].as_str();
        if a == "-m" || a == "--merge" {
            out.merge_tri = 1;
            out.force_two_way_markers = false;
        } else if a == "--no-merge" {
            out.merge_tri = 0;
        } else if a == "--no-conflict" {
            out.conflict_style = None;
            if out.merge_tri == 1 {
                out.force_two_way_markers = true;
            }
        } else if let Some(rest) = a.strip_prefix("--conflict=") {
            match parse_conflict_style_name(rest) {
                Ok(style) => {
                    out.conflict_style = Some(style);
                    if out.merge_tri != 1 {
                        out.merge_tri = -1;
                    }
                }
                Err(_) => {
                    // Defer to main path for exact error message shape
                }
            }
        } else if a == "--conflict" && i + 1 < raw.len() {
            match parse_conflict_style_name(&raw[i + 1]) {
                Ok(style) => {
                    out.conflict_style = Some(style);
                    if out.merge_tri != 1 {
                        out.merge_tri = -1;
                    }
                    i += 1;
                }
                Err(_) => {}
            }
        }
        i += 1;
    }
    if out.merge_tri < 0 && out.conflict_style.is_some() {
        out.merge_tri = 1;
    }
    out
}

fn validate_checkout_conflict_arg(raw: &[String]) -> Result<()> {
    let mut i = 0usize;
    while i < raw.len() {
        let a = raw[i].as_str();
        if let Some(rest) = a.strip_prefix("--conflict=") {
            parse_conflict_style_name(rest)?;
        } else if a == "--conflict" && i + 1 < raw.len() {
            parse_conflict_style_name(&raw[i + 1])?;
            i += 1;
        }
        i += 1;
    }
    Ok(())
}

fn effective_branch_merge_wants_real_merge(cli: &CheckoutMergeCli, args_merge_flag: bool) -> bool {
    if cli.merge_tri == 1 {
        return true;
    }
    if cli.merge_tri == 0 {
        return false;
    }
    args_merge_flag
}

fn effective_path_checkout_merge(cli: &CheckoutMergeCli, args_merge_flag: bool) -> bool {
    if cli.merge_tri == 0 {
        return false;
    }
    if cli.merge_tri == 1 || args_merge_flag {
        return true;
    }
    cli.conflict_style.is_some()
}

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
    static RECURSE_SUBMODULES: Cell<bool> = const { Cell::new(false) };
}

/// Set whether the current `checkout` invocation should recurse into submodules after updating
/// the superproject (used by `run` only).
pub fn set_checkout_recurse_submodules(v: bool) {
    RECURSE_SUBMODULES.set(v);
}

/// Print to stderr unless quiet mode is enabled.
macro_rules! checkout_eprintln {
    ($($arg:tt)*) => {
        QUIET.with(|q| {
            if !q.get() {
                eprintln!($($arg)*);
            }
        })
    };
}

pub fn run(mut args: Args) -> Result<()> {
    QUIET.with(|q| q.set(args.quiet));
    RECURSE_SUBMODULES.with(|r| r.set(args.recurse_submodules));
    let repo = Repository::discover(None).context("not a git repository")?;
    let raw_args: Vec<String> = std::env::args().collect();
    let merge_cli = parse_checkout_merge_flags_from_raw(&raw_args);
    validate_checkout_conflict_arg(&raw_args)?;

    // Validate --pathspec-file-nul requires --pathspec-from-file
    if args.pathspec_file_nul && args.pathspec_from_file.is_none() {
        bail!("the option '--pathspec-file-nul' requires '--pathspec-from-file'");
    }

    // Read pathspecs from file if --pathspec-from-file is given
    if let Some(ref file) = args.pathspec_from_file.clone() {
        // Conflict checks
        if args.detach {
            bail!("options '--pathspec-from-file' and '--detach' cannot be used together");
        }
        if args.patch {
            bail!("options '--pathspec-from-file' and '--patch' cannot be used together");
        }
        // Check for explicit pathspec arguments (after -- separator or if has_separator already)
        // We detect this by checking the raw args for an explicit -- followed by paths
        {
            let has_sep = raw_args.iter().any(|a| a == "--");
            if has_sep {
                let sep_idx = raw_args
                    .iter()
                    .position(|a| a == "--")
                    .unwrap_or(raw_args.len());
                if sep_idx + 1 < raw_args.len() {
                    bail!("'--pathspec-from-file' and pathspec arguments cannot be used together");
                }
            }
        }
        let content = if file == "-" {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
                .context("reading stdin")?;
            s
        } else {
            std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?
        };
        let sep = if args.pathspec_file_nul { b'\0' } else { b'\n' };
        let pathspecs_raw: Vec<String> = content
            .split(|c: char| c as u8 == sep)
            .map(|s| s.trim_end_matches('\r').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        // With --pathspec-file-nul, C-quoting is incompatible — fail if quoted
        if args.pathspec_file_nul {
            for p in &pathspecs_raw {
                if p.trim().starts_with('"') {
                    bail!("pathspec-from-file: line is not NUL terminated: {}", p);
                }
            }
        }
        let pathspecs: Vec<String> = if args.pathspec_file_nul {
            pathspecs_raw
        } else {
            pathspecs_raw
                .into_iter()
                .map(|s| unquote_c_pathspec(&s))
                .collect()
        };
        // Append to existing rest args
        args.rest.extend(pathspecs);
    }

    if grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir)) {
        for r in &mut args.rest {
            *r = grit_lib::unicode_normalization::precompose_utf8_path(r).into_owned();
        }
    }

    // Post-process rest: extract -b/-B/--new-branch/--force-new-branch that
    // appeared after a positional arg (e.g. `checkout <rev> -b <branch>`).
    // Also accept `git switch` spellings: -c/--create and -C/--force-create
    // (switch delegates here with those flags in `rest`).
    // clap's trailing_var_arg consumes these as raw strings when allow_hyphen_values=true.
    {
        let mut new_rest: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.rest.len() {
            let s = &args.rest[i];
            let is_force_create =
                s == "-B" || s == "--force-new-branch" || s == "-C" || s == "--force-create";
            let is_create =
                s == "-b" || s == "--new-branch" || s == "-c" || s == "--create" || is_force_create;
            if is_create
                && args.new_branch.is_none()
                && args.force_branch.is_none()
                && i + 1 < args.rest.len()
            {
                let bname = args.rest[i + 1].clone();
                if is_force_create {
                    args.force_branch = Some(bname);
                } else {
                    args.new_branch = Some(bname);
                }
                i += 2;
                continue;
            }
            new_rest.push(s.clone());
            i += 1;
        }
        args.rest = new_rest;
    }

    // Clap can populate `new_branch` / `force_branch` while still leaving the branch name as the
    // first trailing arg, so `rest` is `[name, start]` instead of `[start]` (t1507).
    if let Some(ref nb) = args.new_branch {
        if args.rest.len() >= 2 && args.rest[0] == *nb {
            args.rest.remove(0);
        } else if args.rest.len() >= 3 {
            let dup = matches!(
                args.rest[0].as_str(),
                "-b" | "--new-branch" | "-c" | "--create"
            ) && args.rest[1] == *nb;
            if dup {
                args.rest.drain(0..2);
            }
        }
    }
    if let Some(ref nb) = args.force_branch {
        if args.rest.len() >= 2 && args.rest[0] == *nb {
            args.rest.remove(0);
        } else if args.rest.len() >= 3 {
            let dup = matches!(
                args.rest[0].as_str(),
                "-B" | "--force-new-branch" | "-C" | "--force-create"
            ) && args.rest[1] == *nb;
            if dup {
                args.rest.drain(0..2);
            }
        }
    }

    // `git switch` forwards `--orphan`, `--detach`, and `-d` via positional `rest`;
    // clap does not parse them there, so peel them off before treating `rest` as refs/paths.
    {
        let mut new_rest: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.rest.len() {
            let s = &args.rest[i];
            if s == "--orphan" && args.orphan.is_none() && i + 1 < args.rest.len() {
                args.orphan = Some(args.rest[i + 1].clone());
                i += 2;
                continue;
            }
            if (s == "--detach" || s == "-d") && !args.detach {
                args.detach = true;
                i += 1;
                continue;
            }
            if (s == "-f" || s == "--force") && !args.force {
                args.force = true;
                i += 1;
                continue;
            }
            new_rest.push(s.clone());
            i += 1;
        }
        args.rest = new_rest;
    }

    // After peeling `-f` / `--force` from `rest` (e.g. `switch --discard-changes` → `-f`).
    let branch_merge_wanted = effective_branch_merge_wants_real_merge(&merge_cli, args.merge);
    let path_merge_wanted = effective_path_checkout_merge(&merge_cli, args.merge);
    let switch_force = args.force || branch_merge_wanted;

    // `git switch -C branch -q` / `checkout -b x -q` pass `-q` as a trailing (or middle) positional.
    // Remove every `-q` / `--quiet` from `rest` so they are never parsed as a start-point or path.
    {
        let mut filtered: Vec<String> = Vec::with_capacity(args.rest.len());
        for s in std::mem::take(&mut args.rest) {
            if s == "-q" || s == "--quiet" {
                args.quiet = true;
                QUIET.with(|q| q.set(true));
                continue;
            }
            filtered.push(s);
        }
        args.rest = filtered;
    }

    // Detect if `--` was used in the original command line. Clap strips a
    // leading `--` from trailing_var_arg, so we check the raw args.
    let has_separator = raw_args.iter().any(|a| a == "--");
    // Determine if `--` is at the end of raw_args (after all positional args).
    let separator_at_end = has_separator && raw_args.last().map(|s| s.as_str()) == Some("--");

    // When `--` is present, count how many args appear before it.
    // If there are 2+ refs before `--`, that's an error.
    if has_separator {
        let args_before_sep = if let Some(sep) = args.rest.iter().position(|a| a == "--") {
            sep
        } else if separator_at_end {
            args.rest.len()
        } else {
            0
        };
        if args_before_sep > 1 {
            bail!(
                "fatal: only one reference expected, {} given.",
                args_before_sep
            );
        }
    }

    // Parse rest into (target, paths) handling `--` separator
    let (mut target, mut paths) = split_target_and_paths(
        &args.rest,
        has_separator,
        separator_at_end,
        args.switch_mode,
    );

    if args.track.is_some() && args.new_branch.is_none() && paths.is_empty() {
        let argv0 = target.as_deref().unwrap_or("");
        if argv0.is_empty() || argv0 == "--" {
            bail!("fatal: --track needs a branch name");
        }
        let mut a = argv0;
        if let Some(r) = a.strip_prefix("refs/") {
            a = r;
        }
        if let Some(r) = a.strip_prefix("remotes/") {
            a = r;
        }
        let has_slash = a.contains('/');
        if !has_slash {
            bail!("fatal: missing branch name; try -b");
        }
    }

    if args.ours || args.theirs {
        if paths.is_empty() {
            if let Some(ref t) = target {
                let is_branch_switch = t == "HEAD"
                    || t == "@"
                    || refs::resolve_ref(&repo.git_dir, &format!("refs/heads/{t}")).is_ok();
                if is_branch_switch {
                    bail!("fatal: '--ours/--theirs' cannot be used with switching branches");
                }
                paths.push(t.clone());
                target = None;
            } else {
                bail!("fatal: option '--ours/--theirs' needs the paths to check out");
            }
        }
    }

    // Resolve @{-N} in start point if present
    let mut target = target.map(|t| resolve_at_minus(&repo, &t).unwrap_or(t));
    let mut paths = paths;

    // `checkout -f a b c` without `--`: when every token exists in the work tree, treat them all
    // as pathspecs (t7201 `checkout -f` on unmerged paths), not `tree-ish` + paths.
    if args.force && !has_separator && !args.detach {
        if let Some(wt) = repo.work_tree.as_deref() {
            let cwd = std::env::current_dir().unwrap_or_default();
            let mut combined: Vec<String> = Vec::new();
            if let Some(t) = target.clone() {
                combined.push(t);
            }
            combined.extend(paths.clone());
            if combined.len() >= 2 {
                let all_exist = combined.iter().all(|p| {
                    let rel = resolve_pathspec(p, wt, &cwd);
                    let abs = wt.join(&rel);
                    abs.exists() || abs.is_symlink()
                });
                if all_exist {
                    paths = combined;
                    target = None;
                }
            }
        }
    }

    // `checkout -b new start` copies tracking from `start` when it is an upstream expression
    // (e.g. `my-side@{u}`), even if `branch.autoSetupMerge` is not `always` (t1507).
    if let (Some(raw_new_branch), Some(start)) = (args.new_branch.as_ref(), target.as_deref()) {
        if resolve_upstream_symbolic_name(&repo, start).is_ok() {
            let resolved_new_branch: String;
            let new_branch_name: &str = if raw_new_branch.starts_with("@{") {
                match refs::resolve_at_n_branch(&repo.git_dir, raw_new_branch) {
                    Ok(name) => {
                        resolved_new_branch = name;
                        &resolved_new_branch
                    }
                    Err(_) => raw_new_branch.as_str(),
                }
            } else {
                raw_new_branch.as_str()
            };
            if !paths.is_empty() || args.rest.len() > 1 {
                if args.track.is_some() {
                    bail!("'--track' cannot be used with updating paths");
                }
                bail!("Cannot update paths and switch to branch at the same time.");
            }
            let pre_head_branch = if args.track.is_some() {
                match resolve_head(&repo.git_dir) {
                    Ok(HeadState::Branch { short_name, .. }) => Some(short_name),
                    _ => None,
                }
            } else {
                None
            };
            let effective_target = Some(start).or(pre_head_branch.as_deref());
            let result = create_and_switch_branch(
                &repo,
                new_branch_name,
                Some(start),
                switch_force,
                args.create_reflog,
                args.track.as_deref().or(Some("direct")),
            );
            if result.is_ok() && !args.no_track {
                // Upstream start points must copy tracking even when `branch.autoSetupMerge` is not
                // `always` (Git behavior; t1507).
                let track = args.track.as_deref().or(Some("direct"));
                maybe_setup_tracking(&repo, new_branch_name, effective_target, track)?;
            }
            return result;
        }
    }

    // Case: checkout -p (interactive patch mode)
    // --patch and --overlay are incompatible
    if args.patch && args.overlay {
        eprintln!("fatal: options '-p' and '--overlay' cannot be used together");
        std::process::exit(1);
    }

    if args.patch {
        return checkout_patch(&repo, target.as_deref(), &paths);
    }

    // Case: checkout --orphan <name> [<start_point>]
    if let Some(ref orphan_name) = args.orphan {
        // Optional start point is the first non-`-q` token in `rest` (tests use
        // `checkout --orphan branch -q`, which would otherwise treat `-q` as
        // the start point).
        let start_point = args
            .rest
            .iter()
            .find(|s| *s != "-q" && *s != "--quiet")
            .map(|s| s.as_str());
        return create_orphan_branch(
            &repo,
            orphan_name,
            start_point,
            CreateOrphanOptions {
                switch_style: false,
                force: args.force,
            },
        );
    }

    // Case: checkout -B <name> [<start_point>] (force create/reset)
    if let Some(ref force_branch_name) = args.force_branch {
        // -B takes at most one positional arg (start point)
        if !paths.is_empty() || args.rest.len() > 1 {
            bail!("too many arguments for -B");
        }
        let result = force_create_and_switch_branch(
            &repo,
            force_branch_name,
            target.as_deref(),
            args.force,
            args.create_reflog,
        );
        if result.is_ok() && !args.no_track {
            maybe_setup_tracking(
                &repo,
                force_branch_name,
                target.as_deref(),
                args.track.as_deref(),
            )?;
        }
        return result;
    }

    // Case 1: checkout -b <new_branch> [<start_point>]
    if let Some(ref raw_new_branch) = args.new_branch {
        // Resolve @{-N} syntax in branch name (e.g. `git checkout -b @{-1}`)
        let resolved_new_branch: String;
        let new_branch_name: &str = if raw_new_branch.starts_with("@{") {
            match refs::resolve_at_n_branch(&repo.git_dir, raw_new_branch) {
                Ok(name) => {
                    resolved_new_branch = name;
                    &resolved_new_branch
                }
                Err(_) => raw_new_branch.as_str(),
            }
        } else {
            raw_new_branch.as_str()
        };
        // -b takes at most one positional arg (start point)
        if !paths.is_empty() || args.rest.len() > 1 {
            if args.track.is_some() {
                bail!("'--track' cannot be used with updating paths");
            }
            bail!("Cannot update paths and switch to branch at the same time.");
        }
        // Capture the current HEAD branch before checkout (for tracking setup)
        let pre_head_branch = if target.is_none() && args.track.is_some() {
            match resolve_head(&repo.git_dir) {
                Ok(HeadState::Branch { short_name, .. }) => Some(short_name),
                _ => None,
            }
        } else {
            None
        };
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let auto = config
            .get("branch.autoSetupMerge")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut effective_target = target.as_deref().or(pre_head_branch.as_deref());
        if effective_target.is_none()
            && (auto == "always" || auto == "inherit")
            && args.track.is_none()
        {
            effective_target = Some("HEAD");
        }
        let result = create_and_switch_branch(
            &repo,
            new_branch_name,
            target.as_deref(),
            switch_force,
            args.create_reflog,
            args.track.as_deref(),
        );
        if result.is_ok() && !args.no_track {
            maybe_setup_tracking(
                &repo,
                new_branch_name,
                effective_target,
                args.track.as_deref(),
            )?;
        }
        return result;
    }

    // Case 2: checkout [<tree-ish>] -- <paths>  (path restore)
    // Not applicable when --detach is set (paths incompatible with --detach)
    if !paths.is_empty() && !args.detach {
        if !has_separator && !args.force {
            if let Some(ref t) = target {
                let is_rev = resolve_revision(&repo, t).is_ok()
                    || refs::resolve_ref(&repo.git_dir, &format!("refs/heads/{t}")).is_ok();
                let cwd = std::env::current_dir().unwrap_or_default();
                let is_path = cwd.join(t).exists();
                if is_rev && is_path {
                    bail!(
                        "fatal: ambiguous argument '{}': both revision and filename\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'",
                        t
                    );
                }
            }
        }
        return checkout_paths(
            &repo,
            target.as_deref(),
            &paths,
            args.no_overlay,
            path_merge_wanted,
            args.force,
            args.ours,
            args.theirs,
            &merge_cli,
        );
    }

    // `checkout .` without `--` is parsed as target "." with no paths; Git still restores from the
    // index (same as `checkout -- .`, t2080 filter / parallel-checkout report tests).
    if paths.is_empty() && !args.detach {
        if let Some(ref t) = target {
            if t == "." || t == "./" {
                return checkout_paths(
                    &repo,
                    None,
                    &[".".to_string()],
                    args.no_overlay,
                    path_merge_wanted,
                    args.force,
                    args.ours,
                    args.theirs,
                    &merge_cli,
                );
            }
        }
    }

    // Case: checkout -f (no args) — force reset working tree to HEAD
    if args.force && target.is_none() && paths.is_empty() {
        return force_reset_to_head(&repo);
    }

    // Case 3: checkout -- (with no paths and no target) is a no-op
    // Case 4: checkout <branch-or-commit>
    let target = match target {
        Some(t) if t.is_empty() => {
            bail!("fatal: empty string is not a valid pathspec or branch name")
        }
        Some(t) => t,
        None => {
            if args.detach {
                // `checkout --detach` with no target: detach at current HEAD
                match resolve_head(&repo.git_dir)? {
                    HeadState::Branch { oid: Some(oid), .. } | HeadState::Detached { oid } => {
                        return detach_head(&repo, &oid, switch_force);
                    }
                    _ => bail!("cannot detach HEAD on unborn branch"),
                }
            }
            bail!("you must specify a branch, commit, or paths to checkout")
        }
    };

    // Handle @{-N} syntax: Nth previously checked out branch
    if target.starts_with("@{-") && target.ends_with('}') {
        if let Ok(n) = target[3..target.len() - 1].parse::<usize>() {
            let prev = resolve_nth_previous_branch(&repo, n)?;
            let branch_ref = format!("refs/heads/{prev}");
            if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
                return switch_branch(
                    &repo,
                    &prev,
                    &branch_ref,
                    switch_force,
                    args.ignore_other_worktrees,
                    branch_merge_wanted,
                    &merge_cli,
                );
            }
            if let Ok(oid) = resolve_to_commit(&repo, &prev) {
                return detach_head(&repo, &oid, switch_force);
            }
            bail!("error: previous branch '{}' not found", prev);
        }
    }

    // Handle "checkout -" — switch to previous branch via reflog
    if target == "-" {
        let prev = resolve_previous_branch(&repo)?;
        let branch_ref = format!("refs/heads/{prev}");
        if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
            return switch_branch(
                &repo,
                &prev,
                &branch_ref,
                switch_force,
                args.ignore_other_worktrees,
                branch_merge_wanted,
                &merge_cli,
            );
        }
        // Not a branch — try as a commit (detached HEAD)
        if let Ok(oid) = resolve_to_commit(&repo, &prev) {
            return detach_head(&repo, &oid, switch_force);
        }
        bail!("error: previous branch '{}' not found", prev);
    }

    // Handle "checkout HEAD" (and "@") — no-op when on a branch (don't detach)
    // But with -f, force-reset the working tree
    if (target == "HEAD" || target == "@") && !args.detach {
        if args.force {
            return force_reset_to_head(&repo);
        }
        return Ok(());
    }

    // If --detach, force detached HEAD even for branch names
    if args.detach {
        // --detach takes at most one argument (no extra paths)
        if !paths.is_empty() || args.rest.len() > 1 {
            bail!("--detach does not take a path argument");
        }
        match resolve_to_commit(&repo, &target) {
            Ok(oid) => return detach_head_explicit(&repo, &oid, switch_force),
            Err(e) => bail!("cannot detach HEAD at '{}': {}", target, e),
        }
    }

    // `checkout other@{u}` / `checkout @{u}` — switch to the configured upstream branch (local
    // branch or detach at remote-tracking tip), matching Git.
    if !args.detach && upstream_suffix_info(&target).is_some() {
        let full = resolve_upstream_symbolic_name(&repo, &target)
            .with_context(|| format!("unknown upstream revision '{target}'"))?;
        if let Some(rest) = full.strip_prefix("refs/heads/") {
            let branch_ref = format!("refs/heads/{rest}");
            return switch_branch(
                &repo,
                rest,
                &branch_ref,
                switch_force,
                args.ignore_other_worktrees,
                branch_merge_wanted,
                &merge_cli,
            );
        }
        if full.strip_prefix("refs/remotes/").is_some() {
            let oid = refs::resolve_ref(&repo.git_dir, &full)
                .with_context(|| format!("cannot resolve '{full}'"))?;
            return detach_head(&repo, &oid, switch_force);
        }
        bail!("cannot checkout upstream: unsupported ref '{full}'");
    }

    // `checkout --track origin/topic` (without `-b`): create local branch named `topic` (path
    // after the first `/` in the remote refspec) tracking the remote-tracking ref (t7201).
    if !args.detach && args.track.is_some() && args.new_branch.is_none() && paths.is_empty() {
        let mut spec = target.as_str();
        if let Some(s) = spec.strip_prefix("refs/remotes/") {
            spec = s;
        } else if let Some(s) = spec.strip_prefix("remotes/") {
            spec = s;
        }
        if let Some(slash) = spec.find('/') {
            let remote = &spec[..slash];
            let branch_on_remote = &spec[slash + 1..];
            if !remote.is_empty() && !branch_on_remote.is_empty() {
                let tracking = format!("refs/remotes/{remote}/{branch_on_remote}");
                if let Ok(oid) = refs::resolve_ref(&repo.git_dir, &tracking) {
                    let local_branch = branch_on_remote.to_string();
                    let local_ref = format!("refs/heads/{local_branch}");
                    if refs::resolve_ref(&repo.git_dir, &local_ref).is_ok() {
                        bail!("fatal: a branch named '{local_branch}' already exists");
                    }
                    refs::write_ref(&repo.git_dir, &local_ref, &oid)?;
                    let cfg_path = repo.git_dir.join("config");
                    let mut cfg_content = std::fs::read_to_string(&cfg_path).unwrap_or_default();
                    let section = format!(
                        "\n[branch \"{local_branch}\"]\
\n\tremote = {remote}\
\n\tmerge = refs/heads/{branch_on_remote}\n"
                    );
                    cfg_content.push_str(&section);
                    let _ = std::fs::write(&cfg_path, cfg_content);
                    checkout_eprintln!(
                        "branch '{local_branch}' set up to track '{remote}/{branch_on_remote}'."
                    );
                    return switch_branch(
                        &repo,
                        &local_branch,
                        &local_ref,
                        switch_force,
                        args.ignore_other_worktrees,
                        branch_merge_wanted,
                        &merge_cli,
                    );
                }
            }
        }
    }

    // Try as a branch first
    let branch_ref = format!("refs/heads/{target}");
    if !args.detach && refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        // Warn if a tag with the same name also exists (ambiguous ref)
        let tag_ref = format!("refs/tags/{target}");
        if refs::resolve_ref(&repo.git_dir, &tag_ref).is_ok() {
            eprintln!("warning: refname '{}' is ambiguous.", target);
        }
        return switch_branch(
            &repo,
            &target,
            &branch_ref,
            switch_force,
            args.ignore_other_worktrees,
            branch_merge_wanted,
            &merge_cli,
        );
    }

    // `checkout tags/<name>` — explicit tag ref (t7201 ambiguous tag vs branch).
    if !args.detach && (target.starts_with("tags/") || target.starts_with("refs/tags/")) {
        let tag_name = target
            .strip_prefix("refs/tags/")
            .unwrap_or_else(|| target.strip_prefix("tags/").unwrap_or(&target));
        let tag_ref = format!("refs/tags/{tag_name}");
        if let Ok(oid) = refs::resolve_ref(&repo.git_dir, &tag_ref) {
            return detach_head(&repo, &oid, switch_force);
        }
    }

    // DWIM: if branch doesn't exist locally, check if exactly one remote has it
    // Skip if --no-guess or checkout.guess=false
    let dwim_enabled = !args.no_guess && {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        config
            .get("checkout.guess")
            .map(|v| v != "false")
            .unwrap_or(true)
    };
    if !args.detach && dwim_enabled {
        let remote_prefix = "refs/remotes/";
        let all_remote_refs = refs::list_refs(&repo.git_dir, remote_prefix).unwrap_or_default();
        let matching: Vec<(String, ObjectId)> = all_remote_refs
            .into_iter()
            .filter(|(r, _)| {
                // refs/remotes/<remote>/<branch>
                let parts: Vec<&str> = r.trim_start_matches(remote_prefix).splitn(2, '/').collect();
                parts.len() == 2 && parts[1] == target
            })
            .collect();
        if matching.len() == 1 {
            let remote_ref = &matching[0].0;
            let oid = matching[0].1;
            // Extract remote name from refs/remotes/<remote>/<branch>
            let remote_part = remote_ref.trim_start_matches(remote_prefix);
            let remote_name = remote_part.split('/').next().unwrap_or("");
            // Create the local branch tracking the remote
            let new_branch_ref = format!("refs/heads/{target}");
            refs::write_ref(&repo.git_dir, &new_branch_ref, &oid)?;
            // Set up tracking configuration
            let cfg_path = repo.git_dir.join("config");
            let mut cfg_content = std::fs::read_to_string(&cfg_path).unwrap_or_default();
            let section = format!(
                "\n[branch \"{}\"]\
\n\tremote = {}\
\n\tmerge = refs/heads/{}\n",
                target, remote_name, target
            );
            cfg_content.push_str(&section);
            let _ = std::fs::write(&cfg_path, cfg_content);
            eprintln!("branch '{target}' set up to track '{remote_name}/{target}'.");
            return switch_branch(
                &repo,
                &target,
                &new_branch_ref,
                switch_force,
                args.ignore_other_worktrees,
                branch_merge_wanted,
                &merge_cli,
            );
        } else if matching.len() > 1 {
            eprintln!(
                "hint: If you meant to check out a remote tracking branch on, e.g. 'origin',"
            );
            eprintln!("hint: try again with the --track option:");
            eprintln!("hint:");
            for (r, _) in &matching {
                let remote_part = r.trim_start_matches(remote_prefix);
                let mut parts = remote_part.splitn(2, '/');
                let rname = parts.next().unwrap_or("");
                let bname = parts.next().unwrap_or("");
                eprintln!("hint:     git checkout --track {rname}/{bname}");
            }
            eprintln!("hint:");
            bail!(
                "'{target}' matched multiple (\'{}\') remote tracking branches",
                matching.len()
            );
        }
    }

    // Try as a commit (detached HEAD)
    match resolve_to_commit(&repo, &target) {
        Ok(oid) => detach_head(&repo, &oid, switch_force),
        Err(_) => {
            // Fallback: try as a pathspec (git checkout <file> without --).
            // If the target looks like a tracked file, restore it from HEAD.
            let paths = vec![target.clone()];
            match checkout_paths(
                &repo,
                None,
                &paths,
                false,
                path_merge_wanted,
                args.force,
                args.ours,
                args.theirs,
                &merge_cli,
            ) {
                Ok(()) => Ok(()),
                Err(_) => bail!(
                    "pathspec '{}' did not match any file(s) known to git",
                    target
                ),
            }
        }
    }
}

/// Split positional arguments into (target, paths) around `--`.
///
/// `has_separator` indicates whether `--` appeared in the raw CLI args.
/// Clap strips the leading `--` when it is the first trailing arg, so we
/// need this external signal to distinguish `checkout -- file` from
/// `checkout file`.
/// C-unquote a pathspec entry from --pathspec-from-file.
/// Handles \ooo octal, \n, \t, etc. and strips surrounding quotes.
fn unquote_c_pathspec(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with('"') {
        return s.to_string();
    }
    let inner = s
        .strip_prefix('"')
        .unwrap_or(s)
        .strip_suffix('"')
        .unwrap_or(s.strip_prefix('"').unwrap_or(s));
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(d @ '0'..='7') => {
                // Octal escape: up to 3 digits
                let mut val = d as u32 - '0' as u32;
                for _ in 0..2 {
                    if let Some(&next) = chars.peek() {
                        if ('0'..='7').contains(&next) {
                            val = val * 8 + (next as u32 - '0' as u32);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                if let Some(ch) = char::from_u32(val) {
                    out.push(ch);
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {
                out.push('\\');
            }
        }
    }
    out
}

fn split_target_and_paths(
    rest: &[String],
    has_separator: bool,
    separator_at_end: bool,
    switch_mode: bool,
) -> (Option<String>, Vec<String>) {
    if rest.is_empty() {
        return (None, vec![]);
    }

    // Look for an explicit `--` still present in the args (happens when
    // there is a target before `--`, e.g. `checkout main -- file`).
    if let Some(sep) = rest.iter().position(|a| a == "--") {
        let target = if sep > 0 { Some(rest[0].clone()) } else { None };
        let paths = rest[sep + 1..].to_vec();
        return (target, paths);
    }

    // Clap stripped `--`.
    if has_separator {
        if rest.is_empty() {
            return (None, vec![]);
        }
        if separator_at_end {
            return (Some(rest[0].clone()), vec![]);
        }
        // `git switch -- <branch>`: a single token after `--` is always the branch name, even if
        // it matches a tracked path (`git switch -- file1.txt-branch`).
        if switch_mode && rest.len() == 1 {
            return (Some(rest[0].clone()), vec![]);
        }
        return (None, rest.to_vec());
    }

    if rest.len() == 1 {
        (Some(rest[0].clone()), vec![])
    } else {
        (Some(rest[0].clone()), rest[1..].to_vec())
    }
}

// ---------------------------------------------------------------------------
// Branch switching
// ---------------------------------------------------------------------------

/// Run Git's `post-checkout` hook: `<old-oid> <new-oid> <flag>` where `flag` is `1` for a branch
/// checkout and `0` for a path (file) checkout. Missing `old_oid` uses the null OID (clone-style).
///
/// # Errors
///
/// Returns an error when the hook exits with a non-zero status.
fn run_post_checkout_hook(
    repo: &Repository,
    old_oid: Option<&ObjectId>,
    new_oid: &ObjectId,
    is_branch_checkout: bool,
) -> Result<()> {
    let z = zero_oid();
    let old_hex = old_oid.unwrap_or(&z).to_hex();
    let new_hex = new_oid.to_hex();
    let flag = if is_branch_checkout { "1" } else { "0" };
    if let HookResult::Failed(code) = run_hook(
        repo,
        "post-checkout",
        &[old_hex.as_str(), new_hex.as_str(), flag],
        None,
    ) {
        bail!("post-checkout hook exited with status {code}");
    }
    Ok(())
}

fn collect_checkout_local_change_lines(repo: &Repository) -> Result<Vec<String>> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no work tree"))?;
    let index_path = repo.index_path();
    let index = repo
        .load_index_at(&index_path)
        .context("loading index for checkout local changes")?;
    let entries =
        diff_index_to_worktree(&repo.odb, &index, work_tree).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(entries
        .into_iter()
        .map(|e| format!("{}\t{}", e.status.letter(), e.path()))
        .collect())
}

fn sparse_checkout_config_enabled(git_dir: &std::path::Path) -> bool {
    ConfigSet::load(Some(git_dir), true)
        .unwrap_or_default()
        .get("core.sparsecheckout")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn refresh_index_blobs_from_worktree(
    repo: &Repository,
    index: &mut Index,
    work_tree: &Path,
) -> Result<()> {
    for entry in &mut index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if entry.mode == MODE_GITLINK {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let abs = work_tree.join(path_str.as_ref());
        if entry.mode == MODE_SYMLINK {
            let target = std::fs::read_link(&abs)
                .with_context(|| format!("readlink for merge refresh '{path_str}'"))?;
            let data = target.to_string_lossy().as_bytes().to_vec();
            entry.oid = repo
                .odb
                .write(ObjectKind::Blob, &data)
                .with_context(|| format!("writing symlink blob for '{path_str}'"))?;
            continue;
        }
        let data = std::fs::read(&abs)
            .with_context(|| format!("reading '{path_str}' for merge refresh"))?;
        entry.oid = repo
            .odb
            .write(ObjectKind::Blob, &data)
            .with_context(|| format!("writing blob for '{path_str}'"))?;
    }
    Ok(())
}

fn index_has_staged_changes_vs_tree(
    repo: &Repository,
    index: &Index,
    head_tree_oid: &ObjectId,
) -> Result<bool> {
    let head_entries = tree_to_flat_entries(repo, head_tree_oid, "")?;
    let head_map: HashMap<Vec<u8>, ObjectId> =
        head_entries.into_iter().map(|e| (e.path, e.oid)).collect();
    for e in &index.entries {
        if e.stage() != 0 {
            continue;
        }
        let is_staged = match head_map.get(&e.path) {
            Some(h) => h != &e.oid,
            None => true,
        };
        if is_staged {
            return Ok(true);
        }
    }
    Ok(false)
}

fn checkout_merged_worktree_from_index(
    repo: &Repository,
    work_tree: &Path,
    old_index: &Index,
    index: &Index,
    conflict_content: &std::collections::BTreeMap<Vec<u8>, ObjectId>,
) -> Result<()> {
    fn same_blob(a: &IndexEntry, b: &IndexEntry) -> bool {
        a.oid == b.oid && a.mode == b.mode
    }
    let old_stage0: HashMap<Vec<u8>, &IndexEntry> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.clone(), e))
        .collect();
    let new_paths: HashSet<Vec<u8>> = index.entries.iter().map(|e| e.path.clone()).collect();
    for entry in &old_index.entries {
        if entry.stage() == 0 && !new_paths.contains(&entry.path) {
            let path_str = String::from_utf8_lossy(&entry.path).into_owned();
            let abs_path = work_tree.join(&path_str);
            if abs_path.exists() || abs_path.is_symlink() {
                let _ = std::fs::remove_file(&abs_path);
                remove_empty_parent_dirs(work_tree, &abs_path);
            }
        }
    }
    let mut written = HashSet::new();
    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        if entry.stage() == 0 {
            if let Some(prev) = old_stage0.get(&entry.path) {
                if same_blob(prev, entry) {
                    written.insert(entry.path.clone());
                    continue;
                }
            }
            write_blob_to_worktree(
                repo, work_tree, &path_str, &entry.oid, entry.mode, index, false,
            )?;
            written.insert(entry.path.clone());
        } else if entry.stage() == 2 && !written.contains(&entry.path) {
            if let Some(marker_oid) = conflict_content.get(&entry.path) {
                let mut marker_entry = entry.clone();
                marker_entry.oid = *marker_oid;
                write_blob_to_worktree(
                    repo,
                    work_tree,
                    &path_str,
                    &marker_entry.oid,
                    marker_entry.mode,
                    index,
                    false,
                )?;
            } else {
                write_blob_to_worktree(
                    repo, work_tree, &path_str, &entry.oid, entry.mode, index, false,
                )?;
            }
            written.insert(entry.path.clone());
        }
    }
    Ok(())
}

fn merge_branch_working_tree(
    repo: &Repository,
    head: &HeadState,
    new_commit_oid: &ObjectId,
    force: bool,
    presentation: TreeMergeConflictPresentation<'_>,
    recurse_submodules: bool,
) -> Result<()> {
    if force {
        let target_tree = commit_to_tree(repo, new_commit_oid)?;
        return switch_to_tree(repo, head, &target_tree, true, recurse_submodules);
    }

    let index_path = repo.index_path();
    let index_before = repo.load_index_at(&index_path).context("loading index")?;
    for e in &index_before.entries {
        if e.stage() != 0 {
            bail!("you need to resolve your current index first");
        }
    }

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let Some(old_oid) = head.oid().copied() else {
        let target_tree = commit_to_tree(repo, new_commit_oid)?;
        return switch_to_tree(repo, head, &target_tree, false, recurse_submodules);
    };

    let new_tree_oid = commit_to_tree(repo, new_commit_oid)?;
    match switch_to_tree(repo, head, &new_tree_oid, false, recurse_submodules) {
        Ok(()) => return Ok(()),
        Err(_) => {}
    }

    let old_tree_oid = commit_to_tree(repo, &old_oid)?;
    if index_has_staged_changes_vs_tree(repo, &index_before, &old_tree_oid)? {
        let mut sb = String::new();
        for e in &index_before.entries {
            if e.stage() != 0 {
                continue;
            }
            let path_str = String::from_utf8_lossy(&e.path);
            let rel = path_str.as_ref();
            let _abs = work_tree.join(rel);
            let in_head = tree_to_flat_entries(repo, &old_tree_oid, "")?
                .into_iter()
                .find(|x| x.path == e.path);
            let head_oid = in_head.as_ref().map(|x| x.oid);
            let is_staged = match head_oid {
                Some(h) => h != e.oid,
                None => true,
            };
            if !is_staged {
                continue;
            }
            let new_in_target = tree_to_flat_entries(repo, &new_tree_oid, "")?
                .into_iter()
                .find(|x| x.path == e.path);
            let target_changes = match (head_oid, new_in_target.as_ref()) {
                (Some(h), Some(ne)) => ne.oid != h,
                (Some(_), None) => true,
                (None, Some(_)) => true,
                (None, None) => false,
            };
            if !target_changes {
                continue;
            }
            if !sb.is_empty() {
                sb.push('\n');
            }
            sb.push_str(rel);
        }
        if !sb.is_empty() {
            bail!(
                "cannot continue with staged changes in the following files:\n{}",
                sb
            );
        }
    }

    let mut index_for_work_tree = index_before.clone();
    refresh_index_blobs_from_worktree(repo, &mut index_for_work_tree, work_tree)?;
    let work_tree_oid = write_tree_from_index(&repo.odb, &index_for_work_tree, "")
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Match Git `merge_ort_nonrecursive`: base = old branch, head = destination (`new`), merge =
    // local work tree. In `merge_trees_three_way` that is `(base, ours, theirs) = (old, new, work)`.
    let merged = merge_trees_three_way(
        repo,
        old_tree_oid,
        new_tree_oid,
        work_tree_oid,
        MergeFavor::None,
        WhitespaceMergeOptions::default(),
        presentation,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    switch_to_tree(repo, head, &new_tree_oid, true, recurse_submodules)?;

    let mut merged_index = merged.index;
    // Compare against the pre-merge index (still reflects the branch we came from), not the
    // post-`switch_to_tree` index (which matches the destination and would skip writes when the
    // merge result OID equals the tip).
    checkout_merged_worktree_from_index(
        repo,
        work_tree,
        &Index::new(),
        &merged_index,
        &merged.conflict_content,
    )?;

    for entry in &mut merged_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let abs = work_tree.join(path_str.as_ref());
        if let Ok(meta) = std::fs::symlink_metadata(&abs) {
            use std::os::unix::fs::MetadataExt as _;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.size = meta.size() as u32;
        }
    }
    repo.write_index_at(&index_path, &mut merged_index)
        .context("writing index after merge checkout")?;

    if recurse_submodules {
        recurse_submodules_after_checkout(repo)?;
    }
    Ok(())
}

/// Switch HEAD to an existing branch, updating the working tree and index.
fn switch_branch(
    repo: &Repository,
    branch_name: &str,
    branch_ref: &str,
    force: bool,
    ignore_other_worktrees: bool,
    branch_merge: bool,
    merge_cli: &CheckoutMergeCli,
) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    // Fail gracefully when HEAD is corrupt (empty or garbage)
    if matches!(head, HeadState::Invalid) {
        bail!("fatal: invalid HEAD - your HEAD file may be corrupt");
    }

    // Check if already on this branch (must come BEFORE branch-in-use check)
    if let HeadState::Branch { ref refname, .. } = head {
        if refname == branch_ref {
            checkout_eprintln!("Already on '{}'", branch_name);
            if force {
                let target_oid = refs::resolve_ref(&repo.git_dir, branch_ref)
                    .with_context(|| format!("cannot resolve branch '{branch_name}'"))?;
                let target_tree = commit_to_tree(repo, &target_oid)?;
                return force_reset_to_tree(repo, &target_tree);
            }
            let target_oid = refs::resolve_ref(&repo.git_dir, branch_ref)
                .with_context(|| format!("cannot resolve branch '{branch_name}'"))?;
            let target_tree = commit_to_tree(repo, &target_oid)?;
            let index_empty = repo
                .load_index()
                .map(|idx| idx.entries.is_empty())
                .unwrap_or(true);
            let sparse_on = sparse_checkout_config_enabled(&repo.git_dir);
            if sparse_on || index_empty || !index_matches_flat_tree(repo, &target_tree)? {
                switch_to_tree(
                    repo,
                    &head,
                    &target_tree,
                    false,
                    RECURSE_SUBMODULES.with(|r| r.get()),
                )?;
            }
            let tip = head
                .oid()
                .copied()
                .ok_or_else(|| anyhow::anyhow!("HEAD has no commit"))?;
            run_post_checkout_hook(repo, Some(&tip), &tip, true)?;
            QUIET.with(|q| {
                if !q.get() {
                    if let Ok(s) =
                        format_tracking_info(repo, branch_name, AheadBehindMode::Full, true)
                    {
                        if !s.is_empty() {
                            eprintln!("{}", s.trim_end_matches('\n'));
                        }
                    }
                }
            });
            return Ok(());
        }
    }

    // Check if branch is already checked out in another worktree
    if !force && !ignore_other_worktrees {
        let common = refs::common_dir(&repo.git_dir).unwrap_or_else(|| repo.git_dir.clone());
        let worktrees_dir = common.join("worktrees");
        if worktrees_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&worktrees_dir) {
                for entry in entries.flatten() {
                    let admin = entry.path();
                    if !admin.is_dir() {
                        continue;
                    }
                    // Skip the current worktree (admin = current git_dir)
                    if admin.canonicalize().unwrap_or(admin.clone())
                        == repo.git_dir.canonicalize().unwrap_or(repo.git_dir.clone())
                    {
                        continue;
                    }
                    let wt_head = admin.join("HEAD");
                    // Read HEAD content, following symlinks properly
                    let head_content = if wt_head.is_symlink() {
                        // Symlink HEAD: the link target IS the branch ref string
                        std::fs::read_link(&wt_head)
                            .ok()
                            .map(|t| format!("ref: {}", t.to_string_lossy()))
                            .or_else(|| std::fs::read_to_string(&wt_head).ok())
                    } else {
                        std::fs::read_to_string(&wt_head).ok()
                    };
                    if let Some(content) = head_content {
                        let content = content.trim();
                        if let Some(refname) = content.strip_prefix("ref: ") {
                            if refname.trim() == branch_ref {
                                // Also get the worktree path
                                let gitdir_file = admin.join("gitdir");
                                let wt_path = if let Ok(raw) = std::fs::read_to_string(&gitdir_file)
                                {
                                    let p = std::path::Path::new(raw.trim());
                                    p.parent().unwrap_or(p).to_string_lossy().to_string()
                                } else {
                                    entry.file_name().to_string_lossy().to_string()
                                };
                                bail!("fatal: '{branch_name}' is already used by worktree at '{wt_path}'");
                            }
                        }
                    }
                }
            }
        }
    }

    let target_oid = refs::resolve_ref(&repo.git_dir, branch_ref)
        .with_context(|| format!("cannot resolve branch '{branch_name}'"))?;

    let old_head_commit = head.oid().copied();

    // If target commit is the same as current HEAD, just re-attach
    // without touching the working tree or index (preserves dirty state).
    // But with -f, always rebuild. With sparse checkout, re-run so edits to
    // `info/sparse-checkout` take effect (t1090).
    let already_at_target = head.oid() == Some(&target_oid);
    let sparse_on = sparse_checkout_config_enabled(&repo.git_dir);
    let recurse = RECURSE_SUBMODULES.with(|r| r.get());
    if !already_at_target || force || sparse_on {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let mut style = match config
            .get("merge.conflictstyle")
            .as_deref()
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("diff3" | "zdiff3") => ConflictStyle::Diff3,
            _ => ConflictStyle::Merge,
        };
        if let Some(s) = merge_cli.conflict_style {
            style = s;
        }
        if merge_cli.force_two_way_markers {
            style = ConflictStyle::Merge;
        }
        let old_label = match &head {
            HeadState::Branch { short_name, .. } => short_name.clone(),
            HeadState::Detached { oid } => abbreviate_object_id(repo, *oid, 7)
                .unwrap_or_else(|_| oid.to_hex()[..7].to_string()),
            _ => "ancestor".to_string(),
        };
        let presentation = TreeMergeConflictPresentation {
            label_ours: branch_name,
            label_theirs: TheirsConflictLabel::Fixed("local"),
            label_base: old_label.as_str(),
            style,
        };

        if branch_merge && !already_at_target {
            merge_branch_working_tree(repo, &head, &target_oid, force, presentation, recurse)?;
        } else {
            let target_tree = commit_to_tree(repo, &target_oid)?;
            switch_to_tree(repo, &head, &target_tree, force, recurse)?;
        }
    }

    // Write reflog entries before updating HEAD
    let old_oid = old_head_commit.unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let msg = format!("checkout: moving from {} to {}", from_desc, branch_name);
    write_checkout_reflog(repo, &head, &old_oid, &target_oid, &msg);

    // Update HEAD to point to the branch
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    run_post_checkout_hook(repo, old_head_commit.as_ref(), &target_oid, true)?;

    checkout_eprintln!("Switched to branch '{}'", branch_name);
    if !QUIET.with(|q| q.get()) {
        if let Ok(lines) = collect_checkout_local_change_lines(repo) {
            for line in lines {
                println!("{line}");
            }
        }
    }
    QUIET.with(|q| {
        if !q.get() {
            if let Ok(s) = format_tracking_info(repo, branch_name, AheadBehindMode::Full, true) {
                if !s.is_empty() {
                    eprintln!("{}", s.trim_end_matches('\n'));
                }
            }
        }
    });
    Ok(())
}

/// Reject branch names that cannot exist as `refs/heads/<name>` (matches `git switch -c`).
fn validate_track_start_for_new_branch(repo: &Repository, start: Option<&str>) -> Result<()> {
    let spec = start.unwrap_or("HEAD");
    if spec == "HEAD" {
        if !matches!(resolve_head(&repo.git_dir)?, HeadState::Branch { .. }) {
            bail!(
                "fatal: cannot set up tracking information; starting point 'HEAD' is not a branch"
            );
        }
        return Ok(());
    }
    if spec.contains("@{") || spec.contains("...") {
        return Ok(());
    }
    let head_ref = format!("refs/heads/{spec}");
    if refs::resolve_ref(&repo.git_dir, &head_ref).is_ok() {
        return Ok(());
    }
    let remote_prefix = "refs/remotes/";
    let has_remote = refs::list_refs(&repo.git_dir, remote_prefix)
        .unwrap_or_default()
        .into_iter()
        .any(|(r, _)| {
            r.strip_prefix(remote_prefix)
                .is_some_and(|rest| rest == spec || rest.ends_with(&format!("/{spec}")))
        });
    if has_remote {
        return Ok(());
    }
    if spec.contains('/') || spec.starts_with("refs/") {
        return Ok(());
    }
    let tag_ref = format!("refs/tags/{spec}");
    if refs::resolve_ref(&repo.git_dir, &tag_ref).is_ok() {
        bail!("fatal: cannot set up tracking information; starting point '{spec}' is not a branch");
    }
    Ok(())
}

fn validate_new_branch_name(name: &str) -> Result<()> {
    let full = format!("refs/heads/{name}");
    let opts = RefNameOptions {
        allow_onelevel: true,
        ..Default::default()
    };
    if check_refname_format(&full, &opts).is_err() {
        bail!("'{name}' is not a valid branch name");
    }
    Ok(())
}

/// Create a new branch and switch to it.
fn create_and_switch_branch(
    repo: &Repository,
    name: &str,
    start: Option<&str>,
    force: bool,
    force_branch_reflog: bool,
    track_mode: Option<&str>,
) -> Result<()> {
    validate_new_branch_name(name)?;
    if track_mode.is_some() {
        validate_track_start_for_new_branch(repo, start)?;
    }
    // Check for HEAD.lock (another process is writing)
    let head_lock = repo.git_dir.join("HEAD.lock");
    if head_lock.exists() {
        bail!(
            "Unable to create '{}': The file exists.",
            head_lock.display()
        );
    }

    // Check the branch doesn't already exist
    let branch_ref = format!("refs/heads/{name}");
    if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        eprintln!("fatal: a branch named '{name}' already exists");
        std::process::exit(128);
    }

    // Resolve start point (default: HEAD)
    let head = resolve_head(&repo.git_dir)?;
    let old_head_commit = head.oid().copied();
    let start_oid = match start {
        Some(s) => {
            if s != "HEAD"
                && !s.contains('/')
                && !s.starts_with("refs/")
                && !s.starts_with("remotes/")
            {
                reject_ambiguous_short_ref(repo, s)?;
            }
            resolve_to_commit(repo, s)?
        }
        None => {
            match head.oid() {
                Some(oid) => *oid,
                None => {
                    // Unborn branch: just switch HEAD to the new branch name
                    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;
                    checkout_eprintln!("Switched to a new branch '{}'", name);
                    return Ok(());
                }
            }
        }
    };

    let target_tree = commit_to_tree(repo, &start_oid)?;

    // Update working tree if start point differs from current HEAD, or if force,
    // or if the worktree is empty (e.g. after clone --no-checkout)
    let worktree_is_empty = if let Some(ref _wt) = repo.work_tree {
        let old_idx = repo.load_index().unwrap_or_default();
        old_idx.entries.is_empty()
    } else {
        false
    };
    if head.oid() != Some(&start_oid) || force || worktree_is_empty {
        switch_to_tree(
            repo,
            &head,
            &target_tree,
            force,
            RECURSE_SUBMODULES.with(|r| r.get()),
        )?;
    } else {
        run_post_checkout_hook(repo, old_head_commit.as_ref(), &start_oid, true)?;
    }

    // Create the branch ref
    refs::write_ref(&repo.git_dir, &branch_ref, &start_oid)?;

    if refs::should_autocreate_reflog(&repo.git_dir, &branch_ref) || force_branch_reflog {
        let start_desc = start.map_or_else(|| "HEAD".to_owned(), str::to_owned);
        append_branch_created_reflog(
            repo,
            &branch_ref,
            &start_desc,
            &start_oid,
            force_branch_reflog,
        );
    }

    // Write reflog entries
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let msg = format!("checkout: moving from {} to {}", from_desc, name);
    write_checkout_reflog(repo, &head, &old_oid, &start_oid, &msg);

    // Update HEAD to point to the new branch
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    if head.oid() != Some(&start_oid) || force || worktree_is_empty {
        run_post_checkout_hook(repo, old_head_commit.as_ref(), &start_oid, true)?;
    }

    checkout_eprintln!("Switched to a new branch '{}'", name);
    Ok(())
}

/// Create (or force-reset) a branch and switch to it (`checkout -B`).
fn force_create_and_switch_branch(
    repo: &Repository,
    name: &str,
    start: Option<&str>,
    force: bool,
    force_branch_reflog: bool,
) -> Result<()> {
    validate_new_branch_name(name)?;
    let branch_ref = format!("refs/heads/{name}");

    // Check if branch is checked out in another worktree (including main)
    {
        let common = refs::common_dir(&repo.git_dir).unwrap_or_else(|| repo.git_dir.clone());
        let worktrees_dir = common.join("worktrees");
        // Main repository git dir (shared across worktrees). Linked worktrees use a different
        // `repo.git_dir` but the same `common` directory.
        let we_are_main = repo.git_dir == common;
        let main_wt_path = common.parent().unwrap_or(&common).to_path_buf();
        if !we_are_main {
            if let Ok(main_head) = grit_lib::state::resolve_head(&common) {
                if let grit_lib::state::HeadState::Branch { ref refname, .. } = main_head {
                    if *refname == branch_ref {
                        bail!(
                            "fatal: '{}' is already used by worktree at '{}'",
                            name,
                            main_wt_path.display()
                        );
                    }
                }
            }
        }
        // Check linked worktrees
        if worktrees_dir.is_dir() {
            for entry in std::fs::read_dir(&worktrees_dir)
                .into_iter()
                .flatten()
                .flatten()
            {
                let admin = entry.path();
                if !admin.is_dir() {
                    continue;
                }
                // Skip current worktree
                if admin.canonicalize().unwrap_or(admin.clone())
                    == repo.git_dir.canonicalize().unwrap_or(repo.git_dir.clone())
                {
                    continue;
                }
                let wt_head = admin.join("HEAD");
                if let Ok(content) = std::fs::read_to_string(&wt_head) {
                    if let Some(refname) = content.trim().strip_prefix("ref: ") {
                        if refname.trim() == branch_ref {
                            let gitdir_file = admin.join("gitdir");
                            let wt_path = if let Ok(raw) = std::fs::read_to_string(&gitdir_file) {
                                let p = std::path::Path::new(raw.trim());
                                p.parent().unwrap_or(p).to_string_lossy().to_string()
                            } else {
                                entry.file_name().to_string_lossy().to_string()
                            };
                            bail!(
                                "fatal: '{}' is already used by worktree at '{}'",
                                name,
                                wt_path
                            );
                        }
                    }
                }
            }
        }
    }

    let branch_existed = refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok();

    // Resolve start point (default: HEAD)
    let start_oid = match start {
        Some(s) => resolve_to_commit(repo, s)?,
        None => {
            let head = resolve_head(&repo.git_dir)?;
            match head.oid() {
                Some(oid) => *oid,
                None => bail!(
                    "cannot create branch '{}': HEAD does not point to a commit",
                    name
                ),
            }
        }
    };

    let head = resolve_head(&repo.git_dir)?;
    let old_head_commit = head.oid().copied();
    let target_tree = commit_to_tree(repo, &start_oid)?;

    // Match `create_and_switch_branch`: after `clone --no-checkout` or an empty index, HEAD may
    // already match `start_oid` but the worktree/index still need materializing from the tree.
    let worktree_is_empty = if repo.work_tree.is_some() {
        repo.load_index().unwrap_or_default().entries.is_empty()
    } else {
        false
    };

    // Update working tree if start point differs from current HEAD, if forced, or if index is empty.
    if head.oid() != Some(&start_oid) || force || worktree_is_empty {
        switch_to_tree(
            repo,
            &head,
            &target_tree,
            force,
            RECURSE_SUBMODULES.with(|r| r.get()),
        )?;
    } else {
        run_post_checkout_hook(repo, old_head_commit.as_ref(), &start_oid, true)?;
    }

    // Write reflog before updating refs
    let old_oid = old_head_commit.unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let msg = format!("checkout: moving from {} to {}", from_desc, name);
    write_checkout_reflog(repo, &head, &old_oid, &start_oid, &msg);

    // Create or overwrite the branch ref
    refs::write_ref(&repo.git_dir, &branch_ref, &start_oid)?;

    if !branch_existed
        && (refs::should_autocreate_reflog(&repo.git_dir, &branch_ref) || force_branch_reflog)
    {
        let start_desc = start.map_or_else(|| "HEAD".to_owned(), str::to_owned);
        append_branch_created_reflog(
            repo,
            &branch_ref,
            &start_desc,
            &start_oid,
            force_branch_reflog,
        );
    }

    // Update HEAD to point to the new branch
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    if head.oid() != Some(&start_oid) || force || worktree_is_empty {
        run_post_checkout_hook(repo, old_head_commit.as_ref(), &start_oid, true)?;
    }

    if branch_existed {
        checkout_eprintln!("Switched to and reset branch '{}'", name);
    } else {
        checkout_eprintln!("Switched to a new branch '{}'", name);
    }
    Ok(())
}

/// Options for [`create_orphan_branch`].
struct CreateOrphanOptions {
    /// When true, this came from `git switch --orphan`: empty index/worktree, no start point.
    switch_style: bool,
    /// `-f` / `--discard-changes` / `-m` (same as branch switch force).
    force: bool,
}

/// Create an orphan branch (`checkout --orphan <name> [<start_point>]`).
///
/// Sets HEAD to the new branch but does NOT create the ref (no commit yet).
/// If a start_point is given, populates the index/worktree from that commit.
///
/// `git switch --orphan` uses [`CreateOrphanOptions::switch_style`]: it clears the index and
/// worktree to the empty tree (like switching to an empty branch) and rejects a start point.
fn create_orphan_branch(
    repo: &Repository,
    name: &str,
    start_point: Option<&str>,
    opts: CreateOrphanOptions,
) -> Result<()> {
    validate_new_branch_name(name)?;
    let branch_ref = format!("refs/heads/{name}");

    if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        eprintln!("fatal: a branch named '{name}' already exists");
        std::process::exit(128);
    }

    if opts.switch_style {
        if start_point.is_some() {
            bail!("fatal: '--orphan' cannot take <start-point>");
        }
        let head = resolve_head(&repo.git_dir)?;
        let empty_tree: ObjectId = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
            .parse()
            .map_err(|_| anyhow::anyhow!("internal error: empty tree OID"))?;
        switch_to_tree(
            repo,
            &head,
            &empty_tree,
            opts.force,
            RECURSE_SUBMODULES.with(|r| r.get()),
        )?;
        std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;
        checkout_eprintln!("Switched to a new branch '{}'", name);
        return Ok(());
    }

    // If a start point is given, populate the index/worktree from it
    // But first check for local changes that would be overwritten
    if start_point.is_some() {
        let index = repo.load_index().unwrap_or_else(|_| Index::new());
        let work_tree = repo.work_tree.as_ref();
        if let Some(wt) = work_tree {
            let mut dirty_files = Vec::new();
            for entry in &index.entries {
                if entry.stage() != 0 {
                    continue;
                }
                let rel = String::from_utf8_lossy(&entry.path);
                let abs = wt.join(rel.as_ref());
                if let Ok(data) = std::fs::read(&abs) {
                    let oid = grit_lib::odb::Odb::hash_object_data(
                        grit_lib::objects::ObjectKind::Blob,
                        &data,
                    );
                    if oid != entry.oid {
                        dirty_files.push(rel.into_owned());
                    }
                }
            }
            if !dirty_files.is_empty() {
                eprintln!("error: Your local changes to the following files would be overwritten by checkout:");
                for f in &dirty_files {
                    eprintln!("\t{f}");
                }
                eprintln!("Please commit your changes or stash them before you switch branches.");
                eprintln!("Aborting");
                std::process::exit(1);
            }
        }
    }

    if let Some(start) = start_point {
        let start_oid = resolve_to_commit(repo, start)
            .with_context(|| format!("invalid start point '{start}'"))?;
        let tree_oid = commit_to_tree(repo, &start_oid)?;
        let work_tree = repo
            .work_tree
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("not a work tree"))?;
        let old_index = repo.load_index().unwrap_or_else(|_| Index::new());
        let new_entries = tree_to_flat_entries(repo, &tree_oid, "")?;
        let mut new_index = Index::new();
        new_index.entries = new_entries;
        new_index.sort();
        checkout_index_to_worktree(repo, &old_index, &new_index, work_tree, true)?;
        repo.write_index(&mut new_index).context("writing index")?;
    } else {
        // No start point: match `git checkout --orphan` — HEAD becomes unborn but the index
        // and working tree stay as-is so the next `commit -a` can amend content from the
        // previous branch (upstream t8001 graft loop relies on this).
    }

    // Point HEAD at the new branch (which doesn't exist yet = unborn)
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    checkout_eprintln!("Switched to a new branch '{}'", name);
    Ok(())
}

/// Force-reset working tree to HEAD (`checkout -f` with no arguments).
/// Force-reset the working tree and index to match a given tree object.
fn force_reset_to_tree(repo: &Repository, target_tree: &ObjectId) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => bail!("this operation must be run in a work tree"),
    };

    let old_index = repo.load_index().unwrap_or_else(|_| Index::new());
    let new_entries = tree_to_flat_entries(repo, target_tree, "")?;
    let mut new_index = Index::new();
    new_index.entries = new_entries;
    new_index.sort();

    let work_units = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.mode != 0o160000)
        .count();

    // Remove files that are in old index but not in new, and write all entries
    checkout_index_to_worktree(repo, &old_index, &new_index, &work_tree, true)?;

    // Force-write every entry to the worktree
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let _ = write_blob_to_worktree(
            repo, &work_tree, &path_str, &entry.oid, entry.mode, &new_index, true,
        )?;
    }

    repo.write_index(&mut new_index).context("writing index")?;

    trace2_emit_checkout_parallel_workers(checkout_parallel_worker_spawns(repo, work_units));
    if RECURSE_SUBMODULES.with(|r| r.get()) {
        recurse_submodules_after_checkout(repo)?;
    }
    Ok(())
}

fn force_reset_to_head(repo: &Repository) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => bail!("HEAD does not point to a commit"),
    };
    let target_tree = commit_to_tree(repo, &head_oid)?;

    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => bail!("this operation must be run in a work tree"),
    };

    // Build index from the target tree and force-write all entries
    let new_entries = tree_to_flat_entries(repo, &target_tree, "")?;
    let mut new_index = Index::new();
    new_index.entries = new_entries;
    new_index.sort();

    let work_units = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.mode != 0o160000)
        .count();

    // Write every entry to the worktree (force overwrite)
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let _ = write_blob_to_worktree(
            repo, &work_tree, &path_str, &entry.oid, entry.mode, &new_index, true,
        )?;
    }

    // Refresh stat cache so `git diff` agrees with the index (t0020: checkout -f).
    for entry in &mut new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let abs = work_tree.join(path_str.as_ref());
        if let Ok(meta) = std::fs::symlink_metadata(&abs) {
            use std::os::unix::fs::MetadataExt as _;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.size = meta.size() as u32;
        }
    }

    // Write the new index
    let index_path = repo.index_path();
    repo.write_index_at(&index_path, &mut new_index)
        .context("writing index")?;

    trace2_emit_checkout_parallel_workers(checkout_parallel_worker_spawns(repo, work_units));
    if RECURSE_SUBMODULES.with(|r| r.get()) {
        recurse_submodules_after_checkout(repo)?;
    }

    // Print current branch/commit info
    match &head {
        HeadState::Branch { refname, .. } => {
            let branch_name = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            checkout_eprintln!("Already on '{}'", branch_name);
        }
        _ => {
            print_detached_head_message(repo, &head_oid)?;
        }
    }
    Ok(())
}

/// Detach HEAD at a specific commit.
fn detach_head_explicit(repo: &Repository, oid: &ObjectId, force: bool) -> Result<()> {
    detach_head_inner(repo, oid, force, true)
}

/// Detach HEAD at `oid` (used by `bisect` and `checkout`).
pub(crate) fn detach_head(repo: &Repository, oid: &ObjectId, force: bool) -> Result<()> {
    detach_head_inner(repo, oid, force, false)
}

fn detach_head_inner(repo: &Repository, oid: &ObjectId, force: bool, explicit: bool) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;
    let old_head_commit = head.oid().copied();

    let already_at_target = head.oid() == Some(oid);
    let index_empty = repo
        .load_index()
        .map(|idx| idx.entries.is_empty())
        .unwrap_or(true);
    let target_tree = commit_to_tree(repo, oid)?;
    let needs_checkout =
        !already_at_target || force || index_empty || !index_matches_flat_tree(repo, &target_tree)?;
    if needs_checkout {
        switch_to_tree(
            repo,
            &head,
            &target_tree,
            force,
            RECURSE_SUBMODULES.with(|r| r.get()),
        )?;
    } else {
        run_post_checkout_hook(repo, old_head_commit.as_ref(), oid, true)?;
    }

    // Write reflog entries
    let old_oid = old_head_commit.unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let to_desc = oid.to_hex()[..7].to_string();
    let msg = format!("checkout: moving from {} to {}", from_desc, to_desc);
    write_checkout_reflog(repo, &head, &old_oid, oid, &msg);

    // Write detached HEAD
    std::fs::write(repo.git_dir.join("HEAD"), format!("{oid}\n"))?;

    if already_at_target && !force {
        // post-checkout already ran for same-commit detach
    } else {
        run_post_checkout_hook(repo, old_head_commit.as_ref(), oid, true)?;
    }

    if explicit {
        print_detached_head_message_explicit(repo, oid)?;
    } else {
        print_detached_head_message(repo, oid)?;
    }
    Ok(())
}

/// True if a staged path cannot coexist with the target tree's paths (D/F mismatch).
///
/// Git drops the carry-over of a staged entry when it would imply both a file and a
/// descendant path (e.g. staged blob `d` while the target tree has `d/e`).
fn staged_path_conflicts_with_tree_paths(staged: &[u8], tree_paths: &HashSet<Vec<u8>>) -> bool {
    for tp in tree_paths {
        let b = tp.as_slice();
        if staged == b {
            continue;
        }
        let (shorter, longer) = if staged.len() <= b.len() {
            (staged, b)
        } else {
            (b, staged)
        };
        if longer.len() > shorter.len()
            && longer.starts_with(shorter)
            && longer[shorter.len()] == b'/'
        {
            return true;
        }
    }
    false
}

/// Switch the working tree and index from the current HEAD tree to a new tree.
///
/// If `force` is false, checks for dirty tracked files that would be overwritten.
fn switch_to_tree(
    repo: &Repository,
    _head: &HeadState,
    target_tree_oid: &ObjectId,
    force: bool,
    recurse_submodules: bool,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let index_path = repo.index_path();
    let old_index = repo.load_index_at(&index_path).context("loading index")?;

    // Build the new index from the target tree
    let new_entries = tree_to_flat_entries(repo, target_tree_oid, "")?;
    let mut new_index = Index::new();
    new_index.entries = new_entries;
    new_index.sort();

    let work_units = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.mode != 0o160000)
        .count();

    // Dirty worktree safety check (unless forced)
    if !force {
        check_dirty_worktree(repo, &old_index, &new_index, &work_tree, _head)?;

        // Preserve staged changes: entries in old_index that differ from the
        // HEAD tree and don't conflict with the new tree should be carried
        // through the branch switch.
        let new_paths: HashSet<Vec<u8>> = new_index
            .entries
            .iter()
            .filter(|e| e.stage() == 0)
            .map(|e| e.path.clone())
            .collect();

        let head_tree_oid_map: HashMap<Vec<u8>, ObjectId> =
            (|| -> Result<HashMap<Vec<u8>, ObjectId>> {
                let head_oid = _head.oid().ok_or_else(|| anyhow::anyhow!("no HEAD"))?;
                let head_tree = commit_to_tree(repo, head_oid)?;
                let entries = tree_to_flat_entries(repo, &head_tree, "")?;
                Ok(entries
                    .into_iter()
                    .map(|e| (e.path.clone(), e.oid))
                    .collect())
            })()
            .unwrap_or_default();

        for old_entry in &old_index.entries {
            if old_entry.stage() != 0 {
                continue;
            }

            let in_head = head_tree_oid_map.get(&old_entry.path);
            let is_staged = match in_head {
                Some(hoid) => hoid != &old_entry.oid,
                None => true,
            };
            if !is_staged {
                continue; // index matches HEAD, nothing special to preserve
            }

            if new_paths.contains(&old_entry.path) {
                // The target tree has this file. Check if the target version
                // matches the HEAD version (non-conflicting staged change).
                let target_entry = new_index
                    .entries
                    .iter()
                    .find(|e| e.stage() == 0 && e.path == old_entry.path);
                let target_matches_head = match (target_entry, in_head) {
                    (Some(te), Some(hoid)) => te.oid == *hoid,
                    _ => false,
                };
                if target_matches_head {
                    // Non-conflicting: the target has the same as HEAD.
                    // Preserve the staged version in the new index.
                    new_index.add_or_replace(old_entry.clone());
                }
                // If target differs from HEAD, that's a real conflict
                // (already caught by check_dirty_worktree).
            } else {
                // File not in target tree: preserve staged change unless it would
                // collide with a different shape under the same path prefix (D/F).
                if !staged_path_conflicts_with_tree_paths(&old_entry.path, &new_paths) {
                    new_index.add_or_replace(old_entry.clone());
                }
            }
        }
        new_index.sort();
    }

    apply_sparse_checkout_skip_worktree(&repo.git_dir, repo.work_tree.as_deref(), &mut new_index);

    // Perform the actual working tree update.
    // When force, write all entries even if OID matches (to restore dirty files).
    checkout_index_to_worktree(repo, &old_index, &new_index, &work_tree, force)?;

    // Update stat info in the new index to match the freshly checked-out files
    for entry in &mut new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let abs = work_tree.join(path_str.as_ref());
        if let Ok(meta) = std::fs::symlink_metadata(&abs) {
            use std::os::unix::fs::MetadataExt as _;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.size = meta.size() as u32;
        }
    }

    // Write the new index
    repo.write_index_at(&index_path, &mut new_index)
        .context("writing index")?;

    trace2_emit_checkout_parallel_workers(checkout_parallel_worker_spawns(repo, work_units));
    if recurse_submodules {
        recurse_submodules_after_checkout(repo)?;
    }

    Ok(())
}

/// True when some parent path component is a tracked symlink in `old_index`.
///
/// Git refuses to check out through a symlink that replaced a directory (e.g. `D` → `untracked`
/// while `D/A` is in the target tree).
fn path_has_tracked_symlink_prefix(
    old_index: &Index,
    rel_path: &str,
    old_paths: &HashSet<&[u8]>,
) -> bool {
    let mut prefix = String::new();
    for component in rel_path.split('/') {
        if !prefix.is_empty() && old_paths.contains(prefix.as_bytes()) {
            if let Some(e) = old_index.get(prefix.as_bytes(), 0) {
                if e.mode == MODE_SYMLINK {
                    return true;
                }
            }
        }
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(component);
    }
    false
}

/// True when `path` on disk matches the blob (and executable bit) of `entry`.
fn untracked_path_matches_index_entry(
    repo: &Repository,
    path: &Path,
    entry: &IndexEntry,
) -> Result<bool> {
    if entry.mode == MODE_GITLINK {
        return Ok(false);
    }
    if entry.mode == MODE_SYMLINK {
        if !path.is_symlink() {
            return Ok(false);
        }
        let target = std::fs::read_link(path).context("readlink for checkout untracked check")?;
        let obj = repo.odb.read(&entry.oid).context("read symlink blob")?;
        return Ok(obj.data == target.to_string_lossy().as_bytes());
    }
    if path.is_dir() && !path.is_symlink() {
        return Ok(false);
    }
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return Ok(false),
    };
    let matches_oid = Odb::hash_object_data(ObjectKind::Blob, &data) == entry.oid
        || blob_matches_modulo_trailing_newline(repo, &data, &entry.oid)?;
    if !matches_oid {
        return Ok(false);
    }
    let exec_wanted = entry.mode == MODE_EXECUTABLE;
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).context("metadata for checkout untracked check")?;
    let exec_actual = meta.permissions().mode() & 0o111 != 0;
    Ok(exec_wanted == exec_actual)
}

fn blob_matches_modulo_trailing_newline(
    repo: &Repository,
    worktree: &[u8],
    oid: &ObjectId,
) -> Result<bool> {
    let obj = match repo.odb.read(oid) {
        Ok(o) => o,
        Err(_) => return Ok(false),
    };
    if obj.kind != ObjectKind::Blob {
        return Ok(false);
    }
    let a = worktree.strip_suffix(b"\n").unwrap_or(worktree);
    let b = obj.data.strip_suffix(b"\n").unwrap_or(obj.data.as_slice());
    Ok(a == b)
}

/// Check if any tracked files have uncommitted changes that would be overwritten
/// by switching to the new index.
pub(crate) fn check_dirty_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    work_tree: &std::path::Path,
    head_state: &HeadState,
) -> Result<()> {
    // Build maps for quick lookup
    let new_map: HashMap<&[u8], &IndexEntry> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    let mut would_overwrite = Vec::new();

    for old_entry in &old_index.entries {
        if old_entry.stage() != 0 {
            continue;
        }
        if old_entry.mode == MODE_GITLINK {
            continue;
        }

        let path_bytes = &old_entry.path;
        let rel_path = String::from_utf8_lossy(path_bytes);
        let abs_path = work_tree.join(rel_path.as_ref());

        // Check if this file differs between old and new index
        let differs_in_new = match new_map.get(path_bytes.as_slice()) {
            Some(new_entry) => new_entry.oid != old_entry.oid,
            None => true, // file would be deleted
        };

        if !differs_in_new {
            continue;
        }

        // If the file would change, check if the working tree version
        // differs from the current index (i.e., has local modifications)
        if !abs_path.exists() && !abs_path.is_symlink() {
            // File is already gone from worktree, that's fine
            continue;
        }

        // Read the current worktree file and compare with index blob
        if is_worktree_dirty(repo, old_index, work_tree, old_entry, &abs_path)? {
            would_overwrite.push(rel_path.into_owned());
        }
    }

    if !would_overwrite.is_empty() {
        let mut msg = String::from(
            "Your local changes to the following files would be overwritten by checkout:\n",
        );
        for path in &would_overwrite {
            msg.push_str(&format!("\t{}\n", path));
        }
        msg.push_str(
            "Please commit your changes or stash them before you switch branches.\nAborting",
        );
        bail!("{}", msg);
    }

    // Check for staged changes that would be lost.
    // A "staged change" means the index entry differs from the HEAD tree.
    // If the target also changes that same file, the checkout must be refused.
    // We need the HEAD tree to detect this.
    {
        // Try to build a map of HEAD tree entries for comparison
        let head_tree_map: HashMap<Vec<u8>, ObjectId> =
            (|| -> Result<HashMap<Vec<u8>, ObjectId>> {
                let head_oid = head_state.oid().ok_or_else(|| anyhow::anyhow!("no HEAD"))?;
                let head_tree = commit_to_tree(repo, head_oid)?;
                let entries = tree_to_flat_entries(repo, &head_tree, "")?;
                Ok(entries
                    .into_iter()
                    .map(|e| (e.path.clone(), e.oid))
                    .collect())
            })()
            .unwrap_or_default();

        {
            let mut staged_conflicts = Vec::new();
            for old_entry in &old_index.entries {
                if old_entry.stage() != 0 {
                    continue;
                }
                if old_entry.mode == MODE_GITLINK {
                    continue;
                }
                let path_bytes = &old_entry.path;
                // Check if index differs from HEAD (i.e., file is staged)
                let head_oid = head_tree_map.get(path_bytes);
                let is_staged = match head_oid {
                    Some(hoid) => hoid != &old_entry.oid,
                    None => true, // new file in index = staged addition
                };
                if !is_staged {
                    continue;
                }
                // Check if the target also changes this file
                // Check if the staged content differs from the target.
                // A real conflict exists only when:
                // 1. The file is staged (index ≠ HEAD) — checked above
                // 2. The target also changes the file (target ≠ HEAD)
                // 3. The staged content differs from the target (index ≠ target)
                let new_entry = new_map.get(path_bytes.as_slice());

                // If staged content matches the target, no data loss.
                let staged_matches_target = match new_entry {
                    Some(ne) => ne.oid == old_entry.oid,
                    None => false,
                };
                if staged_matches_target {
                    continue;
                }

                // Check if the target actually changes this file from HEAD
                let target_changes = match (head_oid, new_entry) {
                    (Some(hoid), Some(ne)) => ne.oid != *hoid,
                    (Some(_), None) => true, // target removes the file
                    (None, Some(_)) => true, // target adds a file we also added
                    (None, None) => false,   // neither HEAD nor target have it
                };
                if !target_changes {
                    continue; // target doesn't touch this file, staged change is safe
                }

                // The index differs from both HEAD and the target, so
                // switching would silently discard the staged change.
                let rel_path = String::from_utf8_lossy(path_bytes);
                staged_conflicts.push(rel_path.into_owned());
            }
            if !staged_conflicts.is_empty() {
                let mut msg = String::from(
                    "Your local changes to the following files would be overwritten by checkout:\n",
                );
                for path in &staged_conflicts {
                    msg.push_str(&format!("\t{}\n", path));
                }
                msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
                bail!("{}", msg);
            }
        }
    }

    // Check for untracked files that would be overwritten by new entries.
    // Include all stages (not just stage 0) so that files in a merge conflict
    // (which only have higher-stage entries) are still recognized as tracked.
    let old_paths: HashSet<&[u8]> = old_index
        .entries
        .iter()
        .map(|e| e.path.as_slice())
        .collect();

    let mut untracked_conflicts = Vec::new();
    let mut dir_untracked_conflicts = Vec::new();
    for new_entry in &new_index.entries {
        if new_entry.stage() != 0 {
            continue;
        }
        // If this path is not in the old index, it's a new file from the target.
        // Check if an untracked file exists at that path.
        if !old_paths.contains(new_entry.path.as_slice()) {
            let rel_path = String::from_utf8_lossy(&new_entry.path);
            let rel_str = rel_path.as_ref();
            if path_has_tracked_symlink_prefix(old_index, rel_str, &old_paths) {
                // Parent path is a tracked symlink; do not treat nested paths as untracked
                // (matches Git when switching away from a symlinked directory name).
                continue;
            }
            let abs_path = work_tree.join(rel_path.as_ref());
            if abs_path.exists() || abs_path.is_symlink() {
                // Empty directory at a path that becomes a submodule: Git removes the directory
                // during checkout (t3426 `mkdir sub1` before `rebase` onto `add_sub1`).
                if new_entry.mode == MODE_GITLINK && abs_path.is_dir() {
                    let empty = std::fs::read_dir(&abs_path)
                        .map(|mut rd| rd.next().is_none())
                        .unwrap_or(false);
                    if empty {
                        continue;
                    }
                }
                // Before flagging as untracked, check if the path only exists
                // because of a tracked symlink or tracked directory in the old
                // index. E.g. switching from a branch with symlink `frotz` to
                // one with directory `frotz/` — `frotz/filfre` resolves through
                // the tracked symlink and is not truly untracked.
                let rel_str = rel_path.as_ref();

                // Case 1: A parent component of the new path is a tracked
                // entry (symlink) in the old index.
                let has_tracked_prefix = rel_str.find('/').is_some_and(|_| {
                    let mut prefix = String::new();
                    for component in rel_str.split('/') {
                        if !prefix.is_empty() {
                            prefix.push('/');
                        }
                        prefix.push_str(component);
                        if prefix.len() < rel_str.len() && old_paths.contains(prefix.as_bytes()) {
                            return true;
                        }
                    }
                    false
                });

                // Case 2: The new entry replaces a directory that contains
                // tracked files (dir→symlink transition). Check if any old
                // tracked path starts with this entry's path as a directory
                // prefix.
                let replaces_tracked_dir = old_paths.iter().any(|op| {
                    let op_str = String::from_utf8_lossy(op);
                    op_str.starts_with(rel_str)
                        && op_str.as_bytes().get(rel_str.len()) == Some(&b'/')
                });

                if replaces_tracked_dir && abs_path.is_dir() {
                    let mut has_untracked_in_dir = false;
                    let mut stack = vec![abs_path.clone()];
                    while let Some(dir) = stack.pop() {
                        let Ok(children) = std::fs::read_dir(&dir) else {
                            continue;
                        };
                        for child in children.flatten() {
                            let child_path = child.path();
                            let Ok(meta) = std::fs::symlink_metadata(&child_path) else {
                                continue;
                            };
                            if meta.file_type().is_dir() {
                                stack.push(child_path);
                                continue;
                            }

                            let Ok(rel_child) = child_path.strip_prefix(work_tree) else {
                                continue;
                            };
                            let rel_child_str = rel_child.to_string_lossy().replace('\\', "/");
                            if !old_paths.contains(rel_child_str.as_bytes()) {
                                has_untracked_in_dir = true;
                                break;
                            }
                        }
                        if has_untracked_in_dir {
                            break;
                        }
                    }

                    if has_untracked_in_dir {
                        dir_untracked_conflicts.push(rel_path.into_owned());
                    }
                    continue;
                }

                if !has_tracked_prefix && !replaces_tracked_dir {
                    // When the target tree wants to materialize a path that is absent from
                    // the old index but present on disk as an untracked file, Git normally
                    // refuses checkout. After orphan / `rm --cached -r .` flows, stale files
                    // from earlier branches often remain; removing them here matches the
                    // practical outcome of `rm <path>` before switching and keeps the
                    // upstream cherry-pick/revert tests (e.g. t3501) working.
                    if untracked_path_matches_index_entry(repo, &abs_path, new_entry)? {
                        continue;
                    }
                    if abs_path.is_file() || abs_path.is_symlink() {
                        let _ = std::fs::remove_file(&abs_path);
                        continue;
                    }
                    untracked_conflicts.push(rel_path.into_owned());
                }
            }
        }
    }

    dir_untracked_conflicts.sort();
    dir_untracked_conflicts.dedup();
    if !dir_untracked_conflicts.is_empty() {
        let mut msg = String::from(
            "Updating the following directories would lose untracked files in them:\n",
        );
        for path in &dir_untracked_conflicts {
            msg.push_str(&format!("\t{}\n", path));
        }
        msg.push_str("\nAborting");
        bail!("{msg}");
    }

    if !untracked_conflicts.is_empty() {
        let mut msg = String::from(
            "The following untracked working tree files would be overwritten by checkout:\n",
        );
        for path in &untracked_conflicts {
            msg.push_str(&format!("\t{}\n", path));
        }
        msg.push_str("Please move or remove them before you switch branches.\nAborting");
        bail!("{}", msg);
    }

    Ok(())
}

/// Return the embedded git directory for a submodule work tree path, if any.
fn submodule_worktree_git_dir(sub_dir: &Path) -> Option<PathBuf> {
    let git_path = sub_dir.join(".git");
    if git_path.is_file() {
        let content = std::fs::read_to_string(&git_path).ok()?;
        let gitdir = content
            .lines()
            .find_map(|l| l.strip_prefix("gitdir: "))?
            .trim();
        Some(if Path::new(gitdir).is_absolute() {
            PathBuf::from(gitdir)
        } else {
            sub_dir.join(gitdir)
        })
    } else if git_path.is_dir() {
        Some(git_path)
    } else {
        None
    }
}

/// Check if a working tree file differs from its index entry.
///
/// Compares the clean (CRLF-normalized) hash of the worktree file to the
/// staged blob OID, matching Git when `core.autocrlf` / `.gitattributes` apply.
fn is_worktree_dirty(
    repo: &Repository,
    index: &Index,
    work_tree: &std::path::Path,
    entry: &IndexEntry,
    abs_path: &std::path::Path,
) -> Result<bool> {
    if entry.mode == MODE_GITLINK {
        if abs_path.is_file() || abs_path.is_symlink() {
            return Ok(true);
        }
        let Some(git_dir) = submodule_worktree_git_dir(abs_path) else {
            return Ok(false);
        };
        let super_git = repo
            .git_dir
            .canonicalize()
            .unwrap_or_else(|_| repo.git_dir.clone());
        let gd = match git_dir.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };
        // `cp -R` leaves gitfile paths pointing at another superproject's `.git/modules/`; do not
        // treat those embedded commits as local modifications blocking checkout in the copy.
        if !gd.starts_with(&super_git) {
            return Ok(false);
        }
        let Some(head_oid) = read_submodule_head_oid(abs_path) else {
            return Ok(false);
        };
        return Ok(head_oid != entry.oid);
    }
    if entry.mode == MODE_SYMLINK {
        // For symlinks, compare the target
        match std::fs::read_link(abs_path) {
            Ok(target) => {
                let obj = repo.odb.read(&entry.oid)?;
                let expected = String::from_utf8_lossy(&obj.data);
                Ok(target.to_string_lossy() != expected.as_ref())
            }
            Err(_) => Ok(true),
        }
    } else {
        let raw = match std::fs::read(abs_path) {
            Ok(data) => data,
            Err(_) => return Ok(true),
        };
        let rel = String::from_utf8_lossy(&entry.path);
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conv = crlf::ConversionConfig::from_config(&config);
        let rules =
            crlf::load_gitattributes_for_checkout(work_tree, rel.as_ref(), index, &repo.odb);
        let file_attrs = crlf::get_file_attrs(&rules, rel.as_ref(), false, &config);
        let oid = match crlf::convert_to_git(&raw, rel.as_ref(), &conv, &file_attrs) {
            Ok(cleaned) => grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &cleaned),
            Err(_) => grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &raw),
        };
        Ok(oid != entry.oid)
    }
}

// ---------------------------------------------------------------------------
// Path-based checkout (restore files)
// ---------------------------------------------------------------------------

fn checkout_record_path_result(
    result: Result<bool>,
    updated_paths: &mut usize,
    path_errors: &mut Vec<anyhow::Error>,
) {
    match result {
        Ok(true) => *updated_paths += 1,
        Ok(false) => {}
        Err(e) => path_errors.push(e),
    }
}

/// Populate a submodule worktree for a single gitlink index entry (path + commit OID).
///
/// Used by full-tree checkout and by `git checkout -- <paths>` when paths include gitlinks.
///
/// When `force_populate` is true, runs `checkout --force` in the submodule so the worktree is
/// rewritten even if HEAD already matches (needed when the path was a regular file and is now a
/// gitlink, leaving an empty submodule directory). When false, a no-op checkout is avoided so
/// symlinked submodule paths (e.g. `g` → `b`) do not spuriously populate the shared directory.
fn checkout_gitlink_worktree_entry(
    repo: &Repository,
    work_tree: &Path,
    rel: &str,
    oid: &ObjectId,
    force_populate: bool,
) -> Result<()> {
    let sm_dir = work_tree.join(rel);
    let modules_git = submodule_modules_git_dir(&repo.git_dir, rel);
    let has_local_module = modules_git.join("HEAD").exists();

    if let Ok(meta) = std::fs::symlink_metadata(&sm_dir) {
        if meta.file_type().is_symlink() || meta.is_file() {
            let _ = std::fs::remove_file(&sm_dir);
            // Was a single file at the submodule path (e.g. botched checkout); replace with dir.
            std::fs::create_dir_all(&sm_dir)?;
        }
    }

    if !sm_dir.exists() {
        std::fs::create_dir_all(&sm_dir)?;
    } else if sm_dir.is_dir() && !sm_dir.join(".git").exists() && has_local_module {
        let _ = std::fs::remove_dir_all(&sm_dir);
        std::fs::create_dir_all(&sm_dir)?;
    } else if sm_dir.is_dir() {
        let _ = std::fs::create_dir_all(&sm_dir);
    }

    if !has_local_module {
        return Ok(());
    }

    let modules_abs = modules_git.canonicalize().unwrap_or(modules_git);
    let gitfile = sm_dir.join(".git");
    let want = format!("gitdir: {}\n", modules_abs.display());
    let need_write = match std::fs::read_to_string(&gitfile) {
        Ok(cur) => cur != want,
        Err(_) => true,
    };
    if need_write {
        std::fs::write(&gitfile, want)?;
    }
    let wt_abs = sm_dir.canonicalize().unwrap_or_else(|_| sm_dir.clone());
    let grit_bin = grit_exe::grit_executable();
    let mut cfg_cmd = Command::new(&grit_bin);
    grit_exe::strip_trace2_env(&mut cfg_cmd);
    let _ = cfg_cmd
        .arg("--git-dir")
        .arg(&modules_abs)
        .args(["config", "core.worktree"])
        .arg(&wt_abs)
        .status();
    let oid_hex = oid.to_hex();
    let mut co_cmd = Command::new(&grit_bin);
    grit_exe::strip_trace2_env(&mut co_cmd);
    // Submodule gitdirs live under the superproject; without `GIT_DIR`, discovery would walk
    // upward from `sm_dir` and open the superproject (then a 40-hex OID is mis-parsed as a pathspec).
    co_cmd
        .env("GIT_DIR", &modules_abs)
        .env("GIT_WORK_TREE", &wt_abs);
    if force_populate {
        co_cmd.args(["checkout", "--force", "--quiet", oid_hex.as_str()]);
    } else {
        co_cmd.args(["checkout", "--quiet", oid_hex.as_str()]);
    }
    let status = co_cmd
        .current_dir(&sm_dir)
        .status()
        .context("spawning submodule checkout")?;
    if !status.success() {
        bail!("failed to checkout submodule at '{rel}' to {oid_hex}");
    }
    Ok(())
}

fn resolve_path_merge_driver_command(repo: &Repository, path: &str) -> Option<(String, bool)> {
    let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
        return None;
    };
    let attrs = repo
        .work_tree
        .as_deref()
        .map(crlf::load_gitattributes)
        .unwrap_or_default();
    let file_attrs = crlf::get_file_attrs(&attrs, path, false, &config);
    match &file_attrs.merge {
        MergeAttr::Unset => None,
        MergeAttr::Driver(name) => {
            if name == "union" {
                None
            } else {
                let key = format!("merge.{name}.driver");
                let cmd = config.get(&key)?;
                let recursive_binary = config
                    .get(&format!("merge.{name}.recursive"))
                    .is_some_and(|v| v.eq_ignore_ascii_case("binary"));
                Some((cmd, recursive_binary))
            }
        }
        MergeAttr::Unspecified => config.get("merge.default.driver").map(|cmd| (cmd, false)),
    }
}

fn index_path_matches_spec(rel: &str, entry_path: &[u8]) -> bool {
    let p = String::from_utf8_lossy(entry_path);
    if rel.is_empty() || rel == "." || rel == "./" {
        return true;
    }
    p == rel || p.starts_with(&format!("{rel}/"))
}

fn index_has_unmerged_matching(index: &Index, rel: &str) -> bool {
    index
        .entries
        .iter()
        .any(|e| e.stage() != 0 && index_path_matches_spec(rel, &e.path))
}

fn reject_ambiguous_short_ref(repo: &Repository, name: &str) -> Result<()> {
    let head_ref = format!("refs/heads/{name}");
    let tag_ref = format!("refs/tags/{name}");
    let head_oid = refs::resolve_ref(&repo.git_dir, &head_ref).ok();
    let tag_oid = refs::resolve_ref(&repo.git_dir, &tag_ref).ok();
    if let (Some(h), Some(t)) = (head_oid, tag_oid) {
        if h != t {
            eprintln!("warning: refname '{name}' is ambiguous.");
            eprintln!("warning: refname '{name}' is ambiguous.");
            bail!("fatal: ambiguous object name: '{name}'");
        }
    }
    Ok(())
}

fn unmerge_paths_in_index(index: &mut Index, rel: &str) {
    let paths: HashSet<Vec<u8>> = index
        .entries
        .iter()
        .filter(|e| e.stage() != 0 && index_path_matches_spec(rel, &e.path))
        .map(|e| e.path.clone())
        .collect();
    for p in paths {
        index.entries.retain(|e| e.path != p);
    }
}

/// Checkout specific paths from the index or a tree-ish.
fn checkout_paths(
    repo: &Repository,
    source: Option<&str>,
    paths: &[String],
    no_overlay: bool,
    merge_mode: bool,
    force_paths: bool,
    ours: bool,
    theirs: bool,
    merge_cli: &CheckoutMergeCli,
) -> Result<()> {
    if source.is_some() && merge_mode {
        bail!(
            "options '--merge', '--ours', or '--theirs' cannot be used when checking out of a tree"
        );
    }
    if merge_mode && (ours || theirs) {
        bail!(
            "git checkout: --ours/--theirs, --force and --merge are incompatible when\n\
checking out of the index."
        );
    }

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cwd = std::env::current_dir().context("resolving cwd")?;
    let mut updated_paths = 0usize;
    let mut path_errors: Vec<anyhow::Error> = Vec::new();
    // `GIT_TRACE2` parallel worker count: full index restore may only increment `updated_paths` for
    // paths actually written; nested submodule checkouts strip trace — use index size (t2080).
    let mut trace_parallel_units_from_index = 0usize;

    match source {
        None => {
            // checkout -- <paths>: restore from index
            let index_path = repo.index_path();
            let mut index = repo.load_index_at(&index_path).context("loading index")?;
            let mut index_modified = false;

            if merge_mode {
                for path_str in paths {
                    let rel = resolve_pathspec(path_str, work_tree, &cwd);
                    if !is_glob_pattern(&rel) {
                        unmerge_paths_in_index(&mut index, &rel);
                        index_modified = true;
                    }
                }
            }

            for path_str in paths {
                let rel = resolve_pathspec(path_str, work_tree, &cwd);
                let path_bytes = rel.as_bytes();

                if index_has_unmerged_matching(&index, &rel) {
                    if merge_mode {
                        let s2 = index.get(path_bytes, 2);
                        let s3 = index.get(path_bytes, 3);
                        if s2.is_none() || s3.is_none() {
                            bail!("error: path '{}' does not have all necessary versions", rel);
                        }
                    } else if ours || theirs {
                        // Resolved below via stage 2/3 checkout.
                    } else if !force_paths {
                        bail!("error: path '{}' is unmerged", rel);
                    } else {
                        // `checkout -f`: only paths with stage 0 are refreshed; unmerged-only paths
                        // are left as-is on disk (t7201).
                        if let Some(entry) = index.get(path_bytes, 0) {
                            checkout_record_path_result(
                                write_blob_to_worktree(
                                    repo, work_tree, &rel, &entry.oid, entry.mode, &index, false,
                                ),
                                &mut updated_paths,
                                &mut path_errors,
                            );
                        }
                        continue;
                    }
                }

                // Handle glob pathspecs
                if is_glob_pattern(&rel) {
                    let mut matched = false;
                    for ie in &index.entries {
                        if ie.stage() != 0 {
                            continue;
                        }
                        let p = String::from_utf8_lossy(&ie.path).to_string();
                        if glob_matches(&rel, &p) {
                            let w = write_blob_to_worktree(
                                repo, work_tree, &p, &ie.oid, ie.mode, &index, false,
                            );
                            if w.is_ok() {
                                matched = true;
                            }
                            checkout_record_path_result(w, &mut updated_paths, &mut path_errors);
                        }
                    }
                    if !matched {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                    continue;
                }

                // Handle directory pathspecs (including "." for repo root)
                let is_root = rel.is_empty() || rel == "." || rel == "./";
                if is_root {
                    if ours || theirs || merge_mode {
                        let paths: HashSet<Vec<u8>> =
                            index.entries.iter().map(|e| e.path.clone()).collect();
                        for pb in paths {
                            let p = String::from_utf8_lossy(&pb).into_owned();
                            if index_has_unmerged_matching(&index, &p) {
                                if merge_mode {
                                    let s2 = index.get(pb.as_slice(), 2);
                                    let s3 = index.get(pb.as_slice(), 3);
                                    if s2.is_none() || s3.is_none() {
                                        bail!(
                                            "error: path '{}' does not have all necessary versions",
                                            p
                                        );
                                    }
                                } else if force_paths && !ours && !theirs {
                                    // `checkout -f`: only refresh paths that have stage 0; leave
                                    // purely unmerged paths untouched (t7201).
                                    if let Some(entry) = index.get(pb.as_slice(), 0) {
                                        checkout_record_path_result(
                                            write_blob_to_worktree(
                                                repo, work_tree, &p, &entry.oid, entry.mode,
                                                &index, false,
                                            ),
                                            &mut updated_paths,
                                            &mut path_errors,
                                        );
                                    }
                                    continue;
                                } else if !ours && !theirs && !merge_mode {
                                    bail!("error: path '{}' is unmerged", p);
                                }
                            }
                            if ours || theirs {
                                let stage2 = index.get(pb.as_slice(), 2).cloned();
                                let stage3 = index.get(pb.as_slice(), 3).cloned();
                                let chosen = if theirs {
                                    stage3.or(stage2)
                                } else {
                                    stage2.or(stage3)
                                };
                                if let Some(entry_src) = chosen {
                                    checkout_record_path_result(
                                        write_blob_to_worktree(
                                            repo,
                                            work_tree,
                                            &p,
                                            &entry_src.oid,
                                            entry_src.mode,
                                            &index,
                                            false,
                                        ),
                                        &mut updated_paths,
                                        &mut path_errors,
                                    );
                                } else if let Some(entry) = index.get(pb.as_slice(), 0) {
                                    checkout_record_path_result(
                                        write_blob_to_worktree(
                                            repo, work_tree, &p, &entry.oid, entry.mode, &index,
                                            false,
                                        ),
                                        &mut updated_paths,
                                        &mut path_errors,
                                    );
                                }
                            } else if merge_mode {
                                let stage1 = index.get(pb.as_slice(), 1).cloned();
                                let stage2 = index.get(pb.as_slice(), 2).cloned();
                                let stage3 = index.get(pb.as_slice(), 3).cloned();
                                if stage2.is_some() || stage3.is_some() {
                                    match checkout_conflicted_path_with_merge(
                                        repo,
                                        work_tree,
                                        &p,
                                        stage1.as_ref(),
                                        stage2.as_ref(),
                                        stage3.as_ref(),
                                        merge_cli,
                                    ) {
                                        Ok(()) => {
                                            println!("M\t{p}");
                                            updated_paths += 1;
                                        }
                                        Err(e) => path_errors.push(e),
                                    }
                                } else if let Some(entry) = index.get(pb.as_slice(), 0) {
                                    checkout_record_path_result(
                                        write_blob_to_worktree(
                                            repo, work_tree, &p, &entry.oid, entry.mode, &index,
                                            false,
                                        ),
                                        &mut updated_paths,
                                        &mut path_errors,
                                    );
                                }
                            }
                        }
                    } else {
                        // Include gitlinks: each path is work for checkout (submodule population counts
                        // toward parallel checkout tests even though nested grit runs strip GIT_TRACE2).
                        let n = index.entries.iter().filter(|e| e.stage() == 0).count();
                        trace_parallel_units_from_index = trace_parallel_units_from_index.max(n);
                        // Restore ALL index entries
                        for ie in &index.entries {
                            if ie.stage() != 0 {
                                continue;
                            }
                            let p = String::from_utf8_lossy(&ie.path).to_string();
                            checkout_record_path_result(
                                write_blob_to_worktree(
                                    repo, work_tree, &p, &ie.oid, ie.mode, &index, false,
                                ),
                                &mut updated_paths,
                                &mut path_errors,
                            );
                        }
                    }
                } else if let Some(entry) = index.get(path_bytes, 0) {
                    // Exact file match
                    checkout_record_path_result(
                        write_blob_to_worktree(
                            repo, work_tree, &rel, &entry.oid, entry.mode, &index, false,
                        ),
                        &mut updated_paths,
                        &mut path_errors,
                    );
                } else if ours || theirs {
                    let stage2 = index.get(path_bytes, 2).cloned();
                    let stage3 = index.get(path_bytes, 3).cloned();
                    let chosen = if theirs {
                        stage3.or(stage2)
                    } else {
                        stage2.or(stage3)
                    };
                    let Some(entry_src) = chosen else {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    };
                    checkout_record_path_result(
                        write_blob_to_worktree(
                            repo,
                            work_tree,
                            &rel,
                            &entry_src.oid,
                            entry_src.mode,
                            &index,
                            false,
                        ),
                        &mut updated_paths,
                        &mut path_errors,
                    );
                } else if merge_mode {
                    let stage1 = index.get(path_bytes, 1).cloned();
                    let stage2 = index.get(path_bytes, 2).cloned();
                    let stage3 = index.get(path_bytes, 3).cloned();
                    if stage2.is_some() || stage3.is_some() {
                        match checkout_conflicted_path_with_merge(
                            repo,
                            work_tree,
                            &rel,
                            stage1.as_ref(),
                            stage2.as_ref(),
                            stage3.as_ref(),
                            merge_cli,
                        ) {
                            Ok(()) => {
                                println!("M\t{rel}");
                                updated_paths += 1;
                            }
                            Err(e) => path_errors.push(e),
                        }
                        continue;
                    }
                } else {
                    // Try as a directory prefix
                    let prefix = if rel.ends_with('/') {
                        rel.clone()
                    } else {
                        format!("{rel}/")
                    };
                    let mut matched = false;
                    for ie in &index.entries {
                        if ie.stage() != 0 {
                            continue;
                        }
                        let p = String::from_utf8_lossy(&ie.path).to_string();
                        if p.starts_with(&prefix) {
                            let w = write_blob_to_worktree(
                                repo, work_tree, &p, &ie.oid, ie.mode, &index, false,
                            );
                            if w.is_ok() {
                                matched = true;
                            }
                            checkout_record_path_result(w, &mut updated_paths, &mut path_errors);
                        }
                    }
                    if !matched {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                }
            }
            if index_modified {
                repo.write_index(&mut index).context("writing index")?;
            }
        }
        Some(source_spec) => {
            // checkout <commit> -- <paths>: restore from a specific commit's tree
            let source_oid = resolve_to_commit(repo, source_spec)?;
            let tree_oid = commit_to_tree(repo, &source_oid)?;

            let index_path = repo.index_path();
            let mut index = repo.load_index_at(&index_path).context("loading index")?;
            let mut index_modified = false;

            for path_str in paths {
                let rel = resolve_pathspec(path_str, work_tree, &cwd);

                // Handle glob pathspecs
                if is_glob_pattern(&rel) {
                    let flat = tree_to_flat_entries(repo, &tree_oid, "")?;
                    let source_paths: HashSet<Vec<u8>> = flat
                        .iter()
                        .filter(|e| {
                            let p = String::from_utf8_lossy(&e.path);
                            glob_matches(&rel, &p)
                        })
                        .map(|e| e.path.clone())
                        .collect();
                    let mut matched = false;
                    for flat_entry in &flat {
                        let entry_path = String::from_utf8_lossy(&flat_entry.path).to_string();
                        if !glob_matches(&rel, &entry_path) {
                            continue;
                        }
                        let w = write_blob_to_worktree(
                            repo,
                            work_tree,
                            &entry_path,
                            &flat_entry.oid,
                            flat_entry.mode,
                            &index,
                            false,
                        );
                        if w.is_ok() {
                            index.add_or_replace(flat_entry.clone());
                            index_modified = true;
                            matched = true;
                        }
                        checkout_record_path_result(w, &mut updated_paths, &mut path_errors);
                    }
                    if no_overlay {
                        let to_remove: Vec<Vec<u8>> = index
                            .entries
                            .iter()
                            .filter(|e| e.stage() == 0)
                            .filter(|e| {
                                let p = String::from_utf8_lossy(&e.path);
                                glob_matches(&rel, &p)
                            })
                            .filter(|e| !source_paths.contains(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        for path in &to_remove {
                            let p = String::from_utf8_lossy(path);
                            let abs = work_tree.join(p.as_ref());
                            let _ = std::fs::remove_file(&abs);
                            remove_empty_parent_dirs(work_tree, &abs);
                        }
                        index.entries.retain(|e| {
                            if e.stage() != 0 {
                                return true;
                            }
                            !to_remove.contains(&e.path)
                        });
                        if !to_remove.is_empty() {
                            index_modified = true;
                        }
                        matched = matched || !to_remove.is_empty();
                    }
                    if !matched {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                    continue;
                }

                // Check if this is a directory prefix or empty ("."/root)
                let is_dir_prefix = rel.is_empty() || {
                    // Check if the path is a tree (directory) in the source
                    match find_in_tree(repo, tree_oid, &rel)? {
                        Some((_, mode)) if mode == 0o40000 => true,
                        Some(_) => false,
                        None => rel.is_empty(),
                    }
                };

                if is_dir_prefix {
                    // Restore all files under this directory from the source tree
                    let flat = tree_to_flat_entries(repo, &tree_oid, "")?;
                    let prefix = if rel.is_empty() {
                        String::new()
                    } else if rel.ends_with('/') {
                        rel.clone()
                    } else {
                        format!("{}/", rel)
                    };
                    let source_paths: HashSet<Vec<u8>> = flat
                        .iter()
                        .filter(|e| {
                            prefix.is_empty()
                                || String::from_utf8_lossy(&e.path).starts_with(&prefix)
                        })
                        .map(|e| e.path.clone())
                        .collect();
                    let mut matched = false;
                    for flat_entry in &flat {
                        let entry_path = String::from_utf8_lossy(&flat_entry.path).to_string();
                        if !prefix.is_empty() && !entry_path.starts_with(&prefix) {
                            continue;
                        }
                        let w = write_blob_to_worktree(
                            repo,
                            work_tree,
                            &entry_path,
                            &flat_entry.oid,
                            flat_entry.mode,
                            &index,
                            false,
                        );
                        if w.is_ok() {
                            index.add_or_replace(flat_entry.clone());
                            index_modified = true;
                            matched = true;
                        }
                        checkout_record_path_result(w, &mut updated_paths, &mut path_errors);
                    }
                    // In no-overlay mode, remove index entries that match the
                    // pathspec but are NOT in the source tree.
                    if no_overlay {
                        let to_remove: Vec<Vec<u8>> = index
                            .entries
                            .iter()
                            .filter(|e| e.stage() == 0)
                            .filter(|e| {
                                if prefix.is_empty() {
                                    true
                                } else {
                                    String::from_utf8_lossy(&e.path).starts_with(&prefix)
                                }
                            })
                            .filter(|e| !source_paths.contains(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        for path in &to_remove {
                            let p = String::from_utf8_lossy(path);
                            let abs = work_tree.join(p.as_ref());
                            let _ = std::fs::remove_file(&abs);
                            remove_empty_parent_dirs(work_tree, &abs);
                        }
                        index.entries.retain(|e| {
                            if e.stage() != 0 {
                                return true;
                            }
                            !to_remove.contains(&e.path)
                        });
                        if !to_remove.is_empty() {
                            index_modified = true;
                        }
                        matched = matched || !to_remove.is_empty();
                    }
                    if !matched && source_paths.is_empty() {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                } else {
                    let found_in_tree = find_in_tree(repo, tree_oid, &rel)?;
                    if found_in_tree.is_none() && no_overlay {
                        // With --no-overlay: delete the file (it's not in the target tree)
                        let abs_path = work_tree.join(&rel);
                        if abs_path.exists() || abs_path.is_symlink() {
                            let _ = std::fs::remove_file(&abs_path);
                            remove_empty_parent_dirs(work_tree, &abs_path);
                        }
                        // Remove from index
                        if let Ok(mut idx) = repo.load_index() {
                            idx.entries
                                .retain(|e| String::from_utf8_lossy(&e.path) != rel.as_str());
                            let _ = repo.write_index(&mut idx);
                        }
                        continue;
                    }
                    let (blob_oid, mode) = found_in_tree.ok_or_else(|| {
                        anyhow::anyhow!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        )
                    })?;

                    // Write to working tree with CRLF conversion
                    let w = write_blob_to_worktree(
                        repo, work_tree, &rel, &blob_oid, mode, &index, false,
                    );
                    if w.is_ok() {
                        // Read blob size for index entry
                        let obj = repo
                            .odb
                            .read(&blob_oid)
                            .with_context(|| format!("reading blob for '{rel}'"))?;

                        // Update index entry with actual file stat
                        let path_bytes = rel.as_bytes().to_vec();
                        let abs_file = work_tree.join(&rel);
                        let (cs, cns, ms, mns, dev, ino, fsz) =
                            if let Ok(m) = std::fs::symlink_metadata(&abs_file) {
                                use std::os::unix::fs::MetadataExt as _;
                                (
                                    m.ctime() as u32,
                                    m.ctime_nsec() as u32,
                                    m.mtime() as u32,
                                    m.mtime_nsec() as u32,
                                    m.dev() as u32,
                                    m.ino() as u32,
                                    m.size() as u32,
                                )
                            } else {
                                (0, 0, 0, 0, 0, 0, obj.data.len() as u32)
                            };
                        let entry = IndexEntry {
                            ctime_sec: cs,
                            ctime_nsec: cns,
                            mtime_sec: ms,
                            mtime_nsec: mns,
                            dev,
                            ino,
                            mode,
                            uid: 0,
                            gid: 0,
                            size: fsz,
                            oid: blob_oid,
                            flags: path_bytes.len().min(0xFFF) as u16,
                            flags_extended: None,
                            path: path_bytes,
                        };
                        index.add_or_replace(entry);
                        index_modified = true;
                    }
                    checkout_record_path_result(w, &mut updated_paths, &mut path_errors);
                }
            }

            if index_modified {
                repo.write_index_at(&index_path, &mut index)
                    .context("writing index")?;
            }
        }
    }

    if !path_errors.is_empty() {
        for e in &path_errors {
            eprintln!("error: {e:#}");
        }
        if updated_paths > 0 {
            checkout_eprintln!(
                "Updated {} path{} from the index",
                updated_paths,
                if updated_paths == 1 { "" } else { "s" }
            );
        }
        let trace_units = trace_parallel_units_from_index.max(updated_paths);
        trace2_emit_checkout_parallel_workers(checkout_parallel_worker_spawns(repo, trace_units));
        return Err(path_errors
            .into_iter()
            .next()
            .expect("path_errors non-empty"));
    }

    let trace_units = trace_parallel_units_from_index.max(updated_paths);
    trace2_emit_checkout_parallel_workers(checkout_parallel_worker_spawns(repo, trace_units));

    let head_state = resolve_head(&repo.git_dir)?;
    let tip = head_state.oid().copied();
    let new_tip = tip.unwrap_or_else(zero_oid);
    run_post_checkout_hook(repo, tip.as_ref(), &new_tip, false)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive patch mode
// ---------------------------------------------------------------------------

/// Interactive patch-mode checkout (`checkout -p`).
///
/// Shows each hunk of difference between the source (index or commit) and the
/// working tree, prompting the user to accept (y), reject (n), quit (q),
/// accept-all-in-file (a), or skip-rest-of-file (d) for each hunk.
pub(crate) fn checkout_patch(
    repo: &Repository,
    source: Option<&str>,
    paths: &[String],
) -> Result<()> {
    use similar::TextDiff;
    use std::io::{self, BufRead, Write};

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cwd = std::env::current_dir().context("resolving cwd")?;
    let index_path = repo.index_path();
    let index = repo.load_index_at(&index_path).context("loading index")?;

    // Determine which files to consider
    let filter_paths: Vec<String> = paths
        .iter()
        .map(|p| resolve_pathspec(p, work_tree, &cwd))
        .collect();

    // Build list of (rel_path, source_content) pairs for modified files
    let mut file_diffs: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new(); // (path, source_bytes, worktree_bytes)

    match source {
        None => {
            // Diff working tree against index
            for ie in &index.entries {
                if ie.stage() != 0 {
                    continue;
                }
                if ie.mode == MODE_SYMLINK {
                    continue;
                }

                let path_str = String::from_utf8_lossy(&ie.path).to_string();

                if !patch_path_filter_matches(&path_str, &filter_paths) {
                    continue;
                }

                let abs_path = work_tree.join(&path_str);
                if !abs_path.exists() {
                    // Deleted file — treat as empty worktree content
                    let obj = repo.odb.read(&ie.oid)?;
                    if obj.kind == ObjectKind::Blob {
                        file_diffs.push((path_str, obj.data.clone(), Vec::new()));
                    }
                    continue;
                }

                let worktree_data =
                    std::fs::read(&abs_path).with_context(|| format!("reading {path_str}"))?;
                let obj = repo.odb.read(&ie.oid)?;
                if obj.kind != ObjectKind::Blob {
                    continue;
                }

                if worktree_data != obj.data {
                    file_diffs.push((path_str, obj.data.clone(), worktree_data));
                }
            }
        }
        Some(source_spec) => {
            // Diff working tree against a specific commit's tree
            let source_oid = resolve_to_commit(repo, source_spec)?;
            let tree_oid = commit_to_tree(repo, &source_oid)?;
            let flat = tree_to_flat_entries(repo, &tree_oid, "")?;

            for flat_entry in &flat {
                if flat_entry.mode == MODE_SYMLINK {
                    continue;
                }
                let path_str = String::from_utf8_lossy(&flat_entry.path).to_string();

                if !patch_path_filter_matches(&path_str, &filter_paths) {
                    continue;
                }

                let abs_path = work_tree.join(&path_str);
                let worktree_data = if abs_path.exists() {
                    std::fs::read(&abs_path).with_context(|| format!("reading {path_str}"))?
                } else {
                    Vec::new()
                };

                let obj = repo.odb.read(&flat_entry.oid)?;
                if obj.kind != ObjectKind::Blob {
                    continue;
                }

                if worktree_data != obj.data {
                    file_diffs.push((path_str, obj.data.clone(), worktree_data));
                }
            }
        }
    }

    if file_diffs.is_empty() {
        return Ok(());
    }

    // Sort for deterministic order
    file_diffs.sort_by(|a, b| a.0.cmp(&b.0));

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stderr();

    for (path, source_data, worktree_data) in &file_diffs {
        let source_str = String::from_utf8_lossy(source_data);
        let worktree_str = String::from_utf8_lossy(worktree_data);

        // The diff shows what changed FROM source TO worktree.
        // "Accepting" a hunk means reverting the worktree back to source.
        let text_diff = TextDiff::from_lines(source_str.as_ref(), worktree_str.as_ref());
        let hunks: Vec<_> = text_diff
            .unified_diff()
            .context_radius(3)
            .iter_hunks()
            .collect();

        if hunks.is_empty() {
            continue;
        }

        let mut accept_all = false;
        let mut skip_file = false;
        let mut accepted_hunks: Vec<bool> = vec![false; hunks.len()];

        for (i, hunk) in hunks.iter().enumerate() {
            if skip_file {
                break;
            }
            if accept_all {
                accepted_hunks[i] = true;
                continue;
            }

            // Display the hunk
            writeln!(stdout, "diff --git a/{path} b/{path}").ok();
            write!(stdout, "--- a/{path}\n+++ b/{path}\n").ok();
            write!(stdout, "{hunk}").ok();
            write!(stdout, "Discard this hunk from worktree [y,n,q,a,d,?]? ").ok();
            stdout.flush().ok();

            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                // EOF — keep remaining changes
                break;
            }
            let answer = line.trim();
            match answer {
                "y" | "Y" => {
                    accepted_hunks[i] = true;
                }
                "n" | "N" => { /* keep this hunk (don't revert) */ }
                "a" | "A" => {
                    accepted_hunks[i] = true;
                    accept_all = true;
                }
                "d" | "D" => {
                    skip_file = true;
                }
                "q" | "Q" => {
                    // Apply what we've accepted so far, then return
                    apply_accepted_hunks(
                        repo,
                        work_tree,
                        path,
                        source_data,
                        worktree_data,
                        &accepted_hunks,
                    )?;
                    return Ok(());
                }
                _ => { /* Unrecognized — treat as 'n' */ }
            }
        }

        // Apply accepted hunks for this file
        apply_accepted_hunks(
            repo,
            work_tree,
            path,
            source_data,
            worktree_data,
            &accepted_hunks,
        )?;
    }

    Ok(())
}

/// Blend `source` and `worktree` line-by-line using a Myers line diff: each contiguous group of
/// non-equal ops is one hunk; accepted hunks take the source side, rejected hunks the worktree
/// side. Matches Git add--interactive / stash patch semantics.
pub(crate) fn blend_line_diff_by_hunks(
    source_data: &[u8],
    worktree_data: &[u8],
    accepted: &[bool],
) -> String {
    let source_str = String::from_utf8_lossy(source_data);
    let worktree_str = String::from_utf8_lossy(worktree_data);
    let source_lines: Vec<&str> = source_str.lines().collect();
    let worktree_lines: Vec<&str> = worktree_str.lines().collect();

    let text_diff = similar::TextDiff::from_lines(source_str.as_ref(), worktree_str.as_ref());

    let ops: Vec<_> = text_diff.ops().to_vec();
    let mut hunk_indices: Vec<usize> = Vec::new();
    let mut current_hunk: usize = 0;
    let mut prev_was_change = false;
    for op in &ops {
        match op {
            similar::DiffOp::Equal { .. } => {
                hunk_indices.push(usize::MAX);
                if prev_was_change {
                    current_hunk += 1;
                    prev_was_change = false;
                }
            }
            _ => {
                hunk_indices.push(current_hunk);
                prev_was_change = true;
            }
        }
    }

    let mut output = String::new();
    for (i, op) in ops.iter().enumerate() {
        let hi = hunk_indices[i];
        let is_accepted = hi != usize::MAX && hi < accepted.len() && accepted[hi];

        match op {
            similar::DiffOp::Equal { old_index, len, .. } => {
                for j in 0..*len {
                    output.push_str(source_lines[old_index + j]);
                    output.push('\n');
                }
            }
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => {
                if is_accepted {
                    for j in 0..*old_len {
                        output.push_str(source_lines[old_index + j]);
                        output.push('\n');
                    }
                }
            }
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => {
                if !is_accepted {
                    for j in 0..*new_len {
                        output.push_str(worktree_lines[new_index + j]);
                        output.push('\n');
                    }
                }
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                if is_accepted {
                    for j in 0..*old_len {
                        output.push_str(source_lines[old_index + j]);
                        output.push('\n');
                    }
                } else {
                    for j in 0..*new_len {
                        output.push_str(worktree_lines[new_index + j]);
                        output.push('\n');
                    }
                }
            }
        }
    }

    output
}

/// Like [`blend_line_diff_by_hunks`], but each entry of `ranges` is an inclusive-exclusive span of
/// op indices for one interactive sub-hunk; `accepted[d]` applies to all change ops in `ranges[d]`.
pub(crate) fn blend_line_diff_by_hunk_ranges(
    source_data: &[u8],
    worktree_data: &[u8],
    ranges: &[(usize, usize)],
    accepted: &[bool],
) -> String {
    let source_str = String::from_utf8_lossy(source_data);
    let worktree_str = String::from_utf8_lossy(worktree_data);
    let source_lines: Vec<&str> = source_str.lines().collect();
    let worktree_lines: Vec<&str> = worktree_str.lines().collect();

    let text_diff = similar::TextDiff::from_lines(source_str.as_ref(), worktree_str.as_ref());
    let ops: Vec<_> = text_diff.ops().to_vec();

    fn op_display_hunk(op_i: usize, ranges: &[(usize, usize)]) -> Option<usize> {
        for (d, &(s, e)) in ranges.iter().enumerate() {
            if op_i >= s && op_i < e {
                return Some(d);
            }
        }
        None
    }

    let mut output = String::new();
    for (i, op) in ops.iter().enumerate() {
        match op {
            similar::DiffOp::Equal { old_index, len, .. } => {
                for j in 0..*len {
                    output.push_str(source_lines[old_index + j]);
                    output.push('\n');
                }
            }
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => {
                let disp = op_display_hunk(i, ranges).unwrap_or(0);
                let is_accepted = disp < accepted.len() && accepted[disp];
                if is_accepted {
                    for j in 0..*old_len {
                        output.push_str(source_lines[old_index + j]);
                        output.push('\n');
                    }
                }
            }
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => {
                let disp = op_display_hunk(i, ranges).unwrap_or(0);
                let is_accepted = disp < accepted.len() && accepted[disp];
                if !is_accepted {
                    for j in 0..*new_len {
                        output.push_str(worktree_lines[new_index + j]);
                        output.push('\n');
                    }
                }
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let disp = op_display_hunk(i, ranges).unwrap_or(0);
                let is_accepted = disp < accepted.len() && accepted[disp];
                if is_accepted {
                    for j in 0..*old_len {
                        output.push_str(source_lines[old_index + j]);
                        output.push('\n');
                    }
                } else {
                    for j in 0..*new_len {
                        output.push_str(worktree_lines[new_index + j]);
                        output.push('\n');
                    }
                }
            }
        }
    }

    output
}

/// Apply per-hunk revert decisions: for each accepted hunk, use `source_data`; otherwise keep
/// `worktree_data`. Used by `checkout -p` and `stash -p` (same semantics as Git's add--interactive).
pub(crate) fn apply_accepted_hunks(
    _repo: &Repository,
    work_tree: &std::path::Path,
    path: &str,
    source_data: &[u8],
    worktree_data: &[u8],
    accepted: &[bool],
) -> Result<()> {
    if !accepted.iter().any(|&a| a) {
        return Ok(());
    }

    let abs_path = work_tree.join(path);

    if accepted.iter().all(|&a| a) {
        if source_data.is_empty() {
            let _ = std::fs::remove_file(&abs_path);
        } else {
            if let Some(parent) = abs_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&abs_path, source_data)?;
        }
        return Ok(());
    }

    let output = blend_line_diff_by_hunks(source_data, worktree_data, accepted);
    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs_path, output.as_bytes())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Output messages
// ---------------------------------------------------------------------------

/// Print detached HEAD message.
fn print_detached_head_message(repo: &Repository, oid: &ObjectId) -> Result<()> {
    print_detached_head_message_inner(repo, oid, false)
}

fn print_detached_head_message_explicit(repo: &Repository, oid: &ObjectId) -> Result<()> {
    print_detached_head_message_inner(repo, oid, true)
}

fn print_detached_head_message_inner(
    repo: &Repository,
    oid: &ObjectId,
    explicit_detach_flag: bool,
) -> Result<()> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Commit {
        return Ok(());
    }
    let commit = parse_commit(&obj.data)?;
    let subject = commit.message.lines().next().unwrap_or("").trim();
    let abbrev =
        abbreviate_object_id(repo, *oid, 12).unwrap_or_else(|_| oid.to_hex()[..12].to_owned());

    // Print detached HEAD advice unless:
    // 1. advice.detachedHead is false
    // 2. Explicit --detach was used (suppresses advice)
    let show_advice = !explicit_detach_flag
        && match ConfigSet::load(Some(&repo.git_dir), true) {
            Ok(config) => match config.get_bool("advice.detachedHead") {
                Some(Ok(val)) => val,
                _ => true, // default: show advice
            },
            Err(_) => true,
        };
    if show_advice {
        checkout_eprintln!(
            "Note: switching to '{}'.\n\
             \n\
             You are in 'detached HEAD' state. You can look around, make experimental\n\
             changes and commit them, and you can discard any commits you make in this\n\
             state without impacting any branches by switching back to a branch.\n\
             \n\
             If you want to create a new branch to retain commits you create, you may\n\
             do so (now or later) by using -c with the switch command. Example:\n\
             \n\
               git switch -c <new-branch-name>\n\
             \n\
             Or undo this operation with:\n\
             \n\
               git switch -\n\
             \n\
             Turn off this advice by setting config variable advice.detachedHead to false\n",
            oid
        );
    }

    checkout_eprintln!("HEAD is now at {} {}", abbrev, subject);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tracking (upstream) configuration
// ---------------------------------------------------------------------------

/// Set up tracking configuration for a newly created branch.
///
/// With `--track`, sets `branch.<name>.remote` and `branch.<name>.merge`.
/// Also respects `branch.autoSetupMerge` config.
fn maybe_setup_tracking(
    repo: &Repository,
    branch_name: &str,
    start_point: Option<&str>,
    track_mode: Option<&str>,
) -> Result<()> {
    let start_raw = match start_point {
        Some(s) => s,
        None => return Ok(()),
    };

    let start_name: String = if start_raw == "HEAD" {
        match resolve_head(&repo.git_dir)? {
            HeadState::Branch { short_name, .. } => short_name,
            _ => {
                bail!("fatal: cannot set up tracking information; starting point 'HEAD' is not a branch");
            }
        }
    } else {
        start_raw.to_string()
    };
    let start = start_name.as_str();

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let effective_mode = if let Some(mode) = track_mode {
        mode.to_string()
    } else {
        let auto = config.get("branch.autoSetupMerge").unwrap_or_default();
        match auto.as_str() {
            "always" => "direct".to_string(),
            "inherit" => "inherit".to_string(),
            "false" | "never" => return Ok(()),
            // Default (unset / true / simple): allow automatic upstream setup from the start
            // point (e.g. `checkout -b topic origin` tracks origin's default branch).
            _ => "direct".to_string(),
        }
    };

    if effective_mode == "inherit" {
        let remote = config
            .get(&format!("branch.{start}.remote"))
            .unwrap_or_default();
        let merge_ref = config
            .get(&format!("branch.{start}.merge"))
            .unwrap_or_default();
        if !remote.is_empty() && !merge_ref.is_empty() {
            let config_path = repo.git_dir.join("config");
            let mut config_content = std::fs::read_to_string(&config_path).unwrap_or_default();
            let section = format!(
                "\n[branch \"{}\"]\
                \n\tremote = {}\
                \n\tmerge = {}\n",
                branch_name, remote, merge_ref
            );
            config_content.push_str(&section);
            std::fs::write(&config_path, config_content)?;
        }
        return Ok(());
    }

    // `checkout -b topic origin` (or other remote name): track the remote's default branch
    // (`refs/remotes/<remote>/HEAD`). Only when `start` is a single path segment — not
    // `origin/main` (that would wrongly use `refs/remotes/origin/main/HEAD` and hit ENOTDIR).
    if !start.contains('/') && !start.is_empty() {
        let remote_head_sym = format!("refs/remotes/{start}/HEAD");
        if refs::read_symbolic_ref(&repo.git_dir, &remote_head_sym)?.is_some()
            || refs::resolve_ref(&repo.git_dir, &remote_head_sym).is_ok()
        {
            let merge_branch = match refs::read_symbolic_ref(&repo.git_dir, &remote_head_sym)? {
                Some(target) => target
                    .strip_prefix(&format!("refs/remotes/{start}/"))
                    .map(|s| s.to_string())
                    .filter(|b| !b.is_empty() && b != "HEAD"),
                None => None,
            };
            if let Some(branch) = merge_branch {
                let config_path = repo.git_dir.join("config");
                let mut config_content = std::fs::read_to_string(&config_path).unwrap_or_default();
                let section = format!(
                    "\n[branch \"{}\"]\
                    \n\tremote = {}\
                    \n\tmerge = refs/heads/{}\n",
                    branch_name, start, branch
                );
                config_content.push_str(&section);
                std::fs::write(&config_path, config_content)?;
                checkout_eprintln!("branch '{branch_name}' set up to track '{start}/{branch}'.");
                return Ok(());
            }
        }
    }

    // `checkout -b topic origin/main` — track the remote-tracking ref `refs/remotes/origin/main`.
    // Split only on the first `/` so remotes are single-segment (`origin`) and the rest is the
    // upstream branch name (may itself contain `/`).
    if let Some(slash) = start.find('/') {
        let remote = &start[..slash];
        let branch_on_remote = &start[slash + 1..];
        if !remote.is_empty() && !branch_on_remote.is_empty() {
            let tracking = format!("refs/remotes/{remote}/{branch_on_remote}");
            if refs::resolve_ref(&repo.git_dir, &tracking).is_ok() {
                let config_path = repo.git_dir.join("config");
                let mut config_content = std::fs::read_to_string(&config_path).unwrap_or_default();
                let section = format!(
                    "\n[branch \"{}\"]\
                    \n\tremote = {}\
                    \n\tmerge = refs/heads/{}\n",
                    branch_name, remote, branch_on_remote
                );
                config_content.push_str(&section);
                std::fs::write(&config_path, config_content)?;
                checkout_eprintln!(
                    "branch '{branch_name}' set up to track '{remote}/{branch_on_remote}'."
                );
                return Ok(());
            }
        }
    }

    let start_ref = format!("refs/heads/{start}");
    if refs::resolve_ref(&repo.git_dir, &start_ref).is_ok() {
        let config_path = repo.git_dir.join("config");
        let mut config_content = std::fs::read_to_string(&config_path).unwrap_or_default();

        let section = format!(
            "\n[branch \"{}\"]\
            \n\tremote = .\
            \n\tmerge = {}\n",
            branch_name, start_ref
        );
        config_content.push_str(&section);
        std::fs::write(&config_path, config_content)?;

        checkout_eprintln!("branch '{}' set up to track '{}'.", branch_name, start);
        return Ok(());
    }

    // Start point may be an upstream expression (e.g. `my-side@{u}`) resolving to a
    // remote-tracking or local merge ref.
    if let Ok(full) = resolve_upstream_symbolic_name(repo, start) {
        let config_path = repo.git_dir.join("config");
        let mut config_content = std::fs::read_to_string(&config_path).unwrap_or_default();
        let section = if let Some(rest) = full.strip_prefix("refs/remotes/") {
            if let Some(slash) = rest.find('/') {
                let remote = &rest[..slash];
                let branch = &rest[slash + 1..];
                format!(
                    "\n[branch \"{}\"]\
                    \n\tremote = {}\
                    \n\tmerge = refs/heads/{}\n",
                    branch_name, remote, branch
                )
            } else {
                return Ok(());
            }
        } else if full.starts_with("refs/heads/") {
            format!(
                "\n[branch \"{}\"]\
                \n\tremote = .\
                \n\tmerge = {}\n",
                branch_name, full
            )
        } else {
            return Ok(());
        };
        config_content.push_str(&section);
        std::fs::write(&config_path, config_content)?;
        checkout_eprintln!("branch '{branch_name}' set up to track '{start}'.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tree / object helpers (local to this command)
// ---------------------------------------------------------------------------

/// Resolve a revision spec to a commit OID, peeling through tags.
fn resolve_to_commit(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    peel_to_commit(repo, oid)
}

/// Peel an OID to a commit (follows tag chains).
fn peel_to_commit(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    for _ in 0..10 {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Commit => return Ok(oid),
            ObjectKind::Tag => {
                let text = std::str::from_utf8(&obj.data).context("tag is not UTF-8")?;
                let target_hex = text
                    .lines()
                    .find_map(|l| l.strip_prefix("object "))
                    .ok_or_else(|| anyhow::anyhow!("tag missing 'object' header"))?
                    .trim();
                oid = target_hex.parse()?;
            }
            _ => bail!("'{}' is not a commit-ish", oid),
        }
    }
    bail!("too many levels of tag dereferencing")
}

/// Extract the tree OID from a commit object.
fn commit_to_tree(repo: &Repository, commit_oid: &ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(commit_oid)?;
    if obj.kind != ObjectKind::Commit {
        bail!("not a commit: {commit_oid}");
    }
    let commit = parse_commit(&obj.data)?;
    Ok(commit.tree)
}

/// Recursively flatten a tree object into a list of [`IndexEntry`] values.
fn tree_to_flat_entries(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            result.extend(tree_to_flat_entries(repo, &te.oid, &path)?);
        } else {
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: te.mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
            });
        }
    }
    Ok(result)
}

/// True when the index's stage-0 paths match the flattened tree (mode + OID per path).
fn index_matches_flat_tree(repo: &Repository, tree_oid: &ObjectId) -> Result<bool> {
    let index_path = repo.index_path();
    let old_index = repo.load_index_at(&index_path).unwrap_or_default();
    let new_entries = tree_to_flat_entries(repo, tree_oid, "")?;
    let old_stage0: Vec<&IndexEntry> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .collect();
    if old_stage0.len() != new_entries.len() {
        return Ok(false);
    }
    let mut old_sorted: Vec<_> = old_stage0
        .into_iter()
        .map(|e| (&e.path[..], e.mode, e.oid))
        .collect();
    let mut new_sorted: Vec<_> = new_entries
        .iter()
        .map(|e| (e.path.as_slice(), e.mode, e.oid))
        .collect();
    old_sorted.sort_by(|a, b| a.0.cmp(b.0));
    new_sorted.sort_by(|a, b| a.0.cmp(b.0));
    Ok(old_sorted == new_sorted)
}

/// Walk a tree to find the blob (OID, mode) at `path` (slash-separated).
fn find_in_tree(
    repo: &Repository,
    tree_oid: ObjectId,
    path: &str,
) -> Result<Option<(ObjectId, u32)>> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    find_recursive(repo, tree_oid, &parts)
}

fn find_recursive(
    repo: &Repository,
    tree_oid: ObjectId,
    parts: &[&str],
) -> Result<Option<(ObjectId, u32)>> {
    if parts.is_empty() {
        return Ok(None);
    }

    let tree_obj = repo
        .odb
        .read(&tree_oid)
        .with_context(|| format!("reading tree {tree_oid}"))?;
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(None);
    }

    let entries = parse_tree(&tree_obj.data)?;
    let name_bytes = parts[0].as_bytes();
    let Some(entry) = entries.iter().find(|e| e.name == name_bytes) else {
        return Ok(None);
    };

    if parts.len() == 1 {
        Ok(Some((entry.oid, entry.mode)))
    } else {
        find_recursive(repo, entry.oid, &parts[1..])
    }
}

// ---------------------------------------------------------------------------
// Working tree helpers
// ---------------------------------------------------------------------------

/// Update the working tree from old_index to new_index: remove deleted files,
/// add new files, update modified files.
fn checkout_index_to_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    work_tree: &std::path::Path,
    force_write_all: bool,
) -> Result<()> {
    let old_stage0: HashSet<Vec<u8>> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    let new_stage0: HashSet<Vec<u8>> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();

    // Build old index map for OID comparison
    let old_map: HashMap<&[u8], &IndexEntry> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    let sparse_checkout = ConfigSet::load(Some(&repo.git_dir), true)
        .unwrap_or_default()
        .get("core.sparsecheckout")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let new_map: HashMap<&[u8], &IndexEntry> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    // Remove paths that are no longer present in the new index.
    for old_path in old_stage0.difference(&new_stage0) {
        if let Some(old_entry) = old_map.get(old_path.as_slice()) {
            // Superproject: do not delete submodule work trees for gitlinks dropped from the index
            // (Git keeps them on disk; t7300-clean). Nested submodule repos under `.git/modules/`
            // still use normal removal so `git checkout` can refresh the nested worktree.
            if old_entry.mode == MODE_GITLINK && !git_dir_is_nested_modules_repo(&repo.git_dir) {
                continue;
            }
        }
        let rel = String::from_utf8_lossy(old_path).into_owned();
        let abs = work_tree.join(&rel);
        // Safety: don't follow symlinks when removing paths.
        // Check if any parent path component is a symlink.
        let path_through_symlink = {
            let mut p = work_tree.to_path_buf();
            let mut through_sym = false;
            for component in std::path::Path::new(&rel).components() {
                p.push(component);
                if let Ok(meta) = std::fs::symlink_metadata(&p) {
                    if meta.file_type().is_symlink() && p != abs {
                        through_sym = true;
                        break;
                    }
                }
            }
            through_sym
        };
        if path_through_symlink {
            continue; // Skip: path goes through a symlink
        }
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        } else if abs.is_dir() {
            // Removing a gitlink from the index must not delete a populated submodule checkout
            // (directory with `.git`), matching Git and `t4137-apply-submodule`.
            let is_populated_submodule = old_map
                .get(old_path.as_slice())
                .is_some_and(|e| e.mode == MODE_GITLINK && abs.join(".git").exists());
            if !is_populated_submodule {
                let _ = std::fs::remove_dir_all(&abs);
            }
        }
        remove_empty_parent_dirs(work_tree, &abs);
    }

    // Sparse checkout: paths still in the index but newly excluded must disappear from the work tree.
    if sparse_checkout {
        for old_path in old_stage0.intersection(&new_stage0) {
            if new_map
                .get(old_path.as_slice())
                .is_some_and(|e| e.skip_worktree())
            {
                if let Some(old_entry) = old_map.get(old_path.as_slice()) {
                    if old_entry.mode == MODE_GITLINK
                        && !git_dir_is_nested_modules_repo(&repo.git_dir)
                    {
                        continue;
                    }
                }
                let rel = String::from_utf8_lossy(old_path).into_owned();
                let abs = work_tree.join(&rel);
                let path_through_symlink = {
                    let mut p = work_tree.to_path_buf();
                    let mut through_sym = false;
                    for component in std::path::Path::new(&rel).components() {
                        p.push(component);
                        if let Ok(meta) = std::fs::symlink_metadata(&p) {
                            if meta.file_type().is_symlink() && p != abs {
                                through_sym = true;
                                break;
                            }
                        }
                    }
                    through_sym
                };
                if path_through_symlink {
                    continue;
                }
                if abs.is_file() || abs.is_symlink() {
                    let _ = std::fs::remove_file(&abs);
                } else if abs.is_dir() {
                    let is_populated_submodule = old_map
                        .get(old_path.as_slice())
                        .is_some_and(|e| e.mode == MODE_GITLINK && abs.join(".git").exists());
                    if !is_populated_submodule {
                        let _ = std::fs::remove_dir_all(&abs);
                    }
                }
                remove_empty_parent_dirs(work_tree, &abs);
            }
        }
    }

    // Write new/modified entries
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if entry.skip_worktree() {
            continue;
        }

        // Skip gitlink (submodule) entries — their OIDs reference commits
        // in the submodule's object store, not blobs in ours.
        if entry.mode == MODE_GITLINK {
            let rel = String::from_utf8_lossy(&entry.path).into_owned();
            let force_populate = match old_map.get(entry.path.as_slice()) {
                None => true,
                Some(old) => old.mode != MODE_GITLINK || old.oid != entry.oid,
            };
            checkout_gitlink_worktree_entry(repo, work_tree, &rel, &entry.oid, force_populate)?;
            continue;
        }

        // Skip unchanged entries (same OID and mode) — but only if file exists
        // and we're not in force mode.
        if !force_write_all {
            if let Some(old_entry) = old_map.get(entry.path.as_slice()) {
                if old_entry.oid == entry.oid && old_entry.mode == entry.mode {
                    let abs_path = work_tree.join(String::from_utf8_lossy(&entry.path).as_ref());
                    if abs_path.exists() || abs_path.is_symlink() {
                        continue;
                    }
                    // File was deleted from worktree, restore it
                }
            }
        }

        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let _ = write_blob_to_worktree(
            repo, work_tree, &path_str, &entry.oid, entry.mode, new_index, true,
        )?;
    }

    let _ = crate::commands::submodule::refresh_submodule_gitfiles(repo);

    Ok(())
}

fn git_dir_is_nested_modules_repo(git_dir: &Path) -> bool {
    git_dir
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "modules")
}

/// Write a blob object to the working tree.
///
/// Returns `Ok(true)` when the path was written or updated, `Ok(false)` when the work tree already
/// matched (so user-facing counts like "Updated N paths" stay accurate; t2080).
///
/// `full_smudge_meta`: when true, process smudge gets `ref=` / `treeish=` (branch/tree checkout).
/// Path-only checkout passes blob id only.
fn write_blob_to_worktree(
    repo: &Repository,
    work_tree: &std::path::Path,
    rel_path: &str,
    oid: &ObjectId,
    mode: u32,
    index: &Index,
    full_smudge_meta: bool,
) -> Result<bool> {
    if mode == MODE_GITLINK {
        // Path checkout from index: always materialize (may follow symlinked submodule paths).
        checkout_gitlink_worktree_entry(repo, work_tree, rel_path, oid, false)?;
        return Ok(true);
    }

    let obj = repo.odb.read(oid).context("reading object for checkout")?;
    if obj.kind != ObjectKind::Blob {
        bail!("cannot checkout non-blob at '{rel_path}'");
    }

    // Path-only checkout: if the work tree already matches the *smudged* blob Git would write
    // (EOL + ident + filters), skip rewriting — matches Git / t0021 filter log expectations.
    // Compare against smudge output, not clean: `core.eol=crlf` can change bytes without changing
    // the index blob (t0027).
    if !full_smudge_meta && mode != MODE_SYMLINK {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conv = crlf::ConversionConfig::from_config(&config);
        let attrs = crlf::load_gitattributes_for_checkout(work_tree, rel_path, index, &repo.odb);
        let file_attrs = crlf::get_file_attrs(&attrs, rel_path, false, &config);
        if file_attrs.working_tree_encoding.is_none() {
            let abs_path = work_tree.join(rel_path);
            if let Ok(wt_raw) = std::fs::read(&abs_path) {
                let oid_hex = format!("{oid}");
                let smudge_meta = filter_process::smudge_meta_blob_only(&oid_hex);
                if let Ok(smudged) = crlf::convert_to_worktree(
                    &obj.data,
                    rel_path,
                    &conv,
                    &file_attrs,
                    Some(&oid_hex),
                    Some(&smudge_meta),
                ) {
                    if smudged == wt_raw {
                        return Ok(false);
                    }
                }
            }
        }
    }

    // Apply CRLF / smudge conversion for checkout
    let data = if mode != MODE_SYMLINK {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conv = crlf::ConversionConfig::from_config(&config);
        let attrs = crlf::load_gitattributes_for_checkout(work_tree, rel_path, index, &repo.odb);
        let file_attrs = crlf::get_file_attrs(&attrs, rel_path, false, &config);
        let oid_hex = format!("{oid}");
        let smudge_meta = if full_smudge_meta {
            filter_process::smudge_meta_for_checkout(repo, &oid_hex)
        } else {
            filter_process::smudge_meta_blob_only(&oid_hex)
        };
        crlf::convert_to_worktree(
            &obj.data,
            rel_path,
            &conv,
            &file_attrs,
            Some(&oid_hex),
            Some(&smudge_meta),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        obj.data
    };

    // Skip writing if the file already has the same content (preserves mtime).
    // Still align the executable bit with the index so `git status` stays clean
    // after a mode-only amend (t3419-rebase-patch-id).
    if mode != MODE_SYMLINK {
        let abs_path = work_tree.join(rel_path);
        if let Ok(existing) = std::fs::read(&abs_path) {
            if existing == *data {
                apply_index_file_mode(&abs_path, mode)?;
                return Ok(false);
            }
        }
    }

    write_to_worktree(work_tree, rel_path, &data, mode)?;
    Ok(true)
}

/// Set `abs_path` permissions to match Git index `mode` (regular vs executable blob).
fn apply_index_file_mode(abs_path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(abs_path)?.permissions();
    let new_mode = if mode == MODE_EXECUTABLE {
        0o755
    } else {
        0o644
    };
    perms.set_mode(new_mode);
    std::fs::set_permissions(abs_path, perms)?;
    Ok(())
}

fn checkout_conflicted_path_with_merge(
    repo: &Repository,
    work_tree: &Path,
    rel_path: &str,
    base: Option<&IndexEntry>,
    ours: Option<&IndexEntry>,
    theirs: Option<&IndexEntry>,
    merge_cli: &CheckoutMergeCli,
) -> Result<()> {
    let ours_entry = ours
        .or(theirs)
        .ok_or_else(|| anyhow::anyhow!("path '{rel_path}' does not have unmerged entries"))?;
    let theirs_entry = theirs
        .or(ours)
        .ok_or_else(|| anyhow::anyhow!("path '{rel_path}' does not have unmerged entries"))?;

    let base_data = if let Some(entry) = base {
        let obj = repo.odb.read(&entry.oid)?;
        if obj.kind != ObjectKind::Blob {
            bail!("cannot checkout non-blob at '{rel_path}'");
        }
        obj.data
    } else {
        Vec::new()
    };

    let ours_obj = repo.odb.read(&ours_entry.oid)?;
    let theirs_obj = repo.odb.read(&theirs_entry.oid)?;
    if ours_obj.kind != ObjectKind::Blob || theirs_obj.kind != ObjectKind::Blob {
        bail!("cannot checkout non-blob at '{rel_path}'");
    }

    if merge_file::is_binary(&ours_obj.data) || merge_file::is_binary(&theirs_obj.data) {
        return write_to_worktree(work_tree, rel_path, &ours_obj.data, ours_entry.mode);
    }

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let attrs = crlf::load_gitattributes(work_tree);
    let file_attrs = crlf::get_file_attrs(&attrs, rel_path, false, &config);
    let marker_size = if let Some(raw) = &file_attrs.conflict_marker_size {
        match raw.parse::<usize>() {
            Ok(size) => size,
            Err(_) => {
                eprintln!("warning: invalid marker-size '{raw}', expecting an integer");
                7
            }
        }
    } else {
        7
    };

    let mut style = match config
        .get("merge.conflictstyle")
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("diff3" | "zdiff3") => ConflictStyle::Diff3,
        _ => ConflictStyle::Merge,
    };
    if let Some(s) = merge_cli.conflict_style {
        style = s;
    }
    if merge_cli.force_two_way_markers {
        style = ConflictStyle::Merge;
    }

    if let Some((driver_cmd, recursive_binary)) = resolve_path_merge_driver_command(repo, rel_path)
    {
        if recursive_binary
            || merge_file::is_binary(&ours_obj.data)
            || merge_file::is_binary(&theirs_obj.data)
        {
            return write_to_worktree(work_tree, rel_path, &ours_obj.data, ours_entry.mode);
        }
        let (merged, _code) = execute_custom_merge_driver(
            &driver_cmd,
            rel_path,
            &base_data,
            &ours_obj.data,
            &theirs_obj.data,
            "base",
            "ours",
            "theirs",
        )?;
        return write_to_worktree(work_tree, rel_path, &merged, ours_entry.mode);
    }

    let merge_out = merge_file::merge(&MergeInput {
        base: &base_data,
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: "ours",
        label_base: "base",
        label_theirs: "theirs",
        favor: MergeFavor::None,
        style,
        marker_size,
        diff_algorithm: None,
        ignore_all_space: false,
        ignore_space_change: false,
        ignore_space_at_eol: false,
        ignore_cr_at_eol: false,
    })?;

    write_to_worktree(work_tree, rel_path, &merge_out.content, ours_entry.mode)
}

/// Ensure each component of `rel_path`'s parent exists as a real directory.
///
/// Replaces a parent path that is a symlink or regular file (e.g. `D` → `untracked` or `D` as a
/// file) so `mkdir -p` can create `D/A` during checkout (`t2080` force checkout cases).
fn prepare_parent_dirs_for_checkout(work_tree: &std::path::Path, rel_path: &str) -> Result<()> {
    use std::path::Component;
    let path = std::path::Path::new(rel_path);
    let Some(parent_rel) = path.parent() else {
        return Ok(());
    };
    if parent_rel.as_os_str().is_empty() {
        return Ok(());
    }
    let mut cur = work_tree.to_path_buf();
    for comp in parent_rel.components() {
        if let Component::Normal(name) = comp {
            cur.push(name);
            if let Ok(meta) = std::fs::symlink_metadata(&cur) {
                if meta.file_type().is_symlink() {
                    std::fs::remove_file(&cur)?;
                } else if !meta.is_dir() {
                    std::fs::remove_file(&cur)?;
                }
            }
        }
    }
    Ok(())
}

/// Write data to a working tree file, handling symlinks and executable bits.
fn write_to_worktree(
    work_tree: &std::path::Path,
    rel_path: &str,
    data: &[u8],
    mode: u32,
) -> Result<()> {
    let abs_path = work_tree.join(rel_path);

    prepare_parent_dirs_for_checkout(work_tree, rel_path)?;
    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent directories for '{rel_path}'"))?;
    }

    // Remove existing file/dir/symlink at target path. Use symlink_metadata + is_symlink so we
    // replace symlinked paths (e.g. `D` → `untracked`) before creating a real directory tree.
    if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
        if meta.file_type().is_symlink() {
            std::fs::remove_file(&abs_path)?;
        } else if meta.is_dir() {
            std::fs::remove_dir_all(&abs_path)?;
        } else {
            std::fs::remove_file(&abs_path)?;
        }
    }

    if mode == MODE_SYMLINK {
        let target = std::str::from_utf8(data)
            .with_context(|| format!("symlink target for '{rel_path}' is not UTF-8"))?;
        std::os::unix::fs::symlink(target, &abs_path)
            .with_context(|| format!("creating symlink '{rel_path}'"))?;
    } else {
        std::fs::write(&abs_path, data).with_context(|| format!("writing '{rel_path}'"))?;

        if mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&abs_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&abs_path, perms)?;
        }
    }

    Ok(())
}

/// Remove empty parent directories up to (but not including) `work_tree`.
fn remove_empty_parent_dirs(work_tree: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match std::fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

/// Check if a pathspec contains glob characters.
fn is_glob_pattern(spec: &str) -> bool {
    spec.contains('*') || spec.contains('?') || spec.contains('[')
}

/// Match a path against a simple glob pattern.
/// Supports `*` (any chars except `/`), `?` (any single char except `/`),
/// and character classes `[abc]`.
fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_matches_inner(pattern.as_bytes(), path.as_bytes())
}

fn glob_matches_inner(pattern: &[u8], path: &[u8]) -> bool {
    let mut pi = 0; // pattern index
    let mut si = 0; // string index
    let mut star_pi = usize::MAX;
    let mut star_si = 0;

    while si < path.len() {
        if pi < pattern.len() && pattern[pi] == b'?' {
            pi += 1;
            si += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            if pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
                // "**" matches everything including '/'
                // For simplicity, try matching rest of pattern at every position
                let rest = &pattern[pi + 2..];
                // Skip optional '/' after **
                let rest = if !rest.is_empty() && rest[0] == b'/' {
                    &rest[1..]
                } else {
                    rest
                };
                for i in si..=path.len() {
                    if glob_matches_inner(rest, &path[i..]) {
                        return true;
                    }
                }
                return false;
            }
            star_pi = pi;
            star_si = si;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            // Character class
            pi += 1;
            let negate = pi < pattern.len() && (pattern[pi] == b'!' || pattern[pi] == b'^');
            if negate {
                pi += 1;
            }
            let mut found = false;
            let ch = path[si];
            while pi < pattern.len() && pattern[pi] != b']' {
                if pi + 2 < pattern.len() && pattern[pi + 1] == b'-' {
                    if ch >= pattern[pi] && ch <= pattern[pi + 2] {
                        found = true;
                    }
                    pi += 3;
                } else {
                    if ch == pattern[pi] {
                        found = true;
                    }
                    pi += 1;
                }
            }
            if pi < pattern.len() {
                pi += 1;
            } // skip ']'
            if found == negate {
                // Mismatch in character class
                if star_pi != usize::MAX {
                    pi = star_pi + 1;
                    star_si += 1;
                    si = star_si;
                } else {
                    return false;
                }
            } else {
                si += 1;
            }
        } else if pi < pattern.len() && pattern[pi] == path[si] {
            pi += 1;
            si += 1;
        } else if star_pi != usize::MAX {
            // Backtrack: '*' matches one more character (including '/')
            pi = star_pi + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    // Consume trailing '*' or '**' in pattern
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// True when `path_str` matches any of the `filter_paths` from interactive patch commands.
pub(crate) fn patch_path_filter_matches(path_str: &str, filter_paths: &[String]) -> bool {
    if filter_paths.is_empty() {
        return true;
    }
    filter_paths.iter().any(|fp| {
        if is_glob_pattern(fp) {
            glob_matches(fp, path_str)
        } else if fp.is_empty() || fp == "." {
            true
        } else if fp.ends_with('/') {
            path_str.starts_with(fp.as_str())
        } else {
            path_str == *fp || path_str.starts_with(&format!("{fp}/"))
        }
    })
}

/// Resolve a pathspec to a repository-relative path (used by `checkout -p` / `reset -p`).
pub(crate) fn resolve_pathspec(spec: &str, work_tree: &Path, cwd: &Path) -> String {
    // Handle :/ prefix (repo root)
    if spec == ":/" || spec.starts_with(":/") {
        let rest = &spec[2..];
        return rest.to_owned();
    }

    let candidate = std::path::PathBuf::from(spec);
    let abs = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(&candidate)
    };

    // Normalize the path (resolve .. and . components) without requiring
    // the path to exist on disk (unlike canonicalize).
    let normalized = normalize_path(&abs);

    if let Ok(rel) = normalized.strip_prefix(work_tree) {
        rel.to_string_lossy().into_owned()
    } else {
        spec.to_owned()
    }
}

/// Normalize a path by resolving `.` and `..` components lexically.
fn normalize_path(path: &Path) -> std::path::PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

/// Write reflog entries for a checkout operation.
/// Resolve the previous branch from the HEAD reflog.
/// Looks for the most recent "checkout: moving from X to Y" entry and returns X.
/// Resolve `@{-N}` syntax to a branch name, returning the original string if not applicable.
fn resolve_at_minus(repo: &Repository, spec: &str) -> Result<String> {
    if spec.starts_with("@{-") && spec.ends_with('}') {
        if let Ok(n) = spec[3..spec.len() - 1].parse::<usize>() {
            return resolve_nth_previous_branch(repo, n);
        }
    }
    Ok(spec.to_string())
}

fn resolve_previous_branch(repo: &Repository) -> Result<String> {
    resolve_nth_previous_branch(repo, 1)
}

/// Resolve the Nth previously checked out branch from the HEAD reflog.
fn resolve_nth_previous_branch(repo: &Repository, n: usize) -> Result<String> {
    let reflog_path = repo.git_dir.join("logs/HEAD");
    let content = std::fs::read_to_string(&reflog_path).context("cannot read HEAD reflog")?;
    let mut seen = Vec::new();
    for line in content.lines().rev() {
        if let Some(msg_start) = line.find("checkout: moving from ") {
            let rest = &line[msg_start + "checkout: moving from ".len()..];
            if let Some(to_idx) = rest.find(" to ") {
                let from = &rest[..to_idx];
                // Only add if not already the most recently seen
                if seen.last().is_none_or(|last: &String| last != from) {
                    seen.push(from.to_string());
                }
                if seen.len() >= n {
                    return Ok(seen[n - 1].clone());
                }
            }
        }
    }
    bail!("no previous branch found in reflog")
}

/// Append the first `branch: Created from …` line for a newly created branch ref (`checkout -b`).
fn append_branch_created_reflog(
    repo: &Repository,
    branch_ref: &str,
    start_desc: &str,
    tip_oid: &ObjectId,
    user_requested_reflog: bool,
) {
    let identity = resolve_checkout_identity(repo);
    let msg = format!("branch: Created from {start_desc}");
    let force_create =
        user_requested_reflog && !refs::should_autocreate_reflog(&repo.git_dir, branch_ref);
    let old_zero = zero_oid();
    let _ = append_reflog(
        &repo.git_dir,
        branch_ref,
        &old_zero,
        tip_oid,
        &identity,
        &msg,
        force_create,
    );
}

fn write_checkout_reflog(
    repo: &Repository,
    _head: &HeadState,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    message: &str,
) {
    let identity = resolve_checkout_identity(repo);

    // Git records `checkout: moving from X to Y` on `logs/HEAD` only. The branch ref we are
    // leaving keeps its reflog unchanged (see t3406-rebase-message reflog expectations).
    let _ = append_reflog(
        &repo.git_dir,
        "HEAD",
        old_oid,
        new_oid,
        &identity,
        message,
        false,
    );
}

/// Resolve the committer identity for reflog entries.
fn resolve_checkout_identity(repo: &Repository) -> String {
    let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_NAME").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_EMAIL").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();
    let timestamp = std::env::var("GIT_COMMITTER_DATE")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok())
        .map(|d| super::commit::parse_date_to_git_timestamp(&d).unwrap_or(d))
        .unwrap_or_else(|| {
            let now = time::OffsetDateTime::now_utc();
            let epoch = now.unix_timestamp();
            let offset = now.offset();
            let hours = offset.whole_hours();
            let minutes = offset.minutes_past_hour().unsigned_abs();
            format!("{epoch} {hours:+03}{minutes:02}")
        });
    format!("{name} <{email}> {timestamp}")
}
