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
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};

use grit_lib::diff::{self, count_changes, DiffEntry};
use grit_lib::merge_base::{is_ancestor, merge_bases_first_vs_rest};
use grit_lib::merge_file::{merge, ConflictStyle, MergeInput};
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::refs::append_reflog;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;

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
}

/// Run the `rebase` command.
pub fn run(mut args: Args) -> Result<()> {
    validate_compat_options(&args)?;

    if args.abort {
        return do_abort();
    }
    if args.r#continue {
        return do_continue();
    }
    if args.skip {
        return do_skip();
    }

    // If a branch argument is given, checkout that branch first.
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
    if args.upstream.is_none() && args.onto.is_none() {
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

    do_rebase(args)
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

fn print_branch_up_to_date(head: &HeadState) {
    if let Some(name) = head.branch_name() {
        println!("Current branch {name} is up to date.");
    } else {
        println!("HEAD is up to date.");
    }
}

fn reflog_identity(repo: &Repository) -> String {
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
    let now = time::OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}

fn rebase_dir(git_dir: &Path) -> std::path::PathBuf {
    git_dir.join("rebase-apply")
}

fn is_rebase_in_progress(git_dir: &Path) -> bool {
    rebase_dir(git_dir).exists()
}

fn choose_rebase_backend(args: &Args) -> RebaseBackend {
    if args.apply {
        RebaseBackend::Apply
    } else {
        RebaseBackend::Merge
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

// ── Main rebase flow ────────────────────────────────────────────────

fn do_rebase(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if is_rebase_in_progress(git_dir) {
        bail!(
            "error: a rebase is already in progress\n\
             hint: use \"grit rebase --continue\" to continue\n\
             hint: or \"grit rebase --abort\" to abort"
        );
    }

    // Check for dirty worktree/index
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
        if !staged.is_empty() {
            bail!(
                "cannot rebase: your index contains uncommitted changes.\n\
                   Please commit or stash them."
            );
        }
        let unstaged = grit_lib::diff::diff_index_to_worktree(&repo.odb, &idx, work_tree)?;
        if !unstaged.is_empty() {
            bail!(
                "error: cannot rebase: You have unstaged changes.\n\
                   Please commit or stash them."
            );
        }
    }

    // Resolve upstream
    let upstream_spec = args.upstream.as_deref().unwrap_or("HEAD");
    let upstream_oid = resolve_revision(&repo, upstream_spec)
        .with_context(|| format!("bad revision '{upstream_spec}'"))?;

    let head_state = resolve_head(git_dir)?;
    let head_oid_early = head_state
        .oid()
        .ok_or_else(|| anyhow::anyhow!("cannot rebase: HEAD is unborn"))?
        .to_owned();

    let onto_oid = if let Some(ref onto_spec) = args.onto {
        resolve_revision(&repo, onto_spec).with_context(|| format!("bad revision '{onto_spec}'"))?
    } else if args.keep_base {
        find_merge_base(&repo, upstream_oid, head_oid_early).unwrap_or(upstream_oid)
    } else {
        upstream_oid
    };

    let head = head_state;
    let head_oid = head_oid_early;

    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_else(|_| ConfigSet::new());
    let want_stat =
        args.stat || (config.get("rebase.stat").as_deref() == Some("true") && !args.no_stat);

    let branch_base_merge = merge_bases_first_vs_rest(&repo, onto_oid, &[head_oid])?;
    let branch_base = if branch_base_merge.len() == 1 {
        Some(branch_base_merge[0])
    } else {
        None
    };

    let allow_preemptive_ff = !args.interactive && args.exec.is_none();

    if allow_preemptive_ff && rebase_can_preemptive_ff(&repo, onto_oid, upstream_oid, head_oid)? {
        if !args.no_ff {
            print_branch_up_to_date(&head);
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

    let commits = collect_commits_to_replay(&repo, head_oid, upstream_oid)?;

    if args.interactive {
        if commits.is_empty() {
            print_branch_up_to_date(&head);
            return Ok(());
        }
        for oid in &commits {
            let obj = repo.odb.read(oid)?;
            let commit = parse_commit(&obj.data)?;
            let subject = commit.message.lines().next().unwrap_or("");
            println!("pick {} {}", &oid.to_hex()[..7], subject);
        }
        return Ok(());
    }

    if !args.no_ff && commits.is_empty() {
        if head_oid == onto_oid {
            print_branch_up_to_date(&head);
            return Ok(());
        }
        if can_fast_forward(&repo, head_oid, onto_oid)? {
            let ff_base = merge_bases_first_vs_rest(&repo, onto_oid, &[head_oid])?
                .into_iter()
                .next();
            return fast_forward_rebase(
                &repo,
                &head,
                head_oid,
                onto_oid,
                upstream_spec,
                ff_base,
                head_oid,
            );
        }
    }

    if commits.is_empty() {
        if let HeadState::Branch { refname, .. } = &head {
            let ident = reflog_identity(&repo);
            let msg = format!("rebase (no-ff): checkout {}", onto_oid.to_hex());
            let _ = append_reflog(git_dir, refname, &head_oid, &head_oid, &ident, &msg, false);
            let _ = append_reflog(git_dir, "HEAD", &head_oid, &head_oid, &ident, &msg, false);
        }
        return Ok(());
    }

    let backend = choose_rebase_backend(&args);
    let rb_dir = rebase_dir(git_dir);
    fs::create_dir_all(&rb_dir)?;

    let head_name = match &head {
        HeadState::Branch { refname, .. } => refname.clone(),
        _ => "detached HEAD".to_string(),
    };
    fs::write(rb_dir.join("head-name"), &head_name)?;
    fs::write(rb_dir.join("orig-head"), head_oid.to_hex())?;
    fs::write(rb_dir.join("onto"), onto_oid.to_hex())?;
    fs::write(rb_dir.join("onto-name"), format!("{upstream_spec}\n"))?;
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

    let todo: Vec<String> = commits.iter().map(|oid| oid.to_hex()).collect();
    let total = todo.len();
    fs::write(rb_dir.join("todo"), todo.join("\n") + "\n")?;
    fs::write(rb_dir.join("end"), total.to_string())?;
    fs::write(rb_dir.join("msgnum"), "1")?;
    fs::write(rb_dir.join("last"), total.to_string())?;
    fs::write(rb_dir.join("next"), "1")?;

    if let Some(ref exec_cmd) = args.exec {
        fs::write(rb_dir.join("exec"), exec_cmd)?;
    }

    let ident = reflog_identity(&repo);
    let ra = rebase_reflog_action();
    let start_msg = format!("{ra} (start): checkout {upstream_spec}");
    // Git records `(start)` on HEAD only; the branch ref keeps its pre-rebase tip until `(finish)`.
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
    let entries = tree_to_index_entries(&repo, &onto_commit.tree, "")?;
    let mut idx = Index::new();
    idx.entries = entries;
    idx.sort();
    let old_index = load_index(&repo)?;
    repo.write_index(&mut idx)?;
    if let Some(wt) = &repo.work_tree {
        checkout_merged_index(&repo, wt, &old_index, &idx)?;
    }

    eprintln!(
        "rebasing {} commits onto {}",
        total,
        &onto_oid.to_hex()[..7]
    );

    replay_remaining(&repo)?;

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
fn replay_remaining(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;
    let rb_dir = rebase_dir(git_dir);
    let ra = load_rebase_reflog_action(&rb_dir);
    let ident = reflog_identity(repo);

    let todo_content = fs::read_to_string(rb_dir.join("todo"))?;
    let todo: Vec<&str> = todo_content.lines().filter(|l| !l.is_empty()).collect();
    let _total: usize = fs::read_to_string(rb_dir.join("end"))?.trim().parse()?;
    let msgnum: usize = fs::read_to_string(rb_dir.join("msgnum"))?.trim().parse()?;

    for i in (msgnum - 1)..todo.len() {
        let commit_hex = todo[i];
        let commit_oid = ObjectId::from_hex(commit_hex)?;

        // Update state
        fs::write(rb_dir.join("current"), commit_hex)?;
        fs::write(rb_dir.join("msgnum"), (i + 1).to_string())?;
        fs::write(rb_dir.join("next"), (i + 1).to_string())?;

        // Read HEAD before cherry-pick for reflog
        let old_head = resolve_head(git_dir)?
            .oid()
            .cloned()
            .unwrap_or_else(diff::zero_oid);

        let backend = load_rebase_backend(&rb_dir);
        match cherry_pick_for_rebase(repo, &commit_oid, backend) {
            Ok(()) => {
                let head = resolve_head(git_dir)?;
                let new_oid = *head
                    .oid()
                    .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?;
                let obj = repo.odb.read(&commit_oid)?;
                let commit = parse_commit(&obj.data)?;
                let subject = commit.message.lines().next().unwrap_or("");
                eprintln!("Applying: {}", subject);
                let msg = format!("{ra} (pick): {subject}");
                let _ = append_reflog(git_dir, "HEAD", &old_head, &new_oid, &ident, &msg, false);

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
                // Conflicts — leave state for --continue
                let remaining: Vec<&str> = todo[i + 1..].to_vec();
                fs::write(rb_dir.join("todo"), remaining.join("\n") + "\n")?;
                fs::write(rb_dir.join("msgnum"), "1")?;
                fs::write(rb_dir.join("end"), remaining.len().to_string())?;

                let obj = repo.odb.read(&commit_oid)?;
                let commit = parse_commit(&obj.data)?;
                let subject = commit.message.lines().next().unwrap_or("");

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

    // Rebase complete — restore branch ref
    finish_rebase(repo)?;
    Ok(())
}

/// Cherry-pick a single commit onto current HEAD for rebase purposes.
fn cherry_pick_for_rebase(
    repo: &Repository,
    commit_oid: &ObjectId,
    backend: RebaseBackend,
) -> Result<()> {
    let git_dir = &repo.git_dir;

    let commit_obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&commit_obj.data)?;

    // Parent tree (base for the cherry-pick)
    let parent_oid = commit
        .parents
        .first()
        .ok_or_else(|| anyhow::anyhow!("cannot cherry-pick root commit in rebase"))?;
    let parent_obj = repo.odb.read(parent_oid)?;
    let parent_commit = parse_commit(&parent_obj.data)?;
    let parent_tree_oid = parent_commit.tree;

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

    // Three-way merge: base=parent_tree, ours=HEAD_tree, theirs=commit_tree
    let base_entries = tree_to_map(tree_to_index_entries(repo, &parent_tree_oid, "")?);
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

    let has_conflicts = merged_index.entries.iter().any(|e| e.stage() != 0);

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
        // Save MERGE_MSG for --continue
        fs::write(git_dir.join("MERGE_MSG"), &commit.message)?;
        bail!("conflicts during cherry-pick of {}", commit_oid.to_hex());
    }

    // Create the rebased commit, preserving the original author
    let tree_oid = write_tree_from_index(&repo.odb, &merged_index, "")?;

    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid],
        author: commit.author.clone(), // preserve original author
        committer: format_ident(&committer, now),
        encoding: commit.encoding.clone(),
        message: commit.message.clone(),
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD (detached)
    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;

    Ok(())
}

/// Finish the rebase: point the original branch at the new HEAD.
fn finish_rebase(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;
    let rb_dir = rebase_dir(git_dir);

    let head_name = fs::read_to_string(rb_dir.join("head-name"))?;
    let head_name = head_name.trim();

    let onto_hex = fs::read_to_string(rb_dir.join("onto"))?;
    let onto_hex = onto_hex.trim();
    let onto_oid = ObjectId::from_hex(onto_hex)?;

    let ra = load_rebase_reflog_action(&rb_dir);
    let ident = reflog_identity(repo);

    let head = resolve_head(git_dir)?;
    let new_tip = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

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

    cleanup_rebase_state(git_dir);

    eprintln!(
        "Successfully rebased and updated {}.",
        if head_name == "detached HEAD" {
            "HEAD"
        } else {
            head_name
        }
    );

    Ok(())
}

// ── --continue ──────────────────────────────────────────────────────

fn do_continue() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_rebase_in_progress(git_dir) {
        bail!("error: no rebase in progress");
    }

    let rb_dir = rebase_dir(git_dir);

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

    // Read message (might have been edited)
    let message = match fs::read_to_string(git_dir.join("MERGE_MSG")) {
        Ok(m) => m,
        Err(_) => original_commit.message.clone(),
    };

    let head = resolve_head(git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no OID"))?
        .to_owned();

    let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid],
        author: original_commit.author.clone(),
        committer: format_ident(&committer, now),
        encoding: original_commit.encoding.clone(),
        message,
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let new_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD (detached)
    fs::write(git_dir.join("HEAD"), format!("{}\n", new_oid.to_hex()))?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    let subject = original_commit.message.lines().next().unwrap_or("");
    eprintln!("Applying: {}", subject);

    let backend = load_rebase_backend(&rb_dir);
    let ra = load_rebase_reflog_action(&rb_dir);
    let ident = reflog_identity(&repo);
    let verb = match backend {
        RebaseBackend::Merge => "continue",
        RebaseBackend::Apply => "pick",
    };
    let msg = format!("{ra} ({verb}): {subject}");
    let _ = append_reflog(git_dir, "HEAD", &head_oid, &new_oid, &ident, &msg, false);

    // Continue with remaining
    replay_remaining(&repo)?;

    Ok(())
}

// ── --skip ──────────────────────────────────────────────────────────

fn do_skip() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_rebase_in_progress(git_dir) {
        bail!("error: no rebase in progress");
    }

    let _rb_dir = rebase_dir(git_dir);

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
    replay_remaining(&repo)?;

    Ok(())
}

// ── --abort ─────────────────────────────────────────────────────────

fn do_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_rebase_in_progress(git_dir) {
        bail!("error: no rebase in progress");
    }

    let rb_dir = rebase_dir(git_dir);

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

    cleanup_rebase_state(git_dir);
    eprintln!("Rebase aborted.");

    Ok(())
}

// ── Cleanup ─────────────────────────────────────────────────────────

fn cleanup_rebase_state(git_dir: &Path) {
    let rb_dir = rebase_dir(git_dir);
    let _ = fs::remove_dir_all(&rb_dir);
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
}

// ── Helpers (mirrored from revert.rs) ───────────────────────────────

fn load_index(repo: &Repository) -> Result<Index> {
    Ok(repo.load_index()?)
}

fn resolve_identity(config: &ConfigSet, kind: &str) -> Result<(String, String)> {
    let name_var = format!("GIT_{kind}_NAME");
    let email_var = format!("GIT_{kind}_EMAIL");

    let name = std::env::var(&name_var)
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var(&email_var)
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    Ok((name, email))
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
