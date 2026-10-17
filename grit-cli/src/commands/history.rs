//! `grit history` — stateless history rewrite helpers.

use std::io::{self, Read, Write};

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use grit_lib::config::ident_resolve::IdentRole;
use grit_lib::config::ConfigSet;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::revwalk::history::{
    apply_fixup_rewrite, apply_rewrite, drop as history_drop, fixup, rebase_onto, reword, split,
    squash, staged_fixup_paths, verify_rewrite_plan, ApplyOptions, EmptyPolicy, FixupOptions,
    HistoryError, RebaseOntoOptions, RefScope, RewriteOptions, RewritePlan, SplitOptions,
    SquashMessage, SquashOptions,
};
use grit_lib::revwalk::rev_parse::resolve_revision_as_commit;
use grit_lib::state::{resolve_head, HeadState};
use schemars::JsonSchema;
use serde::Serialize;

use crate::context;
use crate::json_error;
use crate::markdown;
use crate::output::{outln, HumanRender, MarkdownRender, RenderContext};

/// Which local refs a history rewrite may move.
#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq, Eq)]
pub enum UpdateRefs {
    /// Every local branch descending from the edited commit, plus `HEAD` when applicable.
    #[default]
    Branches,
    /// Only the commit `HEAD` currently points at.
    Head,
}

impl From<UpdateRefs> for RefScope {
    fn from(value: UpdateRefs) -> Self {
        match value {
            UpdateRefs::Branches => RefScope::Branches,
            UpdateRefs::Head => RefScope::Head,
        }
    }
}

/// How to treat commits that become empty during replay.
#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq, Eq)]
pub enum EmptyPolicyArg {
    /// Skip empty replays (map to the new parent).
    #[default]
    Drop,
    /// Keep an empty commit.
    Keep,
    /// Abort when a replay would become empty.
    Abort,
}

impl From<EmptyPolicyArg> for EmptyPolicy {
    fn from(value: EmptyPolicyArg) -> Self {
        match value {
            EmptyPolicyArg::Drop => EmptyPolicy::Drop,
            EmptyPolicyArg::Keep => EmptyPolicy::Keep,
            EmptyPolicyArg::Abort => EmptyPolicy::Abort,
        }
    }
}

/// Shared flags for history rewrite subcommands.
#[derive(Debug, clap::Args)]
pub struct HistoryRewriteArgs {
    /// Print planned ref updates without moving refs (new commit objects are still written).
    #[arg(long = "dry-run")]
    pub dry_run: bool,
    /// Which refs to rewrite: all descendant branches (default) or only `HEAD`.
    #[arg(long = "update-refs", value_enum, default_value_t = UpdateRefs::Branches)]
    pub update_refs: UpdateRefs,
}

/// Result of `grit history reword`, `grit history drop`, `grit history rebase`, and related rewrite subcommands.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoryRewriteOutcome {
    /// Operation name (for example `reword`, `drop`, `fixup`, or `squash`).
    pub operation: String,
    /// Original and replacement commit OIDs for every rewritten commit.
    pub rewritten: Vec<RewrittenPair>,
    /// Planned or applied ref tip updates.
    pub refs: Vec<RefPair>,
    /// Previous and new `HEAD` commit OIDs when `HEAD`'s commit changes; JSON `null` when unchanged.
    pub head: Option<HeadPair>,
    /// Whether refs were left unchanged (`true` for `--dry-run`).
    pub dry_run: bool,
}

/// One commit OID before and after rewrite.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RewrittenPair {
    /// Commit OID before rewrite (full hex).
    pub old: String,
    /// Replacement commit OID (full hex), or JSON `null` when the commit was dropped.
    pub new: Option<String>,
}

/// One ref tip before and after rewrite.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RefPair {
    /// Full ref name.
    pub name: String,
    /// Previous tip OID (full hex).
    pub old: String,
    /// New tip OID (full hex).
    pub new: String,
}

/// `HEAD` commit before and after rewrite.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HeadPair {
    /// Commit OID at `HEAD` before rewrite (full hex).
    pub old: String,
    /// Commit OID at `HEAD` after rewrite (full hex).
    pub new: String,
}

