//! `grit rebase` — reapply commits on top of another base tip.
//!
//! Non-interactive rebase replays a series of commits by cherry-picking each
//! one onto the new base.  For a commit C with parent P being replayed onto
//! current HEAD:
//!
//!   - base   = P.tree     (parent of the commit being replayed)
//!   - ours   = HEAD.tree  (current tip we're building on)
//!   - theirs = C.tree     (the commit being replayed)
//!
//! This three-way merge produces the replayed commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use grit_lib::config::ConfigSet;
use grit_lib::diff::{self, count_changes, diff_index_to_tree, DiffEntry};
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::merge_base::{ancestor_closure, is_ancestor, merge_bases_first_vs_rest};
use grit_lib::merge_file::{merge, ConflictStyle, MergeInput};
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::patch_ids::compute_patch_id;
use grit_lib::refs::append_reflog;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, split_revision_token, OrderingMode, RevListOptions};
use grit_lib::rev_parse::{
    abbreviate_object_id, peel_to_commit_for_merge_base, resolve_revision,
    resolve_revision_for_range_end, split_triple_dot_range,
};
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::whitespace_rule::{fix_blob_bytes, parse_whitespace_rule, WS_DEFAULT_RULE};
use grit_lib::write_tree::write_tree_from_index;

use super::checkout::check_dirty_worktree;
use super::stash;
use crate::ident::{resolve_email, resolve_name, IdentRole};

#[derive(Clone, Copy)]
enum RebaseBackend {
    Merge,
    Apply,
}

#[derive(Clone, Copy)]
struct RebaseConflictContext<'a> {
    backend: RebaseBackend,
    picked_subject: &'a str,
}

impl<'a> RebaseConflictContext<'a> {
    fn style(self, repo: &Repository) -> ConflictStyle {
        let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
            return ConflictStyle::Merge;
        };
        match config
            .get("merge.conflictstyle")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "diff3" => ConflictStyle::Diff3,
            "zdiff3" => ConflictStyle::ZealousDiff3,
            _ => ConflictStyle::Merge,
        }
    }

    fn label_ours(self) -> &'static str {
        "HEAD"
    }

    fn label_base(self) -> String {
        match self.backend {
            RebaseBackend::Merge => format!("parent of {}", self.picked_subject),
            RebaseBackend::Apply => "constructed fake ancestor".to_string(),
        }
    }
}

/// Arguments for `grit rebase`.
#[derive(Debug, Clone, ClapArgs)]
#[command(about = "Reapply commits on top of another base tip")]
pub struct Args {
    /// Upstream branch to rebase onto (default: upstream tracking branch).
    #[arg(value_name = "UPSTREAM")]
    pub upstream: Option<String>,

    /// Rebase onto a specific base (used with `--onto <newbase> <upstream>`).
    #[arg(long)]
    pub onto: Option<String>,

    /// Rebase all commits reachable from the branch tip, not just those after the merge-base with upstream.
    #[arg(long)]
    pub root: bool,

    /// Interactive rebase (write todo list only).
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Continue the rebase after resolving conflicts.
    #[arg(long = "continue")]
    pub r#continue: bool,

    /// Abort the in-progress rebase.
    #[arg(long = "abort")]
    pub abort: bool,

    /// Skip the current commit and continue.
    #[arg(long = "skip")]
    pub skip: bool,

    /// Run a shell command after each commit is applied.
    #[arg(short = 'x', long = "exec")]
    pub exec: Option<String>,

    /// Use the merge backend for rebasing (default, accepted for compatibility).
    #[arg(long = "merge", short = 'm', conflicts_with = "apply")]
    pub merge: bool,

    /// Use the apply backend for rebasing (accepted for compatibility).
    #[arg(long = "apply", conflicts_with = "merge")]
    pub apply: bool,

    /// Rebase merge commits (accepted for compatibility; uses `rebase-merge/` state layout when set).
    #[arg(
        long = "rebase-merges",
        alias = "r",
        conflicts_with = "no_rebase_merges"
    )]
    pub rebase_merges: bool,

    /// Disable rebasing merges even when `rebase.rebaseMerges` is true.
    #[arg(long = "no-rebase-merges", conflicts_with = "rebase_merges")]
    pub no_rebase_merges: bool,

    /// Force rebase even if the current branch is up to date.
    #[arg(long = "no-ff", alias = "force-rebase")]
    pub no_ff: bool,

    /// Keep the base of the branch (rebase onto the merge-base of upstream and branch).
    #[arg(long = "keep-base", action = clap::ArgAction::SetTrue)]
    pub keep_base: bool,

    /// Use the fork-point algorithm to find the merge base.
    #[arg(long = "fork-point", overrides_with = "no_fork_point")]
    pub fork_point: bool,

    /// Do not use the fork-point algorithm.
    #[arg(long = "no-fork-point")]
    pub no_fork_point: bool,

    /// Replay every picked commit even when it matches upstream by patch-id (Git default off).
    #[arg(
        long = "reapply-cherry-picks",
        overrides_with = "no_reapply_cherry_picks"
    )]
    pub reapply_cherry_picks: bool,

    /// Omit commits that match upstream by patch-id (default unless `--keep-base`).
    #[arg(long = "no-reapply-cherry-picks")]
    pub no_reapply_cherry_picks: bool,

    /// Be verbose (show diffs).
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Update stale tracking branches after rebase.
    #[arg(long = "update-refs", conflicts_with = "no_update_refs")]
    pub update_refs: bool,

    /// Do not update other branches even when `rebase.updateRefs` is true.
    #[arg(long = "no-update-refs", conflicts_with = "update_refs")]
    pub no_update_refs: bool,

    /// How to handle commits that become empty (merge backend; accepted for compatibility).
    #[arg(long = "empty", value_name = "mode")]
    pub empty: Option<String>,

    /// Merge strategy (merge backend; accepted for compatibility).
    #[arg(short = 's', long = "strategy", value_name = "strategy")]
    pub strategy: Option<String>,

    /// Options for the merge strategy (merge backend; accepted for compatibility).
    #[arg(short = 'X', long = "strategy-option", value_name = "option")]
    pub strategy_option: Vec<String>,

    /// Branch to rebase (checkout first, then rebase onto upstream).
    #[arg(value_name = "BRANCH")]
    pub branch: Option<String>,

    /// Show a diffstat of what would be replayed (also honors `rebase.stat` config).
    #[arg(long = "stat")]
    pub stat: bool,

    /// Do not show a diffstat (overrides `rebase.stat` config).
    #[arg(short = 'n', long = "no-stat")]
    pub no_stat: bool,

    /// Passed through for compatibility; validated when present.
    #[arg(short = 'C', value_name = "n")]
    pub context_lines: Option<String>,

    /// Passed through for compatibility; validated when present.
    #[arg(long = "whitespace", value_name = "action")]
    pub whitespace: Option<String>,

    /// Stash local changes before starting and restore after (or honor `rebase.autostash`).
    #[arg(long = "autostash")]
    pub autostash: bool,

    /// Do not stash local changes (overrides `rebase.autostash`).
    #[arg(long = "no-autostash")]
    pub no_autostash: bool,

    /// Quit an in-progress rebase, keeping HEAD and working tree as-is.
    #[arg(long = "quit")]
    pub quit: bool,

    /// Move fixup!/squash! commits next to their targets (also implied by `rebase.autosquash` with `-i`).
    #[arg(long = "autosquash")]
    pub autosquash: bool,

    /// Disable autosquash even when `rebase.autosquash` is true.
    #[arg(long = "no-autosquash")]
    pub no_autosquash: bool,

    /// Keep commits that do not change any file (empty patch).
    #[arg(short = 'k', long = "keep-empty")]
    pub keep_empty: bool,
}

/// Expand combined short flags (`-ki`, `-ik`) before clap parsing.
pub fn preprocess_rebase_argv(rest: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for arg in rest {
        // Clap does not accept glued `-C<n>`; Git's rebase passes this through to the apply backend.
        if arg.len() > 2 && arg.starts_with("-C") && !arg.starts_with("--") {
            let suffix = &arg[2..];
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                out.push("-C".to_string());
                out.push(suffix.to_string());
                continue;
            }
        }
        if arg.len() > 2
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.chars().nth(1) != Some('-')
        {
            let flags: String = arg.chars().skip(1).collect();
            let mut expanded = Vec::new();
            for ch in flags.chars() {
                match ch {
                    'i' => expanded.push("-i".to_string()),
                    'k' => expanded.push("-k".to_string()),
                    _ => expanded.push(format!("-{ch}")),
                }
            }
            out.extend(expanded);
        } else {
            out.push(arg.clone());
        }
    }
    out
}

/// Run the `rebase` command.
pub fn run(mut args: Args) -> Result<()> {
    validate_compat_syntax(&args)?;

    if args.keep_base && args.onto.is_some() {
        bail!("options '--keep-base' and '--onto' cannot be used together");
    }

    if args.root {
        if args.keep_base {
            bail!("options '--keep-base' and '--root' cannot be used together");
        }
        if args.fork_point {
            bail!("options '--root' and '--fork-point' cannot be used together");
        }
        if args.onto.is_none() {
            bail!("a base commit must be provided with --onto when using --root");
        }
        if args.upstream.is_some() && args.branch.is_some() {
            bail!("git rebase: too many arguments");
        }
        if args.upstream.is_some() && args.branch.is_none() {
            args.branch = args.upstream.take();
        }
    }

    if args.abort {
        return do_abort();
    }
    if args.r#continue {
        return do_continue();
    }
    if args.skip {
        return do_skip();
    }
    if args.quit {
        return do_quit();
    }

    let pre_rebase_hook_second = args.branch.clone();

    // If a branch argument is given, checkout that branch first.
    // Resolve `upstream` before checkout: `git rebase <upstream> <branch>` uses the pre-checkout
    // meaning of `HEAD` and other relative specs.
    if args.branch.is_some() {
        let repo = Repository::discover(None).context("not a git repository")?;
        let uspec = args.upstream.as_deref().unwrap_or("HEAD");
        let uoid = resolve_revision(&repo, uspec)
            .with_context(|| format!("bad revision '{uspec}'"))?
            .to_hex();
        args.upstream = Some(uoid);
    }

    // Fix up the reflog so @{-N} isn't polluted by the internal checkout.
    if let Some(ref branch) = args.branch {
        let self_exe = std::env::current_exe().context("cannot determine own executable")?;
        let status = std::process::Command::new(&self_exe)
            .arg("checkout")
            .arg("--quiet")
            .arg(branch)
            .status()
            .context("failed to checkout branch")?;
        if !status.success() {
            bail!("checkout {} failed", branch);
        }
        // Replace the checkout reflog entry with a rebase message
        let repo = Repository::discover(None).context("not a git repository")?;
        let reflog_path = repo.git_dir.join("logs/HEAD");
        if let Ok(content) = std::fs::read_to_string(&reflog_path) {
            let lines: Vec<&str> = content.lines().collect();
            if let Some(last) = lines.last() {
                if last.contains("checkout: moving from ") {
                    if let Some(tab_idx) = last.rfind('\t') {
                        let upstream_name = args.upstream.as_deref().unwrap_or("HEAD");
                        let new_line = format!(
                            "{}\trebase (start): checkout {}",
                            &last[..tab_idx],
                            upstream_name
                        );
                        let mut new_lines: Vec<String> = lines[..lines.len() - 1]
                            .iter()
                            .map(|s| s.to_string())
                            .collect();
                        new_lines.push(new_line);
                        let _ = std::fs::write(&reflog_path, new_lines.join("\n") + "\n");
                    }
                }
            }
        }
        args.branch = None;
    }

    // If no upstream specified and no --onto, try to find the upstream tracking branch.
    if args.upstream.is_none() && args.onto.is_none() && !args.root {
        let repo = Repository::discover(None).context("not a git repository")?;
        let head = resolve_head(&repo.git_dir)?;
        let branch_name = match &head {
            HeadState::Branch { short_name, .. } => short_name.clone(),
            _ => bail!("no upstream configured for the current branch"),
        };
        // Try to resolve @{upstream}
        match resolve_revision(&repo, &format!("{}@{{upstream}}", branch_name)) {
            Ok(_) => {
                args.upstream = Some(format!("{}@{{upstream}}", branch_name));
            }
            Err(_) => {
                bail!(
                    "There is no tracking information for the current branch.\n\
                     Please specify which branch you want to rebase against."
                );
            }
        }
    }

    do_rebase(args, pre_rebase_hook_second)
}

// ── Rebase state directory layout ───────────────────────────────────
//
// .git/rebase-apply/
//   head-name   — original branch ref (e.g. refs/heads/topic)
//   orig-head   — original HEAD OID before rebase
//   onto        — OID of the new base
//   todo        — remaining commit OIDs to replay, one per line
//   current     — OID of the commit currently being replayed
//   msgnum      — 1-based index of current patch
//   end         — total number of patches

fn validate_compat_syntax(args: &Args) -> Result<()> {
    if let Some(ref c) = args.context_lines {
        if c.parse::<u32>().is_err() {
            bail!("switch `C' expects a numerical value");
        }
    }
    if let Some(ref ws) = args.whitespace {
        let allowed = ["warn", "nowarn", "error", "error-all", "fix", "strip"];
        if !allowed.contains(&ws.as_str()) {
            bail!("Invalid whitespace option: '{ws}'");
        }
    }
    if let Some(ref empty) = args.empty {
        let e = empty.to_ascii_lowercase();
        if !matches!(e.as_str(), "drop" | "keep" | "stop" | "ask") {
            bail!(
                "unrecognized empty type '{empty}'; valid values are \"drop\", \"keep\", and \"stop\"."
            );
        }
    }
    Ok(())
}

/// True when the user requested the merge-style rebase backend via flags that Git treats as merge-only.
fn merge_backend_requested_by_flags(args: &Args, want_autosquash: bool) -> bool {
    if args.merge || args.interactive || args.exec.is_some() || args.keep_empty {
        return true;
    }
    if args.empty.is_some() {
        return true;
    }
    if want_autosquash {
        return true;
    }
    if args.strategy.is_some() || !args.strategy_option.is_empty() {
        return true;
    }
    let reapply_explicit = args.reapply_cherry_picks || args.no_reapply_cherry_picks;
    if reapply_explicit && !args.keep_base {
        return true;
    }
    if args.root && args.onto.is_none() {
        return true;
    }
    false
}

/// True when options force the apply backend (`git am` style), matching Git's `git_am_opts` / `--apply`.
fn apply_backend_forced(args: &Args) -> bool {
    if args.apply {
        return true;
    }
    if args.context_lines.is_some() {
        return true;
    }
    args.whitespace
        .as_deref()
        .is_some_and(|w| w.eq_ignore_ascii_case("fix") || w.eq_ignore_ascii_case("strip"))
}

/// Reject mixing apply-only and merge-only options (and config) the same way as upstream `git rebase`.
fn validate_apply_merge_backend_combo(
    args: &Args,
    config: &ConfigSet,
    want_autosquash: bool,
) -> Result<()> {
    let apply_forced = apply_backend_forced(args);
    if !apply_forced {
        return Ok(());
    }

    let merge_requested = merge_backend_requested_by_flags(args, want_autosquash);
    if merge_requested {
        bail!("apply options and merge options cannot be used together");
    }

    let rebase_merges_cli = if args.no_rebase_merges {
        Some(false)
    } else if args.rebase_merges {
        Some(true)
    } else {
        None
    };
    let config_rebase_merges = config.get_bool("rebase.rebaseMerges").and_then(|r| r.ok());
    let effective_rebase_merges =
        rebase_merges_cli.unwrap_or(config_rebase_merges.unwrap_or(false));

    let update_refs_cli = if args.no_update_refs {
        Some(false)
    } else if args.update_refs {
        Some(true)
    } else {
        None
    };
    let config_update_refs = config.get_bool("rebase.updateRefs").and_then(|r| r.ok());
    let effective_update_refs = update_refs_cli.unwrap_or(config_update_refs.unwrap_or(false));

    if effective_rebase_merges {
        bail!(
            "apply options are incompatible with rebase.rebaseMerges.  Consider adding --no-rebase-merges"
        );
    }
    if effective_update_refs {
        bail!(
            "apply options are incompatible with rebase.updateRefs.  Consider adding --no-update-refs"
        );
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RebaseTodoCmd {
    Pick,
    Reword,
    Fixup,
    Squash,
}

impl RebaseTodoCmd {
    fn as_str(self) -> &'static str {
        match self {
            RebaseTodoCmd::Pick => "pick",
            RebaseTodoCmd::Reword => "reword",
            RebaseTodoCmd::Fixup => "fixup",
            RebaseTodoCmd::Squash => "squash",
        }
    }

    fn parse_word(word: &str) -> Option<Self> {
        match word {
            "pick" | "p" => Some(RebaseTodoCmd::Pick),
            "reword" | "r" => Some(RebaseTodoCmd::Reword),
            "fixup" | "f" => Some(RebaseTodoCmd::Fixup),
            "squash" | "s" => Some(RebaseTodoCmd::Squash),
            _ => None,
        }
    }
}

/// First line of a commit message with continuation lines folded like `git format_subject(..., " ")`.
fn commit_subject_single_line(message: &str) -> String {
    let mut lines = message.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    let mut out = first.trim_end().to_string();
    for line in lines {
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t.trim_start());
    }
    out
}

fn skip_fixupish_prefix(subject: &str) -> Option<&str> {
    let s = subject.trim_start();
    if let Some(rest) = s.strip_prefix("fixup!") {
        return Some(rest.trim_start());
    }
    if let Some(rest) = s.strip_prefix("amend!") {
        return Some(rest.trim_start());
    }
    if let Some(rest) = s.strip_prefix("squash!") {
        return Some(rest.trim_start());
    }
    None
}

fn strip_fixupish_chain(mut p: &str) -> &str {
    while let Some(rest) = skip_fixupish_prefix(p) {
        p = rest;
        p = p.trim_start();
    }
    p
}

fn format_autosquash_subject_for_match(message: &str) -> String {
    commit_subject_single_line(message)
}

fn is_commit_tree_unchanged(repo: &Repository, oid: &ObjectId) -> Result<bool> {
    const GIT_EMPTY_TREE_HEX: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let obj = repo.odb.read(oid)?;
    let commit = parse_commit(&obj.data)?;
    let parent_tree = if let Some(p) = commit.parents.first() {
        let pobj = repo.odb.read(p)?;
        let pc = parse_commit(&pobj.data)?;
        pc.tree
    } else {
        ObjectId::from_hex(GIT_EMPTY_TREE_HEX).map_err(|e| anyhow::anyhow!("{e}"))?
    };
    Ok(commit.tree == parent_tree)
}

