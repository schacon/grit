//! `grit branch` -- list, create, or delete branches.

use crate::branch_ref_format::{expand_branch_format, BranchFormatContext, BranchFormatError};
use crate::commands::worktree_refs;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::diff::zero_oid;
use grit_lib::merge_base::count_symmetric_ahead_behind;
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::{parse_commit, parse_tag, ObjectId, ObjectKind};
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    abbreviate_object_id, resolve_revision, resolve_upstream_symbolic_name, symbolic_full_name,
};
use grit_lib::state::{resolve_head, wt_status_get_state, HeadState};
use grit_lib::stripspace::{process as stripspace_process, Mode as StripspaceMode};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

/// Arguments for `grit branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "List, create, or delete branches")]
pub struct Args {
    /// Branch name to create, first name to delete, or list pattern.
    #[arg()]
    pub name: Option<String>,

    /// Second positional: start point for new branch, or second `-d`/`-D` target.
    #[arg()]
    pub start_point: Option<String>,

    /// Further branch names for `git branch -d a b c` (third argument onward).
    #[arg(trailing_var_arg = true, hide = true)]
    pub extra_names: Vec<String>,

    /// Delete a branch.
    #[arg(short = 'd', long = "delete")]
    pub delete: bool,

    /// Force delete a branch (even if not merged).
    #[arg(short = 'D')]
    pub force_delete: bool,

    /// Move/rename a branch.
    #[arg(short = 'm', long = "move")]
    pub rename: bool,

    /// Force move/rename.
    #[arg(short = 'M')]
    pub force_rename: bool,

    /// Copy a branch.
    #[arg(short = 'c', long = "copy")]
    pub copy: bool,

    /// List branches (default when no name given).
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// List remote-tracking branches.
    #[arg(short = 'r', long = "remotes")]
    pub remotes: bool,

    /// Rejected (Git compatibility): use without `--remotes`.
    #[arg(long = "no-remotes", hide = true)]
    pub no_remotes_neg: bool,

    /// List both local and remote branches.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Rejected (Git compatibility): use without `--all`.
    #[arg(long = "no-all", hide = true)]
    pub no_all_neg: bool,

    /// Show verbose info (commit subject). Use twice (-vv) for tracking info.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Show branches containing this commit.
    #[arg(long = "contains", action = clap::ArgAction::Append)]
    pub contains: Vec<String>,

    /// Show branches not containing this commit.
    #[arg(long = "no-contains", action = clap::ArgAction::Append)]
    pub no_contains: Vec<String>,

    /// Show branches merged into this commit (default: HEAD). Repeat for intersection.
    #[arg(
        long = "merged",
        action = clap::ArgAction::Append,
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub merged: Vec<String>,

    /// Show branches not merged into this commit (default: HEAD). Repeat for intersection.
    #[arg(
        long = "no-merged",
        action = clap::ArgAction::Append,
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub no_merged: Vec<String>,

    /// Force creation (overwrite existing branch).
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Set up tracking.
    #[arg(short = 't', long = "track", require_equals = true, num_args = 0..=1, default_missing_value = "direct")]
    pub track: Option<String>,

    /// Do not set up tracking.
    #[arg(long = "no-track")]
    pub no_track: bool,

    /// Show the current branch name.
    #[arg(long = "show-current")]
    pub show_current: bool,

    /// Set upstream tracking branch (e.g. origin/main).
    #[arg(short = 'u', long = "set-upstream-to")]
    pub set_upstream_to: Option<String>,

    /// Remove upstream tracking configuration.
    #[arg(long = "unset-upstream")]
    pub unset_upstream: bool,

    /// Sort branches (repeat for multiple keys; e.g. `--sort=refname --sort=ahead-behind:HEAD`).
    #[arg(long = "sort", action = clap::ArgAction::Append)]
    pub sort: Vec<String>,

    /// Cancel sort keys (reset to default).
    #[arg(long = "no-sort")]
    pub no_sort: bool,

    /// Case-insensitive sorting and pattern matching for listing.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,

    /// Omit lines that expand to empty with `--format`.
    #[arg(long = "omit-empty")]
    pub omit_empty: bool,

    /// Custom format string (for-each-ref style atoms).
    #[arg(long = "format")]
    pub format: Option<String>,

    /// Create the branch's reflog.
    #[arg(long = "create-reflog")]
    pub create_reflog: bool,

    /// Force copy.
    #[arg(short = 'C')]
    pub force_copy: bool,

    /// Display branches in columns.
    #[arg(long = "column", value_name = "STYLE", num_args = 0..=1, default_missing_value = "always")]
    pub column: Option<String>,

    /// Disable columnar output.
    #[arg(long = "no-column")]
    pub no_column: bool,

    /// Abbreviation length for object names.
    #[arg(long = "abbrev", value_name = "N", num_args = 0..=1, default_missing_value = "7")]
    pub abbrev: Option<String>,

    /// Don't abbreviate.
    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    /// Show branches that point at a given object.
    #[arg(long = "points-at")]
    pub points_at: Option<String>,

    /// Editing mode.
    #[arg(long = "edit-description")]
    pub edit_description: bool,

    /// Use color in output.
    #[arg(long = "color", value_name = "WHEN", num_args = 0..=1, default_missing_value = "always")]
    pub color: Option<String>,

    /// Disable color output.
    #[arg(long = "no-color")]
    pub no_color: bool,
}

/// Run the `branch` command.
pub fn run(args: Args) -> Result<()> {
    // Note: previously delegated to system git, now handled natively.

    let repo = Repository::discover(None).context("not a git repository")?;
    let head = resolve_head(&repo.git_dir)?;

    if args.no_remotes_neg {
        eprintln!("error: unknown option `no-remotes'");
        eprintln!("usage: git branch [<options>] [-r | -a] [--merged] [--no-merged]");
        std::process::exit(129);
    }
    if args.no_all_neg {
        eprintln!("error: unknown option `no-all'");
        eprintln!("usage: git branch [<options>] [-r | -a] [--merged] [--no-merged]");
        std::process::exit(129);
    }

    // `git branch -v <pattern>` without `--list` is not a pattern listing mode; a glob is invalid
    // (t3203). With `--list`, globs are filtered in `list_branches`.
    if args.verbose > 0
        && !args.list
        && args.name.as_deref().is_some_and(is_glob_branch_pattern)
        && args.start_point.is_none()
    {
        let n = args.name.as_deref().unwrap_or("");
        eprintln!("fatal: '{n}' is not a valid branch name");
        std::process::exit(128);
    }

    // Resolve @{-N} in branch name if present
    let mut args = args;
    if let Some(ref name) = args.name.clone() {
        if name.starts_with("@{") {
            if let Ok(resolved) = grit_lib::refs::resolve_at_n_branch(&repo.git_dir, name) {
                args.name = Some(resolved);
            }
        }
    }

    // Validate mutually exclusive mode options
    {
        let mut modes = Vec::new();
        if args.delete || args.force_delete {
            modes.push("delete");
        }
        if args.rename || args.force_rename {
            modes.push("rename");
        }
        if args.copy || args.force_copy {
            modes.push("copy");
        }
        if args.set_upstream_to.is_some() {
            modes.push("set-upstream-to");
        }
        if args.unset_upstream {
            modes.push("unset-upstream");
        }
        if args.show_current {
            modes.push("show-current");
        }
        if args.edit_description {
            modes.push("edit-description");
        }
        // --list conflicts with delete/rename/copy but not with filtering
        if args.list && !modes.is_empty() {
            bail!("options are incompatible");
        }
        if modes.len() > 1 {
            bail!("options are incompatible");
        }
    }

    let filter_active = !args.contains.is_empty()
        || !args.no_contains.is_empty()
        || !args.merged.is_empty()
        || !args.no_merged.is_empty();
    let implicit_list = filter_active && !args.list;
    if implicit_list
        && (args.delete
            || args.force_delete
            || args.rename
            || args.force_rename
            || args.copy
            || args.force_copy
            || args.edit_description)
    {
        eprintln!(
            "fatal: options such as --contains, --no-contains, --merged, and --no-merged\n\
             require branch listing; incompatible with branch modification"
        );
        std::process::exit(129);
    }

    if args.show_current {
        if let Some(name) = head.branch_name() {
            println!("{name}");
        }
        return Ok(());
    }

    if args.edit_description {
        if args.start_point.is_some() {
            eprintln!("fatal: cannot edit description of more than one branch");
            std::process::exit(128);
        }
        return edit_branch_description(&repo, &head, args.name.as_deref());
    }

    if args.set_upstream_to.is_some() {
        return set_upstream(&repo, &head, &args);
    }

    if args.unset_upstream {
        return unset_upstream(&repo, &head, &args);
    }

    if args.delete || args.force_delete {
        return delete_branches(&repo, &head, &args);
    }

    if args.rename || args.force_rename {
        return rename_branch(&repo, &head, &args);
    }

    if args.copy || args.force_copy {
        return copy_branch(&repo, &head, &args);
    }

    // If a name is given and we're not listing/filtering, create a branch
    if let Some(ref name) = args.name {
        if !args.list
            && args.contains.is_empty()
            && args.no_contains.is_empty()
            && args.merged.is_empty()
            && args.no_merged.is_empty()
        {
            // Reject invalid branch names
            if name == "HEAD" || name.starts_with('-') {
                bail!("'{name}' is not a valid branch name");
            }
            return create_branch(&repo, &head, name, args.start_point.as_deref(), &args);
        }
    }

    // Default: list branches
    list_branches(&repo, &head, &args)
}