impl HumanRender for HistoryRewriteOutcome {
    fn render_human(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        if self.dry_run {
            outln!(w, "Dry run — refs would change:");
        } else if self.operation == "drop" {
            outln!(
                w,
                "Dropped 1 commit, replayed {} descendant commit(s)",
                self.rewritten.len()
            );
        } else if self.operation == "split" {
            outln!(
                w,
                "History split: {} commit(s) rewritten",
                self.rewritten.len()
            );
        } else {
            let verb = if self.operation == "fixup" {
                "Fixed up"
            } else {
                "Rewrote"
            };
            outln!(w, "{verb} {} commit(s)", self.rewritten.len());
        }
        for pair in &self.rewritten {
            let old_short = pair.old.get(..7).unwrap_or(&pair.old);
            match &pair.new {
                Some(new) => {
                    let new_short = new.get(..7).unwrap_or(new);
                    outln!(w, "  commit {old_short} → {new_short}");
                }
                None => outln!(w, "  commit {old_short} → (dropped)"),
            }
        }
        for update in &self.refs {
            let old_short = update.old.get(..7).unwrap_or(&update.old);
            let new_short = update.new.get(..7).unwrap_or(&update.new);
            outln!(w, "  {} {old_short} → {new_short}", update.name);
        }
        Ok(())
    }
}

impl MarkdownRender for HistoryRewriteOutcome {
    fn render_markdown(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        markdown::print_history_rewrite_outcome(w, self)?;
        Ok(())
    }
}

/// Split `commit` by pathspec and replay descendants.
pub fn run_split(
    commit: &str,
    message: Option<String>,
    message_file: Option<String>,
    message_rest: Option<String>,
    message_rest_file: Option<String>,
    pathspec: Vec<String>,
    shared: &HistoryRewriteArgs,
) -> Result<HistoryRewriteOutcome> {
    if pathspec.is_empty() {
        bail!("at least one pathspec is required after `--`");
    }
    let message = read_message(message, message_file)?;
    let rest_message = read_optional_message(message_rest, message_rest_file)?;
    let repo = context::discover()?;
    let commit_oid = resolve_revision_as_commit(&repo, commit)
        .with_context(|| format!("could not resolve commit '{commit}'"))?;
    let env = repo.environment();
    let config =
        ConfigSet::load(env, Some(repo.git_dir()), true).context("could not load config")?;
    let committer = context::identity(
        env,
        &config,
        IdentRole::Committer,
        "GIT_COMMITTER_DATE",
        context::wall_clock_now(env),
    )?;
    let plan = split(
        &repo,
        commit_oid,
        &pathspec,
        message,
        &SplitOptions { rest_message },
        &RewriteOptions {
            scope: shared.update_refs.into(),
            empty: EmptyPolicy::Drop,
            committer,
            preserve_committer: false,
        },
    )
    .map_err(|e| map_history_error(e, "split", None))?;
    verify_rewrite_plan(&repo, &plan).map_err(|e| map_history_error(e, "split", None))?;
    let subject = rewritten_subject(&repo, &plan, commit_oid)?;
    let mut outcome = outcome_from_plan(&repo, &plan, "split", shared.dry_run)?;
    if !shared.dry_run {
        let identity = context::committer_reflog_identity(env, &config)?;
        apply_rewrite(
            &repo,
            &plan,
            &ApplyOptions {
                identity,
                reflog_message: format!("history split: {subject}"),
            },
        )
        .map_err(|e| map_history_error(e, "split", None))?;
    }
    outcome.dry_run = shared.dry_run;
    Ok(outcome)
}