/// Validates `rebase.instructionFormat` similarly to Git's `get_commit_format` for todo generation.
fn validate_rebase_instruction_format(config: &ConfigSet) -> Result<()> {
    let Some(fmt) = config.get("rebase.instructionFormat") else {
        return Ok(());
    };
    if fmt.trim().is_empty() {
        return Ok(());
    }
    if !fmt.contains('%') {
        bail!("invalid --pretty format: {fmt}");
    }
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(spec) = chars.next() else {
            bail!("invalid --pretty format: {fmt}");
        };
        match spec {
            'n' | '%' => {}
            'H' | 'h' | 'T' | 't' | 's' | 'e' | 'b' | 'B' | 'N' => {}
            'P' | 'p' => {}
            'w' | 'W' => {}
            'a' | 'c' => {
                let Some(second) = chars.peek().copied() else {
                    bail!("invalid --pretty format: {fmt}");
                };
                match second {
                    'n' | 'e' | 'd' | 'i' => {
                        chars.next();
                    }
                    'r' if spec == 'a' || spec == 'c' => {
                        chars.next();
                    }
                    _ => bail!("invalid --pretty format: {fmt}"),
                }
            }
            '(' => {
                while let Some(c) = chars.next() {
                    if c == ')' {
                        break;
                    }
                }
            }
            _ => bail!("invalid --pretty format: {fmt}"),
        }
    }
    Ok(())
}

fn format_rebase_todo_line(
    repo: &Repository,
    oid: &ObjectId,
    cmd: RebaseTodoCmd,
    config: &ConfigSet,
    short_oid_field: bool,
) -> Result<String> {
    let obj = repo.odb.read(oid)?;
    let commit = parse_commit(&obj.data)?;
    let subj = commit.message.lines().next().unwrap_or("");
    let empty = is_commit_tree_unchanged(repo, oid).unwrap_or(false);
    let oid_field = if short_oid_field {
        abbreviate_object_id(repo, *oid, 7)?
    } else {
        oid.to_hex()
    };
    let mut line = match config.get("rebase.instructionFormat") {
        None => format!("{} {} # {}", cmd.as_str(), oid_field, subj),
        Some(raw) if raw.trim().is_empty() => {
            format!("{} {} # {}", cmd.as_str(), oid_field, subj)
        }
        Some(tmpl) => {
            let mut t = tmpl.clone();
            if !t.starts_with('#') {
                t = format!("# {t}");
            }
            let rest = crate::commands::show::format_commit_placeholder(&t, oid, &commit);
            format!("{} {} {}", cmd.as_str(), oid_field, rest)
        }
    };
    if empty {
        line.push_str(" # empty");
    }
    Ok(line)
}

fn rearrange_autosquash(
    repo: &Repository,
    oids: Vec<ObjectId>,
) -> Result<Vec<(ObjectId, RebaseTodoCmd)>> {
    let n = oids.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    let mut subjects: Vec<Option<String>> = vec![None; n];
    let mut cmds: Vec<RebaseTodoCmd> = vec![RebaseTodoCmd::Pick; n];
    let mut next: Vec<isize> = vec![-1; n];
    let mut tail: Vec<isize> = vec![-1; n];

    let mut subject_to_index: HashMap<String, usize> = HashMap::new();
    let mut oid_to_index: HashMap<ObjectId, usize> = HashMap::new();
    let mut rearranged = false;

    for i in 0..n {
        let obj = repo.odb.read(&oids[i])?;
        let commit = parse_commit(&obj.data)?;
        let subj = format_autosquash_subject_for_match(&commit.message);
        subjects[i] = Some(subj.clone());

        let mut target_idx: Option<usize> = None;
        if let Some(rest) = skip_fixupish_prefix(&subj) {
            let key = strip_fixupish_chain(rest).trim();
            if subject_to_index.contains_key(key) {
                target_idx = subject_to_index.get(key).copied();
            } else if !key.contains(' ') {
                if let Ok(oid) = resolve_revision(repo, key) {
                    // A branch can point at the fixup commit itself (e.g. `fixup! self-cycle` on
                    // branch `self-cycle`); Git does not treat that as a valid autosquash target.
                    if oid != oids[i] {
                        if let Some(&idx) = oid_to_index.get(&oid) {
                            target_idx = Some(idx);
                        }
                    }
                }
            }
            if target_idx.is_none() {
                for j in 0..i {
                    if let Some(ref sj) = subjects[j] {
                        if sj.starts_with(key) {
                            target_idx = Some(j);
                            break;
                        }
                    }
                }
            }
        }
        if let Some(i2) = target_idx {
            rearranged = true;
            let t = subj.trim_start();
            cmds[i] = if t.starts_with("fixup!") || t.starts_with("amend!") {
                RebaseTodoCmd::Fixup
            } else {
                RebaseTodoCmd::Squash
            };
            if tail[i2] < 0 {
                next[i] = next[i2];
                next[i2] = i as isize;
            } else {
                let t = tail[i2] as usize;
                next[i] = next[t];
                next[t] = i as isize;
            }
            tail[i2] = i as isize;
        }

        if target_idx.is_none() && !subject_to_index.contains_key(&subj) {
            subject_to_index.insert(subj.clone(), i);
            oid_to_index.insert(oids[i], i);
        }
    }

    if !rearranged {
        return Ok(oids.into_iter().map(|o| (o, RebaseTodoCmd::Pick)).collect());
    }

    let mut ordered: Vec<(ObjectId, RebaseTodoCmd)> = Vec::with_capacity(n);
    for i in 0..n {
        if matches!(cmds[i], RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) {
            continue;
        }
        let mut cur = Some(i);
        while let Some(ci) = cur {
            ordered.push((oids[ci], cmds[ci]));
            let nxt = next[ci];
            cur = if nxt >= 0 { Some(nxt as usize) } else { None };
        }
    }
    debug_assert_eq!(ordered.len(), n);
    Ok(ordered)
}

fn parse_todo_line_with_repo(
    repo: Option<&Repository>,
    line: &str,
) -> Result<Option<(ObjectId, RebaseTodoCmd)>> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let mut parts = t.split_whitespace();
    let Some(cmd_word) = parts.next() else {
        return Ok(None);
    };
    let Some(cmd) = RebaseTodoCmd::parse_word(cmd_word) else {
        return Ok(None);
    };
    let Some(hex) = parts.next() else {
        return Ok(None);
    };
    if hex.len() < 4 || hex.len() > 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let oid = if hex.len() == 40 {
        ObjectId::from_hex(hex)?
    } else {
        let Some(r) = repo else {
            return Ok(None);
        };
        resolve_revision(r, hex).with_context(|| format!("todo: bad revision '{hex}'"))?
    };
    Ok(Some((oid, cmd)))
}

/// Normalized rebase todo step for [`replay_remaining`].
#[derive(Debug)]
enum RebaseReplayStep {
    PickLike {
        oid: ObjectId,
        cmd: RebaseTodoCmd,
    },
    Exec(String),
    Edit(ObjectId),
    MergeReuseMessage {
        merge_oid: ObjectId,
        merge_args: String,
    },
}

fn parse_rebase_replay_step(
    repo: &Repository,
    line: &str,
    interactive: bool,
) -> Result<Option<RebaseReplayStep>> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    if interactive {
        return Ok(
            parse_interactive_rebase_todo_line(repo, line)?.map(|p| match p {
                ParsedRebaseTodoLine::Commit { oid, cmd } => {
                    RebaseReplayStep::PickLike { oid, cmd }
                }
                ParsedRebaseTodoLine::Exec(s) => RebaseReplayStep::Exec(s),
                ParsedRebaseTodoLine::Edit(oid) => RebaseReplayStep::Edit(oid),
                ParsedRebaseTodoLine::MergeReuseMessage {
                    merge_oid,
                    merge_args,
                } => RebaseReplayStep::MergeReuseMessage {
                    merge_oid,
                    merge_args,
                },
            }),
        );
    }
    Ok(parse_todo_line_with_repo(Some(repo), line)?
        .map(|(oid, cmd)| RebaseReplayStep::PickLike { oid, cmd }))
}

/// One line from an interactive rebase todo (after the user edits it).
#[derive(Debug)]
enum ParsedRebaseTodoLine {
    /// `pick` / `p` / `fixup` / `f` / `squash` / `s` with an object id.
    Commit { oid: ObjectId, cmd: RebaseTodoCmd },
    /// `exec <shell command>` (rest of line after the command word).
    Exec(String),
    /// `edit` / `e` with an object id.
    Edit(ObjectId),
    /// `merge -C <ref> ...` — replay merge commit message from `merge_oid`, merge heads from the rest.
    MergeReuseMessage {
        merge_oid: ObjectId,
        merge_args: String,
    },
}

fn parse_interactive_rebase_todo_line(
    repo: &Repository,
    line: &str,
) -> Result<Option<ParsedRebaseTodoLine>> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let mut parts = t.split_whitespace();
    let Some(cmd_word) = parts.next() else {
        return Ok(None);
    };
    let cmd_lower = cmd_word.to_ascii_lowercase();
    if cmd_lower == "exec" || cmd_lower == "x" {
        let rest = t[cmd_word.len()..].trim_start();
        return Ok(Some(ParsedRebaseTodoLine::Exec(rest.to_owned())));
    }
    if cmd_lower == "edit" || cmd_lower == "e" {
        let Some(hex) = parts.next() else {
            bail!("malformed rebase todo line: {line}");
        };
        let oid = if hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            ObjectId::from_hex(hex)?
        } else {
            resolve_revision(repo, hex).with_context(|| format!("todo: bad revision '{hex}'"))?
        };
        return Ok(Some(ParsedRebaseTodoLine::Edit(oid)));
    }
    if cmd_lower == "merge" || cmd_lower == "m" {
        let mut rest = parts;
        let Some(flag) = rest.next() else {
            bail!("malformed merge todo line: {line}");
        };
        if !flag.eq_ignore_ascii_case("-C") && !flag.eq_ignore_ascii_case("-c") {
            bail!("unsupported merge todo form (only -C / -c): {line}");
        }
        let Some(merge_ref) = rest.next() else {
            bail!("merge -C missing commit: {line}");
        };
        let merge_oid = resolve_revision(repo, merge_ref)
            .with_context(|| format!("todo merge: bad revision '{merge_ref}'"))?;
        let tail: Vec<&str> = rest.collect();
        let merge_args = tail.join(" ");
        return Ok(Some(ParsedRebaseTodoLine::MergeReuseMessage {
            merge_oid,
            merge_args,
        }));
    }
    if let Some(cmd) = RebaseTodoCmd::parse_word(&cmd_lower) {
        let Some(hex) = parts.next() else {
            bail!("malformed rebase todo line: {line}");
        };
        let oid = if hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            ObjectId::from_hex(hex)?
        } else {
            resolve_revision(repo, hex).with_context(|| format!("todo: bad revision '{hex}'"))?
        };
        return Ok(Some(ParsedRebaseTodoLine::Commit { oid, cmd }));
    }
    bail!("unknown rebase todo command: {line}");
}

/// For post-rewrite pending flush: next line's command category (Git `peek_command` + `is_fixup`).
fn first_interactive_todo_pick_oid(repo: &Repository, todo_lines: &[&str]) -> Option<ObjectId> {
    for line in todo_lines {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Ok(Some(RebaseReplayStep::PickLike {
            oid,
            cmd: RebaseTodoCmd::Pick,
        })) = parse_rebase_replay_step(repo, t, true)
        {
            return Some(oid);
        }
    }
    None
}

fn peek_next_rebase_flush_hint(
    repo: &Repository,
    todo_lines: &[&str],
    start: usize,
    interactive: bool,
) -> Option<RebaseTodoCmd> {
    let mut j = start;
    while j < todo_lines.len() {
        let t = todo_lines[j].trim();
        if t.is_empty() || t.starts_with('#') {
            j += 1;
            continue;
        }
        if let Ok(Some(step)) = parse_rebase_replay_step(repo, t, interactive) {
            return Some(match step {
                RebaseReplayStep::PickLike { cmd, .. } => cmd,
                RebaseReplayStep::Exec(_)
                | RebaseReplayStep::Edit(_)
                | RebaseReplayStep::MergeReuseMessage { .. } => RebaseTodoCmd::Pick,
            });
        }
        j += 1;
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RebaseMergeReuseOutcome {
    Completed,
    Conflict,
    /// Merge aborted before `MERGE_HEAD` (e.g. untracked file would be overwritten); retry on
    /// `rebase --continue` after the user fixes the worktree.
    Blocked,
}

fn run_rebase_merge_subprocess(
    repo: &Repository,
    git_dir: &Path,
    merge_oid: &ObjectId,
    merge_args: &str,
) -> Result<std::process::ExitStatus> {
    let merge_obj = repo.odb.read(merge_oid)?;
    let merge_commit = parse_commit(&merge_obj.data)?;
    let msg_path = git_dir.join("rebase-merge-merge-msg");
    fs::write(&msg_path, &merge_commit.message)?;
    let self_exe = std::env::current_exe().context("cannot determine grit binary path")?;
    let output = std::process::Command::new(&self_exe)
        .args(["merge", "--no-ff", "-F"])
        .arg(msg_path.as_os_str())
        .args(merge_args.split_whitespace())
        .current_dir(repo.work_tree.as_deref().unwrap_or_else(|| Path::new(".")))
        .output()
        .context("run grit merge for rebase merge -C")?;
    let _ = fs::remove_file(&msg_path);
    let status = output.status;
    if !status.success() {
        let err_out = String::from_utf8_lossy(&output.stderr);
        let std_out = String::from_utf8_lossy(&output.stdout);
        if !err_out.trim().is_empty() {
            eprint!("{err_out}");
        }
        if !std_out.trim().is_empty() {
            eprint!("{std_out}");
        }
    }
    Ok(status)
}

fn rebase_merge_reuse_message(
    repo: &Repository,
    git_dir: &Path,
    rb_dir: &Path,
    merge_oid: &ObjectId,
    merge_args: &str,
    next_after_line: Option<RebaseTodoCmd>,
) -> Result<RebaseMergeReuseOutcome> {
    fs::write(
        rb_dir.join("rebase-merge-source"),
        format!("{}\n", merge_oid.to_hex()),
    )?;
    fs::write(rb_dir.join("rebase-merge-args"), format!("{merge_args}\n"))?;
    let status = run_rebase_merge_subprocess(repo, git_dir, merge_oid, merge_args)?;
    if !status.success() {
        if git_dir.join("MERGE_HEAD").exists() {
            return Ok(RebaseMergeReuseOutcome::Conflict);
        }
        return Ok(RebaseMergeReuseOutcome::Blocked);
    }
    let _ = fs::remove_file(rb_dir.join("rebase-merge-source"));
    let _ = fs::remove_file(rb_dir.join("rebase-merge-args"));
    record_rebase_in_rewritten_pending(git_dir, rb_dir, merge_oid, next_after_line)?;
    Ok(RebaseMergeReuseOutcome::Completed)
}

fn rebase_state_todo_lines(
    repo: &Repository,
    config: &ConfigSet,
    entries: &[(ObjectId, RebaseTodoCmd)],
) -> Result<Vec<String>> {
    entries
        .iter()
        .map(|(oid, cmd)| format_rebase_todo_line(repo, oid, *cmd, config, false))
        .collect()
}

fn next_non_fixup_index(
    repo: &Repository,
    todo_lines: &[&str],
    from: usize,
    interactive: bool,
) -> Option<usize> {
    let mut j = from + 1;
    while j < todo_lines.len() {
        if let Ok(Some(step)) = parse_rebase_replay_step(repo, todo_lines[j], interactive) {
            match step {
                RebaseReplayStep::PickLike { cmd, .. } => {
                    if matches!(cmd, RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) {
                        j += 1;
                        continue;
                    }
                    return Some(j);
                }
                RebaseReplayStep::Exec(_)
                | RebaseReplayStep::Edit(_)
                | RebaseReplayStep::MergeReuseMessage { .. } => return Some(j),
            }
        }
        j += 1;
    }
    None
}

fn is_final_fixup_in_todo(
    repo: &Repository,
    todo_lines: &[&str],
    idx: usize,
    interactive: bool,
) -> bool {
    let Ok(Some(step)) = parse_rebase_replay_step(repo, todo_lines[idx], interactive) else {
        return false;
    };
    let RebaseReplayStep::PickLike { cmd, .. } = step else {
        return false;
    };
    if !matches!(cmd, RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) {
        return false;
    }
    next_non_fixup_index(repo, todo_lines, idx, interactive).is_none()
}

#[derive(Clone, Copy, Default)]
struct SquashChainCtx {
    count: usize,
    seen_squash: bool,
}

fn squash_ctx_path(rb_dir: &Path) -> PathBuf {
    rb_dir.join("squash-chain-ctx")
}

fn read_squash_ctx(rb_dir: &Path) -> SquashChainCtx {
    let Ok(s) = fs::read_to_string(squash_ctx_path(rb_dir)) else {
        return SquashChainCtx::default();
    };
    let mut count = 0usize;
    let mut seen = false;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("count=") {
            count = v.parse().unwrap_or(0);
        }
        if line.trim() == "seen_squash=1" {
            seen = true;
        }
    }
    SquashChainCtx {
        count,
        seen_squash: seen,
    }
}

fn write_squash_ctx(rb_dir: &Path, ctx: SquashChainCtx) -> Result<()> {
    fs::write(
        squash_ctx_path(rb_dir),
        format!(
            "count={}\nseen_squash={}\n",
            ctx.count,
            if ctx.seen_squash { 1 } else { 0 }
        ),
    )?;
    Ok(())
}

fn clear_squash_ctx(rb_dir: &Path) {
    let _ = fs::remove_file(squash_ctx_path(rb_dir));
    let _ = fs::remove_file(rb_dir.join("message-squash"));
    let _ = fs::remove_file(rb_dir.join("message-fixup"));
}

fn message_body_after_subject(message: &str) -> &str {
    match message.find('\n') {
        Some(i) => &message[i + 1..],
        None => "",
    }
}

fn first_line_len(body: &str) -> usize {
    match body.find('\n') {
        Some(i) => i,
        None => body.len(),
    }
}

fn squash_comment_subject_prefix(body: &str, cmd: RebaseTodoCmd, seen_squash: bool) -> usize {
    let t = body.trim_start();
    if t.starts_with("amend! ") {
        return first_line_len(body);
    }
    if (cmd == RebaseTodoCmd::Squash || seen_squash)
        && (t.starts_with("squash! ") || t.starts_with("fixup! "))
    {
        return first_line_len(body);
    }
    0
}

fn append_commented(buf: &mut String, text: &str) {
    for line in text.lines() {
        buf.push_str("# ");
        buf.push_str(line);
        buf.push('\n');
    }
}

fn append_nth_squash_message(
    buf: &mut String,
    body: &str,
    cmd: RebaseTodoCmd,
    seen_squash: bool,
    n: usize,
) {
    buf.push_str("\n# This is the commit message #");
    buf.push_str(&n.to_string());
    buf.push_str(":\n\n");
    let pre = squash_comment_subject_prefix(body, cmd, seen_squash).min(body.len());
    if pre > 0 {
        append_commented(buf, &body[..pre]);
    }
    buf.push_str(&body[pre..]);
}

fn run_prepare_commit_msg_hook(repo: &Repository, path: &Path, source: &str) -> Result<()> {
    let p = path.to_string_lossy();
    if let HookResult::Failed(code) =
        run_hook(repo, "prepare-commit-msg", &[p.as_ref(), source], None)
    {
        bail!("prepare-commit-msg hook exited with status {code}");
    }
    Ok(())
}

/// Writes `text` to `COMMIT_EDITMSG`, runs `prepare-commit-msg` with `source`, returns the file
/// contents afterward (matches Git's sequencer `try_to_commit` hook path).
fn commit_message_after_prepare_hook(
    repo: &Repository,
    git_dir: &Path,
    text: &str,
    source: &str,
) -> Result<String> {
    let editmsg = git_dir.join("COMMIT_EDITMSG");
    fs::write(&editmsg, text)?;
    run_prepare_commit_msg_hook(repo, &editmsg, source)?;
    fs::read_to_string(&editmsg).context("read COMMIT_EDITMSG after prepare-commit-msg hook")
}

fn strip_comment_lines_template(msg: &str) -> String {
    let mut out = String::new();
    for line in msg.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn default_commit_msg_cleanup(config: &ConfigSet) -> &'static str {
    match config
        .get("commit.cleanup")
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("strip") => "strip",
        Some("verbatim") => "verbatim",
        Some("whitespace") => "whitespace",
        Some("scissors") => "scissors",
        _ => "default",
    }
}

