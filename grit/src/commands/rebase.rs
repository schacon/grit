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
use std::path::{Path, PathBuf};
use std::process::Command;

use grit_lib::config::ConfigSet;
use grit_lib::diff::{self, count_changes, DiffEntry};
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::merge_base::{ancestor_closure, is_ancestor, merge_bases_first_vs_rest};
use grit_lib::merge_file::{merge, ConflictStyle, MergeInput};
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::patch_ids::compute_patch_id;
use grit_lib::refs::{append_reflog, list_refs, resolve_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, split_revision_token, OrderingMode, RevListOptions};
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::whitespace_rule::{fix_blob_bytes, parse_whitespace_rule, WS_DEFAULT_RULE};
use grit_lib::write_tree::write_tree_from_index;

use super::checkout::check_dirty_worktree;
use super::stash;
use crate::ident::{resolve_email, resolve_name, IdentRole};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RebaseBackend {
    Merge,
    Apply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RebaseTodoCmd {
    Pick,
    Edit,
}

enum ReplayExit {
    Complete,
    StoppedForEdit,
}

fn parse_rebase_todo_line(line: &str) -> Option<(RebaseTodoCmd, ObjectId)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let (cmd, rest) = if let Some(r) = t.strip_prefix("pick ") {
        (RebaseTodoCmd::Pick, r)
    } else if let Some(r) = t.strip_prefix("p ") {
        (RebaseTodoCmd::Pick, r)
    } else if let Some(r) = t.strip_prefix("edit ") {
        (RebaseTodoCmd::Edit, r)
    } else if let Some(r) = t.strip_prefix("e ") {
        (RebaseTodoCmd::Edit, r)
    } else if t.len() == 40 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return ObjectId::from_hex(t)
            .ok()
            .map(|oid| (RebaseTodoCmd::Pick, oid));
    } else {
        return None;
    };
    let hex = rest.split_whitespace().next()?;
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((cmd, ObjectId::from_hex(hex).ok()?))
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

    /// Keep commits that become empty (matches `git rebase --keep-empty`; hidden like upstream).
    #[arg(long = "keep-empty", hide = true)]
    pub keep_empty: bool,

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
    #[arg(long = "merge", short = 'm')]
    pub merge: bool,

    /// Use the apply backend for rebasing (accepted for compatibility).
    #[arg(long = "apply")]
    pub apply: bool,

    /// Rebase merge commits (accepted for compatibility; uses `rebase-merge/` state layout when set).
    #[arg(long = "rebase-merges", alias = "r")]
    pub rebase_merges: bool,

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
    #[arg(long = "update-refs")]
    pub update_refs: bool,

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

    /// Add a Signed-off-by trailer to each replayed commit (matches `git rebase --signoff`).
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// Do not add a Signed-off-by trailer (overrides `--signoff`, including from aliases).
    #[arg(long = "no-signoff")]
    pub no_signoff: bool,
}