/// Rewrite `commit`'s message to `message` and replay descendants.
pub fn run_reword(
    commit: &str,
    message: Option<String>,
    message_file: Option<String>,
    shared: &HistoryRewriteArgs,
) -> Result<HistoryRewriteOutcome> {
    let message = read_message(message, message_file)?;
    let repo = context::discover()?;
    let commit_oid = resolve_revision_as_commit(&repo, commit)
        .with_context(|| format!("could not resolve commit '{commit}'"))?;
    let env = repo.environment();
    let config =
        ConfigSet::load(env, Some(repo.git_dir()), true).context("could not load config")?;
    let committer = context::identity(
        env,
        &config,
        IdentRole::Committer,
        "GIT_COMMITTER_DATE",
        context::wall_clock_now(env),
    )?;
    let plan = reword(
        &repo,
        commit_oid,
        message,
        &RewriteOptions {
            scope: shared.update_refs.into(),
            empty: EmptyPolicy::Drop,
            committer,
            preserve_committer: false,
        },
    )
    .map_err(|e| map_history_error(e, "reword", None))?;
    verify_rewrite_plan(&repo, &plan).map_err(|e| map_history_error(e, "reword", None))?;
    let subject = rewritten_subject(&repo, &plan, commit_oid)?;
    let mut outcome = outcome_from_plan(&repo, &plan, "reword", shared.dry_run)?;
    if !shared.dry_run {
        let identity = context::committer_reflog_identity(env, &config)?;
        apply_rewrite(
            &repo,
            &plan,
            &ApplyOptions {
                identity,
                reflog_message: format!("history reword: {subject}"),
            },
        )
        .map_err(|e| map_history_error(e, "reword", None))?;
    }
    outcome.dry_run = shared.dry_run;
    Ok(outcome)
}

/// Remove `commit` from history and replay descendants onto its parent.
pub fn run_drop(commit: &str, shared: &HistoryRewriteArgs) -> Result<HistoryRewriteOutcome> {
    let repo = context::discover()?;
    let commit_oid = resolve_revision_as_commit(&repo, commit)
        .with_context(|| format!("could not resolve commit '{commit}'"))?;
    let env = repo.environment();
    let config =
        ConfigSet::load(env, Some(repo.git_dir()), true).context("could not load config")?;
    let committer = context::identity(
        env,
        &config,
        IdentRole::Committer,
        "GIT_COMMITTER_DATE",
        context::wall_clock_now(env),
    )?;
    let plan = history_drop(
        &repo,
        commit_oid,
        &RewriteOptions {
            scope: shared.update_refs.into(),
            empty: EmptyPolicy::Drop,
            committer,
            preserve_committer: false,
        },
    )
    .map_err(|e| map_history_error(e, "drop", Some(commit_oid)))?;
    verify_rewrite_plan(&repo, &plan)
        .map_err(|e| map_history_error(e, "drop", Some(commit_oid)))?;
    let subject = dropped_subject(&repo, commit_oid)?;
    let mut outcome = outcome_from_plan(&repo, &plan, "drop", shared.dry_run)?;
    if !shared.dry_run {
        let identity = context::committer_reflog_identity(env, &config)?;
        apply_rewrite(
            &repo,
            &plan,
            &ApplyOptions {
                identity,
                reflog_message: format!("history drop: {subject}"),
            },
        )
        .map_err(|e| map_history_error(e, "drop", Some(commit_oid)))?;
    }
    outcome.dry_run = shared.dry_run;
    Ok(outcome)
}

/// Replay `upstream..branch` onto `onto` and move descendant refs.
pub fn run_rebase(
    branch: Option<String>,
    onto: String,
    upstream: Option<String>,
    keep_applied: bool,
    empty: EmptyPolicyArg,
    shared: &HistoryRewriteArgs,
) -> Result<HistoryRewriteOutcome> {
    let repo = context::discover()?;
    let onto_oid = resolve_revision_as_commit(&repo, &onto)
        .with_context(|| format!("could not resolve onto '{onto}'"))?;
    let env = repo.environment();
    let config =
        ConfigSet::load(env, Some(repo.git_dir()), true).context("could not load config")?;
    let committer = context::identity(
        env,
        &config,
        IdentRole::Committer,
        "GIT_COMMITTER_DATE",
        context::wall_clock_now(env),
    )?;
    let tip_oid = resolve_rebase_tip(&repo, branch.as_deref())?;
    let upstream_oid = match upstream {
        Some(spec) => resolve_revision_as_commit(&repo, &spec)
            .with_context(|| format!("could not resolve upstream '{spec}'"))?,
        None => default_rebase_upstream(&repo, &config, branch.as_deref(), onto_oid)?,
    };
    let plan = rebase_onto(
        &repo,
        upstream_oid,
        onto_oid,
        &[tip_oid],
        &RebaseOntoOptions {
            rewrite: RewriteOptions {
                scope: shared.update_refs.into(),
                empty: empty.into(),
                committer,
                preserve_committer: false,
            },
            skip_applied: !keep_applied,
        },
    )
    .map_err(|e| map_history_error(e, "rebase", None))?;
    verify_rewrite_plan(&repo, &plan).map_err(|e| map_history_error(e, "rebase", None))?;
    let mut outcome = outcome_from_plan(&repo, &plan, "rebase", shared.dry_run)?;
    if !shared.dry_run {
        let identity = context::committer_reflog_identity(env, &config)?;
        apply_rewrite(
            &repo,
            &plan,
            &ApplyOptions {
                identity,
                reflog_message: "history rebase".into(),
            },
        )
        .map_err(|e| map_history_error(e, "rebase", None))?;
    }
    outcome.dry_run = shared.dry_run;
    Ok(outcome)
}