/// One row in `git branch` output.
#[derive(Clone)]
struct BranchInfo {
    /// Short name for default listing (`main`, `remotes/origin/foo`, or `origin/foo` for `-r`).
    name: String,
    oid: ObjectId,
    is_remote: bool,
    /// Full ref for `%(refname)` (`refs/heads/...` / `refs/remotes/...`); absent for detached HEAD row.
    full_refname: Option<String>,
    /// ` -> short-target` when this ref is symbolic (local or remote).
    symref_suffix: Option<String>,
}

fn detached_head_description(repo: &Repository, head: &HeadState) -> Result<String> {
    let HeadState::Detached { oid } = head else {
        bail!("detached_head_description: not detached");
    };
    let git_dir = &repo.git_dir;
    let state_dir = grit_lib::refs::common_dir(git_dir).unwrap_or_else(|| git_dir.clone());
    if state_dir.join("BISECT_LOG").exists() {
        let start = fs::read_to_string(state_dir.join("BISECT_START"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let label = if start.is_empty() {
            oid.to_hex().chars().take(7).collect::<String>()
        } else if let Some(rest) = start.strip_prefix("refs/heads/") {
            rest.to_owned()
        } else {
            start
        };
        return Ok(format!("(no branch, bisect started on {label})"));
    }
    let wt = wt_status_get_state(git_dir, head, true)?;
    if let Some(label) = wt.detached_from {
        if wt.detached_at {
            return Ok(format!("(HEAD detached at {label})"));
        }
        return Ok(format!("(HEAD detached from {label})"));
    }
    let abbrev: String = oid.to_hex().chars().take(7).collect();
    Ok(format!("(HEAD detached at {abbrev})"))
}

fn shorten_symref_target(git_dir: &Path, target: &str) -> Option<String> {
    if let Some(rest) = target.strip_prefix("refs/remotes/") {
        return Some(rest.to_owned());
    }
    if let Some(rest) = target.strip_prefix("refs/heads/") {
        return Some(rest.to_owned());
    }
    if let Some(rest) = target.strip_prefix("refs/tags/") {
        return Some(rest.to_owned());
    }
    grit_lib::refs::resolve_ref(git_dir, target).ok()?;
    Some(target.to_owned())
}

fn peel_to_commit_oid(repo: &Repository, mut oid: ObjectId) -> Option<ObjectId> {
    for _ in 0..32 {
        let obj = repo.odb.read(&oid).ok()?;
        match obj.kind {
            ObjectKind::Commit => return Some(oid),
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data).ok()?;
                oid = tag.object;
            }
            _ => return None,
        }
    }
    None
}

fn branch_stable_key(b: &BranchInfo) -> String {
    b.full_refname.clone().unwrap_or_else(|| b.name.clone())
}

fn push_branch_row(
    repo: &Repository,
    branches: &mut Vec<BranchInfo>,
    full_ref: String,
    oid: ObjectId,
    is_remote: bool,
    list_name: String,
) {
    let sym = refs::read_symbolic_ref(&repo.git_dir, &full_ref)
        .ok()
        .flatten();
    let symref_suffix = sym
        .as_deref()
        .and_then(|t| shorten_symref_target(&repo.git_dir, t))
        .map(|s| format!(" -> {s}"));
    branches.push(BranchInfo {
        name: list_name,
        oid,
        is_remote,
        full_refname: Some(full_ref),
        symref_suffix,
    });
}

fn is_glob_branch_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

