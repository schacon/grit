//! `grit reset` — reset current HEAD to the specified state.
//!
//! Implements the following modes:
//!
//! - `--soft`  : move HEAD only; index and working tree unchanged.
//! - `--mixed` : move HEAD and reset index to the target tree (default).
//! - `--hard`  : move HEAD, reset index, and update working tree.
//! - `--keep`  : like --hard but refuse if uncommitted local changes would be lost.
//!
//! When path arguments are given the HEAD is not moved; only the index entries
//! for those paths are reset to the content of the target commit's tree.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use grit_lib::config::ConfigSet;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::refs::{append_reflog, resolve_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    abbreviate_object_id, resolve_revision, resolve_revision_as_commit,
    revision_spec_contains_ancestry_navigation, split_treeish_colon,
};
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::submodule_gitdir::submodule_modules_git_dir;
use grit_lib::unicode_normalization::precompose_utf8_path;
use similar::{Algorithm, TextDiff};

/// The zero OID for reflog entries when there is no previous value.
fn zero_oid() -> ObjectId {
    ObjectId::zero()
}

/// Whether `reset` should recurse into submodules after updating the superproject work tree.
/// True when argv included `--recurse-submodules` and it was not negated to off.
///
/// Git only relaxes certain safety checks for explicit `--recurse-submodules`; `submodule.recurse`
/// alone should not skip "submodule would be overwritten" for plain `reset --keep` (`t7112`).
fn explicit_recurse_submodules_cli_on(args: &Args) -> bool {
    let Some(ref v) = args.recurse_submodules else {
        return false;
    };
    let l = v.trim().to_ascii_lowercase();
    !matches!(l.as_str(), "no" | "off" | "false" | "0")
}

fn effective_reset_recurse_submodules(repo: &Repository, args: &Args) -> Result<bool> {
    if args.no_recurse_submodules {
        return Ok(false);
    }
    if let Some(v) = args.recurse_submodules.as_deref() {
        let l = v.trim().to_ascii_lowercase();
        if matches!(l.as_str(), "no" | "off" | "false" | "0") {
            return Ok(false);
        }
        if matches!(l.as_str(), "yes" | "on" | "true" | "1" | "") {
            return Ok(true);
        }
        bail!("bad --recurse-submodules argument: {v}");
    }
    if let Ok(cfg) = ConfigSet::load(Some(&repo.git_dir), true) {
        if let Some(v) = cfg.get("submodule.recurse") {
            let l = v.to_ascii_lowercase();
            if l == "true" || l == "1" || l == "yes" {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn submodule_update_after_reset(repo: &Repository) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("submodule recursion requires a work tree"))?;
    let grit_bin = crate::grit_exe::grit_executable();
    let mut cmd = Command::new(&grit_bin);
    crate::grit_exe::strip_trace2_env(&mut cmd);
    let status = cmd
        .args(["submodule", "update", "--init", "--recursive"])
        .current_dir(work_tree)
        .status()
        .context("spawning submodule update after reset")?;
    if !status.success() {
        bail!("Submodule update failed after reset");
    }
    Ok(())
}

/// Restore HEAD, index, and work tree to `old_oid` after a failed `submodule update`.
///
/// `broken_index` is the superproject index that was built for the attempted target commit (before
/// the index file was written). `failed_tip` is the commit HEAD was moved to.
fn rollback_reset_after_failed_submodule_update(
    repo: &Repository,
    head: &HeadState,
    old_oid: &ObjectId,
    failed_tip: &ObjectId,
    broken_index: &Index,
    recurse_submodules: bool,
) -> Result<()> {
    let tree_oid = commit_to_tree(repo, old_oid)?;
    let tree_entries = tree_to_flat_entries(repo, &tree_oid, "")?;
    let mut rollback_index = Index::new();
    rollback_index.entries = tree_entries;
    rollback_index.sort();
    let index_path = repo.index_path();
    let old_before = repo
        .load_index_at(&index_path)
        .unwrap_or_else(|_| Index::new());
    preserve_index_cache_flags_from(&old_before, &mut rollback_index);
    checkout_index_to_worktree(
        repo,
        broken_index,
        &mut rollback_index,
        None,
        recurse_submodules,
        recurse_submodules,
    )?;
    repo.write_index_at(&index_path, &mut rollback_index)
        .context("writing index during rollback")?;
    update_head_ref(&repo.git_dir, head, old_oid)?;
    let identity = resolve_reflog_identity(repo);
    let msg = "reset: rolling back after submodule update failure";
    let _ = append_reflog(
        &repo.git_dir,
        "HEAD",
        failed_tip,
        old_oid,
        &identity,
        msg,
        false,
    );
    if let HeadState::Branch { refname, .. } = head {
        let _ = append_reflog(
            &repo.git_dir,
            refname,
            failed_tip,
            old_oid,
            &identity,
            msg,
            false,
        );
    }
    Ok(())
}

/// When rebuilding the index from a tree (`reset` mixed/hard/merge), preserve cache-entry flags
/// from the previous index for paths that still exist at stage 0.
///
/// Git keeps `CE_SKIP_WORKTREE` and `CE_VALID` (assume-unchanged) across this rebuild so sparse
/// checkout state is not lost (`t7011-skip-worktree-reading`).
fn preserve_index_cache_flags_from(old: &Index, new: &mut Index) {
    for ne in new.entries.iter_mut() {
        if ne.stage() != 0 {
            continue;
        }
        let Some(oe) = old.get(&ne.path, 0) else {
            continue;
        };
        if oe.assume_unchanged() {
            ne.set_assume_unchanged(true);
        }
        if oe.skip_worktree() {
            ne.set_skip_worktree(true);
            if new.version < 3 {
                new.version = 3;
            }
        }
    }
}

/// The reset mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ResetMode {
    Soft,
    #[default]
    Mixed,
    Hard,
    Keep,
    Merge,
}

impl ResetMode {
    fn name(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Mixed => "mixed",
            Self::Hard => "hard",
            Self::Keep => "keep",
            Self::Merge => "merge",
        }
    }
}

/// Arguments for `grit reset`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Move HEAD only; do not touch the index or working tree.
    #[arg(long)]
    pub soft: bool,

    /// Reset index to the target tree but leave working tree unchanged (default).
    #[arg(long)]
    pub mixed: bool,

    /// Reset index and working tree to the target tree.
    #[arg(long)]
    pub hard: bool,

    /// Like --hard but refuse to reset if uncommitted changes would be lost.
    #[arg(long)]
    pub keep: bool,

    /// Reset index and working tree like --hard, but keep local changes where possible.
    #[arg(long = "merge")]
    pub merge: bool,

    /// Suppress feedback messages.
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Record the fact that removed paths will be re-added later (intent-to-add).
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Do not refresh the index after a mixed reset.
    #[arg(long = "no-refresh")]
    pub no_refresh: bool,

    /// Refresh the index after a mixed reset (default).
    #[arg(long = "refresh")]
    pub refresh: bool,

    /// Interactive patch mode.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// After updating the working tree, run `submodule update --init --recursive` (Git-compatible
    /// bool-ish values when `=VALUE` is given).
    #[arg(
        long = "recurse-submodules",
        num_args = 0..=1,
        default_missing_value = "true",
        require_equals = true
    )]
    pub recurse_submodules: Option<String>,

    /// Disable submodule recursion even when `submodule.recurse` is set.
    #[arg(long = "no-recurse-submodules")]
    pub no_recurse_submodules: bool,

    /// Remaining positional arguments: `[<commit>] [--] [<path>…]`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub rest: Vec<String>,

    /// When true, `reset --merge` does not remove `CHERRY_PICK_HEAD` / `REVERT_HEAD` (sequencer abort).
    #[arg(skip)]
    pub skip_sequencer_head_cleanup: bool,
}

/// Pre-validate raw arguments before clap parsing, catching Git-specific
/// negated flags that clap doesn't know about.
pub fn pre_validate_args(raw_args: &[String]) -> Result<()> {
    for arg in raw_args {
        // Check for negated reset mode flags
        for mode in &["soft", "mixed", "hard", "merge", "keep"] {
            if arg == &format!("--no-{mode}") {
                bail!("unknown option `no-{mode}'");
            }
        }
    }
    Ok(())
}

/// Filter out `--end-of-options` from args (replace with `--`).
pub fn filter_args(raw_args: &[String]) -> Vec<String> {
    raw_args
        .iter()
        .map(|a| {
            if a == "--end-of-options" {
                "--".to_owned()
            } else {
                a.clone()
            }
        })
        .collect()
}