fn resolve_rebase_tip(repo: &grit_lib::repo::Repository, branch: Option<&str>) -> Result<ObjectId> {
    match branch {
        Some(name) => {
            let full = if name.starts_with("refs/") {
                name.to_owned()
            } else {
                format!("refs/heads/{name}")
            };
            refs::resolve_ref(repo.git_dir(), &full)
                .with_context(|| format!("could not resolve branch '{name}'"))
        }
        None => head_commit_oid(repo),
    }
}

fn default_rebase_upstream(
    repo: &grit_lib::repo::Repository,
    config: &ConfigSet,
    branch: Option<&str>,
    onto: ObjectId,
) -> Result<ObjectId> {
    let short = match branch {
        Some(name) => name.to_owned(),
        None => match head_branch_short(repo) {
            Ok(name) => name,
            Err(_) => return Ok(onto),
        },
    };
    let short = short.as_str();
    let remote = config
        .get(&format!("branch.{short}.remote"))
        .filter(|r| !r.trim().is_empty());
    let merge = config.get(&format!("branch.{short}.merge"));
    if let (Some(remote), Some(merge)) = (remote, merge) {
        let tracking = if remote == "." {
            merge
                .strip_prefix("refs/heads/")
                .map(|b| format!("refs/heads/{b}"))
                .unwrap_or_else(|| merge.clone())
        } else {
            let mb = merge.strip_prefix("refs/heads/").unwrap_or(merge.as_str());
            format!("refs/remotes/{remote}/{mb}")
        };
        if let Ok(oid) = refs::resolve_ref(repo.git_dir(), &tracking) {
            return Ok(oid);
        }
    }
    Ok(onto)
}

fn head_branch_short(repo: &grit_lib::repo::Repository) -> Result<String> {
    match resolve_head(repo.git_dir())? {
        HeadState::Branch { short_name, .. } => Ok(short_name),
        HeadState::Detached { .. } => bail!("HEAD is detached; pass a branch name or set upstream"),
        HeadState::Invalid => bail!("HEAD is in an unknown state"),
    }
}

/// Fold `range` into the oldest selected commit and replay descendants.
pub fn run_squash(
    range: &[String],
    combine: bool,
    message: Option<String>,
    message_file: Option<String>,
    shared: &HistoryRewriteArgs,
) -> Result<HistoryRewriteOutcome> {
    if range.is_empty() {
        bail!("revision range is required");
    }
    let squash_message = parse_squash_message(combine, message, message_file)?;
    let repo = context::discover()?;
    let env = repo.environment();
    let config =
        ConfigSet::load(env, Some(repo.git_dir()), true).context("could not load config")?;
    let committer = context::identity(
        env,
        &config,
        IdentRole::Committer,
        "GIT_COMMITTER_DATE",
        context::wall_clock_now(env),
    )?;
    let range_refs: Vec<&str> = range.iter().map(String::as_str).collect();
    let plan = squash(
        &repo,
        &range_refs,
        &SquashOptions {
            message: squash_message,
            rewrite: RewriteOptions {
                scope: shared.update_refs.into(),
                empty: EmptyPolicy::Drop,
                committer,
                preserve_committer: false,
            },
        },
    )
    .map_err(|e| map_history_error(e, "squash", None))?;
    let reflog_range = range.join(" ");
    let mut outcome = outcome_from_plan(&repo, &plan, "squash", shared.dry_run)?;
    if !shared.dry_run {
        let identity = context::committer_reflog_identity(env, &config)?;
        apply_rewrite(
            &repo,
            &plan,
            &ApplyOptions {
                identity,
                reflog_message: format!("history squash: updating {reflog_range}"),
            },
        )
        .map_err(|e| map_history_error(e, "squash", None))?;
    }
    outcome.dry_run = shared.dry_run;
    Ok(outcome)
}