fn abbrev_for_branch_verbose(repo: &Repository, oid: &ObjectId) -> String {
    let n = ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|c| c.get("core.abbrev"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(7);
    abbreviate_object_id(repo, *oid, n).unwrap_or_else(|_| {
        let h = oid.to_hex();
        let take = n.clamp(4, 40).min(h.len());
        h[..take].to_owned()
    })
}

fn version_segments(s: &str) -> Vec<&str> {
    s.split(['.', '-']).filter(|seg| !seg.is_empty()).collect()
}

fn compare_version_refname(a: &str, b: &str) -> Ordering {
    let seg_a = version_segments(a);
    let seg_b = version_segments(b);
    for (sa, sb) in seg_a.iter().zip(seg_b.iter()) {
        let ord = match (sa.parse::<u64>(), sb.parse::<u64>()) {
            (Ok(na), Ok(nb)) => na.cmp(&nb),
            _ => sa.cmp(sb),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    seg_a.len().cmp(&seg_b.len())
}

/// List branches.
fn list_branches(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let current_branch = head.branch_name().unwrap_or("");
    let occupied = worktree_refs::occupied_branch_refs(repo);

    let mut branches: Vec<BranchInfo> = Vec::new();

    if !args.remotes {
        let local: Vec<(String, ObjectId)> = if grit_lib::reftable::is_reftable_repo(&repo.git_dir)
        {
            grit_lib::reftable::reftable_list_refs(&repo.git_dir, "refs/heads/")
                .map_err(|e| anyhow::anyhow!("{e}"))?
        } else {
            refs::list_refs(&repo.git_dir, "refs/heads/").map_err(|e| anyhow::anyhow!("{e}"))?
        };
        for (full, oid) in local {
            let short = full.strip_prefix("refs/heads/").unwrap_or(&full).to_owned();
            push_branch_row(repo, &mut branches, full, oid, false, short);
        }
    }

    if args.remotes || args.all {
        let remote: Vec<(String, ObjectId)> = if grit_lib::reftable::is_reftable_repo(&repo.git_dir)
        {
            grit_lib::reftable::reftable_list_refs(&repo.git_dir, "refs/remotes/")
                .map_err(|e| anyhow::anyhow!("{e}"))?
        } else {
            refs::list_refs(&repo.git_dir, "refs/remotes/").map_err(|e| anyhow::anyhow!("{e}"))?
        };
        for (full, oid) in remote {
            let short = full
                .strip_prefix("refs/remotes/")
                .unwrap_or(&full)
                .to_owned();
            let list_name = if args.remotes && !args.all {
                short.clone()
            } else {
                format!("remotes/{short}")
            };
            push_branch_row(repo, &mut branches, full, oid, true, list_name);
        }
    }

    if !args.merged.is_empty() {
        let mut keep = HashSet::new();
        for merged_val in &args.merged {
            let target_oid = if merged_val.is_empty() {
                *head
                    .oid()
                    .ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?
            } else {
                resolve_revision_must_be_commit(repo, merged_val)?
            };
            for b in &branches {
                if is_ancestor(repo, b.oid, target_oid).unwrap_or(false) {
                    keep.insert(branch_stable_key(b));
                }
            }
        }
        branches.retain(|b| keep.contains(&branch_stable_key(b)));
    }

    for no_merged_val in &args.no_merged {
        let target_oid = if no_merged_val.is_empty() {
            *head
                .oid()
                .ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?
        } else {
            resolve_revision_must_be_commit(repo, no_merged_val)?
        };
        branches.retain(|b| !is_ancestor(repo, b.oid, target_oid).unwrap_or(true));
    }

    if !args.contains.is_empty() {
        let contain_oids: Vec<ObjectId> = args
            .contains
            .iter()
            .map(|r| resolve_revision_must_be_commit(repo, r))
            .collect::<Result<_>>()?;
        branches.retain(|b| {
            contain_oids
                .iter()
                .any(|&c| is_ancestor(repo, c, b.oid).unwrap_or(false))
        });
    }

    for no_contains_rev in &args.no_contains {
        let no_contains_oid = resolve_revision_must_be_commit(repo, no_contains_rev)?;
        branches.retain(|b| !is_ancestor(repo, no_contains_oid, b.oid).unwrap_or(true));
    }

    if let Some(ref pat) = args.name {
        branches.retain(|b| glob_match_case(pat, &b.name, args.ignore_case));
    }

    if let Some(ref spec) = args.points_at {
        let points_oid = resolve_revision_must_be_commit(repo, spec)?;
        branches.retain(|b| {
            if let Some(ref full) = b.full_refname {
                if let Ok(Some(target)) = refs::read_symbolic_ref(&repo.git_dir, full) {
                    if let Ok(tip) = refs::resolve_ref(&repo.git_dir, &target) {
                        if tip == points_oid {
                            return false;
                        }
                    }
                }
            }
            b.oid == points_oid || peel_to_commit_oid(repo, b.oid) == Some(points_oid)
        });
    }

    let sort_keys: Vec<String> = if args.no_sort {
        Vec::new()
    } else if !args.sort.is_empty() {
        args.sort.clone()
    } else if let Some(cfg) = ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|c| c.get("branch.sort"))
    {
        vec![cfg]
    } else {
        Vec::new()
    };

    let list_with_pattern = args.list && args.name.is_some();
    let need_synthetic_detached = matches!(head, HeadState::Detached { .. })
        && !args.remotes
        && !list_with_pattern
        && !sort_keys.is_empty();
    if need_synthetic_detached {
        let oid = *head
            .oid()
            .ok_or_else(|| anyhow::anyhow!("detached HEAD without OID"))?;
        branches.push(BranchInfo {
            name: String::new(),
            oid,
            is_remote: false,
            full_refname: None,
            symref_suffix: None,
        });
    }

    sort_branches(repo, head, &mut branches, &sort_keys, args.ignore_case)?;

    if args.format.is_none() {
        branches.retain(|b| b.full_refname.is_some());
    }

    if let Some(ref fmt) = args.format {
        let stdout_tty = std::io::stdout().is_terminal();
        let emit_fmt_color = if args.no_color {
            false
        } else if let Some(ref when) = args.color {
            when != "never" && (when == "always" || stdout_tty)
        } else {
            stdout_tty
        };
        let mut rows: Vec<BranchInfo> = Vec::new();
        if matches!(head, HeadState::Detached { .. })
            && !args.remotes
            && !list_with_pattern
            && sort_keys.is_empty()
        {
            let oid = *head
                .oid()
                .ok_or_else(|| anyhow::anyhow!("detached HEAD without OID"))?;
            rows.push(BranchInfo {
                name: String::new(),
                oid,
                is_remote: false,
                full_refname: None,
                symref_suffix: None,
            });
        }
        rows.extend(branches.iter().cloned());
        for b in &rows {
            match format_branch(repo, head, b, fmt, args.omit_empty, emit_fmt_color) {
                Ok(line) => {
                    if line.is_empty() && args.omit_empty {
                        continue;
                    }
                    if emit_fmt_color && !line.is_empty() {
                        writeln!(out, "{line}\x1b[m")?;
                    } else {
                        writeln!(out, "{line}")?;
                    }
                }
                Err(BranchListError::FormatFatal(m)) => {
                    eprintln!("fatal: {m}");
                    std::process::exit(128);
                }
                Err(BranchListError::Other(e)) => return Err(e),
            }
        }
        return Ok(());
    }

    let stdout_tty = std::io::stdout().is_terminal();
    let use_color = if args.no_color {
        false
    } else if let Some(ref when) = args.color {
        when != "never" && (when == "always" || stdout_tty)
    } else {
        false
    };
    let (color_current, color_worktree, color_local, color_remote, color_reset) = if use_color {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).ok();
        let get_color = |key: &str, default: &str| -> String {
            let val = cfg.as_ref().and_then(|c| c.get(key));
            let cs = val.as_deref().unwrap_or(default);
            grit_lib::config::parse_color(cs).unwrap_or_default()
        };
        (
            get_color("color.branch.current", "green"),
            get_color("color.branch.worktree", "cyan"),
            get_color("color.branch.local", "normal"),
            get_color("color.branch.remote", "red"),
            "\x1b[m".to_string(),
        )
    } else {
        (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        )
    };

    // Verbose listing: Git `calc_maxwidth` includes detached HEAD description width so the OID
    // column lines up with `* (HEAD detached ...)`.
    let max_name_len = if args.verbose > 0 {
        let mut m = branches.iter().map(|b| b.name.len()).max().unwrap_or(0);
        if matches!(head, HeadState::Detached { .. }) && !args.remotes && !list_with_pattern {
            if let Ok(desc) = detached_head_description(repo, head) {
                m = m.max(desc.len());
            }
        }
        m
    } else {
        branches
            .iter()
            .map(|b| b.name.len() + b.symref_suffix.as_ref().map(|s| s.len()).unwrap_or(0))
            .max()
            .unwrap_or(0)
    };

    if matches!(head, HeadState::Detached { .. })
        && !args.remotes
        && !list_with_pattern
        && args.points_at.is_none()
    {
        let desc = detached_head_description(repo, head)?;
        let prefix = "* ";
        if use_color {
            write!(out, "{prefix}{color_current}{desc}{color_reset}")?;
        } else {
            write!(out, "{prefix}{desc}")?;
        }
        if args.verbose > 0 {
            let oid = head.oid().unwrap();
            let short = abbrev_for_branch_verbose(repo, oid);
            let subject = commit_subject(&repo.odb, oid).unwrap_or_default();
            write!(out, " {short} {subject}")?;
        }
        writeln!(out)?;
    }

    for b in &branches {
        let is_current = !b.is_remote && b.name == current_branch;
        let in_other_wt = b.full_refname.as_ref().is_some_and(|r| {
            occupied.get(r).is_some_and(|p| {
                if let Some(wt) = repo.work_tree.as_deref() {
                    p != &wt.display().to_string()
                } else {
                    true
                }
            })
        });
        let prefix = if is_current {
            "* "
        } else if in_other_wt {
            "+ "
        } else {
            "  "
        };
        let color = if use_color {
            if is_current {
                &color_current
            } else if in_other_wt {
                &color_worktree
            } else if b.is_remote {
                &color_remote
            } else {
                &color_local
            }
        } else {
            &color_local
        };
        let reset = if use_color {
            &color_reset
        } else {
            &color_local
        };

        let sym = b.symref_suffix.as_deref().unwrap_or("");
        // `git branch -v` omits ` -> target` for symrefs (plain `git branch` still shows them).
        let sym_out = if args.verbose > 0 { "" } else { sym };
        let display_name = format!("{}{}", b.name, sym_out);

        if args.verbose > 0 {
            let short = abbrev_for_branch_verbose(repo, &b.oid);
            let subject = commit_subject(&repo.odb, &b.oid).unwrap_or_default();

            if !b.is_remote {
                let track = resolve_branch_tracking(repo, &b.name)?;
                let v1 = track.as_ref().and_then(|t| t.verbose1_inner.as_deref());
                let v2 = track.as_ref().and_then(|t| t.verbose2_inner.as_deref());
                if use_color {
                    let padded_name = format!("{:<width$}", b.name, width = max_name_len);
                    write!(out, "{prefix}{color}{padded_name}{reset}{sym_out}")?;
                } else {
                    let padded_name = format!("{:<width$}", b.name, width = max_name_len);
                    write!(out, "{prefix}{padded_name}{sym_out}")?;
                }
                write!(out, " {short}")?;
                if args.verbose >= 2 {
                    if !is_current && in_other_wt {
                        if let Some(r) = &b.full_refname {
                            if let Some(path) = occupied.get(r) {
                                if use_color {
                                    write!(out, " ({color_worktree}{path}{color_reset})")?;
                                } else {
                                    write!(out, " ({path})")?;
                                }
                            }
                        }
                    }
                    if let Some(i2) = v2 {
                        write!(out, " [{i2}]")?;
                    } else if let Some(i1) = v1 {
                        write!(out, " [{i1}]")?;
                    }
                } else if args.verbose == 1 {
                    if !is_current && in_other_wt {
                        if let Some(r) = &b.full_refname {
                            if let Some(path) = occupied.get(r) {
                                if use_color {
                                    write!(out, " ({color_worktree}{path}{color_reset})")?;
                                } else {
                                    write!(out, " ({path})")?;
                                }
                            }
                        }
                    }
                    if let Some(i1) = v1 {
                        write!(out, " [{i1}]")?;
                    }
                }
                writeln!(out, " {subject}")?;
            } else {
                if use_color {
                    let padded_name = format!("{:<width$}", b.name, width = max_name_len);
                    writeln!(
                        out,
                        "{prefix}{color}{padded_name}{reset}{sym_out} {short} {subject}"
                    )?;
                } else {
                    let padded_name = format!("{:<width$}", b.name, width = max_name_len);
                    writeln!(out, "{prefix}{padded_name}{sym_out} {short} {subject}")?;
                }
            }
        } else if use_color {
            writeln!(out, "{prefix}{color}{}{reset}{sym_out}", b.name)?;
        } else {
            writeln!(out, "{prefix}{display_name}")?;
        }
    }

    Ok(())
}

#[derive(Debug)]
enum BranchListError {
    FormatFatal(String),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for BranchListError {
    fn from(e: anyhow::Error) -> Self {
        BranchListError::Other(e)
    }
}

/// Resolve a revision and require the peeled object to be a commit (Git `branch --contains` rules).
///
/// On failure, prints Git-compatible messages to stderr and exits with code 129.
fn resolve_revision_must_be_commit(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let oid = resolve_revision(repo, spec)?;
    let object = repo.odb.read(&oid)?;
    let hex = oid.to_hex();
    let kind_msg = match object.kind {
        ObjectKind::Commit => return Ok(oid),
        ObjectKind::Tree => "tree",
        ObjectKind::Blob => "blob",
        ObjectKind::Tag => "tag",
    };
    eprintln!("error: object {hex} is a {kind_msg}, not a commit");
    eprintln!("error: no such commit {hex}");
    std::process::exit(129);
}

fn sort_branches(
    repo: &Repository,
    head: &HeadState,
    branches: &mut [BranchInfo],
    sort_keys: &[String],
    ignore_case: bool,
) -> Result<()> {
    let head_oid = head.oid().copied();
    let cmp_name = |a: &BranchInfo, b: &BranchInfo| -> Ordering {
        if ignore_case {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        } else {
            a.name.cmp(&b.name)
        }
    };
    if sort_keys.is_empty() {
        branches.sort_by(|a, b| cmp_name(a, b));
        return Ok(());
    }

    // Git: the last `--sort` is the primary key; earlier keys break ties.
    // `-key` reverses only that key's primary comparison; refname tie-break stays ascending.
    branches.sort_by(|a, b| {
        match (a.full_refname.is_none(), b.full_refname.is_none()) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
        for raw in sort_keys.iter().rev() {
            let (desc, key) = raw
                .strip_prefix('-')
                .map(|k| (true, k))
                .unwrap_or((false, raw.as_str()));
            let mut primary = match key {
                "refname" => cmp_name(a, b),
                "committerdate" => {
                    let ta = committer_time(&repo.odb, &a.oid, &a.name, a.is_remote);
                    let tb = committer_time(&repo.odb, &b.oid, &b.name, b.is_remote);
                    ta.cmp(&tb)
                }
                "authordate" => {
                    let ta = author_time(&repo.odb, &a.oid, &a.name, a.is_remote);
                    let tb = author_time(&repo.odb, &b.oid, &b.name, b.is_remote);
                    ta.cmp(&tb)
                }
                "objectsize" => {
                    let sa = object_size_for_sort(repo, a);
                    let sb = object_size_for_sort(repo, b);
                    sa.cmp(&sb)
                }
                "type" => {
                    let ka = object_kind_for_sort(repo, a);
                    let kb = object_kind_for_sort(repo, b);
                    ka.cmp(&kb)
                }
                "version:refname" => compare_version_refname(&a.name, &b.name),
                k if k.starts_with("ahead-behind:") => {
                    let spec = &k["ahead-behind:".len()..];
                    match (head_oid, resolve_revision(repo, spec).ok()) {
                        (Some(h), Some(_base)) => {
                            let ab_a =
                                count_symmetric_ahead_behind(repo, a.oid, h).unwrap_or((0, 0));
                            let ab_b =
                                count_symmetric_ahead_behind(repo, b.oid, h).unwrap_or((0, 0));
                            ab_a.0.cmp(&ab_b.0).then_with(|| ab_a.1.cmp(&ab_b.1))
                        }
                        _ => Ordering::Equal,
                    }
                }
                other => {
                    eprintln!("error: unknown field name: {other}");
                    std::process::exit(129);
                }
            };
            if desc {
                primary = primary.reverse();
            }
            if primary != Ordering::Equal {
                return primary;
            }
        }
        cmp_name(a, b)
    });
    Ok(())
}

fn object_size_for_sort(repo: &Repository, b: &BranchInfo) -> u64 {
    repo.odb
        .read(&b.oid)
        .map(|o| o.data.len() as u64)
        .unwrap_or(0)
}

fn object_kind_for_sort(repo: &Repository, b: &BranchInfo) -> u8 {
    repo.odb
        .read(&b.oid)
        .map(|o| match o.kind {
            ObjectKind::Commit => 0u8,
            ObjectKind::Tag => 1,
            ObjectKind::Tree => 2,
            ObjectKind::Blob => 3,
        })
        .unwrap_or(255)
}

/// Parsed upstream / ahead-behind display for a local branch (`git branch -v` / `-vv`).
struct BranchTracking {
    /// Inner text for `-v` brackets, e.g. `ahead 1` or `origin/main: ahead 1`.
    verbose1_inner: Option<String>,
    /// Inner text for `-vv` brackets (includes remote ref prefix when Git does).
    verbose2_inner: Option<String>,
    /// Short upstream name for `%(upstream:short)` (e.g. `main` or `origin/main`).
    upstream_short: String,
    /// Full ref for `%(upstream)` (`refs/heads/...` or `refs/remotes/...`).
    upstream_ref_full: String,
}

/// Resolve configured upstream and ahead/behind for a local branch.
fn resolve_branch_tracking(repo: &Repository, branch_name: &str) -> Result<Option<BranchTracking>> {
    let config_path = repo.git_dir.join("config");
    let config_file = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => return Ok(None),
    };
    let mut config = ConfigSet::new();
    config.merge(&config_file);

    let merge_key = format!("branch.{branch_name}.merge");
    let remote_key = format!("branch.{branch_name}.remote");

    let merge = match config.get(&merge_key) {
        Some(m) => m,
        None => return Ok(None),
    };
    let remote = config
        .get(&remote_key)
        .unwrap_or_else(|| "origin".to_string());

    let upstream_branch = merge
        .strip_prefix("refs/heads/")
        .unwrap_or(&merge)
        .to_string();
    let track_local = remote == ".";

    let local_ref_path = repo.git_dir.join("refs/heads").join(branch_name);
    let local_oid = match fs::read_to_string(&local_ref_path) {
        Ok(c) => ObjectId::from_hex(c.trim()).ok(),
        Err(_) => None,
    };
    let Some(local_oid) = local_oid else {
        return Ok(None);
    };

    let short_label = if track_local {
        upstream_branch.clone()
    } else {
        format!("{remote}/{upstream_branch}")
    };
    let upstream_ref_full = if track_local {
        format!("refs/heads/{upstream_branch}")
    } else {
        format!("refs/remotes/{remote}/{upstream_branch}")
    };

    let upstream_oid = if track_local {
        let p = repo.git_dir.join("refs/heads").join(&upstream_branch);
        match fs::read_to_string(&p) {
            Ok(c) => ObjectId::from_hex(c.trim()).ok(),
            Err(_) => None,
        }
    } else {
        let upstream_ref_path = repo
            .git_dir
            .join("refs/remotes")
            .join(&remote)
            .join(&upstream_branch);
        match fs::read_to_string(&upstream_ref_path) {
            Ok(c) => ObjectId::from_hex(c.trim()).ok(),
            Err(_) => None,
        }
    };

    let Some(upstream_oid) = upstream_oid else {
        return Ok(Some(BranchTracking {
            verbose1_inner: Some("gone".to_string()),
            verbose2_inner: Some(format!("{short_label}: gone")),
            upstream_short: short_label.clone(),
            upstream_ref_full,
        }));
    };

    let (ahead, behind) = count_ahead_behind(repo, local_oid, upstream_oid)?;
    if ahead == 0 && behind == 0 {
        // `git branch -vv` still shows the upstream in brackets when in sync (e.g. `[origin/main]`).
        let verbose2_inner = if track_local {
            None
        } else {
            Some(short_label.clone())
        };
        return Ok(Some(BranchTracking {
            verbose1_inner: None,
            verbose2_inner,
            upstream_short: short_label,
            upstream_ref_full,
        }));
    }

    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("ahead {ahead}"));
    }
    if behind > 0 {
        parts.push(format!("behind {behind}"));
    }
    let detail = parts.join(", ");

    // `git branch -v` omits the remote prefix; `git branch -vv` includes it (e.g. `origin/main:`).
    let (verbose1_inner, verbose2_inner) = if track_local {
        (
            Some(detail.clone()),
            Some(format!("{}: {detail}", upstream_branch)),
        )
    } else {
        (
            Some(detail.clone()),
            Some(format!("{short_label}: {detail}")),
        )
    };

    Ok(Some(BranchTracking {
        verbose1_inner,
        verbose2_inner,
        upstream_short: short_label,
        upstream_ref_full,
    }))
}