/// Message cleanup for rebase replay after `prepare-commit-msg`, matching Git's sequencer
/// `try_to_commit`: when `commit.cleanup` is unset, `default_msg_cleanup` is `COMMIT_MSG_CLEANUP_NONE`
/// and `strbuf_stripspace` does not strip `#` lines (unlike `git commit`'s default).
fn rebase_commit_msg_cleanup(config: &ConfigSet) -> &'static str {
    match config
        .get("commit.cleanup")
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("strip") => "strip",
        Some("verbatim") => "verbatim",
        Some("whitespace") => "whitespace",
        Some("scissors") => "scissors",
        _ => "verbatim",
    }
}

fn apply_commit_msg_cleanup(msg: &str, mode: &str) -> String {
    match mode {
        "verbatim" => msg.to_string(),
        "whitespace" => {
            let s = msg.replace("\r\n", "\n");
            let lines: Vec<&str> = s.lines().collect();
            let mut start = 0usize;
            while start < lines.len() && lines[start].trim().is_empty() {
                start += 1;
            }
            let mut end = lines.len();
            while end > start && lines[end - 1].trim().is_empty() {
                end -= 1;
            }
            lines[start..end].join("\n") + "\n"
        }
        "strip" | "default" | "scissors" => {
            let mut s = msg.replace("\r\n", "\n");
            let cut = if mode == "scissors" {
                s.find("\n------------------------ >8 ------------------------\n")
            } else {
                None
            };
            if let Some(i) = cut {
                s.truncate(i);
            }
            let lines: Vec<&str> = s.lines().collect();
            let mut out: Vec<&str> = Vec::new();
            for line in lines {
                let t = line.trim_start();
                if t.starts_with('#') {
                    continue;
                }
                out.push(line);
            }
            let mut start = 0usize;
            while start < out.len() && out[start].trim().is_empty() {
                start += 1;
            }
            let mut end = out.len();
            while end > start && out[end - 1].trim().is_empty() {
                end -= 1;
            }
            out[start..end].join("\n") + "\n"
        }
        _ => strip_comment_lines_template(msg),
    }
}

fn update_squash_message_file(
    repo: &Repository,
    rb_dir: &Path,
    git_dir: &Path,
    cmd: RebaseTodoCmd,
    picked: &CommitData,
    ctx: &mut SquashChainCtx,
) -> Result<()> {
    let squash_path = rb_dir.join("message-squash");
    let fixup_path = rb_dir.join("message-fixup");
    let body = message_body_after_subject(&picked.message);

    if ctx.count == 0 {
        let head_oid = resolve_head(git_dir)?
            .oid()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("HEAD is unborn during squash"))?;
        let hobj = repo.odb.read(&head_oid)?;
        let head_commit = parse_commit(&hobj.data)?;
        let hsubj = head_commit.message.lines().next().unwrap_or("");
        let hbody = message_body_after_subject(&head_commit.message);
        let mut buf = String::new();
        buf.push_str("# This is a combination of 2 commits.\n");
        buf.push_str("# The first commit's message is:\n\n");
        if cmd == RebaseTodoCmd::Fixup {
            fs::write(&fixup_path, format!("{hsubj}\n{hbody}"))?;
            append_commented(&mut buf, hsubj);
            if !hbody.is_empty() {
                append_commented(&mut buf, hbody.trim_end_matches('\n'));
            }
        } else {
            buf.push_str(hsubj);
            buf.push('\n');
            buf.push_str(hbody);
            if !hbody.is_empty() && !hbody.ends_with('\n') {
                buf.push('\n');
            }
        }
        append_nth_squash_message(&mut buf, body, cmd, ctx.seen_squash, 2);
        fs::write(&squash_path, buf)?;
    } else {
        let mut buf = fs::read_to_string(&squash_path)?;
        let n = ctx.count + 2;
        if let Some(pos) = buf.find('\n') {
            if buf.starts_with("# This is a combination of") {
                buf.replace_range(
                    ..pos + 1,
                    &format!("# This is a combination of {n} commits.\n"),
                );
            }
        }
        append_nth_squash_message(&mut buf, body, cmd, ctx.seen_squash, ctx.count + 2);
        fs::write(&squash_path, buf)?;
    }

    if cmd == RebaseTodoCmd::Squash {
        ctx.seen_squash = true;
    }
    ctx.count += 1;
    write_squash_ctx(rb_dir, *ctx)?;
    Ok(())
}

fn commit_from_merged_index(
    repo: &Repository,
    _git_dir: &Path,
    merged_index: &Index,
    config: &ConfigSet,
    parents: Vec<ObjectId>,
    author: &str,
    message: String,
) -> Result<ObjectId> {
    let tree_oid = write_tree_from_index(&repo.odb, merged_index, "")?;
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(config, "COMMITTER")?;
    let (message, encoding, raw_message) = finalize_message_for_commit_encoding(message, config);
    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author: author.to_string(),
        committer: format_ident(&committer, now),
        author_raw: Vec::new(),
        committer_raw: Vec::new(),
        encoding,
        message,
        raw_message,
    };
    let bytes = serialize_commit(&commit_data);
    Ok(repo.odb.write(ObjectKind::Commit, &bytes)?)
}

fn rebase_reflog_action() -> String {
    std::env::var("GIT_REFLOG_ACTION").unwrap_or_else(|_| "rebase".to_owned())
}

fn run_post_checkout_hook(repo: &Repository, old_oid: &ObjectId, new_oid: &ObjectId) -> Result<()> {
    let old_hex = old_oid.to_hex();
    let new_hex = new_oid.to_hex();
    let args = [old_hex.as_str(), new_hex.as_str(), "1"];
    if let HookResult::Failed(code) = run_hook(repo, "post-checkout", &args, None) {
        bail!("post-checkout hook exited with status {code}");
    }
    Ok(())
}

fn print_branch_up_to_date(head: &HeadState) {
    if let Some(name) = head.branch_name() {
        println!("Current branch {name} is up to date.");
    } else {
        println!("HEAD is up to date.");
    }
}

fn reflog_identity(repo: &Repository) -> String {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let (name, email) = crate::ident::resolve_loose_committer_parts(&config);
    let now = time::OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}

fn rebase_apply_dir(git_dir: &Path) -> std::path::PathBuf {
    git_dir.join("rebase-apply")
}

fn rebase_merge_dir(git_dir: &Path) -> std::path::PathBuf {
    git_dir.join("rebase-merge")
}

fn rebase_dir(git_dir: &Path) -> std::path::PathBuf {
    if rebase_merge_dir(git_dir).exists() {
        rebase_merge_dir(git_dir)
    } else {
        rebase_apply_dir(git_dir)
    }
}

/// Directory holding in-progress rebase state (`.git/rebase-apply` or `.git/rebase-merge`).
fn active_rebase_dir(git_dir: &Path) -> Option<PathBuf> {
    let merge = rebase_merge_dir(git_dir);
    if merge.exists() {
        return Some(merge);
    }
    let apply = rebase_apply_dir(git_dir);
    if apply.exists() {
        return Some(apply);
    }
    None
}

fn rebase_state_dir_for_backend(git_dir: &Path, backend: RebaseBackend) -> std::path::PathBuf {
    match backend {
        RebaseBackend::Apply => rebase_apply_dir(git_dir),
        RebaseBackend::Merge => rebase_merge_dir(git_dir),
    }
}

fn is_rebase_in_progress(git_dir: &Path) -> bool {
    rebase_apply_dir(git_dir).exists() || rebase_merge_dir(git_dir).exists()
}

fn choose_rebase_backend(args: &Args) -> RebaseBackend {
    if apply_backend_forced(args) {
        RebaseBackend::Apply
    } else {
        // `git rebase --merge` and `git rebase --interactive` both use `.git/rebase-merge/`.
        RebaseBackend::Merge
    }
}

fn load_ws_fix_rule_from_rebase_state(git_dir: &Path) -> Option<u32> {
    let rb_dir = rebase_dir(git_dir);
    let action = fs::read_to_string(rb_dir.join("whitespace-action")).ok()?;
    let a = action.trim();
    if a.eq_ignore_ascii_case("fix") || a.eq_ignore_ascii_case("strip") {
        let config = ConfigSet::load(Some(git_dir), true).unwrap_or_else(|_| ConfigSet::new());
        Some(
            config
                .get("core.whitespace")
                .map(|s| parse_whitespace_rule(&s))
                .unwrap_or(WS_DEFAULT_RULE),
        )
    } else {
        None
    }
}

fn load_rebase_backend(rb_dir: &Path) -> RebaseBackend {
    let marker = fs::read_to_string(rb_dir.join("backend")).unwrap_or_default();
    if marker.trim().eq_ignore_ascii_case("apply") {
        RebaseBackend::Apply
    } else {
        RebaseBackend::Merge
    }
}

fn load_rebase_reflog_action(rb_dir: &Path) -> String {
    fs::read_to_string(rb_dir.join("reflog-action"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(rebase_reflog_action)
}

fn load_onto_name(rb_dir: &Path) -> Option<String> {
    fs::read_to_string(rb_dir.join("onto-name"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Append one `old_oid new_oid` line to `.git/<rebase-dir>/rewritten` for the post-rewrite hook.
fn append_rebase_rewrite_line(rb_dir: &Path, old_oid: &ObjectId, new_oid: &ObjectId) -> Result<()> {
    let path = rb_dir.join("rewritten");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(f, "{} {}", old_oid.to_hex(), new_oid.to_hex())?;
    Ok(())
}

fn rebase_rewritten_pending_path(rb_dir: &Path) -> PathBuf {
    rb_dir.join("rewritten-pending")
}

fn rebase_rewritten_list_path(rb_dir: &Path) -> PathBuf {
    rb_dir.join("rewritten")
}

/// Append `old_oid` to `rewritten-pending`, then flush pending lines to `rewritten` when
/// `next_command` is not `fixup`/`squash` (matches Git's `record_in_rewritten`).
///
/// After a successful pick, pass the next todo command (`peek_command(..., 1)`). When recording
/// from `stopped-sha` during `rebase --continue` after `--skip`, pass the current (skipped) line's
/// command (`peek_command(..., 0)`).
fn record_rebase_in_rewritten_pending(
    git_dir: &Path,
    rb_dir: &Path,
    old_oid: &ObjectId,
    next_command: Option<RebaseTodoCmd>,
) -> Result<()> {
    let pending = rebase_rewritten_pending_path(rb_dir);
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&pending)
        .with_context(|| format!("open {}", pending.display()))?;
    writeln!(f, "{}", old_oid.to_hex())?;

    let flush = match next_command {
        None => true,
        Some(RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) => false,
        Some(RebaseTodoCmd::Pick | RebaseTodoCmd::Reword) => true,
    };
    if flush {
        flush_rebase_rewritten_pending(git_dir, rb_dir)?;
    }
    Ok(())
}

fn flush_rebase_rewritten_pending(git_dir: &Path, rb_dir: &Path) -> Result<()> {
    let pending_path = rebase_rewritten_pending_path(rb_dir);
    let Ok(s) = fs::read_to_string(&pending_path) else {
        return Ok(());
    };
    if s.trim().is_empty() {
        let _ = fs::remove_file(&pending_path);
        return Ok(());
    }
    let new_oid = resolve_head(git_dir)?
        .oid()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
    let list_path = rebase_rewritten_list_path(rb_dir);
    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&list_path)
        .with_context(|| format!("open {}", list_path.display()))?;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        writeln!(out, "{} {}", line, new_oid.to_hex())?;
    }
    let _ = fs::remove_file(&pending_path);
    Ok(())
}

/// If `rewritten` has content, run `post-rewrite rebase` with that file on stdin (matches `git am`).
fn run_post_rewrite_after_rebase(repo: &Repository, rb_dir: &Path) {
    let path = rebase_rewritten_list_path(rb_dir);
    let Ok(meta) = fs::metadata(&path) else {
        return;
    };
    if meta.len() == 0 {
        return;
    }
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    let _ = run_hook(repo, "post-rewrite", &["rebase"], Some(&bytes));
}

/// Message to record when replaying `commit` during a root rebase.
///
/// For two-parent merges, Git records the second parent's subject (the merged branch tip), not the
/// default merge message, when flattening history onto a new base.
fn message_for_root_replayed_commit(
    repo: &Repository,
    commit: &CommitData,
    root_rebase: bool,
) -> String {
    if root_rebase && commit.parents.len() == 2 {
        if let Ok(p2_obj) = repo.odb.read(&commit.parents[1]) {
            if let Ok(p2) = parse_commit(&p2_obj.data) {
                return p2.message;
            }
        }
    }
    commit.message.clone()
}

fn read_autostash_oid(rb_dir: &Path) -> Result<Option<ObjectId>> {
    let p = rb_dir.join("autostash");
    if !p.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&p).unwrap_or_default();
    let hex = s.trim();
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    Ok(Some(ObjectId::from_hex(hex)?))
}

fn reset_index_to_head(repo: &Repository, git_dir: &Path) -> Result<()> {
    let head_oid = resolve_head(git_dir)?
        .oid()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("cannot reset index: HEAD is unborn"))?;
    let obj = repo.odb.read(&head_oid)?;
    let commit = parse_commit(&obj.data)?;
    let entries = tree_to_index_entries(repo, &commit.tree, "")?;
    let mut index = Index::new();
    index.entries = entries;
    index.sort();
    repo.write_index(&mut index)?;
    Ok(())
}

fn is_terminal_dumb() -> bool {
    match std::env::var("TERM") {
        Ok(t) => t == "dumb",
        Err(_) => true,
    }
}

/// Matches Git's `git_editor()`: `GIT_EDITOR`, `core.editor`, `VISUAL` (non-dumb only), `EDITOR`,
/// then `vi`.
fn git_editor_cmd(config: &ConfigSet) -> Result<String> {
    if let Ok(e) = std::env::var("GIT_EDITOR") {
        let s = e.trim();
        if !s.is_empty() {
            return Ok(e);
        }
    }
    if let Some(e) = config.get("core.editor") {
        let s = e.trim();
        if !s.is_empty() {
            return Ok(e);
        }
    }
    if !is_terminal_dumb() {
        if let Ok(v) = std::env::var("VISUAL") {
            let s = v.trim();
            if !s.is_empty() {
                return Ok(v);
            }
        }
    }
    if let Ok(e) = std::env::var("EDITOR") {
        let s = e.trim();
        if !s.is_empty() {
            return Ok(e);
        }
    }
    if is_terminal_dumb() {
        bail!("Terminal is dumb, but EDITOR unset");
    }
    Ok("vi".to_string())
}

/// Resolves the program to run for `rebase -i` / autosquash todo editing (`git_sequence_editor`).
///
/// `GIT_SEQUENCE_EDITOR` and `sequence.editor` take precedence; otherwise falls back to
/// [`git_editor_cmd`].
fn sequence_editor_cmd(config: &ConfigSet) -> Result<String> {
    if let Ok(seq) = std::env::var("GIT_SEQUENCE_EDITOR") {
        let s = seq.trim();
        if !s.is_empty() {
            return Ok(seq);
        }
    }
    if let Some(seq) = config.get("sequence.editor") {
        let s = seq.trim();
        if !s.is_empty() {
            return Ok(seq);
        }
    }
    git_editor_cmd(config)
}

fn run_shell_editor(editor: &str, path: &Path) -> Result<std::process::ExitStatus> {
    let status = if editor.trim() == ":" {
        std::process::Command::new("true").status()
    } else {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{} \"$@\"", editor))
            .arg(editor)
            .arg(path)
            .status()
    }
    .context("failed to run editor")?;
    Ok(status)
}

/// Opens `GIT_EDITOR` on `COMMIT_EDITMSG` for an interactive reword during `rebase -i`.
///
/// Matches Git's sequencer path: seed the file, run `prepare-commit-msg` with source `reword`,
/// then invoke the commit editor. Returns the edited message text (before final cleanup).
fn run_commit_editor_for_reword(
    repo: &Repository,
    git_dir: &Path,
    template: &str,
) -> Result<String> {
    let editmsg = git_dir.join("COMMIT_EDITMSG");
    fs::write(&editmsg, template)?;
    run_prepare_commit_msg_hook(repo, &editmsg, "reword")?;
    let config = ConfigSet::load(Some(git_dir), true)?;
    let editor = git_editor_cmd(&config)?;
    let status = run_shell_editor(&editor, &editmsg)?;
    if !status.success() {
        bail!("there was a problem with the editor");
    }
    fs::read_to_string(&editmsg).context("read COMMIT_EDITMSG after editor")
}

fn worktree_matches_head(repo: &Repository, git_dir: &Path) -> Result<bool> {
    let Some(wt) = repo.work_tree.as_deref() else {
        return Ok(true);
    };
    let idx = repo.load_index().context("failed to read index")?;
    let head_tree = resolve_head(git_dir)?.oid().and_then(|oid| {
        let obj = repo.odb.read(oid).ok()?;
        parse_commit(&obj.data).ok().map(|c| c.tree)
    });
    let staged = grit_lib::diff::diff_index_to_tree(&repo.odb, &idx, head_tree.as_ref())?;
    let unstaged = grit_lib::diff::diff_index_to_worktree(&repo.odb, &idx, wt)?;
    Ok(staged.is_empty() && unstaged.is_empty())
}