fn parse_squash_message(
    combine: bool,
    message: Option<String>,
    message_file: Option<String>,
) -> Result<SquashMessage> {
    let custom = match (message, message_file) {
        (Some(_), Some(_)) => bail!("use either -m/--message or --message-file, not both"),
        (None, None) => None,
        (Some(m), None) => Some(m),
        (None, Some(path)) => {
            if path == "-" {
                let mut buf = String::new();
                io::stdin()
                    .read_to_string(&mut buf)
                    .context("could not read message from stdin")?;
                Some(buf)
            } else {
                Some(
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("could not read message file '{path}'"))?,
                )
            }
        }
    };
    match (combine, custom) {
        (true, Some(_)) => bail!("use either --combine or -m/--message-file, not both"),
        (true, None) => Ok(SquashMessage::Combine),
        (false, Some(text)) => Ok(SquashMessage::Custom(text)),
        (false, None) => Ok(SquashMessage::Oldest),
    }
}
/// Apply staged changes onto `commit` and replay descendants.
pub fn run_fixup(
    commit: &str,
    message: Option<String>,
    message_file: Option<String>,
    empty: EmptyPolicyArg,
    shared: &HistoryRewriteArgs,
) -> Result<HistoryRewriteOutcome> {
    let message = read_optional_message(message, message_file)?;
    let repo = context::discover()?;
    let commit_oid = resolve_revision_as_commit(&repo, commit)
        .with_context(|| format!("could not resolve commit '{commit}'"))?;
    let staged_paths =
        staged_fixup_paths(&repo).map_err(|e| map_history_error(e, "fixup", None))?;
    let env = repo.environment();
    let config =
        ConfigSet::load(env, Some(repo.git_dir()), true).context("could not load config")?;
    let committer = context::identity(
        env,
        &config,
        IdentRole::Committer,
        "GIT_COMMITTER_DATE",
        context::wall_clock_now(env),
    )?;
    let plan = fixup(
        &repo,
        commit_oid,
        &FixupOptions {
            message,
            empty: empty.into(),
            rewrite: RewriteOptions {
                scope: shared.update_refs.into(),
                empty: empty.into(),
                committer,
                preserve_committer: false,
            },
        },
    )
    .map_err(|e| map_history_error(e, "fixup", None))?;
    let subject = fixup_reflog_subject(&repo, &plan, commit_oid, commit)?;
    let mut outcome = outcome_from_plan_fixup(&repo, &plan, shared.dry_run)?;
    if !shared.dry_run {
        let identity = context::committer_reflog_identity(env, &config)?;
        apply_fixup_rewrite(
            &repo,
            &plan,
            &ApplyOptions {
                identity,
                reflog_message: format!("history fixup: {subject}"),
            },
            &staged_paths,
        )
        .map_err(|e| map_history_error(e, "fixup", None))?;
    }
    outcome.dry_run = shared.dry_run;
    Ok(outcome)
}

fn fixup_reflog_subject(
    repo: &grit_lib::repo::Repository,
    plan: &RewritePlan,
    target: ObjectId,
    rev: &str,
) -> Result<String> {
    if let Some(row) = plan.commits.iter().find(|r| r.old == target) {
        if let Some(new) = row.new {
            let commit = context::read_commit(repo, &new)?;
            return Ok(context::subject_line(&commit.message));
        }
    }
    Ok(rev.to_owned())
}