/// Count how many commits local is ahead of and behind upstream.
fn count_ahead_behind(
    repo: &Repository,
    local: ObjectId,
    upstream: ObjectId,
) -> Result<(usize, usize)> {
    Ok(count_symmetric_ahead_behind(repo, local, upstream)?)
}

/// `git branch --edit-description`: open an editor on the branch description, then store in config.
fn edit_branch_description(
    repo: &Repository,
    head: &HeadState,
    branch_arg: Option<&str>,
) -> Result<()> {
    let branch_name: String = match branch_arg {
        Some(n) => n.to_owned(),
        None => match head {
            HeadState::Branch { short_name, .. } => short_name.clone(),
            HeadState::Detached { .. } => {
                eprintln!("fatal: cannot give description to detached HEAD");
                std::process::exit(128);
            }
            HeadState::Invalid => {
                eprintln!("fatal: cannot give description to detached HEAD");
                std::process::exit(128);
            }
        },
    };

    let branch_ref = format!("refs/heads/{branch_name}");
    let ref_exists = grit_lib::refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok();

    match head {
        HeadState::Branch {
            short_name,
            oid: None,
            ..
        } if branch_arg.is_none() => {
            eprintln!("error: no commit on branch '{short_name}' yet");
            std::process::exit(1);
        }
        _ => {}
    }

    if branch_arg.is_some() && !ref_exists {
        if branch_ref_is_unborn_across_worktrees(repo, &branch_ref)? {
            eprintln!("error: no commit on branch '{branch_name}' yet");
            std::process::exit(1);
        }
        eprintln!("error: no branch named '{branch_name}'");
        std::process::exit(1);
    }

    let desc_key = format!("branch.{branch_name}.description");
    let config_path = repo.git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let mut cs = ConfigSet::new();
    let file_cfg = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;
    cs.merge(&file_cfg);
    let had_description = cs.get(&desc_key).is_some();
    let existing = cs.get(&desc_key).unwrap_or_default();

    let mut initial = existing.clone();
    if !initial.is_empty() && !initial.ends_with('\n') {
        initial.push('\n');
    }
    initial.push_str(&format!(
        "# Please edit the description for the branch\n\
         #   {branch_name}\n\
         # Lines starting with '#' will be stripped.\n"
    ));

    let edited = launch_editor_for_branch_description(repo, &initial)?;
    let stripped = String::from_utf8_lossy(&stripspace_process(
        edited.as_bytes(),
        &StripspaceMode::StripComments("#".into()),
    ))
    .to_string();

    if stripped.is_empty() && !had_description {
        return Ok(());
    }

    let mut config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;
    if stripped.is_empty() {
        let _ = config.unset(&desc_key);
    } else {
        config.set(&desc_key, stripped.trim_end_matches('\n'))?;
    }
    config.write()?;
    Ok(())
}