/// Pull mode / misc flags out of `rest` when they appear after the commit (Git-compatible argv).
fn normalize_reset_trailing_args(args: &mut Args) {
    let mut i = 0usize;
    while i < args.rest.len() {
        match args.rest[i].as_str() {
            "--soft" => {
                args.soft = true;
                args.rest.remove(i);
            }
            "--mixed" => {
                args.mixed = true;
                args.rest.remove(i);
            }
            "--hard" => {
                args.hard = true;
                args.rest.remove(i);
            }
            "--keep" => {
                args.keep = true;
                args.rest.remove(i);
            }
            "--merge" => {
                args.merge = true;
                args.rest.remove(i);
            }
            "-q" | "--quiet" => {
                args.quiet = true;
                args.rest.remove(i);
            }
            "-N" | "--intent-to-add" => {
                args.intent_to_add = true;
                args.rest.remove(i);
            }
            "--no-refresh" => {
                args.no_refresh = true;
                args.rest.remove(i);
            }
            "--refresh" => {
                args.refresh = true;
                args.rest.remove(i);
            }
            "--no-recurse-submodules" => {
                args.no_recurse_submodules = true;
                args.rest.remove(i);
            }
            s if s == "--recurse-submodules" || s.starts_with("--recurse-submodules=") => {
                if let Some(eq) = s.find('=') {
                    args.recurse_submodules = Some(s[eq + 1..].to_owned());
                } else {
                    args.recurse_submodules = Some("true".to_owned());
                }
                args.rest.remove(i);
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Run `grit reset`.
pub fn run(mut args: Args) -> Result<()> {
    // Git accepts `reset <commit> --hard`; clap's `trailing_var_arg` collects `--hard` into
    // `rest` so it is never parsed as a flag. Strip known options from `rest` first.
    normalize_reset_trailing_args(&mut args);

    let mode = parse_mode(&args)?;

    let repo = Repository::discover(None).context("not a git repository")?;

    if args.recurse_submodules.is_some() && args.patch {
        bail!("options '--recurse-submodules' and '--patch' cannot be used together");
    }
    if args.recurse_submodules.is_some() && mode == ResetMode::Mixed {
        bail!("fatal: --recurse-submodules cannot be used with --mixed");
    }

    // Handle -p (patch mode): interactive partial unstaging / index application vs a treeish.
    if args.patch {
        return reset_patch(&repo, &args.rest);
    }

    // Split positional args into (commit_spec, paths).
    let (commit_spec, paths) = split_commit_and_paths(&repo, &args.rest);

    if !paths.is_empty() {
        // Pathspec reset: only update index entries, HEAD stays put.
        if mode != ResetMode::Mixed {
            bail!("Cannot do --{} reset with paths.", mode.name());
        }
        return reset_paths(&repo, &commit_spec, &paths, args.quiet, args.intent_to_add);
    }

    reset_commit(&repo, &commit_spec, mode, args.quiet, &mut args)
}

/// Parse the reset mode from the flag combination.
fn parse_mode(args: &Args) -> Result<ResetMode> {
    match (args.soft, args.mixed, args.hard, args.keep, args.merge) {
        (true, false, false, false, false) => Ok(ResetMode::Soft),
        (false, true, false, false, false) => Ok(ResetMode::Mixed),
        (false, false, true, false, false) => Ok(ResetMode::Hard),
        (false, false, false, true, false) => Ok(ResetMode::Keep),
        (false, false, false, false, true) => Ok(ResetMode::Merge),
        (false, false, false, false, false) => Ok(ResetMode::default()),
        _ => bail!("cannot mix --soft, --mixed, --hard, --keep, and --merge"),
    }
}

/// Split positional arguments into `(commit_spec, paths)`.
///
/// Handles the `--` end-of-options separator explicitly (clap passes it
/// through when `trailing_var_arg` is in use).  If the first argument
/// resolves as a commit-ish it is used as the commit spec and the rest are
/// paths; otherwise `"HEAD"` is assumed and all arguments are paths.
fn split_commit_and_paths(repo: &Repository, rest: &[String]) -> (String, Vec<String>) {
    if rest.is_empty() {
        return ("HEAD".to_owned(), vec![]);
    }

    // Skip reset mode / misc flags that can appear in the trailing var-arg slice
    // when argv is parsed loosely (e.g. `reset --hard main` must not treat `--hard`
    // as the first pathspec).
    let mut i = 0usize;
    while i < rest.len() {
        let a = rest[i].as_str();
        if matches!(
            a,
            "--soft"
                | "--mixed"
                | "--hard"
                | "--merge"
                | "--keep"
                | "-q"
                | "--quiet"
                | "-N"
                | "--intent-to-add"
                | "--no-refresh"
                | "--refresh"
                | "--no-recurse-submodules"
        ) || a == "--recurse-submodules"
            || a.starts_with("--recurse-submodules=")
        {
            i += 1;
            continue;
        }
        break;
    }
    let rest = if i > 0 { &rest[i..] } else { rest };
    if rest.is_empty() {
        return ("HEAD".to_owned(), vec![]);
    }

    // Detect an explicit `--` or `--end-of-options` separator.
    if let Some(sep) = rest
        .iter()
        .position(|a| a == "--" || a == "--end-of-options")
    {
        // Everything before `--` is the optional commit; everything after is paths.
        let commit_spec = if sep == 0 {
            "HEAD".to_owned()
        } else {
            rest[0].clone()
        };
        let paths = rest[sep + 1..].to_vec();
        return (commit_spec, paths);
    }

    let first = &rest[0];
    // Resolve first arg as a commit-ish. If a worktree file shadows `main` etc.,
    // still prefer `refs/heads/<name>` when that branch exists (matches Git).
    let mut commit_spec = resolve_reset_first_arg_as_commit(repo, first);
    if commit_spec.is_none()
        && grit_lib::rev_parse::revision_spec_contains_ancestry_navigation(first)
    {
        // When resolution fails but the token still uses Git ancestry syntax (`HEAD~1`,
        // `main^2`, …), treat it as a commit spec anyway so `reset --hard HEAD~1` runs
        // the commit reset path (updating the working tree). Otherwise grit would fall
        // back to pathspec reset against `HEAD`, silently skip checkout, and leave
        // removed index paths on disk — breaking tests that `reset` after `clean`.
        commit_spec = Some(first.to_owned());
    }

    if let Some(spec) = commit_spec {
        let paths: Vec<String> = rest[1..]
            .iter()
            .filter(|a| {
                let s = a.as_str();
                !matches!(
                    s,
                    "--soft"
                        | "--mixed"
                        | "--hard"
                        | "--merge"
                        | "--keep"
                        | "-q"
                        | "--quiet"
                        | "-N"
                        | "--intent-to-add"
                        | "--no-refresh"
                        | "--refresh"
                        | "--no-recurse-submodules"
                ) && s != "--recurse-submodules"
                    && !s.starts_with("--recurse-submodules=")
            })
            .cloned()
            .collect();
        (spec, paths)
    } else {
        ("HEAD".to_owned(), rest.to_vec())
    }
}

/// If `first` names a commit for `git reset`, return the spec to pass to rev-parse.
fn resolve_reset_first_arg_as_commit(repo: &Repository, first: &str) -> Option<String> {
    // Always treat `HEAD` / `@` as a tree-ish so `git reset HEAD <path>` splits into
    // commit `HEAD` and pathspecs — not pathspecs `HEAD` and `<path>` (t3910).
    if first == "HEAD" || first == "@" {
        return Some("HEAD".to_owned());
    }
    if resolve_revision_as_commit(repo, first).is_ok() {
        return Some(first.to_owned());
    }
    if first.contains('/') || first.starts_with('.') {
        return None;
    }
    let full = format!("refs/heads/{first}");
    if resolve_ref(&repo.git_dir, &full).is_ok() && resolve_revision_as_commit(repo, &full).is_ok()
    {
        return Some(full);
    }
    None
}

/// Resolve the committer identity for reflog entries.
fn resolve_reflog_identity(repo: &Repository) -> String {
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

/// Write reflog entries for a reset operation.
fn write_reset_reflog(
    repo: &Repository,
    head: &HeadState,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    commit_spec: &str,
) {
    let identity = resolve_reflog_identity(repo);
    let message = format!("reset: moving to {commit_spec}");

    match head {
        HeadState::Branch { refname, .. } => {
            let _ = append_reflog(
                &repo.git_dir,
                refname,
                old_oid,
                new_oid,
                &identity,
                &message,
                false,
            );
            let _ = append_reflog(
                &repo.git_dir,
                "HEAD",
                old_oid,
                new_oid,
                &identity,
                &message,
                false,
            );
        }
        _ => {
            let _ = append_reflog(
                &repo.git_dir,
                "HEAD",
                old_oid,
                new_oid,
                &identity,
                &message,
                false,
            );
        }
    }
}

/// Map `rev-parse` failures to the same stderr shape as `git reset -p`.
fn map_reset_patch_rev_error(spec: &str, err: grit_lib::error::Error) -> anyhow::Error {
    let s = err.to_string();
    if s.contains("Could not parse object") {
        anyhow::anyhow!("fatal: Could not parse object '{spec}'.")
    } else if s.contains("ambiguous argument") {
        err.into()
    } else if s.contains("unknown revision") || s.contains("ObjectNotFound") {
        anyhow::anyhow!(
            "fatal: ambiguous argument '{spec}': unknown revision or path not in the working tree.\n\
Use '--' to separate paths from revisions, like this:\n\
'git <command> [<revision>...] -- [<file>...]'"
        )
    } else {
        err.into()
    }
}

/// Whether `token` looks like a short/full hex object id (for `reset -p` error propagation).
fn looks_like_hex_object_id(token: &str) -> bool {
    let t = token.trim();
    (4..=40).contains(&t.len()) && t.chars().all(|c| c.is_ascii_hexdigit())
}

/// First positional for `reset -p`: same DWIM as path-less `reset` (commit-ish vs pathspec), plus
/// explicit treeish forms (`rev:path`, `...^{tree}`) that are not commit-ish.
fn resolve_reset_patch_first_arg(repo: &Repository, first: &str) -> Result<Option<String>> {
    let explicit_treeish = split_treeish_colon(first).is_some() || first.contains("^{");
    if explicit_treeish {
        return resolve_revision(repo, first)
            .map(|_| Some(first.to_owned()))
            .map_err(|e| map_reset_patch_rev_error(first, e));
    }

    if let Some(spec) = resolve_reset_first_arg_as_commit(repo, first) {
        return Ok(Some(spec));
    }
    if revision_spec_contains_ancestry_navigation(first) {
        return Ok(Some(first.to_owned()));
    }
    if looks_like_hex_object_id(first) {
        return resolve_revision(repo, first)
            .map(|_| Some(first.to_owned()))
            .map_err(|e| map_reset_patch_rev_error(first, e));
    }
    Ok(None)
}

/// Split `rest` for `reset -p` into `(treeish_spec, pathspecs)` (Git-compatible).
fn split_reset_patch_args(repo: &Repository, rest: &[String]) -> Result<(String, Vec<String>)> {
    if rest.is_empty() {
        return Ok(("HEAD".to_owned(), vec![]));
    }

    let mut i = 0usize;
    while i < rest.len() {
        let a = rest[i].as_str();
        if matches!(
            a,
            "--soft"
                | "--mixed"
                | "--hard"
                | "--merge"
                | "--keep"
                | "-q"
                | "--quiet"
                | "-N"
                | "--intent-to-add"
                | "--no-refresh"
                | "--refresh"
                | "--no-recurse-submodules"
        ) || a == "--recurse-submodules"
            || a.starts_with("--recurse-submodules=")
        {
            i += 1;
            continue;
        }
        break;
    }
    let rest = if i > 0 { &rest[i..] } else { rest };
    if rest.is_empty() {
        return Ok(("HEAD".to_owned(), vec![]));
    }

    if let Some(sep) = rest
        .iter()
        .position(|a| a == "--" || a == "--end-of-options")
    {
        let commit_spec = if sep == 0 {
            "HEAD".to_owned()
        } else {
            rest[0].clone()
        };
        let paths = rest[sep + 1..].to_vec();
        if sep == 0 {
            return Ok((commit_spec, paths));
        }
        let treeish = resolve_reset_patch_first_arg(repo, &commit_spec)?;
        let spec = treeish.unwrap_or(commit_spec);
        return Ok((spec, paths));
    }

    let first = &rest[0];
    let treeish = resolve_reset_patch_first_arg(repo, first)?;
    if let Some(spec) = treeish {
        let paths: Vec<String> = rest[1..]
            .iter()
            .filter(|a| {
                let s = a.as_str();
                !matches!(
                    s,
                    "--soft"
                        | "--mixed"
                        | "--hard"
                        | "--merge"
                        | "--keep"
                        | "-q"
                        | "--quiet"
                        | "-N"
                        | "--intent-to-add"
                        | "--no-refresh"
                        | "--refresh"
                        | "--no-recurse-submodules"
                ) && s != "--recurse-submodules"
                    && !s.starts_with("--recurse-submodules=")
            })
            .cloned()
            .collect();
        Ok((spec, paths))
    } else {
        Ok(("HEAD".to_owned(), rest.to_vec()))
    }
}

/// Peel a resolved object to a tree OID (`commit` → its root tree).
fn tree_oid_for_treeish(repo: &Repository, oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&oid)?;
    match obj.kind {
        ObjectKind::Tree => Ok(oid),
        ObjectKind::Commit => Ok(parse_commit(&obj.data)?.tree),
        _ => bail!("object {oid} is not a commit or tree"),
    }
}

/// Resolve the tree OID for `git reset -p [<treeish>]`. Rejects `rev:path` blob forms.
fn validate_reset_patch_treeish(repo: &Repository, treeish_spec: &str) -> Result<ObjectId> {
    if let Some((lhs, rhs)) = split_treeish_colon(treeish_spec) {
        if !lhs.is_empty() && !rhs.is_empty() {
            let oid = resolve_revision(repo, treeish_spec)
                .map_err(|e| map_reset_patch_rev_error(treeish_spec, e))?;
            let obj = repo
                .odb
                .read(&oid)
                .with_context(|| format!("reading object for '{treeish_spec}'"))?;
            if obj.kind == ObjectKind::Blob {
                bail!("fatal: Could not parse object '{treeish_spec}'.");
            }
        }
    }
    let oid = resolve_revision(repo, treeish_spec)
        .map_err(|e| map_reset_patch_rev_error(treeish_spec, e))?;
    tree_oid_for_treeish(repo, oid)
}

/// Interactive patch-mode reset (`git reset -p`): partial index updates toward `treeish`.
fn reset_patch(repo: &Repository, rest: &[String]) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let (treeish_spec, path_args) = split_reset_patch_args(repo, rest)?;
    let cwd = std::env::current_dir().context("resolving cwd")?;
    let filter_paths: Vec<String> = path_args
        .iter()
        .map(|p| crate::commands::checkout::resolve_pathspec(p, work_tree, &cwd))
        .collect();

    let target_tree_oid = validate_reset_patch_treeish(repo, &treeish_spec)?;
    let target_entries = tree_to_flat_entries(repo, &target_tree_oid, "")?;
    let target_map: HashMap<Vec<u8>, IndexEntry> = target_entries
        .into_iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("loading index")?;

    let mut staged_paths: Vec<Vec<u8>> = Vec::new();
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        if !crate::commands::checkout::patch_path_filter_matches(&path_str, &filter_paths) {
            continue;
        }
        let in_tree = target_map.get(&entry.path);
        let differs = match in_tree {
            Some(te) => te.oid != entry.oid || te.mode != entry.mode,
            None => true,
        };
        if differs {
            staged_paths.push(entry.path.clone());
        }
    }
    for path in target_map.keys() {
        if !index
            .entries
            .iter()
            .any(|e| e.path == *path && e.stage() == 0)
            && !staged_paths.contains(path)
        {
            let path_str = String::from_utf8_lossy(path);
            if crate::commands::checkout::patch_path_filter_matches(&path_str, &filter_paths) {
                staged_paths.push(path.clone());
            }
        }
    }

    if staged_paths.is_empty() {
        return Ok(());
    }

    staged_paths.sort();

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut out = io::stdout();

    for path in staged_paths {
        let path_str = String::from_utf8_lossy(&path).into_owned();

        let index_blob = index.get(&path, 0).map(|e| e.oid);
        let tree_entry = target_map.get(&path);
        let tree_oid = tree_entry.map(|e| e.oid);
        let tree_mode = tree_entry.map(|e| e.mode);

        // Myers diff with **old = target tree**, **new = index** so the printed patch and
        // `blend_line_diff_by_hunk_ranges` match Git (`reset -p` applies the tree side when a hunk
        // is accepted).
        let (index_bytes, tree_bytes) = match (index_blob, tree_oid) {
            (Some(ib), Some(to)) => {
                let iobj = repo.odb.read(&ib)?;
                let tobj = repo.odb.read(&to)?;
                if iobj.kind != ObjectKind::Blob || tobj.kind != ObjectKind::Blob {
                    continue;
                }
                (iobj.data, tobj.data)
            }
            (Some(ib), None) => {
                let iobj = repo.odb.read(&ib)?;
                if iobj.kind != ObjectKind::Blob {
                    continue;
                }
                (iobj.data, Vec::new())
            }
            (None, Some(to)) => {
                let tobj = repo.odb.read(&to)?;
                if tobj.kind != ObjectKind::Blob {
                    continue;
                }
                (Vec::new(), tobj.data)
            }
            (None, None) => continue,
        };

        let tree_str = String::from_utf8_lossy(&tree_bytes);
        let index_str = String::from_utf8_lossy(&index_bytes);
        let text_diff = TextDiff::configure()
            .algorithm(Algorithm::Myers)
            .diff_lines(tree_str.as_ref(), index_str.as_ref());
        let ops: Vec<_> = text_diff.ops().to_vec();
        let has_change = ops
            .iter()
            .any(|o| !matches!(o, similar::DiffOp::Equal { .. }));
        if !has_change {
            continue;
        }

        let n_ops = ops.len();
        let mut hunk_ranges: Vec<(usize, usize)> = vec![(0, n_ops)];
        let mut accepted = vec![false; hunk_ranges.len()];
        let mut hunk_cursor = 0usize;

        let verb = if treeish_spec == "HEAD" || treeish_spec == "@" {
            "Unstage"
        } else {
            "Apply"
        };

        'hunk_loop: loop {
            let n_hunks = hunk_ranges.len();
            if hunk_cursor >= n_hunks {
                break;
            }

            let display_idx = hunk_cursor + 1;
            let (s, e) = hunk_ranges[hunk_cursor];
            let hunk_only = crate::commands::stash::partial_unified_for_op_range(
                path_str.as_str(),
                &tree_bytes,
                &index_bytes,
                &ops[s..e],
                3,
            );

            writeln!(out, "diff --git a/{path_str} b/{path_str}").ok();
            write!(out, "--- a/{path_str}\n+++ b/{path_str}\n").ok();
            write!(out, "{hunk_only}").ok();
            write!(
                out,
                "({display_idx}/{n_hunks}) {verb} this hunk to index [y,n,q,a,d,s,e,?]? "
            )
            .ok();
            out.flush().ok();

            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let answer = line.trim();
            match answer {
                "y" | "Y" => {
                    accepted[hunk_cursor] = true;
                    hunk_cursor += 1;
                }
                "n" | "N" => {
                    hunk_cursor += 1;
                }
                "a" | "A" => {
                    for j in hunk_cursor..n_hunks {
                        accepted[j] = true;
                    }
                    break 'hunk_loop;
                }
                "d" | "D" => {
                    break 'hunk_loop;
                }
                "q" | "Q" => {
                    break;
                }
                "s" | "S" => {
                    if !crate::commands::stash::split_hunk_at_first_gap(
                        &mut hunk_ranges,
                        hunk_cursor,
                        &ops,
                    ) {
                        continue 'hunk_loop;
                    }
                    let n = hunk_ranges.len();
                    accepted.resize(n, false);
                    continue 'hunk_loop;
                }
                _ => {
                    hunk_cursor += 1;
                }
            }
        }

        if !accepted.iter().any(|&a| a) {
            continue;
        }

        let blended = crate::commands::checkout::blend_line_diff_by_hunk_ranges(
            &tree_bytes,
            &index_bytes,
            &hunk_ranges,
            &accepted,
        );
        let blended_bytes = blended.into_bytes();

        if blended_bytes == index_bytes {
            continue;
        }

        index.remove(&path);
        if blended_bytes.is_empty() {
            if let Some(te) = tree_entry {
                if te.mode == MODE_GITLINK {
                    index.add_or_replace(te.clone());
                }
            }
            continue;
        }

        let blob_oid = Odb::hash_object_data(ObjectKind::Blob, &blended_bytes);
        let mode = tree_mode.unwrap_or(0o100644);
        let path_bytes = path.clone();
        let abs_file = work_tree.join(&path_str);
        let (cs, cns, ms, mns, dev, ino, fsz) = if let Ok(m) = std::fs::symlink_metadata(&abs_file)
        {
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
            (0, 0, 0, 0, 0, 0, blended_bytes.len() as u32)
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
    }

    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;
    Ok(())
}

/// Reset specific index entries to match the given commit's tree.
///
/// HEAD is not modified.
fn path_matches_index_or_tree_key(path_str: &str, key: &[u8], precompose_unicode: bool) -> bool {
    if path_str.as_bytes() == key {
        return true;
    }
    if !precompose_unicode {
        return false;
    }
    let key_str = String::from_utf8_lossy(key);
    precompose_utf8_path(path_str).as_ref() == precompose_utf8_path(key_str.as_ref()).as_ref()
}

fn reset_paths(
    repo: &Repository,
    commit_spec: &str,
    paths: &[String],
    _quiet: bool,
    intent_to_add: bool,
) -> Result<()> {
    if repo.work_tree.is_none() {
        bail!("fatal: mixed reset is not allowed in a bare repository");
    }
    // On an unborn branch, the tree is empty (no commit exists yet).
    let tree_entries = match resolve_to_commit(repo, commit_spec) {
        Ok(commit_oid) => {
            let tree_oid = commit_to_tree(repo, &commit_oid)?;
            tree_to_flat_entries(repo, &tree_oid, "")?
        }
        Err(_) => {
            // Check if HEAD is unborn
            let head = resolve_head(&repo.git_dir)?;
            if head.oid().is_none() && commit_spec == "HEAD" {
                Vec::new() // empty tree
            } else {
                bail!(
                    "unknown revision: '{}': object not found: {}",
                    commit_spec,
                    commit_spec
                );
            }
        }
    };

    // Build a lookup table: path bytes → IndexEntry.
    let mut tree_map: HashMap<Vec<u8>, IndexEntry> = HashMap::new();
    for e in tree_entries {
        tree_map.insert(e.path.clone(), e);
    }

    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("loading index")?;
    let precompose_unicode =
        grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir));

    for path_str in paths {
        let path_bytes = path_str.as_bytes().to_vec();

        let tree_key_bytes = tree_map
            .keys()
            .find(|k| path_matches_index_or_tree_key(path_str, k, precompose_unicode))
            .cloned();
        let in_tree = tree_key_bytes.is_some();
        let index_match = index.entries.iter().find(|e| {
            e.stage() == 0 && path_matches_index_or_tree_key(path_str, &e.path, precompose_unicode)
        });
        let in_index = index_match.is_some();
        if !in_tree && !in_index {
            bail!("pathspec '{path_str}' did not match any file(s) known to git");
        }

        let resolved_bytes = index_match
            .map(|e| e.path.clone())
            .or(tree_key_bytes.clone())
            .unwrap_or_else(|| path_bytes.clone());

        // Remove all stages for this path.
        index.remove(&resolved_bytes);
        // Re-add from tree if present.
        if let Some(ref tk) = tree_key_bytes {
            if let Some(entry) = tree_map.get(tk) {
                index.add_or_replace(entry.clone());
                if entry.mode == MODE_GITLINK {
                    continue;
                }
            }
        } else if intent_to_add {
            // With -N, keep removed paths as intent-to-add (empty blob OID, like `add -N`).
            let empty_oid = repo
                .odb
                .write(ObjectKind::Blob, b"")
                .context("writing empty blob for intent-to-add index entry")?;
            let mut ita_entry = IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                size: 0,
                oid: empty_oid,
                flags: resolved_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: resolved_bytes.clone(),
            };
            ita_entry.set_intent_to_add(true);
            if index.version < 3 {
                index.version = 3;
            }
            index.add_or_replace(ita_entry);
        }
        // If not in tree and no -N, path is removed from index (staged deletion).
    }

    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;
    Ok(())
}