fn outcome_from_plan_fixup(
    repo: &grit_lib::repo::Repository,
    plan: &RewritePlan,
    dry_run: bool,
) -> Result<HistoryRewriteOutcome> {
    outcome_from_plan(repo, plan, "fixup", dry_run)
}

fn read_message(message: Option<String>, message_file: Option<String>) -> Result<String> {
    match (message, message_file) {
        (Some(_), Some(_)) => bail!("use either -m/--message or --message-file, not both"),
        (None, None) => bail!("a new message is required (-m/--message or --message-file)"),
        (Some(m), None) => Ok(m),
        (None, Some(path)) => read_message_file(path),
    }
}

fn read_optional_message(
    message: Option<String>,
    message_file: Option<String>,
) -> Result<Option<String>> {
    match (message, message_file) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => bail!("use either --message-rest or --message-rest-file, not both"),
        (Some(m), None) => Ok(Some(m)),
        (None, Some(path)) => read_message_file(path).map(Some),
    }
}

fn read_message_file(path: String) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("could not read message from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(&path)
            .with_context(|| format!("could not read message file '{path}'"))
    }
}

fn dropped_subject(repo: &grit_lib::repo::Repository, dropped: ObjectId) -> Result<String> {
    let commit = context::read_commit(repo, &dropped)?;
    Ok(context::subject_line(&commit.message))
}

fn rewritten_subject(
    repo: &grit_lib::repo::Repository,
    plan: &RewritePlan,
    target: ObjectId,
) -> Result<String> {
    let new_oid = plan
        .commits
        .iter()
        .find(|r| r.old == target)
        .and_then(|r| r.new)
        .ok_or_else(|| anyhow::anyhow!("rewritten commit missing from plan"))?;
    let commit = context::read_commit(repo, &new_oid)?;
    Ok(context::subject_line(&commit.message))
}

fn outcome_from_plan(
    repo: &grit_lib::repo::Repository,
    plan: &RewritePlan,
    operation: &str,
    dry_run: bool,
) -> Result<HistoryRewriteOutcome> {
    let rewritten = plan
        .commits
        .iter()
        .map(|row| RewrittenPair {
            old: row.old.to_hex(),
            new: row.new.map(|new| new.to_hex()),
        })
        .collect();
    let refs = plan
        .ref_updates
        .iter()
        .map(|u| RefPair {
            name: u.name.clone(),
            old: u.old.to_hex(),
            new: u.new.to_hex(),
        })
        .collect();
    let head = head_pair(repo, plan)?;
    Ok(HistoryRewriteOutcome {
        operation: operation.into(),
        rewritten,
        refs,
        head,
        dry_run,
    })
}

fn head_pair(repo: &grit_lib::repo::Repository, plan: &RewritePlan) -> Result<Option<HeadPair>> {
    if !plan.head.commit_changes {
        return Ok(None);
    }
    let old_oid = head_commit_oid(repo)?;
    let new_oid = plan
        .mapped_tip(old_oid)
        .or_else(|| match resolve_head(repo.git_dir()).ok()? {
            HeadState::Branch { refname, .. } => plan
                .ref_updates
                .iter()
                .find(|u| u.name == refname)
                .map(|u| u.new),
            _ => None,
        });
    let Some(new_oid) = new_oid else {
        bail!("rewrite plan missing HEAD ref update");
    };
    Ok(Some(HeadPair {
        old: old_oid.to_hex(),
        new: new_oid.to_hex(),
    }))
}

fn head_commit_oid(repo: &grit_lib::repo::Repository) -> Result<ObjectId> {
    match resolve_head(repo.git_dir())? {
        HeadState::Branch { oid: Some(oid), .. } | HeadState::Detached { oid } => Ok(oid),
        HeadState::Branch { oid: None, .. } | HeadState::Invalid => {
            bail!("HEAD does not point at a commit")
        }
    }
}