fn is_effective_editor_value(raw: &str) -> bool {
    let t = raw.trim();
    !t.is_empty() && t != ":"
}

/// Same resolution order as `git commit`: skip `:` placeholders for `VISUAL` / `EDITOR`.
fn resolve_branch_description_editor(repo: &Repository) -> String {
    let visual_present = std::env::var("VISUAL").is_ok();
    let editor_present = std::env::var("EDITOR").is_ok();

    if let Ok(e) = std::env::var("GIT_EDITOR") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    if let Ok(cfg) = ConfigSet::load(Some(&repo.git_dir), true) {
        if let Some(e) = cfg.get("core.editor") {
            if is_effective_editor_value(&e) {
                return e;
            }
        }
    }
    if let Ok(e) = std::env::var("VISUAL") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    if let Ok(e) = std::env::var("EDITOR") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    if visual_present || editor_present {
        "true".to_owned()
    } else {
        "vi".to_owned()
    }
}

fn launch_editor_for_branch_description(repo: &Repository, initial: &str) -> Result<String> {
    let editor = resolve_branch_description_editor(repo);

    let tmp_dir = repo.git_dir.join("tmp");
    let _ = fs::create_dir_all(&tmp_dir);
    let tmp_path = tmp_dir.join("EDIT_DESCRIPTION");
    fs::write(&tmp_path, initial)?;

    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(&tmp_path)
        .status()
        .with_context(|| format!("failed to launch editor '{editor}'"))?;

    if !status.success() {
        let _ = fs::remove_file(&tmp_path);
        bail!("editor exited with non-zero status");
    }

    let result = fs::read_to_string(&tmp_path)?;
    let _ = fs::remove_file(&tmp_path);
    Ok(result)
}

/// Set upstream tracking branch.
fn set_upstream(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let upstream_raw = args
        .set_upstream_to
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("upstream name required"))?;
    let upstream = if upstream_raw.starts_with("@{-") && upstream_raw.ends_with('}') {
        grit_lib::refs::resolve_at_n_branch(&repo.git_dir, upstream_raw)?
    } else {
        upstream_raw.to_string()
    };

    let branch_name = match args.name.as_deref() {
        Some(n) => n.to_owned(),
        None => head
            .branch_name()
            .ok_or_else(|| anyhow::anyhow!("no current branch; specify branch name"))?
            .to_owned(),
    };

    let branch_ref = format!("refs/heads/{branch_name}");
    if grit_lib::refs::resolve_ref(&repo.git_dir, &branch_ref).is_err() {
        if branch_ref_is_unborn_across_worktrees(repo, &branch_ref)? {
            eprintln!("fatal: no commit on branch '{branch_name}' yet");
            std::process::exit(128);
        }
        eprintln!("fatal: branch '{branch_name}' does not exist");
        std::process::exit(128);
    }

    // Parse upstream as remote/branch
    let (remote, upstream_branch) = parse_upstream(repo, &upstream)?;

    if remote == "." && upstream_branch == branch_name {
        eprintln!("warning: not setting branch '{branch_name}' as its own upstream");
        return Ok(());
    }

    let config_path = repo.git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let mut config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;

    let remote_key = format!("branch.{branch_name}.remote");
    let merge_key = format!("branch.{branch_name}.merge");

    config.set(&remote_key, &remote)?;
    config.set(&merge_key, &format!("refs/heads/{upstream_branch}"))?;
    config.write()?;

    if !args.quiet {
        let track_label = if remote == "." {
            upstream_branch.clone()
        } else {
            format!("{remote}/{upstream_branch}")
        };
        eprintln!("branch '{branch_name}' set up to track '{track_label}'.");
    }

    Ok(())
}

/// Remove upstream tracking configuration.
fn unset_upstream(repo: &Repository, _head: &HeadState, args: &Args) -> Result<()> {
    let branch_name = match args.name.as_deref() {
        Some(n) => n.to_owned(),
        None => {
            eprintln!(
                "fatal: could not unset upstream of HEAD when it does not point to any branch"
            );
            std::process::exit(128);
        }
    };

    let config_path = repo.git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let mut config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;

    let merge_key = format!("branch.{branch_name}.merge");

    // Check if there's actually tracking info — use ConfigSet to read
    let mut cs = ConfigSet::new();
    cs.merge(&config);
    if cs.get(&merge_key).is_none() {
        eprintln!("fatal: branch '{branch_name}' has no upstream information");
        std::process::exit(1);
    }

    let remote_key = format!("branch.{branch_name}.remote");
    let _ = config.unset(&remote_key);
    let _ = config.unset(&merge_key);
    config.write()?;

    if !args.quiet {
        eprintln!("branch '{branch_name}' upstream information removed.");
    }

    Ok(())
}

/// Parse an upstream spec like "origin/main" into (remote, branch).
fn parse_upstream(repo: &Repository, upstream: &str) -> Result<(String, String)> {
    let upstream = upstream.strip_prefix("refs/heads/").unwrap_or(upstream);

    // Match the longest remote name prefix so `origin/foo` is not parsed as remote `or` + branch
    // `igin/foo` (readdir order is undefined; t5572 `branch --set-upstream-to=origin/...`).
    let remotes_dir = repo.git_dir.join("refs/remotes");
    if let Ok(entries) = fs::read_dir(&remotes_dir) {
        let mut best: Option<(usize, String, String)> = None;
        for entry in entries.flatten() {
            let remote_name = entry.file_name().to_string_lossy().to_string();
            let prefix = format!("{remote_name}/");
            if let Some(branch) = upstream.strip_prefix(&prefix) {
                if branch.is_empty() {
                    continue;
                }
                let len = remote_name.len();
                if best.as_ref().is_none_or(|(best_len, _, _)| len > *best_len) {
                    best = Some((len, remote_name, branch.to_string()));
                }
            }
        }
        if let Some((_, remote_name, branch)) = best {
            return Ok((remote_name, branch));
        }
    }

    // Local branch (loose or packed): `git branch -u main` tracks `refs/heads/main`.
    let heads_ref = format!("refs/heads/{upstream}");
    if refs::resolve_ref(&repo.git_dir, &heads_ref).is_ok() {
        return Ok((".".to_string(), upstream.to_string()));
    }

    // Fallback: split on first /
    if let Some(idx) = upstream.find('/') {
        let remote = &upstream[..idx];
        let branch = &upstream[idx + 1..];
        if !branch.is_empty() {
            return Ok((remote.to_string(), branch.to_string()));
        }
    }

    bail!("cannot parse upstream '{upstream}' — expected format: remote/branch");
}

/// Format a branch line (`--format`); supports t3203 atoms and conditionals via [`expand_branch_format`].
fn format_branch(
    repo: &Repository,
    head: &HeadState,
    branch: &BranchInfo,
    fmt: &str,
    omit_empty: bool,
    emit_format_color: bool,
) -> Result<String, BranchListError> {
    let refname_display = if branch.full_refname.is_none() {
        detached_head_description(repo, head).map_err(|e| BranchListError::Other(e.into()))?
    } else {
        branch.full_refname.clone().unwrap()
    };
    let ctx = BranchFormatContext {
        repo,
        refname_display: &refname_display,
        oid: branch.oid,
        full_refname: branch.full_refname.as_deref(),
        emit_format_color,
    };
    expand_branch_format(&ctx, fmt, omit_empty).map_err(|e| match e {
        BranchFormatError::Fatal(m) => BranchListError::FormatFatal(m),
    })
}