/// Reset HEAD (and optionally index + working tree) to the given commit.
fn reset_commit(
    repo: &Repository,
    commit_spec: &str,
    mode: ResetMode,
    quiet: bool,
    extra: &mut Args,
) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    // --soft fails when there are unmerged entries or a merge is in progress.
    if mode == ResetMode::Soft {
        if repo.git_dir.join("MERGE_HEAD").exists() {
            bail!("Cannot do a soft reset in the middle of a merge.");
        }
        if repo.git_dir.join("CHERRY_PICK_HEAD").exists() {
            bail!("Cannot do a soft reset in the middle of a cherry-pick.");
        }
        if repo.git_dir.join("REVERT_HEAD").exists() {
            bail!("Cannot do a soft reset in the middle of a revert.");
        }
        let index_path = repo.index_path();
        let index = repo.load_index_at(&index_path).context("loading index")?;
        if index.entries.iter().any(|e| e.stage() != 0) {
            bail!("Cannot do a soft reset in the middle of a merge.");
        }
    }

    let target_oid = match resolve_to_commit(repo, commit_spec) {
        Ok(oid) => oid,
        Err(_) if head.oid().is_none() => {
            // Unborn branch handling
            match mode {
                ResetMode::Soft => {
                    // --soft on unborn: no-op (nothing to move)
                    return Ok(());
                }
                ResetMode::Mixed => {
                    if repo.work_tree.is_none() {
                        bail!("fatal: mixed reset is not allowed in a bare repository");
                    }
                    // Mixed on unborn: clear the index
                    let index_path = repo.index_path();
                    let mut new_index = Index::new();
                    repo.write_index_at(&index_path, &mut new_index)
                        .context("writing index")?;
                    return Ok(());
                }
                ResetMode::Hard | ResetMode::Merge => {
                    // Unborn branch: reset --hard just clears the index and working tree
                    let index_path = repo.index_path();
                    let old_index = match repo.load_index_at(&index_path) {
                        Ok(idx) => idx,
                        Err(_) => Index::new(),
                    };
                    let mut new_index = Index::new();
                    if let Some(_wt) = &repo.work_tree {
                        checkout_index_to_worktree(
                            repo,
                            &old_index,
                            &mut new_index.clone(),
                            None,
                            false,
                            false,
                        )?;
                    }
                    repo.write_index_at(&index_path, &mut new_index)
                        .context("writing index")?;
                    return Ok(());
                }
                ResetMode::Keep => {
                    return Ok(());
                }
            }
        }
        Err(e) => return Err(e),
    };

    if mode == ResetMode::Mixed && repo.work_tree.is_none() {
        bail!("fatal: mixed reset is not allowed in a bare repository");
    }

    let recurse_submodules = effective_reset_recurse_submodules(repo, extra)?;

    let allow_gitlink_overwrite = explicit_recurse_submodules_cli_on(extra);

    // For --keep, we need to check safety before making any changes.
    if mode == ResetMode::Keep {
        check_keep_safety(repo, &head, &target_oid, allow_gitlink_overwrite)?;
    }

    // `--merge` matches Git's twoway merge reset: refuse when the working tree
    // or index is out of sync with HEAD on paths that differ between HEAD and
    // the target (e.g. cherry-pick --abort after a local edit to an unrelated
    // file). When HEAD already matches the target, this is a no-op check.
    if mode == ResetMode::Merge {
        check_keep_safety(repo, &head, &target_oid, allow_gitlink_overwrite)?;
    }

    // Get the old OID for reflog and ORIG_HEAD.
    let old_oid = head.oid().copied().unwrap_or_else(zero_oid);

    // Save ORIG_HEAD before moving HEAD.
    if head.oid().is_some() {
        write_orig_head(&repo.git_dir, &old_oid)?;
    }

    // Update HEAD (and the branch it points to, if on a branch).
    update_head_ref(&repo.git_dir, &head, &target_oid)?;

    // Write reflog entries.
    write_reset_reflog(repo, &head, &old_oid, &target_oid, commit_spec);

    // Clean up merge/cherry-pick state files on non-soft reset.
    if mode != ResetMode::Soft {
        let _ = std::fs::remove_file(repo.git_dir.join("MERGE_HEAD"));
        let _ = std::fs::remove_file(repo.git_dir.join("MERGE_MSG"));
        let _ = std::fs::remove_file(repo.git_dir.join("MERGE_MODE"));
        if !extra.skip_sequencer_head_cleanup {
            let _ = std::fs::remove_file(repo.git_dir.join("CHERRY_PICK_HEAD"));
            let _ = std::fs::remove_file(repo.git_dir.join("REVERT_HEAD"));
            let seq = repo.git_dir.join("sequencer");
            if seq.is_dir() {
                let _ = std::fs::remove_dir_all(&seq);
            }
        }
    }

    if mode == ResetMode::Soft {
        // Soft: HEAD moved, index and working tree unchanged.
        return Ok(());
    }

    // Mixed / hard / keep: reset index from the commit's tree.
    let tree_oid = commit_to_tree(repo, &target_oid)?;
    let tree_entries = tree_to_flat_entries(repo, &tree_oid, "")?;

    let index_path = repo.index_path();
    let old_index = repo
        .load_index_at(&index_path)
        .context("loading old index")?;
    let mut new_index = Index::new();
    new_index.entries = tree_entries;
    new_index.sort();
    // Git keeps cache-entry flags (assume-unchanged, skip-worktree) across mixed/hard/merge
    // index rebuilds so sparse/skip-worktree paths stay marked (t7011).
    preserve_index_cache_flags_from(&old_index, &mut new_index);

    if mode == ResetMode::Hard || mode == ResetMode::Keep || mode == ResetMode::Merge {
        // Hard/Keep/Merge require a working tree
        if repo.work_tree.is_none() {
            bail!("fatal: this operation must be run in a work tree");
        }
        if repo.work_tree.is_some() {
            let _wt = repo.work_tree.as_deref().expect("worktree checked above");
            if mode == ResetMode::Merge || mode == ResetMode::Keep {
                if let Some((path, is_dir)) =
                    find_untracked_obstruction(repo, &old_index, &new_index)?
                {
                    if is_dir {
                        bail!("Updating '{}' would lose untracked files in it", path);
                    } else {
                        bail!("Updating '{}' would lose untracked files.", path);
                    }
                }
            }
            if let Err(e) = checkout_index_to_worktree(
                repo,
                &old_index,
                &mut new_index,
                Some((&target_oid, Some(commit_spec))),
                recurse_submodules,
                recurse_submodules,
            ) {
                if recurse_submodules {
                    let _ = rollback_reset_after_failed_submodule_update(
                        repo,
                        &head,
                        &old_oid,
                        &target_oid,
                        &new_index,
                        recurse_submodules,
                    );
                }
                return Err(e);
            }
        }
        if !quiet {
            print_head_message(repo, &target_oid)?;
        }
    } else if mode == ResetMode::Mixed && !quiet {
        // Mixed: print unstaged changes after reset.
        // We need to print before writing the new index.
        print_unstaged_changes(repo, &new_index)?;
    }

    // Refresh stat info after hard/merge/keep checkout to prevent false diff-files hits
    if (mode == ResetMode::Hard || mode == ResetMode::Keep || mode == ResetMode::Merge)
        && repo.work_tree.is_some()
    {
        if let Some(ref wt) = repo.work_tree {
            for entry in &mut new_index.entries {
                if entry.stage() != 0 {
                    continue;
                }
                let path_str = String::from_utf8_lossy(&entry.path);
                let abs = wt.join(path_str.as_ref());
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
        }
    }

    if recurse_submodules
        && (mode == ResetMode::Hard || mode == ResetMode::Keep || mode == ResetMode::Merge)
    {
        repo.write_index_at(&index_path, &mut new_index)
            .context("writing index before submodule update")?;
        if let Err(e) = submodule_update_after_reset(repo) {
            let _ = rollback_reset_after_failed_submodule_update(
                repo,
                &head,
                &old_oid,
                &target_oid,
                &new_index,
                recurse_submodules,
            );
            return Err(e);
        }
        if let Some(ref wt) = repo.work_tree {
            for entry in &mut new_index.entries {
                if entry.stage() != 0 {
                    continue;
                }
                let path_str = String::from_utf8_lossy(&entry.path);
                let abs = wt.join(path_str.as_ref());
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
        }
    }

    repo.write_index_at(&index_path, &mut new_index)
        .context("writing index")?;
    crate::commands::sparse_checkout::reapply_sparse_checkout_if_configured(repo)?;
    Ok(())
}

/// Check if `--keep` is safe: refuse if there are local uncommitted changes
/// to files that differ between HEAD and the target.
fn check_keep_safety(
    repo: &Repository,
    head: &HeadState,
    target_oid: &ObjectId,
    allow_gitlink_path_overwrite: bool,
) -> Result<()> {
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => return Ok(()), // unborn branch, nothing to protect
    };

    if head_oid == *target_oid {
        return Ok(()); // no-op reset
    }

    // Get trees for HEAD and target.
    let head_tree_oid = commit_to_tree(repo, &head_oid)?;
    let target_tree_oid = commit_to_tree(repo, target_oid)?;

    let head_entries = tree_to_flat_entries(repo, &head_tree_oid, "")?;
    let target_entries = tree_to_flat_entries(repo, &target_tree_oid, "")?;

    let head_map: HashMap<Vec<u8>, &IndexEntry> =
        head_entries.iter().map(|e| (e.path.clone(), e)).collect();
    let target_map: HashMap<Vec<u8>, &IndexEntry> =
        target_entries.iter().map(|e| (e.path.clone(), e)).collect();

    // Files that differ between HEAD and target.
    let mut changed_paths: HashSet<Vec<u8>> = HashSet::new();

    // Files in HEAD but not in target (or different).
    for (path, head_entry) in &head_map {
        match target_map.get(path) {
            Some(target_entry)
                if target_entry.oid == head_entry.oid && target_entry.mode == head_entry.mode => {}
            _ => {
                changed_paths.insert(path.clone());
            }
        }
    }
    // Files in target but not in HEAD.
    for path in target_map.keys() {
        if !head_map.contains_key(path) {
            changed_paths.insert(path.clone());
        }
    }

    if changed_paths.is_empty() {
        return Ok(());
    }

    // Check if any of these changed paths have local modifications in the
    // working tree or index that differ from HEAD.
    let index_path = repo.index_path();
    let index = repo.load_index_at(&index_path).context("loading index")?;
    let index_map: HashMap<Vec<u8>, &IndexEntry> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.clone(), e))
        .collect();

    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    for path in &changed_paths {
        let path_str = String::from_utf8_lossy(path);

        // Replacing a checked-out submodule (gitlink) with a regular file or a directory of files
        // would destroy the submodule work tree; `reset --keep` / `--merge` must fail (`t7112`).
        // With `--recurse-submodules`, Git allows the destructive transition (tests replace
        // submodule with file / directory).
        if !allow_gitlink_path_overwrite {
            if let Some(h) = head_map.get(path) {
                if h.mode == MODE_GITLINK && target_replaces_gitlink_path(path, &target_map) {
                    let abs_path = work_tree.join(path_str.as_ref());
                    if abs_path.is_dir() && abs_path.join(".git").exists() {
                        bail!(
                            "Entry '{}' would be overwritten by merge. Cannot merge.",
                            path_str
                        );
                    }
                }
            }
        }

        // Check index vs HEAD.
        let head_entry = head_map.get(path);
        let idx_entry = index_map.get(path);

        match (head_entry, idx_entry) {
            (Some(h), Some(i)) => {
                if h.oid != i.oid || h.mode != i.mode {
                    bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
                }
            }
            (None, Some(_)) => {
                // File is in index but not in HEAD — local addition.
                bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
            }
            (Some(_), None) => {
                // File was in HEAD but not in index — staged deletion.
                bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
            }
            (None, None) => {}
        }

        // Check working tree vs index.
        let abs_path = work_tree.join(&*path_str);
        if let Some(idx_e) = idx_entry {
            if idx_e.mode == MODE_GITLINK {
                if abs_path.is_file() || abs_path.is_symlink() {
                    bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
                }
                let dot_git = abs_path.join(".git");
                if abs_path.is_dir() && dot_git.exists() {
                    if let Some(wt_oid) = read_submodule_head_commit_oid(&abs_path) {
                        if wt_oid != idx_e.oid {
                            bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
                        }
                    }
                }
            } else if abs_path.exists() {
                // Compare file content with index entry.
                if let Ok(content) = std::fs::read(&abs_path) {
                    let worktree_oid = hash_blob_content(&content);
                    if worktree_oid != idx_e.oid {
                        bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
                    }
                }
            } else {
                // File in index but deleted in worktree.
                bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
            }
        } else if abs_path.exists() {
            // Untracked file would be overwritten.
            let target_entry = target_map.get(path);
            if let Some(te) = target_entry {
                if te.mode == MODE_GITLINK {
                    let meta = std::fs::symlink_metadata(&abs_path).ok();
                    let is_dir = meta.is_some_and(|m| m.file_type().is_dir());
                    if is_dir && worktree_dir_is_empty_for_new_gitlink(&abs_path) {
                        continue;
                    }
                    if is_dir && head_tracked_directory_prefix(path, &head_map) {
                        if gitlink_replaces_clean_tracked_directory(
                            repo, &work_tree, path, &head_map, &index_map,
                        )? {
                            continue;
                        }
                    }
                    let is_untracked_plain_file = std::fs::symlink_metadata(&abs_path)
                        .map(|m| m.file_type().is_file())
                        .unwrap_or(false);
                    if is_untracked_plain_file {
                        if let Ok(mut im) = IgnoreMatcher::from_repository(repo) {
                            // Pass no index: the path is not in HEAD/index; exclude rules must apply.
                            if im
                                .check_path(repo, None, path_str.as_ref(), false)
                                .map(|(ig, _)| ig)
                                .unwrap_or(false)
                            {
                                continue;
                            }
                        }
                    }
                }
                bail!(
                    "Entry '{}' would be overwritten by merge. Cannot merge.",
                    path_str
                );
            }
        }
    }

    Ok(())
}

