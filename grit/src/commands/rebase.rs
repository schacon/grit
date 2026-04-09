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
use grit_lib::refs::append_reflog;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, split_revision_token, OrderingMode, RevListOptions};
use grit_lib::rev_parse::resolve_revision;
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

    /// Edit the todo list of an in-progress interactive rebase.
    #[arg(long = "edit-todo")]
    pub edit_todo: bool,

    /// Show the patch being applied (accepted for compatibility).
    #[arg(long = "show-current-patch")]
    pub show_current_patch: bool,
}

/// Run the `rebase` command.
pub fn run(mut args: Args) -> Result<()> {
    validate_compat_options(&args)?;

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
    if args.edit_todo {
        return do_edit_todo();
    }
    if args.show_current_patch {
        return Ok(());
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

/// Basename of the interactive rebase todo file inside `rebase-merge/` (matches Git).
fn rebase_todo_basename() -> &'static str {
    "git-rebase-todo"
}

fn default_rebase_todo_path(rb_dir: &Path) -> PathBuf {
    rb_dir.join(rebase_todo_basename())
}

/// Resolved path for the rebase todo file, honoring `GIT_REBASE_TODO` like Git.
fn resolve_rebase_todo_path(repo: &Repository, rb_dir: &Path) -> PathBuf {
    let Ok(raw) = std::env::var("GIT_REBASE_TODO") else {
        return default_rebase_todo_path(rb_dir);
    };
    let s = raw.trim();
    if s.is_empty() {
        return default_rebase_todo_path(rb_dir);
    }
    let p = Path::new(s);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let s = s.replace('\\', "/");
    if let Some(rest) = s.strip_prefix(".git/") {
        return repo.git_dir.join(rest);
    }
    if let Some(wt) = repo.work_tree.as_deref() {
        wt.join(p)
    } else {
        repo.git_dir.join(p)
    }
}

#[derive(Clone, Debug)]
enum RebaseTodoCommand {
    Pick,
    Reword,
    Squash,
    Fixup,
}

#[derive(Clone, Debug)]
struct RebaseTodoItem {
    command: RebaseTodoCommand,
    commit: ObjectId,
    /// Trailing commentary after the object name (subject, etc.).
    rest: String,
}

fn parse_rebase_todo_line(repo: &Repository, line: &str) -> Result<Option<RebaseTodoItem>> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let mut parts = t.split_whitespace();
    let cmd_word = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty todo line"))?;
    let command = match cmd_word.to_ascii_lowercase().as_str() {
        "pick" | "p" => RebaseTodoCommand::Pick,
        "reword" | "r" => RebaseTodoCommand::Reword,
        "squash" | "s" => RebaseTodoCommand::Squash,
        "fixup" | "f" => RebaseTodoCommand::Fixup,
        _ => {
            return Ok(None);
        }
    };
    let oid_token = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing commit for {cmd_word}"))?;
    let commit = resolve_revision(repo, oid_token)
        .with_context(|| format!("could not parse '{oid_token}'"))?;
    let rest: String = parts.collect::<Vec<_>>().join(" ");
    Ok(Some(RebaseTodoItem {
        command,
        commit,
        rest,
    }))
}

fn format_rebase_todo_line(item: &RebaseTodoItem) -> String {
    let verb = match item.command {
        RebaseTodoCommand::Pick => "pick",
        RebaseTodoCommand::Reword => "reword",
        RebaseTodoCommand::Squash => "squash",
        RebaseTodoCommand::Fixup => "fixup",
    };
    if item.rest.is_empty() {
        format!("{} {}", verb, item.commit.to_hex())
    } else {
        format!("{} {} {}", verb, item.commit.to_hex(), item.rest)
    }
}

fn read_rebase_todo_file(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(content.lines().map(|s| s.to_string()).collect())
}