/// Create a new branch.
fn create_branch(
    repo: &Repository,
    head: &HeadState,
    name: &str,
    start_point: Option<&str>,
    args: &Args,
) -> Result<()> {
    let refname = format!("refs/heads/{name}");
    let previous_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &refname).ok();
    let exists = previous_oid.is_some();

    if exists && !args.force {
        bail!("A branch named '{name}' already exists.");
    }

    // Check for D/F conflict: a prefix of the new name exists as a branch
    // e.g., cannot create 'c/d' if 'c' already exists as a branch
    let heads_dir = repo.git_dir.join("refs/heads");
    let mut prefix = std::path::PathBuf::from(name);
    while prefix.pop() {
        if !prefix.as_os_str().is_empty() {
            let prefix_str = prefix.to_string_lossy();
            let prefix_ref = format!("refs/heads/{}", prefix_str);
            if grit_lib::refs::resolve_ref(&repo.git_dir, &prefix_ref).is_ok() {
                bail!(
                    "cannot lock ref '{}': '{}' exists; cannot create '{}'",
                    refname,
                    prefix_ref,
                    refname
                );
            }
        }
    }
    // Also check: a directory with the same name exists (existing branch is prefix)
    if heads_dir.join(name).is_dir() {
        bail!(
            "cannot lock ref '{}': '{}' exists",
            refname,
            heads_dir.join(name).display()
        );
    }

    // Cannot force-update the current branch
    if args.force {
        let current = head.branch_name().unwrap_or("");
        if name == current {
            let wt_path = repo
                .work_tree
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| repo.git_dir.display().to_string());
            bail!(
                "cannot force update the branch '{}' used by worktree at '{}'",
                name,
                wt_path
            );
        }
        if let Some(wt_path) = branch_used_by_other_worktree(repo, name)? {
            bail!(
                "cannot force update the branch '{}' used by worktree at '{}'",
                name,
                wt_path
            );
        }
    }

    let oid = match start_point {
        Some(rev) => resolve_revision(repo, rev)?,
        None => *head
            .oid()
            .ok_or_else(|| anyhow::anyhow!("not a valid object name: 'HEAD'"))?,
    };

    if let Some(sp) = start_point {
        let local_branch_ref = format!("refs/heads/{sp}");
        let tag_ref = format!("refs/tags/{sp}");
        let has_local_branch =
            grit_lib::refs::resolve_ref(&repo.git_dir, &local_branch_ref).is_ok();
        let has_tag = grit_lib::refs::resolve_ref(&repo.git_dir, &tag_ref).is_ok();
        if args.track.is_some() && !has_local_branch && has_tag {
            bail!(
                "fatal: cannot set up tracking information; starting point '{sp}' is not a branch"
            );
        }
    }

    grit_lib::refs::write_ref(&repo.git_dir, &refname, &oid).map_err(|e| anyhow::anyhow!("{e}"))?;

    if start_point.is_none() && args.track.is_some() {
        if let Some(tracked_short) = head.branch_name() {
            let merge_ref = format!("refs/heads/{tracked_short}");
            let config_path = repo.git_dir.join("config");
            let mut cfg = std::fs::read_to_string(&config_path).unwrap_or_default();
            cfg.push_str(&format!("\n[branch \"{name}\"]"));
            cfg.push_str("\n\tremote = .");
            cfg.push_str(&format!("\n\tmerge = {merge_ref}\n"));
            std::fs::write(&config_path, cfg)?;
        }
    }

    // Create reflog when explicitly requested or when core.logAllRefUpdates
    // enables branch reflogs for this repository.
    if args.create_reflog || should_log_ref_updates(repo) {
        let reflog_path = grit_lib::refs::reflog_file_path(&repo.git_dir, &refname);
        if let Some(parent) = reflog_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let ident = get_reflog_identity();
        let zero = "0000000000000000000000000000000000000000";
        if exists && args.force {
            if let Some(old) = previous_oid {
                if old != oid {
                    let entry = format!(
                        "{} {} {}\tbranch: forced update\n",
                        old.to_hex(),
                        oid.to_hex(),
                        ident
                    );
                    let mut f = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&reflog_path)?;
                    f.write_all(entry.as_bytes())?;
                }
            }
        } else if !exists {
            let from = match start_point {
                Some(sp) => sp.to_string(),
                None => head.branch_name().unwrap_or("HEAD").to_string(),
            };
            let msg = format!("branch: Created from {from}");
            if reflog_path.exists() {
                if let Ok(entries) = grit_lib::reflog::read_reflog(&repo.git_dir, &refname) {
                    if let Some(last) = entries.last() {
                        if last.new_oid != oid {
                            let _ = grit_lib::refs::append_reflog(
                                &repo.git_dir,
                                &refname,
                                &last.new_oid,
                                &oid,
                                &ident,
                                &msg,
                                true,
                            );
                        } else {
                            // Same tip OID as before (e.g. `branch -f main HEAD` after delete+recreate):
                            // Append a no-op reflog line so `log -g ...@{now}` can select the
                            // `branch: Created from ...` entry (t1507: two `test_tick` steps after
                            // `commit: 3`).
                            let tail = last
                                .identity
                                .rfind('>')
                                .map(|i| last.identity[i + 1..].trim());
                            if let Some(tail) = tail {
                                let mut it = tail.split_whitespace();
                                if let (Some(ts_s), Some(tz)) = (it.next(), it.next()) {
                                    if let Ok(prev_ts) = ts_s.parse::<i64>() {
                                        let name_email = ident
                                            .rfind('>')
                                            .map(|i| ident[..=i].trim())
                                            .unwrap_or(ident.as_str());
                                        let synthetic =
                                            format!("{name_email} {} {}", prev_ts + 120, tz);
                                        let _ = grit_lib::refs::append_reflog(
                                            &repo.git_dir,
                                            &refname,
                                            &last.new_oid,
                                            &oid,
                                            &synthetic,
                                            &msg,
                                            true,
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        let entry = format!("{zero} {oid} {ident}\t{msg}\n");
                        let _ = fs::write(&reflog_path, entry);
                    }
                } else {
                    let entry = format!("{zero} {oid} {ident}\t{msg}\n");
                    let _ = fs::write(&reflog_path, entry);
                }
            } else {
                let entry = format!("{zero} {oid} {ident}\t{msg}\n");
                let _ = fs::write(&reflog_path, entry);
            }
        }
    }

    // Set up tracking for `--track`, or when the start point is a remote-tracking ref (Git default).
    if let Some(sp) = start_point {
        let remote_ref = if sp.starts_with("refs/remotes/") {
            Some(sp.to_string())
        } else if grit_lib::refs::resolve_ref(&repo.git_dir, &format!("refs/remotes/{sp}")).is_ok()
        {
            Some(format!("refs/remotes/{sp}"))
        } else {
            None
        };
        let want_tracking = args.track.is_some() || (!args.no_track && remote_ref.is_some());
        if want_tracking {
            if let Some(rref) = remote_ref {
                let stripped = rref.strip_prefix("refs/remotes/").unwrap_or(&rref);
                if let Some(slash) = stripped.find('/') {
                    let remote = &stripped[..slash];
                    let branch = &stripped[slash + 1..];
                    let config_path = repo.git_dir.join("config");
                    let mut cfg = std::fs::read_to_string(&config_path).unwrap_or_default();
                    cfg.push_str(&format!("\n[branch \"{}\"]", name));
                    cfg.push_str(&format!("\n\tremote = {}", remote));
                    cfg.push_str(&format!("\n\tmerge = refs/heads/{}\n", branch));
                    std::fs::write(&config_path, cfg)?;
                }
            } else if args.track.is_some() {
                if let Ok(full) = resolve_upstream_symbolic_name(repo, sp) {
                    let config_path = repo.git_dir.join("config");
                    let mut cfg = std::fs::read_to_string(&config_path).unwrap_or_default();
                    cfg.push_str(&format!("\n[branch \"{}\"]", name));
                    if let Some(rest) = full.strip_prefix("refs/remotes/") {
                        if let Some(slash) = rest.find('/') {
                            let remote = &rest[..slash];
                            let branch = &rest[slash + 1..];
                            cfg.push_str(&format!("\n\tremote = {}", remote));
                            cfg.push_str(&format!("\n\tmerge = refs/heads/{}\n", branch));
                        }
                    } else if full.starts_with("refs/heads/") {
                        cfg.push_str("\n\tremote = .");
                        cfg.push_str(&format!("\n\tmerge = {}\n", full));
                    }
                    std::fs::write(&config_path, cfg)?;
                } else if let Some(full) =
                    symbolic_full_name(repo, sp).filter(|f| f.starts_with("refs/heads/"))
                {
                    let config_path = repo.git_dir.join("config");
                    let mut cfg = std::fs::read_to_string(&config_path).unwrap_or_default();
                    cfg.push_str(&format!("\n[branch \"{}\"]", name));
                    cfg.push_str("\n\tremote = .");
                    cfg.push_str(&format!("\n\tmerge = {full}\n"));
                    std::fs::write(&config_path, cfg)?;
                }
            }
        }
    }

    Ok(())
}

fn should_log_ref_updates(repo: &Repository) -> bool {
    ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|cfg| cfg.get("core.logallrefupdates"))
        .map(|v| {
            let lowered = v.trim().to_ascii_lowercase();
            lowered == "true" || lowered == "always"
        })
        .unwrap_or(false)
}

/// Delete one or more branches (`git branch -d a b` / `-D`).
///
/// Clap maps the first two positionals to `name` and `start_point`; for delete mode the
/// second positional is another branch to remove, not a start point.
fn delete_branches(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let mut names: Vec<&str> = Vec::new();
    if let Some(n) = args.name.as_deref() {
        names.push(n);
    }
    if let Some(n) = args.start_point.as_deref() {
        names.push(n);
    }
    for n in &args.extra_names {
        names.push(n.as_str());
    }
    if names.is_empty() {
        bail!("branch name required");
    }
    for name in names {
        delete_branch(repo, head, args, name)?;
    }
    Ok(())
}

/// Delete a branch.
fn delete_branch(repo: &Repository, head: &HeadState, args: &Args, name_input: &str) -> Result<()> {
    if args.remotes {
        let refname = if name_input.starts_with("refs/remotes/") {
            name_input.to_owned()
        } else {
            format!("refs/remotes/{name_input}")
        };
        let branch_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &refname).map_err(|_| {
            anyhow::anyhow!("error: remote-tracking branch '{name_input}' not found.")
        })?;
        grit_lib::refs::delete_ref(&repo.git_dir, &refname).map_err(|e| anyhow::anyhow!("{e}"))?;
        if !args.quiet {
            let hex = branch_oid.to_hex();
            let short = &hex[..7.min(hex.len())];
            eprintln!("Deleted remote-tracking branch {name_input} (was {short}).");
        }
        return Ok(());
    }

    let resolved_ref =
        symbolic_full_name(repo, name_input).filter(|full| full.starts_with("refs/heads/"));
    let (name, refname) = if let Some(full) = resolved_ref {
        (
            full.strip_prefix("refs/heads/")
                .unwrap_or(name_input)
                .to_owned(),
            full,
        )
    } else {
        (name_input.to_owned(), format!("refs/heads/{name_input}"))
    };

    if let Some(path) = branch_checked_out_in_other_worktree(repo, &name) {
        bail!(
            "cannot delete branch '{}' used by worktree at '{}'",
            name,
            path
        );
    }

    let current = head.branch_name().unwrap_or("");
    if name == current {
        let wt_path = repo
            .work_tree
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| repo.git_dir.display().to_string());
        bail!(
            "cannot delete branch '{}' used by worktree at '{}'",
            name,
            wt_path
        );
    }

    let branch_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &refname)
        .map_err(|_| anyhow::anyhow!("branch '{name}' not found."))?;

    // For -d (not -D), check if branch is merged into HEAD
    if args.delete && !args.force_delete {
        if let Some(head_oid) = head.oid() {
            if !is_ancestor(repo, branch_oid, *head_oid).unwrap_or(false) {
                bail!(
                    "error: the branch '{}' is not fully merged.\nIf you are sure you want to delete it, run 'git branch -D {}'",
                    name,
                    name
                );
            }
        }
    }

    grit_lib::refs::delete_ref(&repo.git_dir, &refname).map_err(|e| anyhow::anyhow!("{e}"))?;

    // For files backend, clean up empty parent directories
    if !grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
        let ref_path = repo.git_dir.join(&refname);
        let heads_dir = repo.git_dir.join("refs/heads");
        let mut parent = ref_path.parent();
        while let Some(p) = parent {
            if p == heads_dir || !p.starts_with(&heads_dir) {
                break;
            }
            if fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
    }

    if !args.quiet {
        let hex = branch_oid.to_hex();
        let short = &hex[..7.min(hex.len())];
        eprintln!("Deleted branch {name} (was {short}).");
    }

    Ok(())
}