/// Compute the git blob OID for raw content (without writing to ODB).
fn hash_blob_content(data: &[u8]) -> ObjectId {
    Odb::hash_object_data(ObjectKind::Blob, data)
}

/// True when `target` will replace a gitlink at `path` with a non-submodule tree entry.
fn target_replaces_gitlink_path(path: &[u8], target_map: &HashMap<Vec<u8>, &IndexEntry>) -> bool {
    match target_map.get(path) {
        Some(e) => e.mode != MODE_GITLINK,
        None => false,
    }
}

/// Read the commit OID currently checked out in a submodule work tree (via its `.git` file/dir).
fn read_submodule_head_commit_oid(submodule_worktree: &Path) -> Option<ObjectId> {
    let dot_git = submodule_worktree.join(".git");
    if !dot_git.exists() {
        return None;
    }
    let git_dir = if dot_git.is_file() {
        let content = std::fs::read_to_string(&dot_git).ok()?;
        let line = content.strip_prefix("gitdir: ")?.trim();
        let p = Path::new(line);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            submodule_worktree.join(p)
        }
    } else {
        dot_git
    };
    let head_txt = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head_txt = head_txt.trim();
    let oid_hex = if let Some(name) = head_txt.strip_prefix("ref: ") {
        std::fs::read_to_string(git_dir.join(name))
            .ok()?
            .trim()
            .to_owned()
    } else {
        head_txt.to_owned()
    };
    ObjectId::from_hex(oid_hex.trim()).ok()
}