/// Returns trimmed non-comment todo lines as edited (for replay), and pick/fixup/squash entries for
/// empty-list / up-to-date checks.
fn run_interactive_rebase(
    repo: &Repository,
    git_dir: &Path,
    commits: &[ObjectId],
    config: &ConfigSet,
    autostash_oid: Option<&ObjectId>,
    autosquash: bool,
) -> Result<(Vec<String>, Vec<(ObjectId, RebaseTodoCmd)>)> {
    if autosquash {
        validate_rebase_instruction_format(config)?;
    }
    let entries = if autosquash {
        rearrange_autosquash(repo, commits.to_vec())?
    } else {
        commits
            .iter()
            .cloned()
            .map(|o| (o, RebaseTodoCmd::Pick))
            .collect()
    };
    let mut todo = String::new();
    for (oid, cmd) in &entries {
        todo.push_str(&format_rebase_todo_line(repo, oid, *cmd, config, true)?);
        todo.push('\n');
    }
    let rb_merge = rebase_merge_dir(git_dir);
    let _ = fs::remove_dir_all(&rb_merge);
    fs::create_dir_all(&rb_merge)?;
    fs::write(rb_merge.join("interactive"), "")?;
    let todo_path = rb_merge.join("git-rebase-todo");
    fs::write(&todo_path, todo.as_bytes())?;
    let editor = sequence_editor_cmd(config)?;
    let status = run_shell_editor(&editor, &todo_path)?;
    let edited = fs::read_to_string(&todo_path)?;
    let _ = fs::remove_dir_all(&rb_merge);
    if !status.success() {
        if worktree_matches_head(repo, git_dir)? {
            if let Some(oid) = autostash_oid {
                let _ = stash::pop_autostash_if_top(repo, oid);
            }
        }
        bail!("there was a problem with the editor");
    }
    let mut lines: Vec<String> = Vec::new();
    let mut pick_like: Vec<(ObjectId, RebaseTodoCmd)> = Vec::new();
    for line in edited.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        lines.push(t.to_owned());
        if let Some(pair) = parse_todo_line_with_repo(Some(repo), t)? {
            pick_like.push(pair);
        }
    }
    Ok((lines, pick_like))
}

fn apply_pending_autostash(repo: &Repository, rb_dir: &Path) -> Result<()> {
    let Some(oid) = read_autostash_oid(rb_dir)? else {
        return Ok(());
    };
    reset_index_to_head(repo, &repo.git_dir)?;
    let had_conflict = stash::apply_autostash_for_rebase(repo, &oid)?;
    if had_conflict {
        eprintln!("Applying autostash resulted in conflicts.");
        eprintln!("Your changes are safe in the stash.");
        eprintln!("You can run \"git stash pop\" or \"git stash drop\" at any time.");
    } else {
        eprintln!("Applied autostash.");
        let _ = stash::drop_stash_tip_if_matches(repo, &oid);
    }
    let _ = fs::remove_file(rb_dir.join("autostash"));
    Ok(())
}

fn apply_autostash_after_ff(repo: &Repository, autostash_oid: &ObjectId) -> Result<()> {
    reset_index_to_head(repo, &repo.git_dir)?;
    let had_conflict = stash::apply_autostash_for_rebase(repo, autostash_oid)?;
    if had_conflict {
        eprintln!("Applying autostash resulted in conflicts.");
        eprintln!("Your changes are safe in the stash.");
        eprintln!("You can run \"git stash pop\" or \"git stash drop\" at any time.");
    } else {
        eprintln!("Applied autostash.");
        let _ = stash::drop_stash_tip_if_matches(repo, autostash_oid);
    }
    Ok(())
}

// ── Main rebase flow ────────────────────────────────────────────────