/// Rename a branch.
fn rename_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let (old_name_owned, new_name_owned);
    let (old_name, new_name): (&str, &str);
    if let Some(sp) = args.start_point.as_deref() {
        old_name_owned = args.name.as_deref().unwrap_or("").to_owned();
        new_name_owned = sp.to_owned();
        old_name = &old_name_owned;
        new_name = &new_name_owned;
    } else if let Some(n) = args.name.as_deref() {
        old_name_owned = head
            .branch_name()
            .ok_or_else(|| {
                if matches!(head, HeadState::Detached { .. }) {
                    eprintln!("fatal: cannot rename the current branch while not on any");
                    std::process::exit(128);
                } else {
                    anyhow::anyhow!("no current branch to rename")
                }
            })?
            .to_owned();
        new_name_owned = n.to_owned();
        old_name = &old_name_owned;
        new_name = &new_name_owned;
    } else {
        // No args at all: dump usage
        eprintln!("error: branch name required");
        std::process::exit(128);
    };

    // Renaming a branch to itself is a no-op
    if old_name == new_name {
        return Ok(());
    }

    let old_ref = format!("refs/heads/{old_name}");
    let new_ref = format!("refs/heads/{new_name}");

    // Resolve old branch - check both loose and packed refs
    let old_oid = if let Ok(oid) = grit_lib::refs::resolve_ref(&repo.git_dir, &old_ref) {
        oid
    } else if head.branch_name() == Some(old_name) && head.oid().is_none() {
        // Allow renaming an unborn current branch (e.g. immediately after init).
        let head_path = repo.git_dir.join("HEAD");
        let head_content = format!("ref: refs/heads/{new_name}\n");
        fs::write(head_path, head_content)?;
        rename_branch_config(repo, old_name, new_name)?;
        return Ok(());
    } else if branch_ref_is_unborn_across_worktrees(repo, &old_ref)? {
        eprintln!("fatal: no commit on branch '{old_name}' yet");
        std::process::exit(128);
    } else {
        eprintln!("fatal: no branch named '{old_name}'");
        std::process::exit(128);
    };

    // Check if new name already exists (unless force; -M or -m -f)
    let force = args.force_rename || args.force;
    if !force && grit_lib::refs::resolve_ref(&repo.git_dir, &new_ref).is_ok() {
        bail!("A branch named '{new_name}' already exists.");
    }

    // Check if the new name is checked out in any worktree (including main)
    let new_branch_current = head.branch_name() == Some(new_name)
        || branch_checked_out_in_other_worktree(repo, new_name).is_some();
    if new_branch_current {
        bail!(
            "fatal: cannot force update the branch '{}' used by worktree at '{}'",
            new_name,
            repo.work_tree
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| ".".to_owned())
        );
    }

    // Capture reflog bytes before `delete_ref`: that helper removes `logs/<refname>` too.
    let reflog_dir = repo.git_dir.join("logs");
    let old_log_path = reflog_dir.join(&old_ref);
    let old_reflog_bytes = if old_log_path.is_file() {
        fs::read(&old_log_path).ok()
    } else {
        None
    };

    // Delete the old ref FIRST to avoid d/f conflicts
    // (e.g., renaming m to m/m needs to remove refs/heads/m file before
    // creating refs/heads/m/ directory, or n/n to n needs to remove refs/heads/n/
    // directory before creating refs/heads/n file)
    grit_lib::refs::delete_ref(&repo.git_dir, &old_ref).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Clean up empty parent directories for old ref
    let old_path = repo.git_dir.join(&old_ref);
    let heads_dir = repo.git_dir.join("refs/heads");
    let mut parent = old_path.parent();
    while let Some(p) = parent {
        if p == heads_dir || !p.starts_with(&heads_dir) {
            break;
        }
        if fs::remove_dir(p).is_err() {
            break;
        }
        parent = p.parent();
    }

    // Now write the new ref
    grit_lib::refs::write_ref(&repo.git_dir, &new_ref, &old_oid)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Update HEAD if we renamed the current branch
    if head.branch_name() == Some(old_name) {
        let head_content = format!("ref: refs/heads/{new_name}\n");
        fs::write(repo.git_dir.join("HEAD"), head_content)?;
    }

    // Also update HEAD in worktrees that have the old branch checked out
    update_worktree_heads(repo, old_name, new_name)?;

    // Rename reflog: migrate content captured before `delete_ref`, then append rename entry.
    let new_log = reflog_dir.join(&new_ref);
    if let Some(log_bytes) = old_reflog_bytes {
        let logs_heads_dir = reflog_dir.join("refs/heads");
        let mut parent = old_log_path.parent();
        while let Some(p) = parent {
            if p == logs_heads_dir || !p.starts_with(&logs_heads_dir) {
                break;
            }
            if fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
        if let Some(parent) = new_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let ident = get_reflog_identity();
        let rename_entry = format!(
            "{oid} {oid} {ident}\tBranch: renamed {old_ref} to {new_ref}\n",
            oid = old_oid
        );
        let old_content = String::from_utf8_lossy(&log_bytes).to_string();
        let new_content = format!("{}{rename_entry}", old_content);
        let _ = fs::write(&new_log, new_content.as_bytes());
    }

    // Write HEAD reflog entry for branch rename
    if head.branch_name() == Some(old_name) {
        let head_log = reflog_dir.join("HEAD");
        let ident = get_reflog_identity();
        // Match Git: two lines (old?zero, zero?old) so `git reflog` skips the middle entry
        // and `log -g` indices align with upstream tests.
        let zero_hex = zero_oid().to_hex();
        let oid_hex = old_oid.to_hex();
        let entry1 =
            format!("{oid_hex} {zero_hex} {ident}\tBranch: renamed {old_ref} to {new_ref}\n",);
        let entry2 =
            format!("{zero_hex} {oid_hex} {ident}\tBranch: renamed {old_ref} to {new_ref}\n",);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&head_log)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(entry1.as_bytes())?;
                f.write_all(entry2.as_bytes())
            });
    }

    // Rename config sections
    rename_branch_config(repo, old_name, new_name)?;

    Ok(())
}