pub(crate) fn check_untracked_cherry_pick_obstruction(
    work_tree: &Path,
    old_index: &Index,
    merged_index: &Index,
) -> Result<()> {
    let Ok(repo) = Repository::discover(Some(work_tree)) else {
        return Ok(());
    };
    if let Some((path, _is_dir)) = find_untracked_obstruction(&repo, old_index, merged_index)? {
        bail!(
            "The following untracked working tree files would be overwritten by merge:\n\t{path}\nPlease move or remove them before you merge."
        );
    }
    Ok(())
}

/// True when `HEAD` records tracked paths under `dir_path/` (flattened tree paths).
fn head_tracked_directory_prefix(
    dir_path: &[u8],
    head_map: &HashMap<Vec<u8>, &IndexEntry>,
) -> bool {
    let mut prefix = dir_path.to_vec();
    prefix.push(b'/');
    head_map.keys().any(|k| k.starts_with(&prefix))
}

/// True when every `HEAD` path under `dir_path/` matches the index and the work tree matches the index.
fn gitlink_replaces_clean_tracked_directory(
    repo: &Repository,
    work_tree: &Path,
    dir_path: &[u8],
    head_map: &HashMap<Vec<u8>, &IndexEntry>,
    index_map: &HashMap<Vec<u8>, &IndexEntry>,
) -> Result<bool> {
    let mut prefix = dir_path.to_vec();
    prefix.push(b'/');
    for (p, h) in head_map {
        if !p.starts_with(&prefix) {
            continue;
        }
        let Some(i) = index_map.get(p) else {
            return Ok(false);
        };
        if h.oid != i.oid || h.mode != i.mode {
            return Ok(false);
        }
        let path_str = String::from_utf8_lossy(p);
        let abs = work_tree.join(path_str.as_ref());
        if h.mode == MODE_SYMLINK {
            if !abs.is_symlink() {
                return Ok(false);
            }
            let target = std::fs::read_link(&abs)?;
            let obj = repo.odb.read(&i.oid)?;
            let expected = String::from_utf8_lossy(&obj.data);
            if target.to_string_lossy() != expected.as_ref() {
                return Ok(false);
            }
        } else if h.mode == MODE_GITLINK {
            return Ok(false);
        } else {
            if !abs.is_file() {
                return Ok(false);
            }
            let disk = std::fs::read(&abs)?;
            if hash_blob_content(&disk) != i.oid {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// True when `dir` exists, is a directory, and contains nothing except `.` / `..` / `.git`.
///
/// Git allows `reset --keep` / `--merge` to place a submodule gitlink where the user created an
/// empty directory first, or where only a leftover `.git` file/dir remains (`t7112-reset-submodule`).
fn worktree_dir_is_empty_for_new_gitlink(dir: &Path) -> bool {
    let Ok(read) = std::fs::read_dir(dir) else {
        return false;
    };
    for e in read.flatten() {
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        if name == ".git" {
            continue;
        }
        return false;
    }
    true
}

fn open_submodule_repo_for_ignore(super_repo: &Repository, sub_rel: &str) -> Option<Repository> {
    let wt = super_repo.work_tree.as_ref()?;
    let sm_dir = wt.join(sub_rel);
    let modules = submodule_modules_git_dir(&super_repo.git_dir, sub_rel);
    Repository::open(&modules, Some(&sm_dir))
        .or_else(|_| Repository::discover(Some(&sm_dir)))
        .ok()
}

fn path_is_ignored_for_obstruction(
    super_repo: &Repository,
    ign: &mut IgnoreMatcher,
    old_index: &Index,
    rel: &str,
    is_dir: bool,
    old_paths: &HashSet<Vec<u8>>,
) -> bool {
    // Use `old_index`: `new_index` may already list paths we're about to materialize (e.g. a new
    // gitlink), which would wrongly make `check_path` treat disk paths as "tracked" and not
    // ignorable (`t7112` `.git/info/exclude`).
    if let Ok((true, _)) = ign.check_path(super_repo, Some(old_index), rel, is_dir) {
        return true;
    }
    if let Some(slash) = rel.find('/') {
        let prefix = &rel[..slash];
        let suffix = &rel[slash + 1..];
        if old_paths.contains(prefix.as_bytes()) {
            if let Some(sub_repo) = open_submodule_repo_for_ignore(super_repo, prefix) {
                if let Ok(mut sub_ign) = IgnoreMatcher::from_repository(&sub_repo) {
                    let sub_index = sub_repo.load_index().ok();
                    if let Ok((true, _)) =
                        sub_ign.check_path(&sub_repo, sub_index.as_ref(), suffix, is_dir)
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn find_untracked_obstruction(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
) -> Result<Option<(String, bool)>> {
    let work_tree = repo
        .work_tree
        .as_ref()
        .context("obstruction check needs work tree")?;
    let old_paths: HashSet<Vec<u8>> = old_index.entries.iter().map(|e| e.path.clone()).collect();
    let mut ignore_matcher = IgnoreMatcher::from_repository(repo).ok();

    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if old_paths.contains(&entry.path) {
            continue;
        }

        let rel = String::from_utf8_lossy(&entry.path).into_owned();
        let abs = work_tree.join(&rel);
        if !abs.exists() && !abs.is_symlink() {
            continue;
        }

        let has_tracked_prefix = rel.find('/').is_some_and(|_| {
            let mut prefix = String::new();
            for component in rel.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                if prefix.len() < rel.len() && old_paths.contains(prefix.as_bytes()) {
                    return true;
                }
            }
            false
        });
        if has_tracked_prefix {
            continue;
        }

        let replaces_tracked_dir = old_paths
            .iter()
            .any(|op| op.starts_with(rel.as_bytes()) && op.get(rel.len()) == Some(&b'/'));
        if replaces_tracked_dir {
            continue;
        }

        let is_dir = std::fs::symlink_metadata(&abs)
            .map(|m| m.file_type().is_dir())
            .unwrap_or(false);
        if is_dir && entry.mode == MODE_GITLINK && worktree_dir_is_empty_for_new_gitlink(&abs) {
            continue;
        }
        if let Some(ref mut im) = ignore_matcher {
            if path_is_ignored_for_obstruction(repo, im, old_index, &rel, is_dir, &old_paths) {
                continue;
            }
        }
        return Ok(Some((rel, is_dir)));
    }

    Ok(None)
}

/// Print "Unstaged changes after reset:" with modified files (mixed mode).
///
/// Compares the new index against the working tree. Files that differ are
/// printed as `M\t<path>`.
fn print_unstaged_changes(repo: &Repository, new_index: &Index) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let mut modified: Vec<String> = Vec::new();

    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if !abs_path.exists() {
            // File in index but not in worktree — deleted, counts as modified.
            modified.push(path_str);
            continue;
        }

        // Compare content.
        if entry.mode == MODE_SYMLINK {
            if let Ok(target) = std::fs::read_link(&abs_path) {
                let target_str = target.to_string_lossy();
                let obj = repo.odb.read(&entry.oid);
                if let Ok(obj) = obj {
                    let index_target = String::from_utf8_lossy(&obj.data);
                    if target_str != index_target.as_ref() {
                        modified.push(path_str);
                    }
                }
            } else {
                modified.push(path_str);
            }
        } else if let Ok(content) = std::fs::read(&abs_path) {
            let worktree_oid = hash_blob_content(&content);
            if worktree_oid != entry.oid {
                modified.push(path_str);
            }
        } else {
            modified.push(path_str);
        }
    }

    if !modified.is_empty() {
        println!("Unstaged changes after reset:");
        for path in &modified {
            println!("M\t{path}");
        }
    }

    Ok(())
}

/// Write `.git/ORIG_HEAD`.
fn write_orig_head(git_dir: &Path, oid: &ObjectId) -> Result<()> {
    std::fs::write(git_dir.join("ORIG_HEAD"), format!("{oid}\n"))?;
    Ok(())
}

/// Update HEAD and the branch ref it resolves to.
fn update_head_ref(git_dir: &Path, head: &HeadState, new_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            write_ref(git_dir, refname, new_oid)?;
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            std::fs::write(git_dir.join("HEAD"), format!("{new_oid}\n"))?;
        }
    }
    Ok(())
}

/// Print `"HEAD is now at <abbrev> <subject>\n"` to stdout.
fn print_head_message(repo: &Repository, oid: &ObjectId) -> Result<()> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Commit {
        return Ok(());
    }
    let commit = parse_commit(&obj.data)?;
    let subject = commit.message.lines().next().unwrap_or("").trim();
    let abbrev =
        abbreviate_object_id(repo, *oid, 7).unwrap_or_else(|_| oid.to_hex()[..7].to_owned());
    println!("HEAD is now at {abbrev} {subject}");
    Ok(())
}

/// Resolve a revision spec to a commit OID, peeling through tags.
fn resolve_to_commit(repo: &Repository, spec: &str) -> Result<ObjectId> {
    resolve_revision_as_commit(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))
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

/// Like [`checkout_index_to_worktree`] but for `reset --merge` after sequencer
/// rollback: keep local modifications to paths whose staged (target) blob is
/// unchanged from the pre-reset index, matching Git's twoway merge behavior.
fn checkout_merge_reset_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &mut Index,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let old_stage0: HashMap<Vec<u8>, &IndexEntry> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.clone(), e))
        .collect();

    let old_paths: HashSet<Vec<u8>> = old_index.entries.iter().map(|e| e.path.clone()).collect();
    let new_paths: HashSet<Vec<u8>> = new_index.entries.iter().map(|e| e.path.clone()).collect();

    for old_path in old_paths.difference(&new_paths) {
        let rel = String::from_utf8_lossy(old_path).into_owned();
        let abs = work_tree.join(&rel);
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        } else if abs.is_dir() {
            let _ = std::fs::remove_dir_all(&abs);
        }
        remove_empty_parent_dirs(&work_tree, &abs);
    }

    let old_unmerged_paths: HashSet<Vec<u8>> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() != 0)
        .map(|e| e.path.clone())
        .collect();
    for path in &old_unmerged_paths {
        if !new_paths.contains(path) {
            let rel = String::from_utf8_lossy(path).into_owned();
            let abs = work_tree.join(&rel);
            if abs.is_file() || abs.is_symlink() {
                let _ = std::fs::remove_file(&abs);
                remove_empty_parent_dirs(&work_tree, &abs);
            }
        }
    }

    for entry in &mut new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if entry.mode == 0o160000 || entry.mode == MODE_GITLINK {
            let path_str = String::from_utf8_lossy(&entry.path).into_owned();
            let submodule_dir = work_tree.join(&path_str);
            let _ = std::fs::create_dir_all(&submodule_dir);
            continue;
        }

        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(old_e) = old_stage0.get(&entry.path) {
            if old_e.oid == entry.oid
                && old_e.mode == entry.mode
                && local_worktree_differs_from_blob(repo, &abs_path, entry)?
            {
                continue;
            }
        }

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if entry.mode == MODE_SYMLINK {
            let obj = repo
                .odb
                .read(&entry.oid)
                .context("reading object for merge-reset checkout")?;
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if abs_path.exists() || abs_path.is_symlink() {
                std::fs::remove_file(&abs_path)?;
            }
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            let obj = repo
                .odb
                .read(&entry.oid)
                .context("reading object for merge-reset checkout")?;
            if obj.kind != ObjectKind::Blob {
                bail!("cannot checkout non-blob at '{path_str}'");
            }
            if abs_path.is_dir() {
                std::fs::remove_dir_all(&abs_path)?;
            }
            let attr_rules = grit_lib::crlf::load_gitattributes(&work_tree);
            let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
            let conv = config
                .as_ref()
                .map(grit_lib::crlf::ConversionConfig::from_config);
            let data = if let (Some(ref cfg), Some(ref cv)) = (&config, &conv) {
                let file_attrs = grit_lib::crlf::get_file_attrs(&attr_rules, &path_str, false, cfg);
                let oid_hex = format!("{}", entry.oid);
                grit_lib::crlf::convert_to_worktree(
                    &obj.data,
                    &path_str,
                    cv,
                    &file_attrs,
                    Some(&oid_hex),
                    None,
                )
                .map_err(|e| anyhow::anyhow!("smudge filter failed for {path_str}: {e}"))?
            } else {
                obj.data.clone()
            };
            std::fs::write(&abs_path, &data)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&abs_path)?.permissions();
            if entry.mode == MODE_EXECUTABLE {
                perms.set_mode(0o755);
            } else {
                perms.set_mode(0o644);
            }
            std::fs::set_permissions(&abs_path, perms)?;
        }

        if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
            use std::os::unix::fs::MetadataExt;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.uid = meta.uid();
            entry.gid = meta.gid();
            entry.size = meta.len() as u32;
        }
    }

    Ok(())
}