fn map_history_error(
    err: HistoryError,
    operation: &str,
    drop_target: Option<ObjectId>,
) -> anyhow::Error {
    match err {
        HistoryError::EmptyMessage => anyhow::anyhow!("commit message cannot be empty"),
        HistoryError::BareRepository => {
            anyhow::anyhow!("cannot run fixup in a bare repository")
        }
        HistoryError::NothingStaged => anyhow::anyhow!("nothing to fixup: no staged changes"),
        HistoryError::FixupMakesTargetEmpty { commit } => {
            let short = &commit.to_hex()[..7];
            anyhow::anyhow!("fixup makes commit {short} empty")
        }
        HistoryError::SplitSelectsNothing => {
            anyhow::anyhow!("pathspec did not select any changes from the commit")
        }
        HistoryError::SplitSelectsEverything => {
            anyhow::anyhow!("pathspec selected every change in the commit")
        }
        HistoryError::Conflict { mut paths, .. } => {
            paths.sort();
            paths.dedup();
            json_error::operation_conflict(&format!("history {operation}"), paths)
        }
        HistoryError::Planning { message } => {
            anyhow::anyhow!("history rewrite planning failed: {message}")
        }
        HistoryError::RangeNotConnected => {
            anyhow::anyhow!("the selected commits are not a connected revision range")
        }
        HistoryError::MultipleTips => {
            anyhow::anyhow!("the revision range contains more than one tip commit")
        }
        HistoryError::MarkerTargetOutsideRange { commit } => {
            let short = &commit.to_hex()[..7];
            anyhow::anyhow!(
                "cannot squash {short}: its fixup/amend/squash target is outside the range"
            )
        }
        HistoryError::SquashDiscardsMessage { commit } => {
            let short = &commit.to_hex()[..7];
            anyhow::anyhow!(
                "squashing {short} would discard its message; use --combine or -m/--message"
            )
        }
        HistoryError::InsertBeforeOnMerge { commit } => {
            let short = &commit.to_hex()[..7];
            anyhow::anyhow!("insert-before on merge commit {short} is not supported")
        }
        HistoryError::MergeCommitInReplay { commit } => {
            let short = &commit.to_hex()[..7];
            if operation == "drop" && drop_target == Some(commit) {
                anyhow::anyhow!("cannot drop merge commit {short}")
            } else if operation == "drop" {
                anyhow::anyhow!("replaying merge commits is not supported yet (commit {short})")
            } else {
                anyhow::anyhow!(
                    "{short} is a merge commit — grit history {operation} cannot replay merges yet"
                )
            }
        }
        HistoryError::RootCommit { .. } if operation == "squash" => {
            anyhow::anyhow!("cannot squash down to root commit")
        }
        HistoryError::RootCommit { commit } => {
            let short = &commit.to_hex()[..7];
            if operation == "drop" {
                anyhow::anyhow!("cannot drop root commit {short}")
            } else {
                anyhow::anyhow!(
                    "{short} is a root commit and cannot be dropped or replayed without a parent"
                )
            }
        }
        HistoryError::BecameEmpty { commit } => {
            let short = &commit.to_hex()[..7];
            anyhow::anyhow!("replay of {short} would become empty — refusing rewrite")
        }
        HistoryError::NotInHistory { commit } => {
            let short = &commit.to_hex()[..7];
            anyhow::anyhow!("{short} is not reachable from HEAD for this rewrite scope")
        }
        HistoryError::DivergentBranch { name } => anyhow::anyhow!(
            "branch {name} diverged before the rewritten commits and cannot be updated"
        ),
        HistoryError::BranchCheckedOutElsewhere { name, worktree } => {
            anyhow::anyhow!("branch {name} is checked out in worktree {worktree}")
        }
        HistoryError::LocalChangesWouldBeOverwritten { mut paths } => {
            paths.sort();
            paths.dedup();
            anyhow::anyhow!(
                "local changes would be overwritten:\n  {}",
                paths.join("\n  ")
            )
        }
        HistoryError::RefCasMismatch { name } => anyhow::anyhow!(
            "ref {name} changed since the rewrite plan was built — retry the command"
        ),
        HistoryError::RefApply { message } => {
            anyhow::anyhow!("ref update failed: {message}")
        }
        HistoryError::WorktreeApply { message } => {
            anyhow::anyhow!("worktree update failed: {message}")
        }
        _ => anyhow::anyhow!("{err}"),
    }
}