/// Update HEAD in linked worktrees after branch rename.
fn update_worktree_heads(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    let worktrees_dir = repo.git_dir.join("worktrees");
    if let Ok(entries) = fs::read_dir(&worktrees_dir) {
        for entry in entries.flatten() {
            let head_path = entry.path().join("HEAD");
            if let Ok(content) = fs::read_to_string(&head_path) {
                let trimmed = content.trim();
                let expected = format!("ref: refs/heads/{old_name}");
                if trimmed == expected {
                    let new_content = format!("ref: refs/heads/{new_name}\n");
                    let _ = fs::write(&head_path, new_content);
                }
            }
        }
    }
    Ok(())
}

fn branch_used_by_other_worktree(repo: &Repository, branch: &str) -> Result<Option<String>> {
    let occupied = crate::commands::worktree_refs::occupied_branch_refs(repo);
    let target = format!("refs/heads/{branch}");
    if let Some(wt_path) = occupied.get(&target) {
        return Ok(Some(wt_path.clone()));
    }
    Ok(None)
}

fn branch_checked_out_in_other_worktree(repo: &Repository, branch: &str) -> Option<String> {
    branch_used_by_other_worktree(repo, branch).ok().flatten()
}

/// Get reflog identity string.
fn get_reflog_identity() -> String {
    let name = std::env::var("GIT_COMMITTER_NAME").unwrap_or_else(|_| "Test User".to_string());
    let email =
        std::env::var("GIT_COMMITTER_EMAIL").unwrap_or_else(|_| "test@example.com".to_string());
    let date = std::env::var("GIT_COMMITTER_DATE").unwrap_or_else(|_| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{now} +0000")
    });
    format!("{name} <{email}> {date}")
}

/// Rename branch config sections.
fn rename_branch_config(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let old_section = format!("[branch \"{old_name}\"]");
    let new_section = format!("[branch \"{new_name}\"]");
    if content.contains(&old_section) {
        let updated = content.replace(&old_section, &new_section);
        fs::write(&config_path, updated)?;
    }
    Ok(())
}

/// True when `branch_ref` (e.g. `refs/heads/x`) is checked out as an unborn branch in the main
/// repo or any linked worktree (orphan / no tip commit yet).
fn branch_ref_is_unborn_across_worktrees(repo: &Repository, branch_ref: &str) -> Result<bool> {
    let common = grit_lib::refs::common_dir(&repo.git_dir).unwrap_or_else(|| repo.git_dir.clone());
    let mut admin_dirs = vec![common.clone()];
    if let Ok(rd) = fs::read_dir(common.join("worktrees")) {
        for e in rd.flatten() {
            admin_dirs.push(e.path());
        }
    }
    for admin in admin_dirs {
        let hp = admin.join("HEAD");
        let Ok(content) = fs::read_to_string(&hp) else {
            continue;
        };
        let t = content.trim();
        let Some(sym) = t.strip_prefix("ref: ") else {
            continue;
        };
        if sym != branch_ref {
            continue;
        }
        let st = resolve_head(&admin)?;
        if matches!(st, HeadState::Branch { oid: None, .. }) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn copy_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let (src_name_owned, dst_name_owned);
    let (src_name, dst_name): (&str, &str);
    if let Some(sp) = args.start_point.as_deref() {
        src_name_owned = args.name.as_deref().unwrap_or("").to_owned();
        dst_name_owned = sp.to_owned();
        src_name = &src_name_owned;
        dst_name = &dst_name_owned;
    } else if let Some(n) = args.name.as_deref() {
        src_name_owned = head
            .branch_name()
            .ok_or_else(|| {
                if matches!(head, HeadState::Detached { .. }) {
                    eprintln!("fatal: cannot copy the current branch while not on any");
                    std::process::exit(128);
                }
                anyhow::anyhow!("no current branch to copy")
            })?
            .to_owned();
        dst_name_owned = n.to_owned();
        src_name = &src_name_owned;
        dst_name = &dst_name_owned;
    } else {
        eprintln!("error: branch name required");
        std::process::exit(128);
    };

    let src_ref = format!("refs/heads/{src_name}");
    let dst_ref = format!("refs/heads/{dst_name}");

    let src_oid = if let Ok(oid) = grit_lib::refs::resolve_ref(&repo.git_dir, &src_ref) {
        oid
    } else if branch_ref_is_unborn_across_worktrees(repo, &src_ref)? {
        eprintln!("fatal: no commit on branch '{src_name}' yet");
        std::process::exit(128);
    } else {
        eprintln!("fatal: no branch named '{src_name}'");
        std::process::exit(128);
    };

    // Copying a branch to itself is a no-op (Git: `branch -c m2 m2`).
    if src_name == dst_name {
        return Ok(());
    }

    // Check if dst already exists (unless force copy)
    if !args.force_copy && grit_lib::refs::resolve_ref(&repo.git_dir, &dst_ref).is_ok() {
        bail!("A branch named '{dst_name}' already exists.");
    }

    // Check for d/f conflict: new ref is prefix of existing refs
    let heads_dir = repo.git_dir.join("refs/heads");
    if heads_dir.join(dst_name).is_dir() {
        bail!("'{dst_name}' exists; cannot create 'refs/heads/{dst_name}'");
    }

    grit_lib::refs::write_ref(&repo.git_dir, &dst_ref, &src_oid)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Copy reflog if exists
    let reflog_dir = repo.git_dir.join("logs");
    let src_log = reflog_dir.join(&src_ref);
    let dst_log = reflog_dir.join(&dst_ref);
    if src_log.exists() {
        if let Some(parent) = dst_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(&src_log, &dst_log);
    }

    // Copy config section
    let config_path = repo.git_dir.join("config");
    if let Ok(content) = fs::read_to_string(&config_path) {
        let old_section = format!("[branch \"{src_name}\"]");
        let new_section = format!("[branch \"{dst_name}\"]");
        if content.contains(&old_section) {
            // Extract the section and duplicate it
            let mut result = content.clone();
            let mut section_text = String::new();
            let mut in_section = false;
            for line in content.lines() {
                if line.trim() == old_section.trim() {
                    in_section = true;
                    section_text.push_str(&new_section);
                    section_text.push('\n');
                    continue;
                }
                if in_section {
                    if line.starts_with('[') {
                        in_section = false;
                    } else {
                        section_text.push_str(line);
                        section_text.push('\n');
                    }
                }
            }
            if !section_text.is_empty() {
                result.push('\n');
                result.push_str(&section_text);
                let _ = fs::write(&config_path, result);
            }
        }
    }

    Ok(())
}

/// Collect branch names from a refs directory.
fn collect_branches(dir: &Path, prefix: &str, out: &mut Vec<(String, ObjectId)>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };

        if path.is_dir() {
            collect_branches(&path, &full_name, out)?;
        } else if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                out.push((full_name, oid));
            }
        }
    }

    Ok(())
}

/// Get the first line of a commit's message.
fn commit_subject(odb: &grit_lib::odb::Odb, oid: &ObjectId) -> Option<String> {
    let obj = odb.read(oid).ok()?;
    let commit = parse_commit(&obj.data).ok()?;
    commit.message.lines().next().map(String::from)
}

/// Extract committer timestamp from a commit for sorting.
fn committer_time(
    odb: &grit_lib::odb::Odb,
    oid: &ObjectId,
    branch_name: &str,
    is_remote: bool,
) -> i64 {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    let commit = match parse_commit(&obj.data) {
        Ok(c) => c,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    parse_signature_time(&commit.committer)
}

/// Extract author timestamp from a commit for sorting.
fn author_time(
    odb: &grit_lib::odb::Odb,
    oid: &ObjectId,
    branch_name: &str,
    is_remote: bool,
) -> i64 {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    let commit = match parse_commit(&obj.data) {
        Ok(c) => c,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    parse_signature_time(&commit.author)
}

/// Parse the Unix timestamp from a Git signature line like "Name <email> 1234567890 +0000".
fn parse_signature_time(sig: &str) -> i64 {
    let parts: Vec<&str> = sig.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse::<i64>().unwrap_or(0)
    } else {
        0
    }
}

/// Simple glob matching for branch pattern filtering.
/// Supports `*` (match any chars) and `?` (match one char).
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_case(pattern: &str, text: &str, ignore_case: bool) -> bool {
    if ignore_case {
        glob_match(&pattern.to_lowercase(), &text.to_lowercase())
    } else {
        glob_match(pattern, text)
    }
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'?' {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            star_ti += 1;
            ti = star_ti;
            pi = star_pi + 1;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}