/// True when the path exists on disk and its content differs from `entry`'s blob.
fn local_worktree_differs_from_blob(
    repo: &Repository,
    abs_path: &Path,
    entry: &IndexEntry,
) -> Result<bool> {
    if entry.mode == MODE_SYMLINK {
        if !abs_path.is_symlink() {
            return Ok(true);
        }
        let target = std::fs::read_link(abs_path)?;
        let obj = repo.odb.read(&entry.oid)?;
        let expected = String::from_utf8_lossy(&obj.data);
        return Ok(target.to_string_lossy() != expected.as_ref());
    }
    if entry.mode == 0o040000 {
        return Ok(false);
    }
    if !abs_path.is_file() {
        return Ok(true);
    }
    let disk = std::fs::read(abs_path)?;
    let wt_oid = hash_blob_content(&disk);
    Ok(wt_oid != entry.oid)
}

/// Update the working tree to match the new index (used for `--hard`/`--keep` reset).
///
/// Deletes files removed from the index and writes/updates files added or
/// changed.
///
/// When `smudge` is `Some((commit, spec))`, process smudge metadata matches `git reset --hard`:
/// `ref=` / `treeish=` for symbolic ref specs (e.g. `old-main`), `treeish=` only for raw hex.
/// `Some((commit, None))` is treeish-only (e.g. restore by commit OID). `None` uses checkout-style
/// metadata from `HEAD`.
fn checkout_index_to_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &mut Index,
    smudge: Option<(&ObjectId, Option<&str>)>,
    recurse_submodules: bool,
    force_submodule_removal: bool,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    // Load gitattributes and config for CRLF conversion
    let attr_rules = grit_lib::crlf::load_gitattributes(&work_tree);
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let conv = config
        .as_ref()
        .map(grit_lib::crlf::ConversionConfig::from_config);

    let old_paths: HashSet<Vec<u8>> = old_index.entries.iter().map(|e| e.path.clone()).collect();
    let new_paths: HashSet<Vec<u8>> = new_index.entries.iter().map(|e| e.path.clone()).collect();
    let old_map: HashMap<&[u8], &IndexEntry> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    // Remove paths that are no longer present in the new index.
    // Sort by descending path length so nested files are removed before parent directories
    // (HashSet iteration order is unspecified; wrong order can leave stale files on disk).
    let mut to_drop: Vec<Vec<u8>> = old_paths.difference(&new_paths).cloned().collect();
    to_drop.sort_by(|a, b| b.len().cmp(&a.len()));
    for old_path in to_drop {
        let rel = String::from_utf8_lossy(&old_path).into_owned();
        let abs = work_tree.join(&rel);
        if let Some(oe) = old_map.get(old_path.as_slice()) {
            if oe.mode == MODE_GITLINK {
                if force_submodule_removal {
                    remove_submodule_worktree_for_reset(
                        repo,
                        &work_tree,
                        &rel,
                        recurse_submodules,
                        true,
                    )?;
                    remove_empty_parent_dirs(&work_tree, &abs);
                }
                // Without `--recurse-submodules`, match Git: keep the submodule work tree on disk
                // when the gitlink disappears from the index (`t7112`).
                continue;
            }
        }
        remove_worktree_path_best_effort(&abs);
        remove_empty_parent_dirs(&work_tree, &abs);
    }

    // Remove worktree files for paths that only have unmerged (conflict) entries
    // in old index and no entry at all in the new index.
    let old_unmerged_paths: std::collections::HashSet<Vec<u8>> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() != 0)
        .map(|e| e.path.clone())
        .collect();
    for path in &old_unmerged_paths {
        if !new_paths.contains(path) {
            let rel = String::from_utf8_lossy(path).into_owned();
            let abs = work_tree.join(&rel);
            remove_worktree_path_best_effort(&abs);
            remove_empty_parent_dirs(&work_tree, &abs);
        }
    }

    // Write all stage-0 entries from the new index.
    for entry in &mut new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if let Some(old_e) = old_map.get(entry.path.as_slice()) {
            if old_e.mode == MODE_GITLINK && entry.mode != MODE_GITLINK {
                let dot_git = abs_path.join(".git");
                let submodule_checked_out = abs_path.exists() && dot_git.exists();
                if !force_submodule_removal {
                    if submodule_checked_out {
                        bail!("Cannot update submodule:\n{path_str}");
                    }
                } else {
                    remove_submodule_worktree_for_reset(
                        repo,
                        &work_tree,
                        &path_str,
                        recurse_submodules,
                        false,
                    )?;
                }
            }
        }

        if entry.mode == MODE_GITLINK {
            if recurse_submodules {
                let force_populate = match old_map.get(entry.path.as_slice()) {
                    None => true,
                    Some(old) => old.mode != MODE_GITLINK || old.oid != entry.oid,
                };
                crate::commands::checkout::checkout_gitlink_worktree_entry(
                    repo,
                    &work_tree,
                    &path_str,
                    &entry.oid,
                    force_populate,
                )?;
            } else {
                // Submodules are represented as gitlinks: their OIDs are commit
                // objects in the submodule's object store, not blobs in ours.
                // Materialize only the directory path in the superproject.
                if abs_path.is_file() || abs_path.is_symlink() {
                    std::fs::remove_file(&abs_path)?;
                } else if abs_path.is_dir() && abs_path.join(".git").exists() {
                    if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
                        use std::os::unix::fs::MetadataExt;
                        entry.ctime_sec = meta.ctime() as u32;
                        entry.ctime_nsec = meta.ctime_nsec() as u32;
                        entry.mtime_sec = meta.mtime() as u32;
                        entry.mtime_nsec = meta.mtime_nsec() as u32;
                        entry.dev = meta.dev() as u32;
                        entry.ino = meta.ino() as u32;
                        entry.uid = meta.uid();
                        entry.gid = meta.gid();
                        entry.size = meta.len() as u32;
                    }
                    continue;
                } else if abs_path.is_dir() {
                    std::fs::remove_dir_all(&abs_path)?;
                }
                std::fs::create_dir_all(&abs_path)?;
            }

            if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
                use std::os::unix::fs::MetadataExt;
                entry.ctime_sec = meta.ctime() as u32;
                entry.ctime_nsec = meta.ctime_nsec() as u32;
                entry.mtime_sec = meta.mtime() as u32;
                entry.mtime_nsec = meta.mtime_nsec() as u32;
                entry.dev = meta.dev() as u32;
                entry.ino = meta.ino() as u32;
                entry.uid = meta.uid();
                entry.gid = meta.gid();
                entry.size = meta.len() as u32;
            }
            continue;
        }

        let obj = repo
            .odb
            .read(&entry.oid)
            .context("reading object for checkout")?;
        if obj.kind != ObjectKind::Blob {
            bail!("cannot checkout non-blob at '{path_str}'");
        }

        if abs_path.is_dir() {
            std::fs::remove_dir_all(&abs_path)?;
        }

        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if abs_path.exists() || abs_path.is_symlink() {
                std::fs::remove_file(&abs_path)?;
            }
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            // Apply CRLF conversion if configured
            let data = if let (Some(ref cfg), Some(ref cv)) = (&config, &conv) {
                let file_attrs = grit_lib::crlf::get_file_attrs(&attr_rules, &path_str, false, cfg);
                let oid_hex = format!("{}", entry.oid);
                let smudge_meta = match smudge {
                    None => grit_lib::filter_process::smudge_meta_for_checkout(repo, &oid_hex),
                    Some((tip, None)) => grit_lib::filter_process::smudge_meta_treeish_only(
                        &tip.to_string(),
                        &oid_hex,
                    ),
                    Some((tip, Some(spec))) => {
                        grit_lib::filter_process::smudge_meta_for_reset(repo, spec, tip, &oid_hex)
                    }
                };
                grit_lib::crlf::convert_to_worktree(
                    &obj.data,
                    &path_str,
                    cv,
                    &file_attrs,
                    Some(&oid_hex),
                    Some(&smudge_meta),
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?
            } else {
                obj.data.clone()
            };
            std::fs::write(&abs_path, &data)?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&abs_path)?.permissions();
            if entry.mode == MODE_EXECUTABLE {
                perms.set_mode(0o755);
            } else {
                perms.set_mode(0o644);
            }
            std::fs::set_permissions(&abs_path, perms)?;
        }

        // Refresh stat data in the index entry so that subsequent
        // `stat_matches` calls see up-to-date values (prevents
        // spurious re-staging by `git add`).
        if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
            use std::os::unix::fs::MetadataExt;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.uid = meta.uid();
            entry.gid = meta.gid();
            entry.size = meta.len() as u32;
        }
    }

    Ok(())
}