fn write_rebase_todo_file(path: &Path, lines: &[String]) -> Result<()> {
    let mut out = lines.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn todo_lines_to_script(lines: &[String]) -> String {
    lines.join("\n") + if lines.is_empty() { "" } else { "\n" }
}

fn parse_todo_script_to_items(repo: &Repository, script: &str) -> Result<Vec<RebaseTodoItem>> {
    let mut out = Vec::new();
    for line in script.lines() {
        if let Some(item) = parse_rebase_todo_line(repo, line)? {
            out.push(item);
        }
    }
    Ok(out)
}

fn inject_exec_after_each_pick(script: &str, exec_cmd: &str) -> String {
    let exec_cmd = exec_cmd.trim();
    if exec_cmd.is_empty() {
        return script.to_string();
    }
    let mut buf = String::new();
    for line in script.lines() {
        buf.push_str(line);
        buf.push('\n');
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("pick ")
            || lower.starts_with("p ")
            || lower.starts_with("reword ")
            || lower.starts_with("r ")
        {
            buf.push_str(&format!("exec {exec_cmd}\n"));
        }
    }
    buf
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

fn usable_editor_value(s: Option<String>) -> Option<String> {
    let s = s?.trim().to_string();
    if s.is_empty() || s == ":" {
        return None;
    }
    Some(s)
}

fn sequence_editor_cmd(config: &ConfigSet) -> Result<String> {
    usable_editor_value(std::env::var("GIT_SEQUENCE_EDITOR").ok())
        .or_else(|| usable_editor_value(config.get("sequence.editor")))
        .or_else(|| usable_editor_value(std::env::var("GIT_EDITOR").ok()))
        .or_else(|| usable_editor_value(config.get("core.editor")))
        .or_else(|| usable_editor_value(std::env::var("VISUAL").ok()))
        .or_else(|| usable_editor_value(std::env::var("EDITOR").ok()))
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
    initial_script: &str,
    config: &ConfigSet,
    autostash_oid: Option<&ObjectId>,
) -> Result<String> {
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new().context("create temp file for rebase todo")?;
    tmp.as_file()
        .write_all(initial_script.as_bytes())
        .context("write temp rebase todo")?;
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
        if worktree_matches_head(repo, git_dir)? {
            if let Some(oid) = autostash_oid {
                let _ = stash::pop_autostash_if_top(repo, oid);
            }
        }
        bail!("there was a problem with the editor");
    }
    Ok(fs::read_to_string(&path)?)
}

fn count_non_comment_todo_lines(script: &str) -> usize {
    script
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .count()
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
        if let Some(onto_spec) = args.onto.as_deref() {
            let onto = resolve_revision(&repo, onto_spec)
                .with_context(|| format!("bad revision '{onto_spec}'"))?;
            ("--root".to_owned(), onto, onto, onto_spec.to_owned())
        } else {
            let onto = squash_onto_for_root_without_onto(&repo)?;
            let label = onto.to_hex();
            ("--root".to_owned(), onto, onto, label)
        }
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

    let reapply_cherry_picks =
        args.reapply_cherry_picks || (args.keep_base && !args.no_reapply_cherry_picks);
    let collected_commits = if args.root {
        collect_commits_for_root_rebase(&repo, head_oid, onto_oid)?
    } else {
        collect_rebase_todo_commits(
            &repo,
            head_oid,
            upstream_oid,
            !reapply_cherry_picks,
            !args.rebase_merges,
        )?
    };

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
        !args.interactive && !whitespace_forces_replay && collected_commits.is_empty();

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

    let mut todo_script: Option<String> = None;
    if args.interactive {
        if collected_commits.is_empty() {
            print_branch_up_to_date(&head);
            if let Some(ref oid) = autostash_oid {
                apply_autostash_after_ff(&repo, oid)?;
            }
            return Ok(());
        }
        let mut initial = String::new();
        for oid in &collected_commits {
            let obj = repo.odb.read(oid)?;
            let commit = parse_commit(&obj.data)?;
            let subject = commit.message.lines().next().unwrap_or("");
            initial.push_str(&format!("pick {} {}\n", oid.to_hex(), subject));
        }
        let pre_editor_n = count_non_comment_todo_lines(&initial);
        let edited =
            run_interactive_rebase(&repo, git_dir, &initial, &config, autostash_oid.as_ref())?;
        let post_n = count_non_comment_todo_lines(&edited);
        if post_n == 0 {
            if pre_editor_n > 0 {
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
        todo_script = Some(edited);
    }

    let commits = if let Some(ref script) = todo_script {
        parse_todo_script_to_items(&repo, script)?
            .into_iter()
            .filter(|i| {
                matches!(
                    i.command,
                    RebaseTodoCommand::Pick | RebaseTodoCommand::Reword
                )
            })
            .map(|i| i.commit)
            .collect::<Vec<_>>()
    } else {
        collected_commits.clone()
    };

    if !args.no_ff && collected_commits.is_empty() && todo_script.is_none() {
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

    if commits.is_empty() && todo_script.is_none() {
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

    let todo_path = resolve_rebase_todo_path(&repo, &rb_dir);
    let mut script_lines: Vec<String> = if let Some(ref s) = todo_script {
        s.lines().map(|l| l.to_string()).collect()
    } else {
        commits
            .iter()
            .map(|oid| {
                let obj = repo.odb.read(oid).ok();
                let subject = obj
                    .and_then(|o| parse_commit(&o.data).ok())
                    .and_then(|c| c.message.lines().next().map(|s| s.to_string()))
                    .unwrap_or_default();
                if subject.is_empty() {
                    format!("pick {}", oid.to_hex())
                } else {
                    format!("pick {} {}", oid.to_hex(), subject)
                }
            })
            .collect()
    };
    if let Some(ref exec_cmd) = args.exec {
        let joined = todo_lines_to_script(&script_lines);
        let with_exec = inject_exec_after_each_pick(&joined, exec_cmd);
        script_lines = with_exec.lines().map(|l| l.to_string()).collect();
    }
    let total = script_lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .count();
    write_rebase_todo_file(&todo_path, &script_lines)?;
    fs::write(rb_dir.join("interactive"), "")?;
    let legacy_todo: Vec<String> = commits.iter().map(|oid| oid.to_hex().to_string()).collect();
    fs::write(rb_dir.join("todo"), legacy_todo.join("\n") + "\n")?;
    fs::write(rb_dir.join("end"), total.to_string())?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("last"), total.to_string())?;
    fs::write(rb_dir.join("next"), "1")?;

    if let Some(ref ws) = args.whitespace {
        if ws.eq_ignore_ascii_case("fix") || ws.eq_ignore_ascii_case("strip") {
            fs::write(rb_dir.join("whitespace-action"), format!("{ws}\n"))?;
        }
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

/// Find the merge-base of two commits.  Returns `None` when there is no
/// common ancestor.
fn find_merge_base(repo: &Repository, a: ObjectId, b: ObjectId) -> Option<ObjectId> {
    grit_lib::merge_base::merge_bases_first_vs_rest(repo, a, &[b])
        .ok()
        .and_then(|bases| bases.into_iter().next())
}

/// Synthetic root commit Git creates for `rebase --root` without `--onto` (empty tree, no parents).
fn squash_onto_for_root_without_onto(repo: &Repository) -> Result<ObjectId> {
    const GIT_EMPTY_TREE_HEX: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let tree = ObjectId::from_hex(GIT_EMPTY_TREE_HEX)
        .map_err(|e| anyhow::anyhow!("invalid empty tree oid: {e}"))?;
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;
    let ident_line = format_ident(&committer, now);
    let commit_data = CommitData {
        tree,
        parents: vec![],
        author: ident_line.clone(),
        committer: ident_line,
        author_raw: Vec::new(),
        committer_raw: Vec::new(),
        encoding: None,
        message: String::new(),
        raw_message: None,
    };
    let bytes = serialize_commit(&commit_data);
    Ok(repo.odb.write(ObjectKind::Commit, &bytes)?)
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
    omit_merge_commits: bool,
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
    // Merge commits are omitted from `git format-patch` output used by `rebase --apply`, and
    // the default merge backend todo matches that set (t3425: `c...w` is d,n,o,e — not tip w).
    if omit_merge_commits {
        commits.retain(|oid| {
            repo.odb
                .read(oid)
                .ok()
                .and_then(|obj| parse_commit(&obj.data).ok())
                .is_some_and(|c| c.parents.len() <= 1)
        });
    }
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

fn count_action_lines(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .count()
}

fn todo_suffix_from(lines: &[String], start_idx: usize) -> Vec<String> {
    lines[start_idx..].to_vec()
}

/// Drop the first non-empty, non-comment line (the insn that just completed).
fn pop_first_actionable_line(lines: &[String]) -> Vec<String> {
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        if t.is_empty() || t.starts_with('#') {
            i += 1;
            continue;
        }
        return todo_suffix_from(lines, i + 1);
    }
    Vec::new()
}

fn find_todo_line_index_for_commit(lines: &[String], commit_hex: &str) -> Option<usize> {
    lines.iter().position(|l| l.contains(commit_hex))
}

fn squash_fixup_chain_continues(lines: &[String], after_idx: usize) -> bool {
    let mut i = after_idx.saturating_add(1);
    while i < lines.len() {
        let t = lines[i].trim();
        if t.is_empty() || t.starts_with('#') {
            i += 1;
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("fixup ")
            || lower.starts_with("f ")
            || lower.starts_with("squash ")
            || lower.starts_with("s ")
        {
            return true;
        }
        return false;
    }
    false
}

fn write_rebase_state_after_step(
    repo: &Repository,
    rb_dir: &Path,
    todo_path: &Path,
    remaining_lines: &[String],
    done_count: usize,
) -> Result<()> {
    write_rebase_todo_file(todo_path, remaining_lines)?;
    let hex_lines: Vec<String> = remaining_lines
        .iter()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            let lower = t.to_ascii_lowercase();
            if lower.starts_with("pick ")
                || lower.starts_with("p ")
                || lower.starts_with("reword ")
                || lower.starts_with("r ")
                || lower.starts_with("squash ")
                || lower.starts_with("s ")
                || lower.starts_with("fixup ")
                || lower.starts_with("f ")
            {
                let token = t.split_whitespace().nth(1)?;
                let oid = resolve_revision(repo, token).ok()?;
                return Some(oid.to_hex());
            }
            None
        })
        .collect();
    fs::write(rb_dir.join("todo"), hex_lines.join("\n") + "\n")?;
    let total_remaining = count_action_lines(remaining_lines);
    fs::write(rb_dir.join("end"), total_remaining.to_string())?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("next"), done_count.to_string())?;
    Ok(())
}

fn is_effective_editor_value(raw: &str) -> bool {
    let t = raw.trim();
    !t.is_empty() && t != ":"
}

fn resolve_rebase_message_editor(repo: &Repository) -> String {
    let visual_present = std::env::var("VISUAL").is_ok();
    let editor_present = std::env::var("EDITOR").is_ok();
    if let Ok(e) = std::env::var("GIT_EDITOR") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    if let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) {
        if let Some(e) = config.get("core.editor") {
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

fn launch_commit_message_editor(repo: &Repository, initial: &str) -> Result<String> {
    let editor = resolve_rebase_message_editor(repo);
    // Use `COMMIT_EDITMSG` so upstream-style fake editors (t3429 `lib-rebase.sh`) match `*/COMMIT_EDITMSG`.
    let tmp_path = repo.git_dir.join("COMMIT_EDITMSG");
    fs::write(&tmp_path, initial)?;
    let status = std::process::Command::new("sh")
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

fn amend_head_message(repo: &Repository, new_message: &str) -> Result<ObjectId> {
    let git_dir = &repo.git_dir;
    let config = ConfigSet::load(Some(git_dir), true)?;
    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();
    let head_obj = repo.odb.read(&head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let index = load_index(repo)?;
    let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;
    let (message, encoding, raw_message) =
        finalize_message_for_commit_encoding(new_message.to_string(), &config);
    let commit_data = CommitData {
        tree: tree_oid,
        parents: head_commit.parents.clone(),
        author: head_commit.author.clone(),
        committer: format_ident(&committer, now),
        author_raw: head_commit.author_raw.clone(),
        committer_raw: head_commit.committer_raw.clone(),
        encoding,
        message,
        raw_message,
    };
    let bytes = serialize_commit(&commit_data);
    let new_oid = repo.odb.write(ObjectKind::Commit, &bytes)?;
    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
    Ok(new_oid)
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
    let todo_path = resolve_rebase_todo_path(repo, rb_dir);

    let rewind_marker = rb_dir.join("rewind-notice");
    let initial_lines = read_rebase_todo_file(&todo_path)?;
    if !rewind_marker.exists() && !initial_lines.is_empty() {
        println!("First, rewinding head to replay your work on top of it...");
        let _ = fs::write(&rewind_marker, "");
    }

    let mut done_nr = 0usize;

    loop {
        let lines = read_rebase_todo_file(&todo_path)?;
        let mut line_idx = 0usize;
        while line_idx < lines.len() {
            let t = lines[line_idx].trim();
            if t.is_empty() || t.starts_with('#') {
                line_idx += 1;
                continue;
            }
            break;
        }
        if line_idx >= lines.len() {
            break;
        }

        let raw_line = lines[line_idx].clone();
        let trimmed = raw_line.trim();

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("exec ") || lower.starts_with("x ") {
            done_nr += 1;
            let exec_cmd = trimmed
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            let exec_cmd = exec_cmd.trim();
            if !exec_cmd.is_empty() {
                eprintln!("Executing: {}", exec_cmd);
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(exec_cmd)
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
                    let remaining = todo_suffix_from(&lines, line_idx + 1);
                    write_rebase_state_after_step(repo, rb_dir, &todo_path, &remaining, done_nr)?;
                    std::process::exit(code);
                }
            }
            let after_exec = read_rebase_todo_file(&todo_path)?;
            let remaining = todo_suffix_from(&after_exec, line_idx + 1);
            write_rebase_state_after_step(repo, rb_dir, &todo_path, &remaining, done_nr)?;
            continue;
        }

        let Some(item) = parse_rebase_todo_line(repo, trimmed)? else {
            let remaining = todo_suffix_from(&lines, line_idx + 1);
            write_rebase_state_after_step(repo, rb_dir, &todo_path, &remaining, done_nr)?;
            continue;
        };

        done_nr += 1;
        let commit_oid = item.commit;
        let commit_hex = commit_oid.to_hex();
        fs::write(rb_dir.join("current"), &commit_hex)?;
        fs::write(rb_dir.join("msgnum"), "1")?;
        fs::write(rb_dir.join("next"), done_nr.to_string())?;

        let old_head = resolve_head(git_dir)?
            .oid()
            .cloned()
            .unwrap_or_else(diff::zero_oid);

        let pick_backend = load_rebase_backend(rb_dir);
        let opts = PickOptions {
            command: item.command.clone(),
        };
        match cherry_pick_for_rebase(repo, &commit_oid, pick_backend, opts) {
            Ok(()) => {
                if matches!(item.command, RebaseTodoCommand::Reword) {
                    let head_oid = resolve_head(git_dir)?
                        .oid()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                    let obj = repo.odb.read(&head_oid)?;
                    let cur = parse_commit(&obj.data)?;
                    let edited = launch_commit_message_editor(repo, &cur.message)?;
                    let new_oid = amend_head_message(repo, edited.trim())?;
                    let subject = edited.lines().next().unwrap_or("").trim();
                    let msg = format!("{ra} (reword): {subject}");
                    let _ =
                        append_reflog(git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false);
                } else {
                    let obj = repo.odb.read(&commit_oid)?;
                    let commit = parse_commit(&obj.data)?;
                    let root_rebase = rb_dir.join("root").exists();
                    let msg_for_log = message_for_root_replayed_commit(repo, &commit, root_rebase);
                    let subject = msg_for_log.lines().next().unwrap_or("");
                    eprintln!("Applying: {}", subject);
                    let head = resolve_head(git_dir)?;
                    let new_oid = *head
                        .oid()
                        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                    let msg = format!("{ra} (pick): {subject}");
                    let _ =
                        append_reflog(git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false);
                }

                let fresh = read_rebase_todo_file(&todo_path)?;
                let remaining = pop_first_actionable_line(&fresh);
                write_rebase_state_after_step(repo, rb_dir, &todo_path, &remaining, done_nr)?;
            }
            Err(_e) => {
                let remaining = todo_suffix_from(&read_rebase_todo_file(&todo_path)?, line_idx);
                write_rebase_state_after_step(repo, rb_dir, &todo_path, &remaining, done_nr)?;

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

    finish_rebase(repo, rb_dir, autostash_oid, backend, had_rebase_autostash)?;
    Ok(())
}

#[derive(Clone)]
struct PickOptions {
    command: RebaseTodoCommand,
}

/// Cherry-pick a single commit onto current HEAD for rebase purposes.
fn cherry_pick_for_rebase(
    repo: &Repository,
    commit_oid: &ObjectId,
    backend: RebaseBackend,
    opts: PickOptions,
) -> Result<()> {
    let git_dir = &repo.git_dir;

    let commit_obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let config = ConfigSet::load(Some(git_dir), true)?;

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
    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD is unborn during rebase"))?
        .to_owned();
    let head_obj = repo.odb.read(&head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let head_tree_oid = head_commit.tree;

    let rb_dir = rebase_dir(git_dir);
    let root_rebase = rb_dir.join("root").exists();

    // Two-parent merge during a non-root rebase: Git flattens the merge onto the replayed first
    // parent. If replaying `n` then `o` already produced the merge result tree, the merge commit
    // is redundant (t3425-rebase-topology-merges). Otherwise replay the merge with a three-way
    // using the merge-base of the original parents as the base tree.
    if !root_rebase && commit.parents.len() == 2 {
        if head_tree_oid == commit_tree_oid {
            return Ok(());
        }
        let p1 = commit.parents[0];
        let p2 = commit.parents[1];
        let bases = merge_bases_first_vs_rest(repo, p1, &[p2])?;
        let Some(base_oid) = bases.first().copied() else {
            bail!(
                "cannot replay merge {}: no merge base for parents",
                commit_oid.to_hex()
            );
        };
        let base_obj = repo.odb.read(&base_oid)?;
        let base_commit = parse_commit(&base_obj.data)?;
        let base_tree_oid = base_commit.tree;

        let ws_fix_rule = load_ws_fix_rule_from_rebase_state(git_dir);
        let base_tree_oid = if ws_fix_rule.is_some() {
            head_tree_oid
        } else {
            base_tree_oid
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

        let old_index = load_index(repo)?;
        repo.write_index(&mut merged_index)?;
        if let Some(wt) = &repo.work_tree {
            checkout_merged_index(repo, wt, &old_index, &merged_index)?;
            if has_conflicts {
                write_rebase_conflict_files(wt, &merge_result.conflict_files)?;
            }
        }

        if has_conflicts {
            let _ =
                grit_lib::rerere::repo_rerere(repo, grit_lib::rerere::RerereAutoupdate::FromConfig);
            fs::write(git_dir.join("MERGE_MSG"), &commit.message)?;
            bail!("conflicts during cherry-pick of {}", commit_oid.to_hex());
        }

        let tree_oid = write_tree_from_index(&repo.odb, &merged_index, "")?;
        let now = time::OffsetDateTime::now_utc();
        let committer = resolve_identity(&config, "COMMITTER")?;
        let (message, encoding, raw_message) = transcoded_replayed_message(&commit, &config);
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
        return Ok(());
    }

    let is_fixup = matches!(opts.command, RebaseTodoCommand::Fixup);
    let is_squash = matches!(opts.command, RebaseTodoCommand::Squash);

    // Already at the picked commit's parent tip — nothing to replay (matches Git's noop pick).
    if matches!(opts.command, RebaseTodoCommand::Pick) {
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
                fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
                return Ok(());
            }
        }
    }

    // Three-way merge: base=parent_tree, ours=HEAD_tree, theirs=commit_tree
    let ws_fix_rule = load_ws_fix_rule_from_rebase_state(git_dir);
    let base_tree_oid = if ws_fix_rule.is_some() {
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

    let old_index = load_index(repo)?;
    repo.write_index(&mut merged_index)?;

    if let Some(wt) = &repo.work_tree {
        checkout_merged_index(repo, wt, &old_index, &merged_index)?;
        if has_conflicts {
            write_rebase_conflict_files(wt, &merge_result.conflict_files)?;
        }
    }

    if has_conflicts {
        let _ = grit_lib::rerere::repo_rerere(repo, grit_lib::rerere::RerereAutoupdate::FromConfig);
        fs::write(git_dir.join("MERGE_MSG"), &commit.message)?;
        bail!("conflicts during cherry-pick of {}", commit_oid.to_hex());
    }

    let tree_oid = write_tree_from_index(&repo.odb, &merged_index, "")?;

    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    if is_fixup {
        let (message, encoding, raw_message) = transcoded_replayed_message(&head_commit, &config);
        let commit_data = CommitData {
            tree: tree_oid,
            parents: head_commit.parents.clone(),
            author: head_commit.author.clone(),
            committer: format_ident(&committer, now),
            author_raw: head_commit.author_raw.clone(),
            committer_raw: head_commit.committer_raw.clone(),
            encoding,
            message,
            raw_message,
        };
        let commit_bytes = serialize_commit(&commit_data);
        let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
        fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
        let rb_merge = rebase_merge_dir(git_dir);
        if rb_merge.join("defer-squash-editor").exists() {
            let pending_path = rb_merge.join("pending-squash-msg");
            let pending = fs::read_to_string(&pending_path).unwrap_or_default();
            if !pending.is_empty() {
                let edited = launch_commit_message_editor(repo, &pending)?;
                let _ = amend_head_message(repo, edited.trim())?;
            }
            let _ = fs::remove_file(rb_merge.join("defer-squash-editor"));
            let _ = fs::remove_file(&pending_path);
        }
        return Ok(());
    }

    if is_squash {
        let squash_path = git_dir.join("SQUASH_MSG");
        let head_msg = commit_message_unicode(&head_commit);
        let cur = fs::read_to_string(&squash_path).unwrap_or_default();
        let appended = if cur.is_empty() {
            head_msg
        } else {
            format!("{cur}\n\n{}", commit.message)
        };
        fs::write(&squash_path, &appended)?;
        let rb_merge = rebase_merge_dir(git_dir);
        let todo_path = resolve_rebase_todo_path(repo, &rb_merge);
        let todo_lines = read_rebase_todo_file(&todo_path).unwrap_or_default();
        let after_idx =
            find_todo_line_index_for_commit(&todo_lines, &commit_oid.to_hex()).unwrap_or(0);
        let defer_squash_editor = squash_fixup_chain_continues(&todo_lines, after_idx);
        let (message, encoding, raw_message) = if defer_squash_editor {
            let _ = fs::write(rb_merge.join("defer-squash-editor"), "");
            let _ = fs::write(rb_merge.join("pending-squash-msg"), &appended);
            finalize_message_for_commit_encoding(appended.clone(), &config)
        } else {
            let edited = launch_commit_message_editor(repo, &appended)?;
            finalize_message_for_commit_encoding(edited.trim().to_string(), &config)
        };
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
        let _ = fs::remove_file(&squash_path);
        return Ok(());
    }

    let (message, encoding, raw_message) = if root_rebase {
        let msg = message_for_root_replayed_commit(repo, &commit, true);
        (msg, commit.encoding.clone(), None)
    } else {
        transcoded_replayed_message(&commit, &config)
    };
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

    // Commit the current cherry-pick
    let current_hex = fs::read_to_string(rb_dir.join("current"))?;
    let current_hex = current_hex.trim();
    let current_oid = ObjectId::from_hex(current_hex)?;

    let commit_obj = repo.odb.read(&current_oid)?;
    let original_commit = parse_commit(&commit_obj.data)?;

    let config = ConfigSet::load(Some(git_dir), true)?;
    let (message, encoding, raw_message) =
        read_rebase_continue_message(git_dir, &original_commit, &config)?;

    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

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
    let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD (detached)
    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
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

    let todo_path = resolve_rebase_todo_path(&repo, &rb_dir);
    if let Ok(todo_lines) = read_rebase_todo_file(&todo_path) {
        let mut i = 0usize;
        while i < todo_lines.len() {
            let t = todo_lines[i].trim();
            if t.is_empty() || t.starts_with('#') {
                i += 1;
                continue;
            }
            break;
        }
        if i < todo_lines.len() {
            let first_line = todo_lines[i].trim();
            if let Some(item) = parse_rebase_todo_line(&repo, first_line)? {
                if matches!(item.command, RebaseTodoCommand::Reword) {
                    let head_oid2 = resolve_head(git_dir)?
                        .oid()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                    let obj2 = repo.odb.read(&head_oid2)?;
                    let cur2 = parse_commit(&obj2.data)?;
                    let edited = launch_commit_message_editor(&repo, &cur2.message)?;
                    let new_oid2 = amend_head_message(&repo, edited.trim())?;
                    let subj = edited.lines().next().unwrap_or("").trim();
                    let msg2 = format!("{ra} (reword): {subj}");
                    let _ =
                        append_reflog(git_dir, "HEAD", &head_oid2, &new_oid2, &ident, &msg2, false);
                }
            }
            let remaining = todo_suffix_from(&todo_lines, i + 1);
            let prev_done: usize = fs::read_to_string(rb_dir.join("next"))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            write_rebase_state_after_step(&repo, &rb_dir, &todo_path, &remaining, prev_done)?;
        }
    }

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

fn do_edit_todo() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    if !is_rebase_in_progress(git_dir) {
        bail!("no rebase in progress");
    }
    let rb_dir = active_rebase_dir(git_dir)
        .ok_or_else(|| anyhow::anyhow!("internal: no rebase state directory"))?;
    let todo_path = resolve_rebase_todo_path(&repo, &rb_dir);
    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let editor = sequence_editor_cmd(&config)?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$1\"", editor))
        .arg("sh")
        .arg(&todo_path)
        .status()
        .context("failed to run sequence editor")?;
    if !status.success() {
        bail!("there was a problem with the editor");
    }
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