fn do_rebase(args: Args, pre_rebase_hook_second: Option<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if is_rebase_in_progress(git_dir) {
        bail!(
            "error: a rebase is already in progress\n\
             hint: use \"grit rebase --continue\" to continue\n\
             hint: or \"grit rebase --abort\" to abort"
        );
    }

    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let config_autostash = config
        .get_bool("rebase.autostash")
        .and_then(|r| r.ok())
        .unwrap_or(false);
    let want_autostash = (args.autostash || config_autostash) && !args.no_autostash;
    let config_autosquash = config
        .get_bool("rebase.autosquash")
        .and_then(|r| r.ok())
        .unwrap_or(false);
    let want_autosquash =
        (args.autosquash || (config_autosquash && args.interactive)) && !args.no_autosquash;

    validate_apply_merge_backend_combo(&args, &config, want_autosquash)?;

    let mut autostash_oid: Option<ObjectId> = None;
    let mut had_rebase_autostash = false;

    // Check for dirty worktree/index (optional autostash)
    {
        let work_tree = repo
            .work_tree
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
        let idx = repo.load_index().context("failed to read index")?;
        let head_tree = resolve_head(git_dir)?.oid().and_then(|oid| {
            let obj = repo.odb.read(oid).ok()?;
            parse_commit(&obj.data).ok().map(|c| c.tree)
        });
        let staged = grit_lib::diff::diff_index_to_tree(&repo.odb, &idx, head_tree.as_ref())?;
        let unstaged = grit_lib::diff::diff_index_to_worktree(&repo.odb, &idx, work_tree)?;
        let dirty = !staged.is_empty() || !unstaged.is_empty();
        if dirty {
            if !want_autostash {
                if !staged.is_empty() {
                    bail!(
                        "cannot rebase: your index contains uncommitted changes.\n\
                   Please commit or stash them."
                    );
                }
                bail!(
                    "error: cannot rebase: You have unstaged changes.\n\
                   Please commit or stash them."
                );
            }
            autostash_oid = stash::autostash_for_rebase(&repo)?;
            had_rebase_autostash = autostash_oid.is_some();
            if autostash_oid.is_none() {
                if !staged.is_empty() {
                    bail!(
                        "cannot rebase: your index contains uncommitted changes.\n\
                   Please commit or stash them."
                    );
                }
                bail!(
                    "error: cannot rebase: You have unstaged changes.\n\
                   Please commit or stash them."
                );
            }
        }
    }

    // Resolve upstream / onto / HEAD
    let head_state = resolve_head(git_dir)?;
    let head_oid_early = head_state
        .oid()
        .ok_or_else(|| anyhow::anyhow!("cannot rebase: HEAD is unborn"))?
        .to_owned();

    let (upstream_spec, upstream_oid, onto_oid, onto_name_for_state) = if args.root {
        let onto_spec = args
            .onto
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal: --root without --onto"))?;
        let onto = resolve_revision(&repo, onto_spec)
            .with_context(|| format!("bad revision '{onto_spec}'"))?;
        ("--root".to_owned(), onto, onto, onto_spec.to_owned())
    } else {
        let upstream_spec = args.upstream.as_deref().unwrap_or("HEAD").to_owned();
        let up_oid = resolve_revision(&repo, &upstream_spec)
            .with_context(|| format!("bad revision '{upstream_spec}'"))?;
        let (onto, onto_label) = if args.keep_base {
            let branch_name = head_state
                .branch_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "HEAD".to_string());
            let onto_spec = format!("{upstream_spec}...{branch_name}");
            let onto =
                merge_base_from_triple_dot_onto(&repo, &onto_spec, true, upstream_spec.as_str())?;
            (onto, onto_spec)
        } else if let Some(ref onto_spec) = args.onto {
            if split_triple_dot_range(onto_spec).is_some() {
                let onto =
                    merge_base_from_triple_dot_onto(&repo, onto_spec, false, onto_spec.as_str())?;
                (onto, onto_spec.clone())
            } else {
                let oid = resolve_revision(&repo, onto_spec)
                    .with_context(|| format!("bad revision '{onto_spec}'"))?;
                (oid, onto_spec.clone())
            }
        } else {
            (up_oid, upstream_spec.clone())
        };
        (upstream_spec, up_oid, onto, onto_label)
    };

    let head = head_state;
    let head_oid = head_oid_early;

    let want_stat =
        args.stat || (config.get("rebase.stat").as_deref() == Some("true") && !args.no_stat);

    let branch_base_merge = merge_bases_first_vs_rest(&repo, onto_oid, &[head_oid])?;
    let branch_base = if branch_base_merge.len() == 1 {
        Some(branch_base_merge[0])
    } else {
        None
    };

    let whitespace_forces_replay = args
        .whitespace
        .as_deref()
        .is_some_and(|w| w.eq_ignore_ascii_case("fix") || w.eq_ignore_ascii_case("strip"));
    let allow_preemptive_ff =
        !args.interactive && args.exec.is_none() && !whitespace_forces_replay && !args.autosquash;

    if allow_preemptive_ff && rebase_can_preemptive_ff(&repo, onto_oid, upstream_oid, head_oid)? {
        if !args.no_ff {
            print_branch_up_to_date(&head);
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
        if let Some(name) = head.branch_name() {
            println!("Current branch {name} is up to date, rebase forced.");
        } else {
            println!("HEAD is up to date, rebase forced.");
        }
    }

    if want_stat {
        if args.verbose {
            match branch_base {
                Some(bb) => println!(
                    "Changes from {} to {}:",
                    &bb.to_hex()[..7],
                    &onto_oid.to_hex()[..7]
                ),
                None => println!("Changes to {}:", onto_oid.to_hex()),
            }
        }
        print_rebase_diffstat(&repo, branch_base, onto_oid)?;
    }

    let reapply_cherry_picks = if args.reapply_cherry_picks {
        true
    } else if args.no_reapply_cherry_picks {
        false
    } else {
        args.keep_base
    };
    let upstream_for_replay = if args.keep_base && reapply_cherry_picks {
        onto_oid
    } else {
        upstream_oid
    };
    // Interactive rebase normally skips this filter (todo lists all commits); `--keep-base` with
    // `--no-reapply-cherry-picks` must still omit patch-id duplicates like the merge backend.
    let filter_cherry_equivalents = !reapply_cherry_picks && (!args.interactive || args.keep_base);
    let mut commits = if args.root {
        collect_commits_for_root_rebase(&repo, head_oid, onto_oid)?
    } else {
        collect_rebase_todo_commits(
            &repo,
            head_oid,
            upstream_for_replay,
            filter_cherry_equivalents,
        )?
    };

    if !args.keep_empty && !args.interactive {
        commits.retain(|oid| !is_commit_tree_unchanged(&repo, oid).unwrap_or(false));
    }

    let hook_arg1: &str = if args.root {
        "--root"
    } else {
        upstream_spec.as_str()
    };
    let hook_arg2: Option<&str> = pre_rebase_hook_second.as_deref();
    let hook_args: Vec<&str> = match hook_arg2 {
        Some(s) => vec![hook_arg1, s],
        None => vec![hook_arg1],
    };
    if let HookResult::Failed(_) = run_hook(&repo, "pre-rebase", &hook_args, None) {
        bail!("The pre-rebase hook refused to rebase.");
    }

    let (rebase_todo_lines, rebase_interactive) = if args.interactive {
        if commits.is_empty() {
            print_branch_up_to_date(&head);
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
        let pre_editor_len = commits.len();
        let (edited_lines, _) = run_interactive_rebase(
            &repo,
            git_dir,
            &commits,
            &config,
            autostash_oid.as_ref(),
            want_autosquash,
        )?;
        if edited_lines.is_empty() {
            if pre_editor_len > 0 {
                if worktree_matches_head(&repo, git_dir)? {
                    if let Some(ref oid) = autostash_oid {
                        let _ = stash::pop_autostash_if_top(&repo, oid);
                    }
                }
                bail!("there was a problem with the editor");
            }
            print_branch_up_to_date(&head);
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
        (edited_lines, true)
    } else if want_autosquash {
        validate_rebase_instruction_format(&config)?;
        let entries = rearrange_autosquash(&repo, commits)?;
        (rebase_state_todo_lines(&repo, &config, &entries)?, false)
    } else {
        let entries: Vec<(ObjectId, RebaseTodoCmd)> = commits
            .into_iter()
            .map(|o| (o, RebaseTodoCmd::Pick))
            .collect();
        (rebase_state_todo_lines(&repo, &config, &entries)?, false)
    };

    if !args.no_ff && rebase_todo_lines.is_empty() {
        if head_oid == onto_oid {
            print_branch_up_to_date(&head);
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
        if can_fast_forward(&repo, head_oid, onto_oid)? {
            let ff_base = merge_bases_first_vs_rest(&repo, onto_oid, &[head_oid])?
                .into_iter()
                .next();
            fast_forward_rebase(
                &repo,
                &head,
                head_oid,
                onto_oid,
                onto_name_for_state.as_str(),
                ff_base,
                head_oid,
            )?;
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
    }

    if rebase_todo_lines.is_empty() {
        if let HeadState::Branch { refname, .. } = &head {
            let ident = reflog_identity(&repo);
            let msg = format!("rebase (no-ff): checkout {}", onto_oid.to_hex());
            let _ = append_reflog(git_dir, refname, &head_oid, &head_oid, &ident, &msg, false);
            let _ = append_reflog(git_dir, "HEAD", &head_oid, &head_oid, &ident, &msg, false);
        }
        if let Some(ref oid) = autostash_oid {
            apply_autostash_after_ff(&repo, oid)?;
        }
        return Ok(());
    }

    let backend = choose_rebase_backend(&args);
    // Remove any stale rebase state from either backend so `active_rebase_dir` cannot pick the
    // wrong directory (merge is checked before apply).
    cleanup_rebase_state(git_dir);
    let rb_dir = rebase_state_dir_for_backend(git_dir, backend);
    fs::create_dir_all(&rb_dir)?;

    let head_name = match &head {
        HeadState::Branch { refname, .. } => refname.clone(),
        _ => "detached HEAD".to_string(),
    };
    fs::write(rb_dir.join("head-name"), &head_name)?;
    fs::write(rb_dir.join("orig-head"), head_oid.to_hex())?;
    fs::write(rb_dir.join("onto"), onto_oid.to_hex())?;
    fs::write(rb_dir.join("onto-name"), format!("{onto_name_for_state}\n"))?;
    fs::write(
        rb_dir.join("reflog-action"),
        format!("{}\n", rebase_reflog_action()),
    )?;
    fs::write(
        rb_dir.join("backend"),
        match backend {
            RebaseBackend::Merge => "merge\n",
            RebaseBackend::Apply => "apply\n",
        },
    )?;
    fs::write(rb_dir.join("rebasing"), "")?;
    if args.root {
        fs::write(rb_dir.join("root"), "")?;
    }
    if args.keep_empty {
        fs::write(rb_dir.join("keep-empty"), "")?;
    }

    let todo = rebase_todo_lines;
    let total = todo.len();
    if rebase_interactive {
        fs::write(rb_dir.join("interactive"), "")?;
    }
    fs::write(rb_dir.join("todo"), todo.join("\n") + "\n")?;
    fs::write(rb_dir.join("end"), total.to_string())?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("last"), total.to_string())?;
    fs::write(rb_dir.join("next"), "1")?;

    if let Some(ref ws) = args.whitespace {
        if ws.eq_ignore_ascii_case("fix") || ws.eq_ignore_ascii_case("strip") {
            fs::write(rb_dir.join("whitespace-action"), format!("{ws}\n"))?;
        }
    }

    if let Some(ref exec_cmd) = args.exec {
        fs::write(rb_dir.join("exec"), exec_cmd)?;
    }

    if let Some(ref oid) = autostash_oid {
        fs::write(rb_dir.join("autostash"), format!("{}\n", oid.to_hex()))?;
    }

    let ident = reflog_identity(&repo);
    let ra = rebase_reflog_action();
    let start_msg = format!("{ra} (start): checkout {onto_name_for_state}");
    // Git records `(start)` on HEAD only; the branch ref keeps its pre-rebase tip until `(finish)`.
    let _ = append_reflog(
        git_dir, "HEAD", &head_oid, &onto_oid, &ident, &start_msg, false,
    );

    let checkout_onto = || -> Result<()> {
        let onto_obj = repo.odb.read(&onto_oid)?;
        let onto_commit = parse_commit(&onto_obj.data)?;
        let entries = tree_to_index_entries(&repo, &onto_commit.tree, "")?;
        let mut idx = Index::new();
        idx.entries = entries;
        idx.sort();
        let old_index = load_index(&repo)?;
        if let Some(wt) = &repo.work_tree {
            check_dirty_worktree(&repo, &old_index, &idx, wt, &head)?;
        }

        fs::write(git_dir.join("HEAD"), format!("{}\n", onto_oid.to_hex()))?;
        fs::write(
            git_dir.join("ORIG_HEAD"),
            format!("{}\n", head_oid.to_hex()),
        )?;

        repo.write_index(&mut idx)?;
        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(&repo, wt, &old_index, &idx)?;
        }
        run_post_checkout_hook(&repo, &head_oid, &onto_oid)?;
        Ok(())
    };

    if let Err(e) = checkout_onto() {
        if let Some(ref oid) = autostash_oid {
            let _ = stash::pop_autostash_if_top(&repo, oid);
        }
        let _ = fs::remove_dir_all(&rb_dir);
        return Err(e);
    }

    eprintln!(
        "rebasing {} commits onto {}",
        total,
        &onto_oid.to_hex()[..7]
    );

    replay_remaining(&repo, &rb_dir, autostash_oid, backend, had_rebase_autostash)?;

    Ok(())
}

fn fast_forward_rebase(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    onto_oid: ObjectId,
    onto_name: &str,
    branch_base: Option<ObjectId>,
    orig_head: ObjectId,
) -> Result<()> {
    let git_dir = &repo.git_dir;
    if branch_base != Some(orig_head) {
        bail!("internal: fast-forward branch base mismatch");
    }

    println!("First, rewinding head to replay your work on top of it...");

    let ident = reflog_identity(repo);
    let ra = rebase_reflog_action();
    let start_msg = format!("{ra} (start): checkout {onto_name}");
    let _ = append_reflog(
        git_dir, "HEAD", &head_oid, &onto_oid, &ident, &start_msg, false,
    );

    fs::write(git_dir.join("HEAD"), format!("{}\n", onto_oid.to_hex()))?;
    fs::write(
        git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    let onto_obj = repo.odb.read(&onto_oid)?;
    let onto_commit = parse_commit(&onto_obj.data)?;
    let entries = tree_to_index_entries(repo, &onto_commit.tree, "")?;
    let mut idx = Index::new();
    idx.entries = entries;
    idx.sort();
    let old_index = load_index(repo)?;
    repo.write_index(&mut idx)?;
    if let Some(wt) = &repo.work_tree {
        checkout_merged_index(repo, wt, &old_index, &idx)?;
    }

    run_post_checkout_hook(repo, &head_oid, &onto_oid)?;

    let branch_disp = head
        .branch_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "HEAD".to_owned());

    if let HeadState::Branch { refname, .. } = head {
        let finish_branch = format!("{ra} (finish): {refname} onto {}", onto_oid.to_hex());
        let finish_head = format!("{ra} (finish): returning to {refname}");
        let _ = append_reflog(
            git_dir,
            refname,
            &head_oid,
            &onto_oid,
            &ident,
            &finish_branch,
            false,
        );
        let _ = append_reflog(
            git_dir,
            "HEAD",
            &onto_oid,
            &onto_oid,
            &ident,
            &finish_head,
            false,
        );

        let ref_path = git_dir.join(refname);
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ref_path, format!("{}\n", onto_oid.to_hex()))?;
        fs::write(git_dir.join("HEAD"), format!("ref: {refname}\n"))?;
    }

    println!("Fast-forwarded {branch_disp} to {onto_name}.");
    Ok(())
}

/// Resolve `A...B` in an `--onto` or synthesized `--keep-base` onto name to a single merge base.
///
/// When `keep_base` is true, Git reports `'<upstream>': need exactly one merge base with branch`;
/// otherwise `'<onto>': need exactly one merge base`.
fn merge_base_from_triple_dot_onto(
    repo: &Repository,
    onto_spec: &str,
    keep_base: bool,
    upstream_label: &str,
) -> Result<ObjectId> {
    let Some((left_raw, right_raw)) = split_triple_dot_range(onto_spec) else {
        bail!("internal: expected symmetric-diff revision in onto spec");
    };
    let left_tip = if left_raw.is_empty() {
        resolve_revision_for_range_end(repo, "HEAD")?
    } else {
        resolve_revision_for_range_end(repo, left_raw)?
    };
    let right_tip = if right_raw.is_empty() {
        resolve_revision_for_range_end(repo, "HEAD")?
    } else {
        resolve_revision_for_range_end(repo, right_raw)?
    };
    let left_c = peel_to_commit_for_merge_base(repo, left_tip)?;
    let right_c = peel_to_commit_for_merge_base(repo, right_tip)?;
    let bases = merge_bases_first_vs_rest(repo, left_c, &[right_c])?;
    if bases.len() != 1 {
        if keep_base {
            bail!("'{upstream_label}': need exactly one merge base with branch");
        } else {
            bail!("'{onto_spec}': need exactly one merge base");
        }
    }
    Ok(bases[0])
}

/// Commits to replay for a non-interactive rebase, oldest-first.
///
/// When `filter_cherry_equivalents` is true (Git's default without `--keep-base`), commits whose
/// patch-id matches a commit on the upstream side of the symmetric range `upstream...head` are
/// omitted, matching `sequencer_make_script` with `--cherry-pick --right-only`.
fn collect_rebase_todo_commits(
    repo: &Repository,
    head: ObjectId,
    upstream: ObjectId,
    filter_cherry_equivalents: bool,
) -> Result<Vec<ObjectId>> {
    if !filter_cherry_equivalents {
        return collect_commits_to_replay(repo, head, upstream);
    }

    let bases = merge_bases_first_vs_rest(repo, upstream, &[head])?;
    let negative: Vec<String> = bases.iter().map(|b| b.to_hex()).collect();
    let result = rev_list(
        repo,
        &[upstream.to_hex(), head.to_hex()],
        &negative,
        &RevListOptions {
            cherry_pick: true,
            right_only: true,
            left_right: true,
            symmetric_left: Some(upstream),
            symmetric_right: Some(head),
            ordering: OrderingMode::Topo,
            ..Default::default()
        },
    )?;

    let mut commits = result.commits;
    commits.reverse();
    Ok(commits)
}

/// Collect commits to replay: ancestors of `head` that are not ancestors of the merge-base
/// of `upstream` and `head`. Stops at the merge base only (not at `upstream`), matching Git.
/// Returns them oldest-first.
fn collect_commits_to_replay(
    repo: &Repository,
    head: ObjectId,
    upstream: ObjectId,
) -> Result<Vec<ObjectId>> {
    let bases = merge_bases_first_vs_rest(repo, upstream, &[head])?;
    let stop_set: HashSet<ObjectId> = bases.into_iter().collect();

    let mut commits = Vec::new();
    let mut current = head;

    loop {
        if stop_set.contains(&current) {
            break;
        }
        let obj = repo.odb.read(&current)?;
        if obj.kind != ObjectKind::Commit {
            break;
        }
        let commit = parse_commit(&obj.data)?;
        commits.push(current);
        if commit.parents.is_empty() {
            break;
        }
        current = commit.parents[0];
    }

    commits.reverse();
    Ok(commits)
}

/// Commits to replay for `rebase --root --onto <onto>`: same set as `git rev-list <onto>..<head>`.
///
/// Order matches `git rev-list` default output reversed (oldest first), including merge topology.
fn collect_commits_for_root_rebase(
    repo: &Repository,
    head: ObjectId,
    onto: ObjectId,
) -> Result<Vec<ObjectId>> {
    let range = format!("{}..{}", onto.to_hex(), head.to_hex());
    let (positive, negative) = split_revision_token(&range);
    let mut opts = RevListOptions::default();
    opts.first_parent = true;
    opts.ordering = OrderingMode::Default;
    opts.reverse = true;
    let listed = rev_list(repo, &positive, &negative, &opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    filter_redundant_patch_commits(repo, onto, &listed.commits)
}

/// Drop commits whose patch-id already exists on `onto` or earlier in the replay list.
///
/// Matches Git's "skipped previously applied commit" behaviour during `rebase --root`.
fn filter_redundant_patch_commits(
    repo: &Repository,
    onto: ObjectId,
    ordered: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let mut seen_patch_ids: HashSet<ObjectId> = HashSet::new();
    for oid in ancestor_closure(repo, onto)? {
        let obj = match repo.odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = match parse_commit(&obj.data) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if commit.parents.len() > 1 {
            continue;
        }
        if let Some(pid) = compute_patch_id(&repo.odb, &oid)? {
            seen_patch_ids.insert(pid);
        }
    }

    let mut out = Vec::new();
    for &oid in ordered {
        let obj = repo.odb.read(&oid)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = parse_commit(&obj.data)?;
        if commit.parents.len() > 1 {
            out.push(oid);
            continue;
        }
        let Some(pid) = compute_patch_id(&repo.odb, &oid)? else {
            out.push(oid);
            continue;
        };
        if seen_patch_ids.contains(&pid) {
            continue;
        }
        seen_patch_ids.insert(pid);
        out.push(oid);
    }
    Ok(out)
}

/// Whether `onto` is a strict fast-forward of `head` (linear single-parent history from `head` to `onto`).
fn can_fast_forward(repo: &Repository, head: ObjectId, onto: ObjectId) -> Result<bool> {
    if head == onto {
        return Ok(false);
    }
    if !is_ancestor(repo, head, onto)? {
        return Ok(false);
    }
    let bases = merge_bases_first_vs_rest(repo, onto, &[head])?;
    if bases.len() != 1 || bases[0] != head {
        return Ok(false);
    }
    is_linear_history(repo, head, onto)
}

fn is_linear_history(repo: &Repository, from: ObjectId, to: ObjectId) -> Result<bool> {
    let mut current = to;
    loop {
        if current == from {
            return Ok(true);
        }
        let obj = repo.odb.read(&current)?;
        let commit = parse_commit(&obj.data)?;
        if commit.parents.len() != 1 {
            return Ok(false);
        }
        current = commit.parents[0];
    }
}

/// Git's `can_fast_forward` for preemptive up-to-date / noop detection.
fn rebase_can_preemptive_ff(
    repo: &Repository,
    onto: ObjectId,
    upstream: ObjectId,
    head: ObjectId,
) -> Result<bool> {
    let bases = merge_bases_first_vs_rest(repo, onto, &[head])?;
    if bases.len() != 1 || bases[0] != onto {
        return Ok(false);
    }
    let up_bases = merge_bases_first_vs_rest(repo, upstream, &[head])?;
    if up_bases.len() != 1 || up_bases[0] != onto {
        return Ok(false);
    }
    is_linear_history(repo, onto, head)
}

fn print_rebase_diffstat(
    repo: &Repository,
    branch_base: Option<ObjectId>,
    onto_oid: ObjectId,
) -> Result<()> {
    let old_tree = if let Some(bb) = branch_base {
        let obj = repo.odb.read(&bb)?;
        let c = parse_commit(&obj.data)?;
        Some(c.tree)
    } else {
        None
    };
    let new_obj = repo.odb.read(&onto_oid)?;
    let new_commit = parse_commit(&new_obj.data)?;
    let entries = diff::diff_trees(&repo.odb, old_tree.as_ref(), Some(&new_commit.tree), "")?;
    print_diffstat_from_entries(repo, &entries);
    Ok(())
}

fn print_diffstat_from_entries(repo: &Repository, entries: &[DiffEntry]) {
    if entries.is_empty() {
        return;
    }

    struct StatEntry {
        path: String,
        insertions: usize,
        deletions: usize,
        is_new: bool,
        is_deleted: bool,
        new_mode: Option<u32>,
    }

    let mut stats: Vec<StatEntry> = Vec::new();
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for entry in entries {
        let path = entry
            .new_path
            .as_deref()
            .or(entry.old_path.as_deref())
            .unwrap_or("unknown");
        let is_new = entry.old_oid == diff::zero_oid();
        let is_deleted = entry.new_oid == diff::zero_oid();

        let old_content = if !is_new {
            repo.odb
                .read(&entry.old_oid)
                .ok()
                .map(|o| String::from_utf8_lossy(&o.data).to_string())
        } else {
            None
        };
        let new_content = if !is_deleted {
            repo.odb
                .read(&entry.new_oid)
                .ok()
                .map(|o| String::from_utf8_lossy(&o.data).to_string())
        } else {
            None
        };

        let (ins, del) = count_changes(
            old_content.as_deref().unwrap_or(""),
            new_content.as_deref().unwrap_or(""),
        );

        total_ins += ins;
        total_del += del;

        let mode_num = u32::from_str_radix(&entry.new_mode, 8).unwrap_or(0o100644);
        stats.push(StatEntry {
            path: path.to_owned(),
            insertions: ins,
            deletions: del,
            is_new,
            is_deleted,
            new_mode: if is_new { Some(mode_num) } else { None },
        });
    }

    let display_names: Vec<String> = stats.iter().map(|s| s.path.clone()).collect();
    let max_path_len = display_names.iter().map(|s| s.len()).max().unwrap_or(0);
    let max_change = stats
        .iter()
        .map(|s| s.insertions + s.deletions)
        .max()
        .unwrap_or(0);
    let count_width = if max_change == 0 {
        1
    } else {
        format!("{}", max_change).len()
    };

    for (i, s) in stats.iter().enumerate() {
        let total = s.insertions + s.deletions;
        let plus = "+".repeat(s.insertions.min(50));
        let minus = "-".repeat(s.deletions.min(50));
        println!(
            " {:<width$} | {:>cw$} {}{}",
            display_names[i],
            total,
            plus,
            minus,
            width = max_path_len,
            cw = count_width
        );
    }

    let files_changed = stats.len();
    let mut parts = Vec::new();
    parts.push(format!(
        "{} file{} changed",
        files_changed,
        if files_changed != 1 { "s" } else { "" }
    ));
    if total_ins > 0 {
        parts.push(format!(
            "{} insertion{}",
            total_ins,
            if total_ins != 1 { "s(+)" } else { "(+)" }
        ));
    }
    if total_del > 0 {
        parts.push(format!(
            "{} deletion{}",
            total_del,
            if total_del != 1 { "s(-)" } else { "(-)" }
        ));
    }
    println!(" {}", parts.join(", "));
}

/// Replay all remaining commits from the todo list.
fn replay_remaining(
    repo: &Repository,
    rb_dir: &Path,
    autostash_oid: Option<ObjectId>,
    backend: RebaseBackend,
    had_rebase_autostash: bool,
) -> Result<()> {
    let git_dir = &repo.git_dir;
    let ra = load_rebase_reflog_action(rb_dir);
    let ident = reflog_identity(repo);

    let _ = fs::remove_file(rb_dir.join("stopped-sha"));

    let rebase_interactive = rb_dir.join("interactive").exists();

    let todo_content = fs::read_to_string(rb_dir.join("todo"))?;
    let todo: Vec<&str> = todo_content.lines().filter(|l| !l.is_empty()).collect();
    let _total: usize = fs::read_to_string(rb_dir.join("end"))?.trim().parse()?;
    let msgnum: usize = fs::read_to_string(rb_dir.join("msgnum"))?.trim().parse()?;

    let rewind_marker = rb_dir.join("rewind-notice");
    if !rewind_marker.exists() && !todo.is_empty() {
        println!("First, rewinding head to replay your work on top of it...");
        let _ = fs::write(&rewind_marker, "");
    }

    for i in (msgnum - 1)..todo.len() {
        let line = todo[i];
        let step = parse_rebase_replay_step(repo, line, rebase_interactive)?
            .ok_or_else(|| anyhow::anyhow!("malformed rebase todo line: {line}"))?;

        fs::write(rb_dir.join("msgnum"), (i + 1).to_string())?;
        fs::write(rb_dir.join("next"), (i + 1).to_string())?;

        match step {
            RebaseReplayStep::Exec(exec_cmd) => {
                let _ = fs::remove_file(rb_dir.join("current"));
                let _ = fs::remove_file(rb_dir.join("current-cmd"));
                let _ = fs::remove_file(rb_dir.join("current-final-fixup"));
                eprintln!("Executing: {}", exec_cmd);
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&exec_cmd)
                    .current_dir(repo.work_tree.as_deref().unwrap_or_else(|| Path::new(".")))
                    .status()
                    .with_context(|| format!("failed to execute: {}", exec_cmd))?;
                if !status.success() {
                    let code = status.code().unwrap_or(1);
                    eprintln!(
                        "warning: execution failed for: {}\n\
                         hint: You can fix the problem, and then run\n\
                         hint:   grit rebase --continue",
                        exec_cmd
                    );
                    let remaining: Vec<&str> = todo[i + 1..].to_vec();
                    fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                    fs::write(rb_dir.join("msgnum"), "1")?;
                    fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                    std::process::exit(code);
                }
            }
            RebaseReplayStep::MergeReuseMessage {
                merge_oid,
                merge_args,
            } => {
                let _ = fs::remove_file(rb_dir.join("current"));
                let _ = fs::remove_file(rb_dir.join("current-cmd"));
                let _ = fs::remove_file(rb_dir.join("current-final-fixup"));
                let old_head = resolve_head(git_dir)?
                    .oid()
                    .cloned()
                    .unwrap_or_else(diff::zero_oid);
                let next_after =
                    peek_next_rebase_flush_hint(repo, &todo, i + 1, rebase_interactive);
                match rebase_merge_reuse_message(
                    repo,
                    git_dir,
                    rb_dir,
                    &merge_oid,
                    merge_args.as_str(),
                    next_after,
                ) {
                    Ok(RebaseMergeReuseOutcome::Completed) => {
                        let head = resolve_head(git_dir)?;
                        let new_oid = *head
                            .oid()
                            .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                        let merge_obj = repo.odb.read(&merge_oid)?;
                        let mc = parse_commit(&merge_obj.data)?;
                        let subject = mc.message.lines().next().unwrap_or("");
                        eprintln!("Applying: {}", subject);
                        let msg = format!("{ra} (pick): {subject}");
                        let _ = append_reflog(
                            git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false,
                        );
                        let rest: Vec<&str> = todo[i + 1..].to_vec();
                        fs::write(rb_dir.join("todo"), rest.join("\n") + "\n")?;
                        fs::write(rb_dir.join("msgnum"), "1")?;
                        fs::write(rb_dir.join("end"), rest.len().to_string())?;
                        return replay_remaining(
                            repo,
                            rb_dir,
                            autostash_oid,
                            backend,
                            had_rebase_autostash,
                        );
                    }
                    Ok(RebaseMergeReuseOutcome::Conflict) => {
                        let _ = fs::remove_file(rb_dir.join("current"));
                        let _ = fs::remove_file(rb_dir.join("current-cmd"));
                        let _ = fs::remove_file(rb_dir.join("current-final-fixup"));
                        let remaining_merge: Vec<&str> = todo[i..].to_vec();
                        fs::write(rb_dir.join("todo"), remaining_merge.join("\n") + "\n")?;
                        fs::write(rb_dir.join("msgnum"), "1")?;
                        fs::write(rb_dir.join("end"), remaining_merge.len().to_string())?;
                        let _ = fs::write(
                            rb_dir.join("stopped-sha"),
                            format!("{}\n", merge_oid.to_hex()),
                        );
                        let merge_obj_cf = repo.odb.read(&merge_oid)?;
                        let mc_cf = parse_commit(&merge_obj_cf.data)?;
                        let subj_cf = mc_cf.message.lines().next().unwrap_or("");
                        eprintln!(
                            "error: could not apply {}... {}\n\
                             hint: Resolve all conflicts manually, mark them as resolved with\n\
                             hint: \"grit add <pathspec>\", then run \"grit rebase --continue\".\n\
                             hint: To skip this commit, run \"grit rebase --skip\".\n\
                             hint: To abort, run \"grit rebase --abort\".",
                            &merge_oid.to_hex()[..7],
                            subj_cf
                        );
                        std::process::exit(1);
                    }
                    Ok(RebaseMergeReuseOutcome::Blocked) => {
                        let remaining_blk: Vec<&str> = todo[i..].to_vec();
                        fs::write(rb_dir.join("todo"), remaining_blk.join("\n") + "\n")?;
                        fs::write(rb_dir.join("msgnum"), "1")?;
                        fs::write(rb_dir.join("end"), remaining_blk.len().to_string())?;
                        std::process::exit(1);
                    }
                    Err(e) => {
                        let _ = fs::remove_file(rb_dir.join("rebase-merge-source"));
                        let _ = fs::remove_file(rb_dir.join("rebase-merge-args"));
                        eprintln!("{e:#}");
                        std::process::exit(1);
                    }
                }
            }
            RebaseReplayStep::Edit(commit_oid) => {
                let todo_cmd = RebaseTodoCmd::Pick;

                let commit_hex = commit_oid.to_hex();
                fs::write(rb_dir.join("current"), format!("{commit_hex}\n"))?;
                fs::write(
                    rb_dir.join("current-cmd"),
                    format!("{}\n", todo_cmd.as_str()),
                )?;
                let _ = fs::remove_file(rb_dir.join("current-final-fixup"));

                let old_head = resolve_head(git_dir)?
                    .oid()
                    .cloned()
                    .unwrap_or_else(diff::zero_oid);

                let pick_backend = load_rebase_backend(rb_dir);
                let final_fixup = is_final_fixup_in_todo(repo, &todo, i, rebase_interactive);
                let next_after_this =
                    peek_next_rebase_flush_hint(repo, &todo, i + 1, rebase_interactive);
                match cherry_pick_for_rebase(
                    repo,
                    rb_dir,
                    &commit_oid,
                    pick_backend,
                    todo_cmd,
                    final_fixup,
                    next_after_this,
                    false,
                ) {
                    Ok(()) => {
                        let head = resolve_head(git_dir)?;
                        let new_oid = *head
                            .oid()
                            .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                        let obj = repo.odb.read(&commit_oid)?;
                        let commit = parse_commit(&obj.data)?;
                        let root_rebase = rb_dir.join("root").exists();
                        let msg_for_log =
                            message_for_root_replayed_commit(repo, &commit, root_rebase);
                        let subject = msg_for_log.lines().next().unwrap_or("");
                        eprintln!("Applying: {}", subject);
                        let msg = format!("{ra} (pick): {subject}");
                        let _ = append_reflog(
                            git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false,
                        );

                        let remaining: Vec<&str> = todo[i + 1..].to_vec();
                        fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                        fs::write(rb_dir.join("msgnum"), "1")?;
                        fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                        let _ = fs::write(
                            rb_dir.join("stopped-sha"),
                            format!("{}\n", commit_oid.to_hex()),
                        );
                        let _ = fs::write(rb_dir.join("rebase-amend-continue"), "1\n");
                        std::process::exit(0);
                    }
                    Err(_e) => {
                        let remaining: Vec<&str> = todo[i + 1..].to_vec();
                        fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                        fs::write(rb_dir.join("msgnum"), "1")?;
                        fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                        let ff = is_final_fixup_in_todo(repo, &todo, i, rebase_interactive);
                        fs::write(
                            rb_dir.join("current-final-fixup"),
                            if ff { "1\n" } else { "0\n" },
                        )?;
                        let _ = fs::write(
                            rb_dir.join("stopped-sha"),
                            format!("{}\n", commit_oid.to_hex()),
                        );

                        let obj = repo.odb.read(&commit_oid)?;
                        let commit = parse_commit(&obj.data)?;
                        let root_rebase = rb_dir.join("root").exists();
                        let msg_for_log =
                            message_for_root_replayed_commit(repo, &commit, root_rebase);
                        let subject = msg_for_log.lines().next().unwrap_or("");

                        eprintln!(
                            "error: could not apply {}... {}\n\
                             hint: Resolve all conflicts manually, mark them as resolved with\n\
                             hint: \"grit add <pathspec>\", then run \"grit rebase --continue\".\n\
                             hint: To skip this commit, run \"grit rebase --skip\".\n\
                             hint: To abort, run \"grit rebase --abort\".",
                            &commit_oid.to_hex()[..7],
                            subject
                        );
                        std::process::exit(1);
                    }
                }
            }
            RebaseReplayStep::PickLike {
                oid: commit_oid,
                cmd: todo_cmd,
            } => {
                let commit_hex = commit_oid.to_hex();
                fs::write(rb_dir.join("current"), format!("{commit_hex}\n"))?;
                fs::write(
                    rb_dir.join("current-cmd"),
                    format!("{}\n", todo_cmd.as_str()),
                )?;
                let _ = fs::remove_file(rb_dir.join("current-final-fixup"));

                let old_head = resolve_head(git_dir)?
                    .oid()
                    .cloned()
                    .unwrap_or_else(diff::zero_oid);

                let pick_backend = load_rebase_backend(rb_dir);
                let final_fixup = is_final_fixup_in_todo(repo, &todo, i, rebase_interactive);
                let next_after_this =
                    peek_next_rebase_flush_hint(repo, &todo, i + 1, rebase_interactive);
                match cherry_pick_for_rebase(
                    repo,
                    rb_dir,
                    &commit_oid,
                    pick_backend,
                    todo_cmd,
                    final_fixup,
                    next_after_this,
                    true,
                ) {
                    Ok(()) => {
                        let head = resolve_head(git_dir)?;
                        let new_oid = *head
                            .oid()
                            .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                        let obj = repo.odb.read(&commit_oid)?;
                        let commit = parse_commit(&obj.data)?;
                        let root_rebase = rb_dir.join("root").exists();
                        let msg_for_log =
                            message_for_root_replayed_commit(repo, &commit, root_rebase);
                        let subject = msg_for_log.lines().next().unwrap_or("");
                        eprintln!("Applying: {}", subject);
                        let msg = format!("{ra} (pick): {subject}");
                        let _ = append_reflog(
                            git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false,
                        );

                        if let Ok(global_exec) = fs::read_to_string(rb_dir.join("exec")) {
                            let global_exec = global_exec.trim();
                            if !global_exec.is_empty() {
                                eprintln!("Executing: {}", global_exec);
                                let status = std::process::Command::new("sh")
                                    .arg("-c")
                                    .arg(global_exec)
                                    .current_dir(
                                        repo.work_tree.as_deref().unwrap_or_else(|| Path::new(".")),
                                    )
                                    .status()
                                    .with_context(|| {
                                        format!("failed to execute: {}", global_exec)
                                    })?;
                                if !status.success() {
                                    let code = status.code().unwrap_or(1);
                                    eprintln!(
                                        "warning: execution failed for: {}\n\
                                         hint: You can fix the problem, and then run\n\
                                         hint:   grit rebase --continue",
                                        global_exec
                                    );
                                    let remaining: Vec<&str> = todo[i + 1..].to_vec();
                                    fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                                    fs::write(rb_dir.join("msgnum"), "1")?;
                                    fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                                    std::process::exit(code);
                                }
                            }
                        }
                    }
                    Err(_e) => {
                        let remaining: Vec<&str> = todo[i + 1..].to_vec();
                        fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                        fs::write(rb_dir.join("msgnum"), "1")?;
                        fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                        let ff = is_final_fixup_in_todo(repo, &todo, i, rebase_interactive);
                        fs::write(
                            rb_dir.join("current-final-fixup"),
                            if ff { "1\n" } else { "0\n" },
                        )?;
                        let _ = fs::write(
                            rb_dir.join("stopped-sha"),
                            format!("{}\n", commit_oid.to_hex()),
                        );

                        let obj = repo.odb.read(&commit_oid)?;
                        let commit = parse_commit(&obj.data)?;
                        let root_rebase = rb_dir.join("root").exists();
                        let msg_for_log =
                            message_for_root_replayed_commit(repo, &commit, root_rebase);
                        let subject = msg_for_log.lines().next().unwrap_or("");

                        eprintln!(
                            "error: could not apply {}... {}\n\
                             hint: Resolve all conflicts manually, mark them as resolved with\n\
                             hint: \"grit add <pathspec>\", then run \"grit rebase --continue\".\n\
                             hint: To skip this commit, run \"grit rebase --skip\".\n\
                             hint: To abort, run \"grit rebase --abort\".",
                            &commit_oid.to_hex()[..7],
                            subject
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    // Rebase complete — restore branch ref
    finish_rebase(repo, rb_dir, autostash_oid, backend, had_rebase_autostash)?;
    Ok(())
}

fn rebase_keep_empty(rb_dir: &Path) -> bool {
    rb_dir.join("keep-empty").exists()
}

/// Cherry-pick a single commit onto current HEAD for rebase purposes.
///
/// `rb_dir` is the active state directory (`rebase-apply` or `rebase-merge`), not `rebase_dir()`
/// (which wrongly prefers `rebase-merge` whenever that path exists).
fn cherry_pick_for_rebase(
    repo: &Repository,
    rb_dir: &Path,
    commit_oid: &ObjectId,
    backend: RebaseBackend,
    todo_cmd: RebaseTodoCmd,
    final_fixup: bool,
    next_after_line: Option<RebaseTodoCmd>,
    record_rewrite: bool,
) -> Result<()> {
    let git_dir = &repo.git_dir;
    let keep_empty = rebase_keep_empty(rb_dir);

    let commit_obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let config = ConfigSet::load(Some(git_dir), true)?;

    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD is unborn during rebase"))?
        .to_owned();

    if keep_empty && todo_cmd == RebaseTodoCmd::Pick && is_commit_tree_unchanged(repo, commit_oid)?
    {
        let head_obj = repo.odb.read(&head_oid)?;
        let head_commit = parse_commit(&head_obj.data)?;
        let now = time::OffsetDateTime::now_utc();
        let committer = resolve_identity(&config, "COMMITTER")?;
        let (message, encoding, raw_message) = transcoded_replayed_message(&commit, &config);
        let commit_data = CommitData {
            tree: head_commit.tree,
            parents: vec![head_oid],
            author: commit.author.clone(),
            committer: format_ident(&committer, now),
            author_raw: commit.author_raw.clone(),
            committer_raw: commit.committer_raw.clone(),
            encoding,
            message,
            raw_message,
        };
        let bytes = serialize_commit(&commit_data);
        let new_oid = repo.odb.write(ObjectKind::Commit, &bytes)?;
        fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
        if record_rewrite {
            record_rebase_in_rewritten_pending(git_dir, rb_dir, commit_oid, next_after_line)?;
        }
        return Ok(());
    }

    // Parent tree (base for the cherry-pick). Root commits use Git's empty tree as base.
    const GIT_EMPTY_TREE_HEX: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let parent_tree_oid = if let Some(parent_oid) = commit.parents.first() {
        let parent_obj = repo.odb.read(parent_oid)?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        parent_commit.tree
    } else {
        ObjectId::from_hex(GIT_EMPTY_TREE_HEX)
            .map_err(|e| anyhow::anyhow!("invalid empty tree oid: {e}"))?
    };

    // Commit's tree (theirs — the changes we want)
    let commit_tree_oid = commit.tree;

    // HEAD tree (ours — the current state)
    let head_obj = repo.odb.read(&head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let head_tree_oid = head_commit.tree;
    let root_rebase = rb_dir.join("root").exists();
    let ws_fix_rule = load_ws_fix_rule_from_rebase_state(git_dir);

    // Already at the picked commit's parent tip — nothing to replay (matches Git's noop pick).
    // Fixup/squash must still run merge + message folding even when parent == HEAD.
    // Reword still needs the commit editor even when the tree is already applied.
    if todo_cmd == RebaseTodoCmd::Pick {
        if let Some(p) = commit.parents.first() {
            if head_oid == *p {
                let old_index = load_index(repo)?;
                let mut idx = Index::new();
                idx.entries = tree_to_index_entries(repo, &commit_tree_oid, "")?;
                idx.sort();
                if let Some(rule) = ws_fix_rule {
                    apply_ws_fix_to_index(repo, &mut idx, rule)?;
                }
                repo.write_index(&mut idx)?;
                if let Some(wt) = &repo.work_tree {
                    checkout_merged_index(repo, wt, &old_index, &idx)?;
                }
                if ws_fix_rule.is_some() {
                    let tree_oid = write_tree_from_index(&repo.odb, &idx, "")?;
                    let now = time::OffsetDateTime::now_utc();
                    let committer = resolve_identity(&config, "COMMITTER")?;
                    let (message, encoding, raw_message) = if root_rebase {
                        let msg = message_for_root_replayed_commit(repo, &commit, true);
                        (msg, commit.encoding.clone(), None)
                    } else {
                        transcoded_replayed_message(&commit, &config)
                    };
                    let raw_msg =
                        commit_message_after_prepare_hook(repo, git_dir, &message, "message")?;
                    let message =
                        apply_commit_msg_cleanup(&raw_msg, rebase_commit_msg_cleanup(&config));
                    let commit_data = CommitData {
                        tree: tree_oid,
                        parents: vec![head_oid],
                        author: commit.author.clone(),
                        committer: format_ident(&committer, now),
                        author_raw: commit.author_raw.clone(),
                        committer_raw: commit.committer_raw.clone(),
                        encoding,
                        message,
                        raw_message,
                    };
                    let commit_bytes = serialize_commit(&commit_data);
                    let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
                    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
                    append_rebase_rewrite_line(rb_dir, commit_oid, &new_oid)?;
                } else {
                    fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
                }
                return Ok(());
            }
        }
    }
    if todo_cmd == RebaseTodoCmd::Reword {
        if let Some(p) = commit.parents.first() {
            if head_oid == *p {
                let old_index = load_index(repo)?;
                let mut idx = Index::new();
                idx.entries = tree_to_index_entries(repo, &commit_tree_oid, "")?;
                idx.sort();
                repo.write_index(&mut idx)?;
                if let Some(wt) = &repo.work_tree {
                    checkout_merged_index(repo, wt, &old_index, &idx)?;
                }
                let (template, _enc, _raw) = if root_rebase {
                    let msg = message_for_root_replayed_commit(repo, &commit, true);
                    (msg, commit.encoding.clone(), None)
                } else {
                    transcoded_replayed_message(&commit, &config)
                };
                let after_editor = run_commit_editor_for_reword(repo, git_dir, &template)?;
                let cleaned =
                    apply_commit_msg_cleanup(&after_editor, rebase_commit_msg_cleanup(&config));
                let (message, encoding, raw_message) =
                    finalize_message_for_commit_encoding(cleaned, &config);
                let now = time::OffsetDateTime::now_utc();
                let committer = resolve_identity(&config, "COMMITTER")?;
                let commit_data = CommitData {
                    tree: commit_tree_oid,
                    parents: vec![head_oid],
                    author: commit.author.clone(),
                    committer: format_ident(&committer, now),
                    author_raw: commit.author_raw.clone(),
                    committer_raw: commit.committer_raw.clone(),
                    encoding,
                    message,
                    raw_message,
                };
                let commit_bytes = serialize_commit(&commit_data);
                let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
                fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
                append_rebase_rewrite_line(rb_dir, commit_oid, &new_oid)?;
                return Ok(());
            }
        }
    }

    if matches!(todo_cmd, RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) {
        let mut ctx = read_squash_ctx(rb_dir);
        update_squash_message_file(repo, rb_dir, git_dir, todo_cmd, &commit, &mut ctx)?;
    }

    // Three-way merge: base=parent_tree, ours=HEAD_tree, theirs=commit_tree
    let base_tree_oid = if ws_fix_rule.is_some() {
        // After an earlier replay, HEAD can differ from the picked commit's parent tree in the ODB
        // (e.g. `rebase --whitespace=fix`). Use the current tip tree as the merge base so the
        // merge sees ours==base and applies the commit's tree as the new result.
        head_tree_oid
    } else {
        parent_tree_oid
    };
    let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree_oid, "")?);
    let ours_entries = tree_to_map(tree_to_index_entries(repo, &head_tree_oid, "")?);
    let theirs_entries = tree_to_map(tree_to_index_entries(repo, &commit_tree_oid, "")?);
    let conflict_ctx = RebaseConflictContext {
        backend,
        picked_subject: commit.message.lines().next().unwrap_or("replayed commit"),
    };
    let merge_result = three_way_merge_with_content(
        repo,
        &base_entries,
        &ours_entries,
        &theirs_entries,
        &conflict_ctx,
    )?;
    let mut merged_index = merge_result.index;

    if let Some(rule) = ws_fix_rule {
        apply_ws_fix_to_index(repo, &mut merged_index, rule)?;
    }

    let has_conflicts = merged_index.entries.iter().any(|e| e.stage() != 0)
        || !merge_result.conflict_files.is_empty();

    // Write index
    let old_index = load_index(repo)?;
    repo.write_index(&mut merged_index)?;

    // Update worktree
    if let Some(wt) = &repo.work_tree {
        checkout_merged_index(repo, wt, &old_index, &merged_index)?;
        if has_conflicts {
            write_rebase_conflict_files(wt, &merge_result.conflict_files)?;
        }
    }

    if has_conflicts {
        let _ = grit_lib::rerere::repo_rerere(repo, grit_lib::rerere::RerereAutoupdate::FromConfig);
        if todo_cmd == RebaseTodoCmd::Reword {
            let (unicode, _enc, _raw) = transcoded_replayed_message(&commit, &config);
            write_rebase_conflict_message(git_dir, &commit, &config)?;
            fs::write(rb_dir.join("message"), unicode)?;
        } else {
            fs::write(git_dir.join("MERGE_MSG"), &commit.message)?;
        }
        bail!("conflicts during cherry-pick of {}", commit_oid.to_hex());
    }

    if matches!(todo_cmd, RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) {
        let head_obj = repo.odb.read(&head_oid)?;
        let hc = parse_commit(&head_obj.data)?;
        let amend_parent = hc.parents.first().copied().unwrap_or(head_oid);

        if todo_cmd == RebaseTodoCmd::Fixup && !final_fixup {
            let fixup_path = rb_dir.join("message-fixup");
            let msg = if fixup_path.exists() {
                fs::read_to_string(&fixup_path)?
            } else {
                hc.message.clone()
            };
            let new_oid = commit_from_merged_index(
                repo,
                git_dir,
                &merged_index,
                &config,
                vec![amend_parent],
                &hc.author,
                msg,
            )?;
            fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
            record_rebase_in_rewritten_pending(git_dir, rb_dir, commit_oid, next_after_line)?;
            return Ok(());
        }
        if todo_cmd == RebaseTodoCmd::Squash && !final_fixup {
            let tmpl = fs::read_to_string(rb_dir.join("message-squash"))?;
            let raw = commit_message_after_prepare_hook(repo, git_dir, &tmpl, "message")?;
            let cleaned = apply_commit_msg_cleanup(&raw, default_commit_msg_cleanup(&config));
            let new_oid = commit_from_merged_index(
                repo,
                git_dir,
                &merged_index,
                &config,
                vec![amend_parent],
                &hc.author,
                cleaned,
            )?;
            fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
            if record_rewrite {
                record_rebase_in_rewritten_pending(git_dir, rb_dir, commit_oid, next_after_line)?;
            }
            return Ok(());
        }

        if todo_cmd == RebaseTodoCmd::Fixup {
            let fixup_path = rb_dir.join("message-fixup");
            let template = if fixup_path.exists() {
                fs::read_to_string(&fixup_path)?
            } else {
                let subj = hc.message.lines().next().unwrap_or("");
                let body = message_body_after_subject(&hc.message);
                format!("{subj}\n{body}")
            };
            let raw = commit_message_after_prepare_hook(repo, git_dir, &template, "squash")?;
            let cleaned = apply_commit_msg_cleanup(&raw, rebase_commit_msg_cleanup(&config));
            let new_oid = commit_from_merged_index(
                repo,
                git_dir,
                &merged_index,
                &config,
                vec![amend_parent],
                &hc.author,
                cleaned,
            )?;
            fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
            clear_squash_ctx(&rb_dir);
            if record_rewrite {
                record_rebase_in_rewritten_pending(git_dir, rb_dir, commit_oid, next_after_line)?;
            }
            return Ok(());
        }

        let squash_path = rb_dir.join("message-squash");
        let fixup_path = rb_dir.join("message-fixup");
        let tmpl = if fixup_path.exists() {
            fs::read_to_string(&fixup_path)?
        } else {
            fs::read_to_string(&squash_path)?
        };
        let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
        let raw = commit_message_after_prepare_hook(repo, git_dir, &tmpl, "squash")?;
        let cleaned = apply_commit_msg_cleanup(&raw, rebase_commit_msg_cleanup(&config));
        let new_oid = commit_from_merged_index(
            repo,
            git_dir,
            &merged_index,
            &config,
            vec![amend_parent],
            &hc.author,
            cleaned,
        )?;
        fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
        clear_squash_ctx(&rb_dir);
        if record_rewrite {
            record_rebase_in_rewritten_pending(git_dir, rb_dir, commit_oid, next_after_line)?;
        }
        return Ok(());
    }

    // Create the rebased commit, preserving the original author (normal pick / reword)
    let tree_oid = write_tree_from_index(&repo.odb, &merged_index, "")?;

    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    let (message, encoding, raw_message) = if todo_cmd == RebaseTodoCmd::Reword {
        let template = if root_rebase {
            message_for_root_replayed_commit(repo, &commit, true)
        } else {
            let (unicode, _enc, _raw) = transcoded_replayed_message(&commit, &config);
            unicode
        };
        let after_editor = run_commit_editor_for_reword(repo, git_dir, &template)?;
        let cleaned = apply_commit_msg_cleanup(&after_editor, rebase_commit_msg_cleanup(&config));
        finalize_message_for_commit_encoding(cleaned, &config)
    } else {
        let (msg_base, _enc_base, _raw_base) = if root_rebase {
            let msg = message_for_root_replayed_commit(repo, &commit, true);
            (msg, commit.encoding.clone(), None)
        } else {
            transcoded_replayed_message(&commit, &config)
        };
        let raw_msg = commit_message_after_prepare_hook(repo, git_dir, &msg_base, "message")?;
        let message = apply_commit_msg_cleanup(&raw_msg, rebase_commit_msg_cleanup(&config));
        finalize_message_for_commit_encoding(message, &config)
    };

    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid],
        author: commit.author.clone(), // preserve original author
        committer: format_ident(&committer, now),
        author_raw: commit.author_raw.clone(),
        committer_raw: commit.committer_raw.clone(),
        encoding,
        message,
        raw_message,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD (detached)
    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;

    if record_rewrite {
        record_rebase_in_rewritten_pending(git_dir, rb_dir, commit_oid, next_after_line)?;
    }

    Ok(())
}

/// Finish the rebase: point the original branch at the new HEAD.
fn finish_rebase(
    repo: &Repository,
    rb_dir: &Path,
    autostash_oid: Option<ObjectId>,
    backend: RebaseBackend,
    had_rebase_autostash: bool,
) -> Result<()> {
    let git_dir = &repo.git_dir;

    let head_name = fs::read_to_string(rb_dir.join("head-name"))?;
    let head_name = head_name.trim();

    let onto_hex = fs::read_to_string(rb_dir.join("onto"))?;
    let onto_hex = onto_hex.trim();
    let onto_oid = ObjectId::from_hex(onto_hex)?;

    let ra = load_rebase_reflog_action(rb_dir);
    let ident = reflog_identity(repo);

    let head = resolve_head(git_dir)?;
    let new_tip = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

    let autostash_oid_finish = autostash_oid.or_else(|| read_autostash_oid(rb_dir).ok().flatten());
    let had_autostash_finish = had_rebase_autostash || autostash_oid_finish.is_some();

    if head_name != "detached HEAD" {
        let ref_path = git_dir.join(head_name);
        let old_branch_oid = fs::read_to_string(&ref_path)
            .ok()
            .and_then(|s| ObjectId::from_hex(s.trim()).ok())
            .unwrap_or(new_tip);

        let finish_branch = format!("{ra} (finish): {head_name} onto {}", onto_oid.to_hex());
        let finish_head = format!("{ra} (finish): returning to {head_name}");
        let _ = append_reflog(
            git_dir,
            head_name,
            &old_branch_oid,
            &new_tip,
            &ident,
            &finish_branch,
            false,
        );
        let _ = append_reflog(
            git_dir,
            "HEAD",
            &new_tip,
            &new_tip,
            &ident,
            &finish_head,
            false,
        );

        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ref_path, format!("{}\n", new_tip.to_hex()))?;
        fs::write(git_dir.join("HEAD"), format!("ref: {head_name}\n"))?;
    }

    let success_target = if head_name == "detached HEAD" {
        "HEAD"
    } else {
        head_name
    };

    flush_rebase_rewritten_pending(git_dir, rb_dir)?;
    run_post_rewrite_after_rebase(repo, rb_dir);

    match backend {
        RebaseBackend::Merge => {
            if autostash_oid_finish.is_some() {
                apply_pending_autostash(repo, rb_dir)?;
            }
            cleanup_rebase_state(git_dir);
            eprintln!("Successfully rebased and updated {success_target}.");
        }
        RebaseBackend::Apply => {
            cleanup_rebase_state(git_dir);
            if let Some(oid) = autostash_oid_finish {
                apply_autostash_after_ff(repo, &oid)?;
            }
            // With `--apply`, Git omits the "Successfully rebased" line on stdout when autostash
            // was used (see t3420 `create_expected_success_apply`).
            if !had_autostash_finish {
                eprintln!("Successfully rebased and updated {success_target}.");
            }
        }
    }

    Ok(())
}

// ── --continue ──────────────────────────────────────────────────────

fn read_current_rebase_todo_cmd(rb_dir: &Path) -> RebaseTodoCmd {
    let Ok(s) = fs::read_to_string(rb_dir.join("current-cmd")) else {
        return RebaseTodoCmd::Pick;
    };
    match s.trim() {
        "reword" => RebaseTodoCmd::Reword,
        "fixup" => RebaseTodoCmd::Fixup,
        "squash" => RebaseTodoCmd::Squash,
        _ => RebaseTodoCmd::Pick,
    }
}

fn read_current_final_fixup(rb_dir: &Path) -> bool {
    fs::read_to_string(rb_dir.join("current-final-fixup"))
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn pop_first_nonempty_todo_line(rb_dir: &Path) -> Result<()> {
    let path = rb_dir.join("todo");
    let s = fs::read_to_string(&path)?;
    let mut lines: Vec<String> = s.lines().map(|l| l.to_owned()).collect();
    while let Some(idx) = lines.iter().position(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    }) {
        lines.remove(idx);
        break;
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    let remaining: usize = out.lines().filter(|l| !l.trim().is_empty()).count();
    fs::write(&path, out)?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("end"), remaining.to_string())?;
    Ok(())
}

fn trim_completed_merge_line_from_rebase_todo(rb_dir: &Path) -> Result<()> {
    let path = rb_dir.join("todo");
    let s = fs::read_to_string(&path)?;
    let mut lines: Vec<String> = s.lines().map(|l| l.to_owned()).collect();
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        if t.is_empty() || t.starts_with('#') {
            i += 1;
            continue;
        }
        if t.split_whitespace()
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case("merge"))
        {
            lines.remove(i);
            break;
        }
        break;
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    let remaining: usize = out.lines().filter(|l| !l.trim().is_empty()).count();
    fs::write(&path, out)?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("end"), remaining.to_string())?;
    Ok(())
}

fn do_continue() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_rebase_in_progress(git_dir) {
        bail!("no rebase in progress");
    }

    let rb_dir = active_rebase_dir(git_dir)
        .ok_or_else(|| anyhow::anyhow!("internal: no rebase state directory"))?;
    let autostash_continue = read_autostash_oid(&rb_dir)?;
    let had_autostash_continue = autostash_continue.is_some();
    let backend_continue = load_rebase_backend(&rb_dir);

    let interactive_continue = rb_dir.join("interactive").exists();

    if rb_dir.join("rebase-merge-source").exists()
        && rb_dir.join("rebase-merge-args").exists()
        && !git_dir.join("MERGE_HEAD").exists()
    {
        let src_hex = fs::read_to_string(rb_dir.join("rebase-merge-source"))?;
        let merge_src_oid = ObjectId::from_hex(src_hex.trim())?;
        let merge_args = fs::read_to_string(rb_dir.join("rebase-merge-args"))?;
        let merge_args = merge_args.trim();
        let todo_retry = fs::read_to_string(rb_dir.join("todo"))?;
        let todo_lines_retry: Vec<&str> = todo_retry.lines().filter(|l| !l.is_empty()).collect();
        let next_after_retry =
            peek_next_rebase_flush_hint(&repo, &todo_lines_retry, 1, interactive_continue);
        let st = run_rebase_merge_subprocess(&repo, git_dir, &merge_src_oid, merge_args)?;
        if st.success() {
            let _ = fs::remove_file(rb_dir.join("rebase-merge-source"));
            let _ = fs::remove_file(rb_dir.join("rebase-merge-args"));
            record_rebase_in_rewritten_pending(git_dir, &rb_dir, &merge_src_oid, next_after_retry)?;
            pop_first_nonempty_todo_line(&rb_dir)?;
            trim_completed_merge_line_from_rebase_todo(&rb_dir)?;
            return replay_remaining(
                &repo,
                &rb_dir,
                autostash_continue,
                backend_continue,
                had_autostash_continue,
            );
        }
        if git_dir.join("MERGE_HEAD").exists() {
            let _ = fs::remove_file(rb_dir.join("rebase-merge-args"));
            let _ = fs::write(
                rb_dir.join("stopped-sha"),
                format!("{}\n", merge_src_oid.to_hex()),
            );
            bail!(
                "merge conflicts during rebase merge; resolve, then run 'grit rebase --continue'"
            );
        }
        bail!("merge still blocked; fix the reported issue and run 'grit rebase --continue'");
    }

    if git_dir.join("MERGE_HEAD").exists() && rb_dir.join("rebase-merge-source").exists() {
        let src_hex = fs::read_to_string(rb_dir.join("rebase-merge-source"))?;
        let merge_src_oid = ObjectId::from_hex(src_hex.trim())?;
        let self_exe = std::env::current_exe().context("cannot determine grit binary path")?;
        let st = std::process::Command::new(&self_exe)
            .args(["merge", "--continue"])
            .current_dir(repo.work_tree.as_deref().unwrap_or_else(|| Path::new(".")))
            .status()
            .context("run grit merge --continue during rebase")?;
        if !st.success() {
            bail!("merge --continue failed");
        }
        let _ = fs::remove_file(rb_dir.join("rebase-merge-source"));
        let _ = fs::remove_file(rb_dir.join("rebase-merge-args"));
        pop_first_nonempty_todo_line(&rb_dir)?;
        trim_completed_merge_line_from_rebase_todo(&rb_dir)?;
        let todo_after = fs::read_to_string(rb_dir.join("todo"))?;
        let todo_lines_after: Vec<&str> = todo_after.lines().filter(|l| !l.is_empty()).collect();
        let next_peek =
            peek_next_rebase_flush_hint(&repo, &todo_lines_after, 0, interactive_continue);
        record_rebase_in_rewritten_pending(git_dir, &rb_dir, &merge_src_oid, next_peek)?;
        return replay_remaining(
            &repo,
            &rb_dir,
            autostash_continue,
            backend_continue,
            had_autostash_continue,
        );
    }

    let todo_content_continue = fs::read_to_string(rb_dir.join("todo"))?;
    let todo_lines_continue: Vec<&str> = todo_content_continue
        .lines()
        .filter(|l| !l.is_empty())
        .collect();

    if rb_dir.join("rebase-amend-continue").exists() {
        let index = load_index(&repo)?;
        if index.entries.iter().any(|e| e.stage() != 0) {
            bail!(
                "error: commit is not possible because you have unmerged files\n\
                 hint: fix conflicts and then run 'grit rebase --continue'"
            );
        }
        let config = ConfigSet::load(Some(git_dir), true)?;
        let head = resolve_head(git_dir)?;
        let head_oid = head
            .oid()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
        let head_obj = repo.odb.read(&head_oid)?;
        let hc = parse_commit(&head_obj.data)?;
        let amend_parent = hc.parents.first().copied().unwrap_or(head_oid);
        let msg_src = if git_dir.join("COMMIT_EDITMSG").exists() {
            fs::read_to_string(git_dir.join("COMMIT_EDITMSG"))?
        } else {
            hc.message.clone()
        };
        let raw_msg = commit_message_after_prepare_hook(&repo, git_dir, msg_src.trim(), "message")?;
        let message = apply_commit_msg_cleanup(&raw_msg, rebase_commit_msg_cleanup(&config));
        let new_oid = commit_from_merged_index(
            &repo,
            git_dir,
            &index,
            &config,
            vec![amend_parent],
            &hc.author,
            message,
        )?;
        fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
        let amend_stopped_hex = fs::read_to_string(rb_dir.join("stopped-sha")).unwrap_or_default();
        let amend_stopped_hex = amend_stopped_hex.trim();
        let amend_old_oid = if amend_stopped_hex.len() == 40 {
            ObjectId::from_hex(amend_stopped_hex)?
        } else {
            head_oid
        };
        let next_peek_amend =
            peek_next_rebase_flush_hint(&repo, &todo_lines_continue, 0, interactive_continue);
        record_rebase_in_rewritten_pending(git_dir, &rb_dir, &amend_old_oid, next_peek_amend)?;
        let _ = fs::remove_file(rb_dir.join("stopped-sha"));
        let _ = fs::remove_file(rb_dir.join("rebase-amend-continue"));
        let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
        return replay_remaining(
            &repo,
            &rb_dir,
            autostash_continue,
            backend_continue,
            had_autostash_continue,
        );
    }

    let stopped_path = rb_dir.join("stopped-sha");
    let stopped_oid = fs::read_to_string(&stopped_path).ok().and_then(|s| {
        let hex = s.lines().next()?.trim();
        ObjectId::from_hex(hex).ok()
    });
    let _ = fs::remove_file(&stopped_path);

    if !rb_dir.join("current").exists() {
        let first_line = todo_lines_continue.iter().copied().find(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        });
        if let Some(line) = first_line {
            if let Ok(Some(RebaseReplayStep::Exec(_))) =
                parse_rebase_replay_step(&repo, line, interactive_continue)
            {
                return replay_remaining(
                    &repo,
                    &rb_dir,
                    autostash_continue,
                    backend_continue,
                    had_autostash_continue,
                );
            }
        }
    }

    // Check for unresolved conflicts
    let index = load_index(&repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        bail!(
            "error: commit is not possible because you have unmerged files\n\
             hint: fix conflicts and then run 'grit rebase --continue'"
        );
    }

    // Commit the current cherry-pick
    let current_hex = fs::read_to_string(rb_dir.join("current"))?;
    let current_hex = current_hex.trim();
    let mut current_oid = ObjectId::from_hex(current_hex)?;

    if interactive_continue {
        if let Some(first_pick) = first_interactive_todo_pick_oid(&repo, &todo_lines_continue) {
            if first_pick != current_oid {
                if let Ok(pick_obj) = repo.odb.read(&first_pick) {
                    if let Ok(pick_commit) = parse_commit(&pick_obj.data) {
                        if diff_index_to_tree(&repo.odb, &index, Some(&pick_commit.tree))
                            .map(|d| d.is_empty())
                            .unwrap_or(false)
                        {
                            current_oid = first_pick;
                            fs::write(
                                rb_dir.join("current"),
                                format!("{}\n", current_oid.to_hex()),
                            )?;
                            fs::write(rb_dir.join("current-cmd"), "pick\n")?;
                        }
                    }
                }
            }
        }
    }

    let commit_obj = repo.odb.read(&current_oid)?;
    let original_commit = parse_commit(&commit_obj.data)?;

    let config = ConfigSet::load(Some(git_dir), true)?;
    let todo_cmd = read_current_rebase_todo_cmd(&rb_dir);
    let final_fixup = read_current_final_fixup(&rb_dir);

    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

    let new_oid = if matches!(todo_cmd, RebaseTodoCmd::Fixup | RebaseTodoCmd::Squash) {
        let head_obj = repo.odb.read(&head_oid)?;
        let hc = parse_commit(&head_obj.data)?;
        let amend_parent = hc.parents.first().copied().unwrap_or(head_oid);

        if todo_cmd == RebaseTodoCmd::Fixup && !final_fixup {
            let fixup_path = rb_dir.join("message-fixup");
            let msg = if git_dir.join("MERGE_MSG").exists() {
                fs::read_to_string(git_dir.join("MERGE_MSG"))?
            } else if fixup_path.exists() {
                fs::read_to_string(&fixup_path)?
            } else {
                hc.message.clone()
            };
            commit_from_merged_index(
                &repo,
                git_dir,
                &index,
                &config,
                vec![amend_parent],
                &hc.author,
                msg,
            )?
        } else if todo_cmd == RebaseTodoCmd::Squash && !final_fixup {
            let tmpl = fs::read_to_string(rb_dir.join("message-squash"))?;
            let raw = commit_message_after_prepare_hook(&repo, git_dir, &tmpl, "message")?;
            let cleaned = apply_commit_msg_cleanup(&raw, default_commit_msg_cleanup(&config));
            commit_from_merged_index(
                &repo,
                git_dir,
                &index,
                &config,
                vec![amend_parent],
                &hc.author,
                cleaned,
            )?
        } else if todo_cmd == RebaseTodoCmd::Fixup {
            let fixup_path = rb_dir.join("message-fixup");
            let template = if fixup_path.exists() {
                fs::read_to_string(&fixup_path)?
            } else {
                let subj = hc.message.lines().next().unwrap_or("");
                let body = message_body_after_subject(&hc.message);
                format!("{subj}\n{body}")
            };
            let raw = commit_message_after_prepare_hook(&repo, git_dir, &template, "squash")?;
            let cleaned = apply_commit_msg_cleanup(&raw, rebase_commit_msg_cleanup(&config));
            let oid = commit_from_merged_index(
                &repo,
                git_dir,
                &index,
                &config,
                vec![amend_parent],
                &hc.author,
                cleaned,
            )?;
            clear_squash_ctx(&rb_dir);
            oid
        } else {
            let squash_path = rb_dir.join("message-squash");
            let fixup_path = rb_dir.join("message-fixup");
            let tmpl = if fixup_path.exists() {
                fs::read_to_string(&fixup_path)?
            } else {
                fs::read_to_string(&squash_path)?
            };
            let raw = commit_message_after_prepare_hook(&repo, git_dir, &tmpl, "squash")?;
            let cleaned = apply_commit_msg_cleanup(&raw, rebase_commit_msg_cleanup(&config));
            let oid = commit_from_merged_index(
                &repo,
                git_dir,
                &index,
                &config,
                vec![amend_parent],
                &hc.author,
                cleaned,
            )?;
            clear_squash_ctx(&rb_dir);
            oid
        }
    } else if todo_cmd == RebaseTodoCmd::Reword {
        let template = {
            let rb_msg = rb_dir.join("message");
            if rb_msg.exists() {
                fs::read_to_string(&rb_msg)?
            } else {
                let (unicode, _enc, _raw) = transcoded_replayed_message(&original_commit, &config);
                unicode
            }
        };
        let after_editor = run_commit_editor_for_reword(&repo, git_dir, &template)?;
        let cleaned = apply_commit_msg_cleanup(&after_editor, rebase_commit_msg_cleanup(&config));
        let (message, encoding, raw_message) =
            finalize_message_for_commit_encoding(cleaned, &config);
        let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
        let now = time::OffsetDateTime::now_utc();
        let committer = resolve_identity(&config, "COMMITTER")?;
        let commit_data = CommitData {
            tree: tree_oid,
            parents: vec![head_oid],
            author: original_commit.author.clone(),
            committer: format_ident(&committer, now),
            author_raw: original_commit.author_raw.clone(),
            committer_raw: original_commit.committer_raw.clone(),
            encoding,
            message,
            raw_message,
        };
        let commit_bytes = serialize_commit(&commit_data);
        repo.odb.write(ObjectKind::Commit, &commit_bytes)?
    } else {
        let (message, encoding, raw_message) =
            read_rebase_continue_message(git_dir, &original_commit, &config)?;
        let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
        let now = time::OffsetDateTime::now_utc();
        let committer = resolve_identity(&config, "COMMITTER")?;
        let raw_msg = commit_message_after_prepare_hook(&repo, git_dir, &message, "message")?;
        let message = apply_commit_msg_cleanup(&raw_msg, rebase_commit_msg_cleanup(&config));
        let commit_data = CommitData {
            tree: tree_oid,
            parents: vec![head_oid],
            author: original_commit.author.clone(),
            committer: format_ident(&committer, now),
            author_raw: original_commit.author_raw.clone(),
            committer_raw: original_commit.committer_raw.clone(),
            encoding,
            message,
            raw_message,
        };
        let commit_bytes = serialize_commit(&commit_data);
        repo.odb.write(ObjectKind::Commit, &commit_bytes)?
    };

    // Update HEAD (detached)
    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(rb_dir.join("message"));
    let _ = fs::remove_file(rb_dir.join("current-final-fixup"));

    let (oid_for_rewrite, next_after_continue) = if stopped_oid.is_some() {
        (
            stopped_oid.as_ref().unwrap(),
            peek_next_rebase_flush_hint(&repo, &todo_lines_continue, 0, interactive_continue),
        )
    } else {
        (
            &current_oid,
            peek_next_rebase_flush_hint(&repo, &todo_lines_continue, 1, interactive_continue),
        )
    };
    record_rebase_in_rewritten_pending(git_dir, &rb_dir, oid_for_rewrite, next_after_continue)?;

    let subject = original_commit.message.lines().next().unwrap_or("");
    eprintln!("Applying: {}", subject);

    let pick_backend = load_rebase_backend(&rb_dir);
    let ra = load_rebase_reflog_action(&rb_dir);
    let ident = reflog_identity(&repo);
    let verb = match pick_backend {
        RebaseBackend::Merge => "continue",
        RebaseBackend::Apply => "pick",
    };
    let msg = format!("{ra} ({verb}): {subject}");
    let _ = append_reflog(git_dir, "HEAD", &head_oid, &new_oid, &ident, &msg, false);

    // Continue with remaining
    replay_remaining(
        &repo,
        &rb_dir,
        autostash_continue,
        backend_continue,
        had_autostash_continue,
    )?;

    Ok(())
}