/// Remove a submodule work tree (and nested submodules when `recurse_submodules`) before dropping
/// a gitlink from the superproject index (`reset --recurse-submodules`).
///
/// When `remove_modules_git_dir` is true, also remove `.git/modules/<rel>` after clearing the
/// work tree (used when the gitlink is removed from the index entirely). When replacing a gitlink
/// with a tracked blob at the same path, pass `false` so user state under `.git/modules` (e.g.
/// `info/exclude`) is preserved (`t7112-reset-submodule`).
fn remove_submodule_worktree_for_reset(
    super_repo: &Repository,
    super_work_tree: &Path,
    rel: &str,
    recurse_submodules: bool,
    remove_modules_git_dir: bool,
) -> Result<()> {
    let sm_dir = super_work_tree.join(rel);
    let modules_git = submodule_modules_git_dir(&super_repo.git_dir, rel);
    let dot_git = sm_dir.join(".git");
    let had_in_tree_git_dir = sm_dir.exists() && dot_git.is_dir();
    if sm_dir.exists() && dot_git.is_dir() {
        let _ =
            crate::commands::submodule::absorb_submodule_dot_git_dir_into_modules(super_repo, rel);
    }
    if sm_dir.exists() && dot_git.exists() {
        if let Ok(sub_repo) = Repository::open(&modules_git, Some(&sm_dir)) {
            let sub_index_path = sub_repo.index_path();
            let sub_old = sub_repo
                .load_index_at(&sub_index_path)
                .unwrap_or_else(|_| Index::new());
            let mut sub_new = Index::new();
            checkout_index_to_worktree(
                &sub_repo,
                &sub_old,
                &mut sub_new,
                None,
                recurse_submodules,
                recurse_submodules,
            )?;
        }
    }
    if remove_modules_git_dir && !had_in_tree_git_dir && modules_git.exists() {
        let _ = std::fs::remove_dir_all(&modules_git);
    }
    if sm_dir.exists() {
        remove_worktree_path_best_effort(&sm_dir);
    }
    Ok(())
}

/// Best-effort removal of a path that is no longer in the index.
///
/// Uses [`std::fs::symlink_metadata`] so we do not follow symlinks (matches Git checkout).
/// Tries `remove_dir_all` when the path is a directory, otherwise `remove_file`.
fn remove_worktree_path_best_effort(abs: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(abs) else {
        return;
    };
    let ft = meta.file_type();
    if ft.is_dir() {
        let _ = std::fs::remove_dir_all(abs);
    } else {
        let _ = std::fs::remove_file(abs);
    }
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