/// Run the `rebase` command.
pub fn run(mut args: Args) -> Result<()> {
    validate_compat_options(&args)?;

    if args.no_signoff {
        args.signoff = false;
    }

    if args.rebase_merges {
        args.merge = true;
    }

    if args.root {
        if args.keep_base {
            bail!("options '--keep-base' and '--root' cannot be used together");
        }
        if args.fork_point {
            bail!("options '--root' and '--fork-point' cannot be used together");
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

fn validate_compat_options(args: &Args) -> Result<()> {
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
    Ok(())
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
    if args.apply {
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

/// Canonical SHA-1 of Git's empty tree (used for root rebases and parent-less commits).
const GIT_EMPTY_TREE_HEX: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

fn git_empty_tree_oid() -> Result<ObjectId> {
    ObjectId::from_hex(GIT_EMPTY_TREE_HEX)
        .map_err(|e| anyhow::anyhow!("invalid empty tree oid: {e}"))
}

/// Create a root commit with an empty tree and empty message (matches Git's `squash_onto` for `rebase --root` without `--onto`).
fn write_synthetic_root_commit(repo: &Repository) -> Result<ObjectId> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;
    let ident = format_ident(&committer, now);
    let commit_data = CommitData {
        tree: git_empty_tree_oid()?,
        parents: Vec::new(),
        author: ident.clone(),
        committer: ident,
        encoding: None,
        message: String::new(),
        raw_message: None,
    };
    let bytes = serialize_commit(&commit_data);
    repo.odb
        .write(ObjectKind::Commit, &bytes)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Whether `commit_oid` would replay as an empty patch during `rebase --root` onto `onto` (tree unchanged vs its parent base).
fn is_root_rebase_empty_commit(
    repo: &Repository,
    commit_oid: &ObjectId,
    onto: ObjectId,
) -> Result<bool> {
    let empty_tree = git_empty_tree_oid()?;
    let obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&obj.data)?;
    let parent_tree = if let Some(p) = commit.parents.first() {
        let pobj = repo.odb.read(p)?;
        let parent_commit = parse_commit(&pobj.data)?;
        parent_commit.tree
    } else {
        empty_tree
    };
    Ok(commit.tree == parent_tree && commit.tree == onto)
}

/// True when this rebase was started with `--signoff` (Git stores `.git/rebase-*/signoff`).
fn rebase_signoff_enabled(rb_dir: &Path) -> bool {
    rb_dir.join("signoff").exists()
}

/// Update `HEAD` after a successful pick. When the rebase was started on a branch (`head-name` is
/// a ref under `refs/`), advance that ref and keep `HEAD` symbolic; otherwise stay detached.
fn replay_tip_path(git_dir: &Path) -> PathBuf {
    git_dir.join("GRIT_REPLAY_HEAD")
}

fn write_rebase_head_after_pick(git_dir: &Path, rb_dir: &Path, new_oid: &ObjectId) -> Result<()> {
    let stored = fs::read_to_string(rb_dir.join("head-name")).unwrap_or_default();
    let head_name = stored.trim();
    if head_name.is_empty() || head_name == "detached HEAD" {
        // `--apply` stores `head-name` = `detached HEAD` but `orig-branch` names the branch being
        // rebased; advance it each pick so `HEAD` and the branch stay aligned (matches Git).
        if let Ok(ob) = fs::read_to_string(rb_dir.join("orig-branch")) {
            let r = ob.trim();
            if !r.is_empty() {
                let ref_path = git_dir.join(r);
                if let Some(parent) = ref_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&ref_path, format!("{}\n", new_oid.to_hex()))?;
                fs::write(git_dir.join("HEAD"), format!("ref: {r}\n"))?;
                let tip_line = format!("{}\n", new_oid.to_hex());
                let _ = fs::write(rb_dir.join("replay-head"), &tip_line);
                let _ = fs::write(replay_tip_path(git_dir), &tip_line);
                return Ok(());
            }
        }
        fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
        let tip_line = format!("{}\n", new_oid.to_hex());
        let _ = fs::write(rb_dir.join("replay-head"), &tip_line);
        let _ = fs::write(replay_tip_path(git_dir), &tip_line);
        return Ok(());
    }
    let ref_path = git_dir.join(head_name);
    if let Some(parent) = ref_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&ref_path, format!("{}\n", new_oid.to_hex()))?;
    fs::write(git_dir.join("HEAD"), format!("ref: {head_name}\n"))?;
    let tip_line = format!("{}\n", new_oid.to_hex());
    let _ = fs::write(rb_dir.join("replay-head"), &tip_line);
    let _ = fs::write(replay_tip_path(git_dir), &tip_line);
    Ok(())
}

/// Local branch refs (`refs/heads/*`) whose tip equals `oid`.
/// Append `Signed-off-by` using committer identity when rebase `--signoff` is active.
fn append_rebase_signoff_to_unicode(msg: &str, git_dir: &Path) -> Result<String> {
    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();
    let trailer = format!("Signed-off-by: {name} <{email}>");
    if msg.contains(&trailer) {
        return Ok(msg.to_owned());
    }
    let trimmed = msg.trim_end();
    Ok(format!("{trimmed}\n\n{trailer}\n"))
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

fn sequence_editor_cmd(config: &ConfigSet) -> Result<String> {
    std::env::var("GIT_SEQUENCE_EDITOR")
        .ok()
        .or_else(|| config.get("sequence.editor"))
        .or_else(|| std::env::var("GIT_EDITOR").ok())
        .or_else(|| config.get("core.editor"))
        .or_else(|| std::env::var("VISUAL").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Terminal is dumb, but EDITOR unset"))
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

fn run_interactive_rebase(
    repo: &Repository,
    git_dir: &Path,
    commits: &[ObjectId],
    config: &ConfigSet,
    autostash_oid: Option<&ObjectId>,
) -> Result<Vec<(ObjectId, RebaseTodoCmd)>> {
    use std::io::Write;
    let mut todo = String::new();
    for oid in commits {
        let obj = repo.odb.read(oid)?;
        let commit = parse_commit(&obj.data)?;
        let subject = commit.message.lines().next().unwrap_or("");
        todo.push_str(&format!("pick {} {}\n", oid.to_hex(), subject));
    }
    let tmp = tempfile::NamedTempFile::new().context("create temp file for rebase todo")?;
    tmp.as_file().write_all(todo.as_bytes())?;
    let path = tmp.path().to_owned();
    let editor = sequence_editor_cmd(config)?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$1\"", editor))
        .arg("sh")
        .arg(&path)
        .status()
        .context("failed to run sequence editor")?;
    if !status.success() {
        // If the editor failed without touching the tree, restore the autostash (matches Git).
        if worktree_matches_head(repo, git_dir)? {
            if let Some(oid) = autostash_oid {
                let _ = stash::pop_autostash_if_top(repo, oid);
            }
        }
        bail!("there was a problem with the editor");
    }
    let edited = fs::read_to_string(&path)?;
    let mut out = Vec::new();
    for line in edited.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let (cmd, rest) = if let Some(r) = t.strip_prefix("pick ") {
            (RebaseTodoCmd::Pick, r)
        } else if let Some(r) = t.strip_prefix("p ") {
            (RebaseTodoCmd::Pick, r)
        } else if let Some(r) = t.strip_prefix("edit ") {
            (RebaseTodoCmd::Edit, r)
        } else if let Some(r) = t.strip_prefix("e ") {
            (RebaseTodoCmd::Edit, r)
        } else {
            continue;
        };
        let hex: String = rest.split_whitespace().next().unwrap_or("").to_string();
        if hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            out.push((ObjectId::from_hex(&hex)?, cmd));
        }
    }
    Ok(out)
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
        let (onto, onto_label) = if let Some(ref onto_spec) = args.onto {
            let oid = resolve_revision(&repo, onto_spec)
                .with_context(|| format!("bad revision '{onto_spec}'"))?;
            (oid, onto_spec.clone())
        } else {
            let oid = write_synthetic_root_commit(&repo)?;
            (oid, oid.to_hex())
        };
        ("--root".to_owned(), onto, onto, onto_label)
    } else {
        let upstream_spec = args.upstream.as_deref().unwrap_or("HEAD").to_owned();
        let up_oid = resolve_revision(&repo, &upstream_spec)
            .with_context(|| format!("bad revision '{upstream_spec}'"))?;
        let (onto, onto_label) = if let Some(ref onto_spec) = args.onto {
            let oid = resolve_revision(&repo, onto_spec)
                .with_context(|| format!("bad revision '{onto_spec}'"))?;
            (oid, onto_spec.clone())
        } else if args.keep_base {
            let oid = find_merge_base(&repo, up_oid, head_oid_early).unwrap_or(up_oid);
            (oid, upstream_spec.clone())
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
    // Git sets `REBASE_FORCE` when `--signoff` is given so `can_fast_forward` does not exit early:
    // commits must be replayed to add/remove trailers (t3428-rebase-signoff).
    let allow_preemptive_ff =
        !args.interactive && args.exec.is_none() && !whitespace_forces_replay && !args.signoff;

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

    let reapply_cherry_picks =
        args.reapply_cherry_picks || (args.keep_base && !args.no_reapply_cherry_picks);
    let commit_oids = if args.root {
        let apply_patch_dedup = args.onto.is_some();
        let list = collect_commits_for_root_rebase(&repo, head_oid, onto_oid, apply_patch_dedup)?;
        if args.keep_empty {
            list
        } else {
            let mut kept = Vec::new();
            for oid in list {
                if !is_root_rebase_empty_commit(&repo, &oid, onto_oid)? {
                    kept.push(oid);
                }
            }
            kept
        }
    } else {
        collect_rebase_todo_commits(&repo, head_oid, upstream_oid, !reapply_cherry_picks)?
    };

    let mut todo_items: Vec<(ObjectId, RebaseTodoCmd)> = commit_oids
        .into_iter()
        .map(|oid| (oid, RebaseTodoCmd::Pick))
        .collect();

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

    if args.interactive {
        if todo_items.is_empty() {
            print_branch_up_to_date(&head);
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
        let pre_editor_len = todo_items.len();
        let oids_only: Vec<ObjectId> = todo_items.iter().map(|(o, _)| *o).collect();
        todo_items =
            run_interactive_rebase(&repo, git_dir, &oids_only, &config, autostash_oid.as_ref())?;
        if todo_items.is_empty() {
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
        // Git runs the sequence editor then proceeds with the replay; upstream tests expect
        // `rebase -i` to apply the edited todo.
    }

    if !args.no_ff && todo_items.is_empty() {
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

    if todo_items.is_empty() {
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

    // Git's `--apply` backend records `head-name` as `detached HEAD` even when the branch being
    // rebased is checked out; the branch ref is advanced only in `(finish)` using `orig-head`.
    let head_name = match (&head, backend) {
        (HeadState::Branch { refname, .. }, RebaseBackend::Merge) => refname.clone(),
        _ => "detached HEAD".to_string(),
    };
    fs::write(rb_dir.join("head-name"), &head_name)?;
    if let HeadState::Branch { refname, .. } = &head {
        fs::write(rb_dir.join("orig-branch"), format!("{refname}\n"))?;
    } else if let Ok(raw) = fs::read_to_string(git_dir.join("HEAD")) {
        let t = raw.trim();
        if let Some(r) = t.strip_prefix("ref: ") {
            let r = r.trim();
            if r.starts_with("refs/heads/") {
                let _ = fs::write(rb_dir.join("orig-branch"), format!("{r}\n"));
            }
        }
    }
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

    let todo_lines: Vec<String> = todo_items
        .iter()
        .map(|(oid, cmd)| {
            let verb = match cmd {
                RebaseTodoCmd::Pick => "pick",
                RebaseTodoCmd::Edit => "edit",
            };
            format!("{verb} {}", oid.to_hex())
        })
        .collect();
    let total = todo_lines.len();
    fs::write(rb_dir.join("todo"), todo_lines.join("\n") + "\n")?;
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

    if args.signoff {
        fs::write(rb_dir.join("signoff"), "--signoff\n")?;
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

    let _ = replay_remaining(&repo, &rb_dir, autostash_oid, backend, had_rebase_autostash)?;

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

/// Find the merge-base of two commits.  Returns `None` when there is no
/// common ancestor.
fn find_merge_base(repo: &Repository, a: ObjectId, b: ObjectId) -> Option<ObjectId> {
    grit_lib::merge_base::merge_bases_first_vs_rest(repo, a, &[b])
        .ok()
        .and_then(|bases| bases.into_iter().next())
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
///
/// When `apply_patch_dedup` is false (Git's implicit squash root for `rebase --root` without `--onto`),
/// skip patch-id deduplication: the synthetic root matches empty commits on the branch (t3428).
fn collect_commits_for_root_rebase(
    repo: &Repository,
    head: ObjectId,
    onto: ObjectId,
    apply_patch_dedup: bool,
) -> Result<Vec<ObjectId>> {
    let range = format!("{}..{}", onto.to_hex(), head.to_hex());
    let (positive, negative) = split_revision_token(&range);
    let mut opts = RevListOptions::default();
    opts.first_parent = true;
    opts.ordering = OrderingMode::Default;
    opts.reverse = true;
    let listed = rev_list(repo, &positive, &negative, &opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    if apply_patch_dedup {
        filter_redundant_patch_commits(repo, onto, &listed.commits)
    } else {
        Ok(listed.commits)
    }
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
) -> Result<ReplayExit> {
    let git_dir = &repo.git_dir;
    let ra = load_rebase_reflog_action(rb_dir);
    let ident = reflog_identity(repo);

    let todo_content = fs::read_to_string(rb_dir.join("todo"))?;
    let todo_lines: Vec<&str> = todo_content.lines().filter(|l| !l.is_empty()).collect();
    let parsed: Vec<(RebaseTodoCmd, ObjectId)> = todo_lines
        .iter()
        .filter_map(|l| parse_rebase_todo_line(l))
        .collect();
    let _total: usize = fs::read_to_string(rb_dir.join("end"))?.trim().parse()?;
    let msgnum: usize = fs::read_to_string(rb_dir.join("msgnum"))?.trim().parse()?;

    let rewind_marker = rb_dir.join("rewind-notice");
    if !rewind_marker.exists() && !parsed.is_empty() {
        println!("First, rewinding head to replay your work on top of it...");
        let _ = fs::write(&rewind_marker, "");
    }

    for i in (msgnum - 1)..parsed.len() {
        let (cmd, commit_oid) = parsed[i];
        let commit_hex = commit_oid.to_hex();

        // Update state
        fs::write(rb_dir.join("current"), &commit_hex)?;
        fs::write(rb_dir.join("msgnum"), (i + 1).to_string())?;
        fs::write(rb_dir.join("next"), (i + 1).to_string())?;

        // Read HEAD before cherry-pick for reflog
        let old_head = resolve_head(git_dir)?
            .oid()
            .cloned()
            .unwrap_or_else(diff::zero_oid);

        let pick_backend = load_rebase_backend(rb_dir);
        match cherry_pick_for_rebase(repo, rb_dir, &commit_oid, pick_backend) {
            Ok(()) => {
                let head = resolve_head(git_dir)?;
                let new_oid = *head
                    .oid()
                    .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                let obj = repo.odb.read(&commit_oid)?;
                let commit = parse_commit(&obj.data)?;
                let root_rebase = rb_dir.join("root").exists();
                let msg_for_log = message_for_root_replayed_commit(repo, &commit, root_rebase);
                let subject = msg_for_log.lines().next().unwrap_or("");
                eprintln!("Applying: {}", subject);
                let msg = format!("{ra} (pick): {subject}");
                let _ = append_reflog(git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false);

                if cmd == RebaseTodoCmd::Edit {
                    let remaining: Vec<String> = todo_lines[i + 1..]
                        .iter()
                        .map(|s| (*s).to_string())
                        .collect();
                    fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                    fs::write(rb_dir.join("msgnum"), "1")?;
                    fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                    let _ = fs::write(rb_dir.join("awaiting_amend"), "1\n");
                    eprintln!(
                        "Stopped at {commit_hex}...  {subject}\n\
                         You can amend the commit now, with\n\n\
                           git commit --amend '-S'\n\n\
                         Once you are satisfied with your changes, run\n\n\
                           git rebase --continue"
                    );
                    return Ok(ReplayExit::StoppedForEdit);
                }

                // Run --exec command if present
                if let Ok(exec_cmd) = fs::read_to_string(rb_dir.join("exec")) {
                    let exec_cmd = exec_cmd.trim();
                    if !exec_cmd.is_empty() {
                        eprintln!("Executing: {}", exec_cmd);
                        let status = std::process::Command::new("sh")
                            .arg("-c")
                            .arg(exec_cmd)
                            .current_dir(
                                repo.work_tree.as_deref().unwrap_or_else(|| Path::new(".")),
                            )
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
                            // Save remaining todo for --continue
                            let remaining: Vec<String> = todo_lines[i + 1..]
                                .iter()
                                .map(|s| (*s).to_string())
                                .collect();
                            fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                            fs::write(rb_dir.join("msgnum"), "1")?;
                            fs::write(rb_dir.join("end"), remaining.len().to_string())?;
                            std::process::exit(code);
                        }
                    }
                }
            }
            Err(_e) => {
                // Conflicts — leave state for --continue (current commit stays first in todo)
                let remaining: Vec<String> =
                    todo_lines[i..].iter().map(|s| (*s).to_string()).collect();
                fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                fs::write(rb_dir.join("msgnum"), "1")?;
                fs::write(rb_dir.join("end"), remaining.len().to_string())?;

                let obj = repo.odb.read(&commit_oid)?;
                let commit = parse_commit(&obj.data)?;
                let root_rebase = rb_dir.join("root").exists();
                let msg_for_log = message_for_root_replayed_commit(repo, &commit, root_rebase);
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

    // Align `refs/heads/<orig-branch>` with the current replay tip (`HEAD`) before `(finish)`.
    if let Ok(ob) = fs::read_to_string(rb_dir.join("orig-branch")) {
        let r = ob.trim();
        if !r.is_empty() {
            if let Some(tip) = resolve_head(git_dir)?.oid().cloned() {
                let ref_path = git_dir.join(r);
                if let Some(parent) = ref_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&ref_path, format!("{}\n", tip.to_hex()))?;
                fs::write(git_dir.join("HEAD"), format!("ref: {r}\n"))?;
            }
        }
    }

    // Rebase complete — restore branch ref
    finish_rebase(repo, rb_dir, autostash_oid, backend, had_rebase_autostash)?;
    Ok(ReplayExit::Complete)
}

/// Cherry-pick a single commit onto current HEAD for rebase purposes.
fn cherry_pick_for_rebase(
    repo: &Repository,
    rb_dir: &Path,
    commit_oid: &ObjectId,
    backend: RebaseBackend,
) -> Result<()> {
    let git_dir = &repo.git_dir;

    let commit_obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let config = ConfigSet::load(Some(git_dir), true)?;

    // Parent tree (base for the cherry-pick). Root commits use Git's empty tree as base.
    let parent_tree_oid = if let Some(parent_oid) = commit.parents.first() {
        let parent_obj = repo.odb.read(parent_oid)?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        parent_commit.tree
    } else {
        git_empty_tree_oid()?
    };

    // Commit's tree (theirs — the changes we want)
    let commit_tree_oid = commit.tree;

    // HEAD tree (ours — the current state)
    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD is unborn during rebase"))?
        .to_owned();
    let head_obj = repo.odb.read(&head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let head_tree_oid = head_commit.tree;

    // Already at the picked commit's parent tip — tree matches without a merge, but still record a
    // new commit whose parent is the *current* replay tip (not the original parent OID). Using the
    // original `commit_oid` here would rewind `HEAD` off the rebased history (breaks t3428).
    //
    // The merge backend (`rebase -m`) must not take this path: Git always runs the merge machinery
    // so conflicts are reported (t3428). The apply backend may fast-path when HEAD still matches
    // the patch's parent OID in the object database.
    if backend == RebaseBackend::Apply {
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
                let root_rebase = rb_dir.join("root").exists();
                let now = time::OffsetDateTime::now_utc();
                let committer = resolve_identity(&config, "COMMITTER")?;
                let (message, encoding, raw_message) = if root_rebase {
                    let msg = message_for_root_replayed_commit(repo, &commit, true);
                    (msg, commit.encoding.clone(), None)
                } else {
                    transcoded_replayed_message(&commit, &config)
                };
                let (message, encoding, raw_message) = apply_rebase_signoff_to_commit_message(
                    git_dir,
                    &rb_dir,
                    &config,
                    message,
                    encoding,
                    raw_message,
                )?;
                let commit_data = CommitData {
                    tree: commit_tree_oid,
                    parents: vec![head_oid],
                    author: commit.author.clone(),
                    committer: format_ident(&committer, now),
                    encoding,
                    message,
                    raw_message,
                };
                let commit_bytes = serialize_commit(&commit_data);
                let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
                write_rebase_head_after_pick(git_dir, rb_dir, &new_oid)?;
                return Ok(());
            }
        }
    }

    // Three-way merge: base=parent_tree, ours=HEAD_tree, theirs=commit_tree
    let ws_fix_rule = load_ws_fix_rule_from_rebase_state(git_dir);
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

    let root_rebase = rb_dir.join("root").exists();

    if has_conflicts {
        let _ = grit_lib::rerere::repo_rerere(repo, grit_lib::rerere::RerereAutoupdate::FromConfig);
        write_rebase_conflict_message(git_dir, &commit, &config)?;
        if backend == RebaseBackend::Merge {
            append_conflicts_hint_to_merge_msg(git_dir, &merged_index)?;
            let _ = fs::write(
                git_dir.join("CHERRY_PICK_HEAD"),
                format!("{}\n", commit_oid.to_hex()),
            );
        }
        bail!("conflicts during cherry-pick of {}", commit_oid.to_hex());
    }

    // Create the rebased commit, preserving the original author
    let tree_oid = write_tree_from_index(&repo.odb, &merged_index, "")?;

    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    let (message, encoding, raw_message) = if root_rebase {
        let msg = message_for_root_replayed_commit(repo, &commit, true);
        (msg, commit.encoding.clone(), None)
    } else {
        transcoded_replayed_message(&commit, &config)
    };
    let (message, encoding, raw_message) = apply_rebase_signoff_to_commit_message(
        git_dir,
        &rb_dir,
        &config,
        message,
        encoding,
        raw_message,
    )?;
    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid],
        author: commit.author.clone(), // preserve original author
        committer: format_ident(&committer, now),
        encoding,
        message,
        raw_message,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    write_rebase_head_after_pick(git_dir, rb_dir, &new_oid)?;

    Ok(())
}

/// Finish the rebase: point the original branch at the new HEAD.
fn finish_rebase(
    repo: &Repository,
    rb_dir: &Path,
    autostash_oid: Option<ObjectId>,
    _backend_arg: RebaseBackend,
    had_rebase_autostash: bool,
) -> Result<()> {
    let git_dir = &repo.git_dir;
    let backend = load_rebase_backend(rb_dir);

    let head_name = fs::read_to_string(rb_dir.join("head-name"))?;
    let head_name = head_name.trim();

    let onto_hex = fs::read_to_string(rb_dir.join("onto"))?;
    let onto_hex = onto_hex.trim();
    let onto_oid = ObjectId::from_hex(onto_hex)?;

    let orig_head_hex = fs::read_to_string(rb_dir.join("orig-head"))?;
    let orig_head_oid = ObjectId::from_hex(orig_head_hex.trim())?;

    let ra = load_rebase_reflog_action(rb_dir);
    let ident = reflog_identity(repo);

    let orig_branch_hint = fs::read_to_string(rb_dir.join("orig-branch"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let head = resolve_head(git_dir)?;
    let head_oid_from_head = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

    let mut new_tip = head_oid_from_head;
    for p in [replay_tip_path(git_dir), rb_dir.join("replay-head")] {
        if let Ok(rh) = fs::read_to_string(&p) {
            if let Ok(t) = ObjectId::from_hex(rh.trim()) {
                new_tip = t;
                break;
            }
        }
    }
    if new_tip == head_oid_from_head {
        if let Some(ref r) = orig_branch_hint {
            if let Ok(t) = resolve_ref(git_dir, r) {
                new_tip = t;
            }
        }
    }

    let autostash_oid_finish = autostash_oid.or_else(|| read_autostash_oid(rb_dir).ok().flatten());
    let had_autostash_finish = had_rebase_autostash || autostash_oid_finish.is_some();

    let finish_branch_ref: Option<String> = if let Some(r) = orig_branch_hint
        .clone()
        .filter(|x| resolve_ref(git_dir, x).is_ok())
    {
        fs::write(git_dir.join("HEAD"), format!("ref: {r}\n"))?;
        Some(r)
    } else if head_name != "detached HEAD" {
        Some(head_name.to_string())
    } else if let HeadState::Branch { refname, .. } = &head {
        Some(refname.clone())
    } else {
        None
    };

    let success_target = finish_branch_ref.as_deref().unwrap_or("HEAD");

    if let Some(ref branch_ref) = finish_branch_ref {
        let ref_path = git_dir.join(branch_ref);
        let old_branch_oid = fs::read_to_string(&ref_path)
            .ok()
            .and_then(|s| ObjectId::from_hex(s.trim()).ok())
            .unwrap_or(new_tip);

        let finish_branch = format!("{ra} (finish): {branch_ref} onto {}", onto_oid.to_hex());
        let finish_head = format!("{ra} (finish): returning to {branch_ref}");
        let _ = append_reflog(
            git_dir,
            branch_ref,
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
        fs::write(git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;
    } else {
        let mut stale = Vec::new();
        for (name, tip) in list_refs(git_dir, "refs/heads/")? {
            if tip == orig_head_oid {
                stale.push(name);
            }
        }
        if let Ok(ob) = fs::read_to_string(rb_dir.join("orig-branch")) {
            let r = ob.trim();
            if !r.is_empty() {
                let ref_path = git_dir.join(r);
                if let Ok(s) = fs::read_to_string(&ref_path) {
                    if let Ok(oid) = ObjectId::from_hex(s.trim()) {
                        if oid == orig_head_oid && !stale.iter().any(|x| x == r) {
                            stale.push(r.to_string());
                        }
                    }
                }
            }
        }
        if stale.is_empty() {
            if let Ok(ob) = fs::read_to_string(rb_dir.join("orig-branch")) {
                let r = ob.trim();
                if !r.is_empty() && git_dir.join(r).exists() && !stale.iter().any(|x| x == r) {
                    stale.push(r.to_string());
                }
            }
        }
        stale.sort();
        for refname in &stale {
            let ref_path = git_dir.join(refname);
            let old_branch_oid = fs::read_to_string(&ref_path)
                .ok()
                .and_then(|s| ObjectId::from_hex(s.trim()).ok())
                .unwrap_or(new_tip);
            let finish_branch = format!("{ra} (finish): {refname} onto {}", onto_oid.to_hex());
            let _ = append_reflog(
                git_dir,
                refname,
                &old_branch_oid,
                &new_tip,
                &ident,
                &finish_branch,
                false,
            );
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", new_tip.to_hex()))?;
        }
        if stale.len() == 1 {
            let refname = stale[0].as_str();
            let finish_head = format!("{ra} (finish): returning to {refname}");
            let _ = append_reflog(
                git_dir,
                "HEAD",
                &new_tip,
                &new_tip,
                &ident,
                &finish_head,
                false,
            );
            fs::write(git_dir.join("HEAD"), format!("ref: {refname}\n"))?;
        }
    }

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

    // Check for unresolved conflicts
    let index = load_index(&repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        bail!(
            "error: commit is not possible because you have unmerged files\n\
             hint: fix conflicts and then run 'grit rebase --continue'"
        );
    }

    if rb_dir.join("awaiting_amend").exists() {
        let _ = fs::remove_file(rb_dir.join("awaiting_amend"));
        let _ = replay_remaining(
            &repo,
            &rb_dir,
            autostash_continue,
            backend_continue,
            had_autostash_continue,
        )?;
        return Ok(());
    }

    // Commit the current cherry-pick
    let current_hex = fs::read_to_string(rb_dir.join("current"))?;
    let current_hex = current_hex.trim();
    let current_oid = ObjectId::from_hex(current_hex)?;

    let commit_obj = repo.odb.read(&current_oid)?;
    let original_commit = parse_commit(&commit_obj.data)?;

    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

    let new_oid = if backend_continue == RebaseBackend::Merge {
        let exe = std::env::current_exe().context("cannot determine grit executable")?;
        let status = Command::new(&exe)
            .arg("commit")
            .arg("--cleanup=strip")
            .status()
            .context("failed to run grit commit for rebase --continue")?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        resolve_head(git_dir)?
            .oid()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("HEAD has no OID after commit"))?
    } else {
        let config = ConfigSet::load(Some(git_dir), true)?;
        let (message, encoding, raw_message) =
            read_rebase_continue_message(git_dir, &original_commit, &config)?;
        let (message, encoding, raw_message) = apply_rebase_signoff_to_commit_message(
            git_dir,
            &rb_dir,
            &config,
            message,
            encoding,
            raw_message,
        )?;

        let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
        let now = time::OffsetDateTime::now_utc();
        let committer = resolve_identity(&config, "COMMITTER")?;

        let commit_data = CommitData {
            tree: tree_oid,
            parents: vec![head_oid.clone()],
            author: original_commit.author.clone(),
            committer: format_ident(&committer, now),
            encoding,
            message,
            raw_message,
        };

        let commit_bytes = serialize_commit(&commit_data);
        repo.odb.write(ObjectKind::Commit, &commit_bytes)?
    };

    write_rebase_head_after_pick(git_dir, &rb_dir, &new_oid)?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(rb_dir.join("message"));

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

    // The resolved commit was the first line of `todo`; drop it before replaying the rest.
    let todo_path = rb_dir.join("todo");
    let todo_raw = fs::read_to_string(&todo_path)?;
    let mut rest_lines: Vec<&str> = todo_raw.lines().filter(|l| !l.is_empty()).collect();
    if !rest_lines.is_empty() {
        rest_lines.remove(0);
    }
    fs::write(&todo_path, rest_lines.join("\n") + "\n")?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("end"), rest_lines.len().to_string())?;

    let _ = replay_remaining(
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
    let _ = replay_remaining(
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
    let _ = fs::remove_file(replay_tip_path(git_dir));
    let _ = fs::remove_file(rebase_apply_dir(git_dir).join("awaiting_amend"));
    let _ = fs::remove_file(rebase_merge_dir(git_dir).join("awaiting_amend"));
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

/// Append Git-style `# Conflicts:` lines for unmerged index paths (sequencer `append_conflicts_hint`).
fn append_conflicts_hint_to_merge_msg(git_dir: &Path, index: &Index) -> Result<()> {
    let merge_msg = git_dir.join("MERGE_MSG");
    let mut out = if merge_msg.exists() {
        fs::read_to_string(&merge_msg)?
    } else {
        String::new()
    };
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str("# Conflicts:\n");
    let mut seen_paths: BTreeSet<Vec<u8>> = BTreeSet::new();
    for e in &index.entries {
        if e.stage() == 0 {
            continue;
        }
        if !seen_paths.insert(e.path.clone()) {
            continue;
        }
        let p = String::from_utf8_lossy(trim_trailing_index_path_slash(&e.path));
        out.push_str("#\t");
        out.push_str(&p);
        out.push('\n');
    }
    let out_bytes = out.into_bytes();
    fs::write(&merge_msg, &out_bytes)?;
    if rebase_merge_dir(git_dir).exists() {
        fs::write(rebase_merge_dir(git_dir).join("message"), &out_bytes)?;
    }
    Ok(())
}

fn trim_trailing_index_path_slash(path: &[u8]) -> &[u8] {
    if path.last() == Some(&b'/') {
        &path[..path.len() - 1]
    } else {
        path
    }
}

fn write_rebase_conflict_message(
    git_dir: &Path,
    commit: &CommitData,
    config: &ConfigSet,
) -> Result<()> {
    let (unicode, _enc, raw_opt) = transcoded_replayed_message(commit, config);
    let rb_dir = rebase_dir(git_dir);
    let unicode = if rebase_signoff_enabled(&rb_dir) {
        append_rebase_signoff_to_unicode(&unicode, git_dir)?
    } else {
        unicode
    };
    let merge_msg = git_dir.join("MERGE_MSG");
    let bytes = raw_opt.unwrap_or_else(|| unicode.into_bytes());
    fs::write(&merge_msg, &bytes)?;
    if rebase_merge_dir(git_dir).exists() {
        fs::write(rebase_merge_dir(git_dir).join("message"), bytes)?;
    }
    Ok(())
}

/// Apply `--signoff` to a replayed commit message, preserving non-UTF-8 encoding when configured.
fn apply_rebase_signoff_to_commit_message(
    git_dir: &Path,
    rb_dir: &Path,
    config: &ConfigSet,
    message: String,
    encoding: Option<String>,
    raw_message: Option<Vec<u8>>,
) -> Result<(String, Option<String>, Option<Vec<u8>>)> {
    if !rebase_signoff_enabled(rb_dir) {
        return Ok((message, encoding, raw_message));
    }
    let unicode = if let Some(raw) = &raw_message {
        crate::git_commit_encoding::decode_bytes(encoding.as_deref(), raw)
    } else {
        message.clone()
    };
    let appended = append_rebase_signoff_to_unicode(&unicode, git_dir)?;
    Ok(finalize_message_for_commit_encoding(appended, config))
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
        } else if abs_path.is_dir() && abs_path.join(".git").exists() {
            return Ok(());
        } else if abs_path.is_dir() {
            let _ = fs::remove_dir_all(abs_path);
        }
        let _ = fs::create_dir_all(abs_path);
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