// ── --skip ──────────────────────────────────────────────────────────

fn do_skip() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_rebase_in_progress(git_dir) {
        bail!("no rebase in progress");
    }

    let rb_dir = active_rebase_dir(git_dir)
        .ok_or_else(|| anyhow::anyhow!("internal: no rebase state directory"))?;
    let autostash_skip = read_autostash_oid(&rb_dir)?;
    let had_autostash_skip = autostash_skip.is_some();
    let backend_skip = load_rebase_backend(&rb_dir);

    let todo_content_skip = fs::read_to_string(rb_dir.join("todo"))?;
    let todo_lines_skip: Vec<&str> = todo_content_skip
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    let interactive_skip = rb_dir.join("interactive").exists();
    let current_hex_skip = fs::read_to_string(rb_dir.join("current")).unwrap_or_default();
    let current_hex_skip = current_hex_skip.trim();
    if !interactive_skip {
        if let Ok(skipped_oid) = ObjectId::from_hex(current_hex_skip) {
            let next_cmd =
                peek_next_rebase_flush_hint(&repo, &todo_lines_skip, 0, interactive_skip);
            record_rebase_in_rewritten_pending(git_dir, &rb_dir, &skipped_oid, next_cmd)?;
        }
    }
    let _ = fs::remove_file(rb_dir.join("stopped-sha"));

    // Clean up any conflict state
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    // Reset index and worktree to HEAD
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let obj = repo.odb.read(head_oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let mut index = Index::new();
        index.entries = entries;
        index.sort();
        let old_index = load_index(&repo)?;
        repo.write_index(&mut index)?;
        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(&repo, wt, &old_index, &index)?;
        }
    }

    // Advance past the current commit in the todo list
    // (replay_remaining reads todo and msgnum, so just advance msgnum or trim todo)
    // The todo was already trimmed when conflicts happened, so just continue
    replay_remaining(
        &repo,
        &rb_dir,
        autostash_skip,
        backend_skip,
        had_autostash_skip,
    )?;

    Ok(())
}

// ── --quit ──────────────────────────────────────────────────────────

fn do_quit() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    if !is_rebase_in_progress(git_dir) {
        bail!("no rebase in progress");
    }
    let _rb_dir = active_rebase_dir(git_dir)
        .ok_or_else(|| anyhow::anyhow!("internal: no rebase state directory"))?;
    cleanup_rebase_state(git_dir);
    // Like Git: `--quit` clears rebase state without popping the autostash; the WIP stays on
    // `refs/stash` for `stash pop`/`drop`.
    Ok(())
}

// ── --abort ─────────────────────────────────────────────────────────

fn do_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_rebase_in_progress(git_dir) {
        bail!("no rebase in progress");
    }

    let rb_dir = active_rebase_dir(git_dir)
        .ok_or_else(|| anyhow::anyhow!("internal: no rebase state directory"))?;

    let autostash_oid = read_autostash_oid(&rb_dir)?;

    // Read original HEAD and branch name
    let orig_head_hex = fs::read_to_string(rb_dir.join("orig-head"))?;
    let orig_head_hex = orig_head_hex.trim();
    let orig_head_oid = ObjectId::from_hex(orig_head_hex)?;

    let head_name = fs::read_to_string(rb_dir.join("head-name"))?;
    let head_name = head_name.trim().to_string();

    let ra = load_rebase_reflog_action(&rb_dir);
    let ident = reflog_identity(&repo);
    let cur_head = resolve_head(git_dir)?;
    let cur_oid = cur_head.oid().cloned().unwrap_or_else(diff::zero_oid);
    let abort_return = if head_name == "detached HEAD" {
        orig_head_oid.to_hex()
    } else {
        head_name.clone()
    };
    let abort_msg = format!("{ra} (abort): returning to {abort_return}");
    let _ = append_reflog(
        git_dir,
        "HEAD",
        &cur_oid,
        &orig_head_oid,
        &ident,
        &abort_msg,
        false,
    );

    // Restore HEAD
    if head_name != "detached HEAD" {
        // Update branch ref
        let ref_path = git_dir.join(&head_name);
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ref_path, format!("{}\n", orig_head_oid.to_hex()))?;
        // Re-attach HEAD
        fs::write(git_dir.join("HEAD"), format!("ref: {}\n", head_name))?;
    } else {
        fs::write(
            git_dir.join("HEAD"),
            format!("{}\n", orig_head_oid.to_hex()),
        )?;
    }

    // Restore index and worktree to orig HEAD
    let obj = repo.odb.read(&orig_head_oid)?;
    let commit = parse_commit(&obj.data)?;
    let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
    let mut index = Index::new();
    index.entries = entries;
    index.sort();

    let old_index = load_index(&repo)?;
    repo.write_index(&mut index)?;

    if let Some(wt) = &repo.work_tree {
        checkout_merged_index(&repo, wt, &old_index, &index)?;
    }

    if let Some(oid) = autostash_oid {
        let _ = stash::pop_autostash_if_top(&repo, &oid);
    }

    cleanup_rebase_state(git_dir);
    eprintln!("Rebase aborted.");

    Ok(())
}

// ── Cleanup ─────────────────────────────────────────────────────────

fn cleanup_rebase_state(git_dir: &Path) {
    let _ = fs::remove_dir_all(rebase_apply_dir(git_dir));
    let _ = fs::remove_dir_all(rebase_merge_dir(git_dir));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(git_dir.join("SQUASH_MSG"));
}

fn commit_message_unicode(commit: &CommitData) -> String {
    if let Some(raw) = &commit.raw_message {
        return crate::git_commit_encoding::decode_bytes(commit.encoding.as_deref(), raw);
    }
    commit.message.clone()
}

fn finalize_message_for_commit_encoding(
    unicode: String,
    config: &ConfigSet,
) -> (String, Option<String>, Option<Vec<u8>>) {
    let commit_enc = config
        .get("i18n.commitEncoding")
        .or_else(|| config.get("i18n.commitencoding"));
    let is_utf8 = match commit_enc.as_deref() {
        None => true,
        Some(e) => e.eq_ignore_ascii_case("utf-8") || e.eq_ignore_ascii_case("utf8"),
    };
    if is_utf8 {
        return (unicode, None, None);
    }
    let Some(label) = commit_enc else {
        return (unicode, None, None);
    };
    let Some(raw) = crate::git_commit_encoding::encode_unicode(&label, &unicode) else {
        return (unicode, None, None);
    };
    (unicode, Some(label), Some(raw))
}

fn transcoded_replayed_message(
    commit: &CommitData,
    config: &ConfigSet,
) -> (String, Option<String>, Option<Vec<u8>>) {
    finalize_message_for_commit_encoding(commit_message_unicode(commit), config)
}

fn write_rebase_conflict_message(
    git_dir: &Path,
    commit: &CommitData,
    config: &ConfigSet,
) -> Result<()> {
    let (unicode, _enc, raw_opt) = transcoded_replayed_message(commit, config);
    let merge_msg = git_dir.join("MERGE_MSG");
    let bytes = raw_opt.unwrap_or_else(|| unicode.into_bytes());
    fs::write(&merge_msg, &bytes)?;
    if rebase_merge_dir(git_dir).exists() {
        fs::write(rebase_merge_dir(git_dir).join("message"), bytes)?;
    }
    Ok(())
}

fn read_rebase_continue_message(
    git_dir: &Path,
    original: &CommitData,
    config: &ConfigSet,
) -> Result<(String, Option<String>, Option<Vec<u8>>)> {
    let rb = rebase_dir(git_dir);
    let from_state = rb.join("message");
    let bytes = if from_state.exists() {
        fs::read(&from_state)?
    } else {
        let merge_msg = git_dir.join("MERGE_MSG");
        if merge_msg.exists() {
            fs::read(&merge_msg)?
        } else {
            return Ok(transcoded_replayed_message(original, config));
        }
    };
    let enc_name = config
        .get("i18n.commitEncoding")
        .or_else(|| config.get("i18n.commitencoding"));
    let unicode = match enc_name.as_deref() {
        Some(e) if !e.eq_ignore_ascii_case("utf-8") && !e.eq_ignore_ascii_case("utf8") => {
            crate::git_commit_encoding::decode_bytes(Some(e), &bytes)
        }
        _ => String::from_utf8(bytes.clone()).unwrap_or_else(|_| {
            crate::git_commit_encoding::decode_bytes(enc_name.as_deref(), &bytes)
        }),
    };
    Ok(finalize_message_for_commit_encoding(unicode, config))
}

// ── Helpers (mirrored from revert.rs) ───────────────────────────────

fn load_index(repo: &Repository) -> Result<Index> {
    Ok(repo.load_index()?)
}

fn resolve_identity(config: &ConfigSet, kind: &str) -> Result<(String, String)> {
    let role = match kind {
        "AUTHOR" => IdentRole::Author,
        _ => IdentRole::Committer,
    };
    Ok((resolve_name(config, role)?, resolve_email(config, role)?))
}

fn format_ident(ident: &(String, String), now: time::OffsetDateTime) -> String {
    let (name, email) = ident;
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();

    let date_str = std::env::var("GIT_COMMITTER_DATE").ok();
    let timestamp = date_str
        .map(|d| super::commit::parse_date_to_git_timestamp(&d).unwrap_or(d))
        .unwrap_or_else(|| format!("{epoch} {hours:+03}{minutes:02}"));
    format!("{name} <{email}> {timestamp}")
}

fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            let sub = tree_to_index_entries(repo, &te.oid, &path)?;
            result.extend(sub);
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

fn tree_to_map(entries: Vec<IndexEntry>) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in entries {
        out.insert(e.path.clone(), e);
    }
    out
}

fn same_blob(a: &IndexEntry, b: &IndexEntry) -> bool {
    a.oid == b.oid && a.mode == b.mode
}

fn apply_ws_fix_to_index(repo: &Repository, index: &mut Index, rule: u32) -> Result<()> {
    for entry in &mut index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if entry.mode == MODE_SYMLINK || entry.mode == 0o160000 {
            continue;
        }
        let obj = match repo.odb.read(&entry.oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if grit_lib::merge_file::is_binary(&obj.data) {
            continue;
        }
        let fixed = fix_blob_bytes(&obj.data, rule);
        if fixed != obj.data {
            let new_oid = repo.odb.write(ObjectKind::Blob, &fixed)?;
            entry.oid = new_oid;
        }
    }
    Ok(())
}

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

struct RebaseMergeResult {
    index: Index,
    conflict_files: Vec<(Vec<u8>, Vec<u8>)>,
}

fn three_way_merge_with_content(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    conflict_ctx: &RebaseConflictContext,
) -> Result<RebaseMergeResult> {
    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut out = Index::new();
    let mut conflict_files: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    for path in all_paths {
        let b = base.get(&path);
        let o = ours.get(&path);
        let t = theirs.get(&path);

        match (b, o, t) {
            (_, Some(oe), Some(te)) if same_blob(oe, te) => {
                out.entries.push(oe.clone());
            }
            (Some(be), Some(oe), Some(te)) if same_blob(be, oe) => {
                out.entries.push(te.clone());
            }
            (Some(be), Some(oe), Some(te)) if same_blob(be, te) => {
                out.entries.push(oe.clone());
            }
            // Mode-only change: same blob OID on all three sides (Git tree can store 644 vs 755).
            (Some(be), Some(oe), Some(te))
                if be.oid == oe.oid
                    && oe.oid == te.oid
                    && (be.mode != te.mode || oe.mode != te.mode) =>
            {
                out.entries.push(te.clone());
            }
            // Submodule gitlinks: OIDs name commits in the submodule ODB, not blobs here.
            (Some(be), Some(oe), Some(te))
                if be.mode == 0o160000 && oe.mode == 0o160000 && te.mode == 0o160000 =>
            {
                if same_blob(oe, te) {
                    out.entries.push(oe.clone());
                } else if same_blob(be, oe) {
                    out.entries.push(te.clone());
                } else if same_blob(be, te) {
                    out.entries.push(oe.clone());
                } else if be.oid == oe.oid
                    && oe.oid == te.oid
                    && (be.mode != te.mode || oe.mode != te.mode)
                {
                    out.entries.push(te.clone());
                } else {
                    stage_entry(&mut out, be, 1);
                    stage_entry(&mut out, oe, 2);
                    stage_entry(&mut out, te, 3);
                }
            }
            (Some(be), Some(oe), Some(te)) => {
                content_merge_or_conflict(
                    repo,
                    &mut out,
                    &mut conflict_files,
                    &path,
                    be,
                    oe,
                    te,
                    conflict_ctx,
                )?;
            }
            (None, Some(oe), None) => {
                out.entries.push(oe.clone());
            }
            (None, None, Some(te)) => {
                out.entries.push(te.clone());
            }
            (None, Some(oe), Some(te)) if same_blob(oe, te) => {
                out.entries.push(oe.clone());
            }
            (None, Some(oe), Some(te)) => {
                stage_entry(&mut out, oe, 2);
                stage_entry(&mut out, te, 3);
            }
            (Some(_), None, None) => {}
            (Some(be), Some(oe), None) if same_blob(be, oe) => {}
            (Some(be), None, Some(te)) if same_blob(be, te) => {}
            (Some(be), Some(oe), None) => {
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, oe, 2);
            }
            (Some(be), None, Some(te)) => {
                stage_entry(&mut out, be, 1);
                stage_entry(&mut out, te, 3);
            }
            (None, None, None) => {}
        }
    }

    out.sort();
    Ok(RebaseMergeResult {
        index: out,
        conflict_files,
    })
}

fn content_merge_or_conflict(
    repo: &Repository,
    index: &mut Index,
    conflict_files: &mut Vec<(Vec<u8>, Vec<u8>)>,
    path: &[u8],
    base: &IndexEntry,
    ours: &IndexEntry,
    theirs: &IndexEntry,
    ctx: &RebaseConflictContext<'_>,
) -> Result<()> {
    if base.mode == 0o160000 || ours.mode == 0o160000 || theirs.mode == 0o160000 {
        stage_entry(index, base, 1);
        stage_entry(index, ours, 2);
        stage_entry(index, theirs, 3);
        return Ok(());
    }

    let base_obj = repo.odb.read(&base.oid)?;
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    if grit_lib::merge_file::is_binary(&base_obj.data)
        || grit_lib::merge_file::is_binary(&ours_obj.data)
        || grit_lib::merge_file::is_binary(&theirs_obj.data)
    {
        stage_entry(index, base, 1);
        stage_entry(index, ours, 2);
        stage_entry(index, theirs, 3);
        return Ok(());
    }

    let path_str = String::from_utf8_lossy(path);
    let base_label = ctx.label_base();
    let input = MergeInput {
        base: &base_obj.data,
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: ctx.label_ours(),
        label_base: &base_label,
        label_theirs: &path_str,
        favor: Default::default(),
        style: ctx.style(repo),
        marker_size: 7,
        diff_algorithm: None,
        ignore_all_space: false,
        ignore_space_change: false,
        ignore_space_at_eol: false,
        ignore_cr_at_eol: false,
    };

    let result = merge(&input)?;

    if result.conflicts > 0 {
        stage_entry(index, base, 1);
        stage_entry(index, ours, 2);
        stage_entry(index, theirs, 3);
        conflict_files.push((path.to_vec(), result.content));
    } else {
        let merged_oid = repo.odb.write(ObjectKind::Blob, &result.content)?;
        let mut entry = ours.clone();
        entry.oid = merged_oid;
        if base.mode == ours.mode && base.mode != theirs.mode {
            entry.mode = theirs.mode;
        }
        index.entries.push(entry);
    }

    Ok(())
}

fn write_rebase_conflict_files(
    work_tree: &Path,
    conflict_files: &[(Vec<u8>, Vec<u8>)],
) -> Result<()> {
    for (path, content) in conflict_files {
        let rel = String::from_utf8_lossy(path);
        let abs = work_tree.join(rel.as_ref());
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(abs, content)?;
    }
    Ok(())
}

fn checkout_merged_index(
    repo: &Repository,
    work_tree: &Path,
    old_index: &Index,
    index: &Index,
) -> Result<()> {
    let new_paths: HashSet<Vec<u8>> = index.entries.iter().map(|e| e.path.clone()).collect();

    for entry in &old_index.entries {
        if entry.stage() == 0 && !new_paths.contains(&entry.path) {
            let path_str = String::from_utf8_lossy(&entry.path).into_owned();
            let abs_path = work_tree.join(&path_str);
            if abs_path.exists() || abs_path.is_symlink() {
                if abs_path.is_dir() {
                    let _ = fs::remove_dir_all(&abs_path);
                } else {
                    let _ = fs::remove_file(&abs_path);
                }
                remove_empty_parent_dirs(work_tree, &abs_path);
            }
        }
    }

    let mut written = HashSet::new();

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if entry.stage() == 0 {
            write_entry_to_worktree(repo, &abs_path, entry)?;
            written.insert(entry.path.clone());
        } else if entry.stage() == 2 && !written.contains(&entry.path) {
            write_entry_to_worktree(repo, &abs_path, entry)?;
            written.insert(entry.path.clone());
        }
    }

    Ok(())
}

fn remove_empty_parent_dirs(work_tree: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

fn write_entry_to_worktree(repo: &Repository, abs_path: &Path, entry: &IndexEntry) -> Result<()> {
    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Gitlink (submodule) entries: ensure the directory exists but don't
    // try to check out content — the OID references a commit in the
    // submodule's own object store.
    if entry.mode == 0o160000 {
        if abs_path.is_file() || abs_path.is_symlink() {
            let _ = fs::remove_file(abs_path);
        } else if abs_path.is_dir() {
            let _ = fs::remove_dir_all(abs_path);
        }
        fs::create_dir_all(abs_path)?;
        return Ok(());
    }

    let obj = repo
        .odb
        .read(&entry.oid)
        .context("reading object for checkout")?;

    if entry.mode == MODE_SYMLINK {
        let target =
            String::from_utf8(obj.data).map_err(|_| anyhow::anyhow!("symlink not UTF-8"))?;
        if abs_path.exists() || abs_path.is_symlink() {
            let _ = fs::remove_file(abs_path);
        }
        std::os::unix::fs::symlink(target, abs_path)?;
    } else {
        if abs_path.is_dir() {
            fs::remove_dir_all(abs_path)?;
        }
        fs::write(abs_path, &obj.data)?;
        if entry.mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(abs_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(abs_path, perms)?;
        }
    }

    Ok(())
}
