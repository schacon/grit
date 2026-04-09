//! `grit am` — apply patches from mailbox-format files.
//!
//! Reads one or more mbox-format patch files (as produced by `git format-patch`)
//! and applies each patch as a new commit, preserving the original author,
//! date, and commit message from the email headers.
//!
//! Modes:
//! - `grit am <mbox>...` — apply patches from mbox files
//! - `grit am --continue` — continue after resolving conflicts
//! - `grit am --abort` — abort the current am session

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use grit_lib::config::{parse_bool, ConfigSet};
use grit_lib::index::{Index, IndexEntry, MODE_REGULAR};
use grit_lib::objects::{parse_commit, serialize_commit, CommitData, ObjectId, ObjectKind};
use grit_lib::refs::{delete_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rerere::rerere_clear;
use grit_lib::rev_parse::resolve_revision_for_patch_old_blob;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;

/// Arguments for `grit am`.
#[derive(Debug, ClapArgs)]
#[command(about = "Apply patches from mailbox")]
pub struct Args {
    /// Mbox file(s) containing patches.
    #[arg(value_name = "MBOX")]
    pub mbox: Vec<String>,

    /// Continue applying patches after resolving a conflict.
    #[arg(long = "continue", alias = "resolved")]
    pub r#continue: bool,

    /// Retry the current patch in an existing am session.
    #[arg(long = "retry")]
    pub retry: bool,

    /// Abort the current am session.
    #[arg(long = "abort")]
    pub abort: bool,

    /// Skip the current patch.
    #[arg(long = "skip")]
    pub skip: bool,

    /// Attempt three-way merge if patch doesn't apply cleanly.
    #[arg(short = '3', long = "3way")]
    pub three_way: bool,

    /// Quiet mode — suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Disable quiet mode for resumed patch application.
    #[arg(long = "no-quiet")]
    pub no_quiet: bool,

    /// Do not apply the patch, just show what would be applied.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Read patches from stdin (default if no files given).
    #[arg(long = "stdin")]
    pub stdin: bool,

    /// Interactively choose whether to apply each patch.
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Add Signed-off-by trailer.
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// Disable Signed-off-by trailer for resumed patch application.
    #[arg(long = "no-signoff")]
    pub no_signoff: bool,

    /// Keep the [PATCH] prefix in the subject.
    #[arg(short = 'k', long = "keep")]
    pub keep: bool,

    /// Keep CR at end of lines.
    #[arg(long = "keep-cr")]
    pub keep_cr: bool,

    /// Remove CR at end of lines.
    #[arg(long = "no-keep-cr")]
    pub no_keep_cr: bool,

    /// Keep non-patch bracket content in the subject.
    #[arg(long = "keep-non-patch")]
    pub keep_non_patch: bool,

    /// Strip everything before scissors line.
    #[arg(long = "scissors")]
    pub scissors: bool,

    /// Override scissors with --no-scissors.
    #[arg(long = "no-scissors")]
    pub no_scissors: bool,

    /// Set committer date to author date.
    #[arg(long = "committer-date-is-author-date")]
    pub committer_date_is_author_date: bool,

    /// Show the current patch.
    #[arg(long = "show-current-patch", value_name = "MODE", num_args = 0..=1, default_missing_value = "raw")]
    pub show_current_patch: Option<String>,

    /// Skip hook execution.
    #[arg(long = "no-verify")]
    pub no_verify: bool,

    /// Leave rejected hunks in *.rej files.
    #[arg(long = "reject")]
    pub reject: bool,

    /// Disable reject file generation.
    #[arg(long = "no-reject")]
    pub no_reject: bool,

    /// Add Message-Id trailer to commit messages.
    #[arg(long = "message-id")]
    pub message_id: bool,

    /// What to do with empty patches (stop/drop/keep).
    #[arg(long = "empty", value_name = "ACTION")]
    pub empty: Option<String>,

    /// Allow empty commits.
    #[arg(long = "allow-empty")]
    pub allow_empty: bool,

    /// Override patch format detection.
    #[arg(long = "patch-format", value_name = "FORMAT")]
    pub patch_format: Option<String>,

    /// Disable three-way merge fallback.
    #[arg(long = "no-3way")]
    pub no_three_way: bool,

    /// Use the current timestamp as author date instead of the patch's date.
    #[arg(long = "ignore-date")]
    pub ignore_date: bool,

    /// How to handle quoted CRLF in patch payloads.
    #[arg(long = "quoted-cr", value_name = "ACTION")]
    pub quoted_cr: Option<String>,
}

/// A parsed patch from an mbox message.
#[derive(Debug)]
struct MboxPatch {
    /// Author name + email (e.g. "Name <email>").
    author: String,
    /// Author date string (for the ident line).
    date: String,
    /// Commit message (subject + body).
    message: String,
    /// The unified diff portion.
    diff: String,
    /// Message-ID from the email headers.
    message_id: String,
}

/// Run the `am` command.
/// Options threaded through the apply loop.
#[derive(Debug, Clone)]
struct AmOptions {
    quiet: bool,
    three_way: bool,
    keep_cr: bool,
    no_verify: bool,
    signoff: bool,
    reject: bool,
    committer_date_is_author_date: bool,
    ignore_date: bool,
    message_id: bool,
    empty: String,
    allow_empty: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct AmOptionOverrides {
    quiet: Option<bool>,
    three_way: Option<bool>,
    keep_cr: Option<bool>,
    signoff: Option<bool>,
    reject: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuotedCrAction {
    Warn,
    Strip,
    Nowarn,
}

pub fn run(args: Args) -> Result<()> {
    if let Some(ref mode) = args.show_current_patch {
        return do_show_current_patch(mode);
    }
    if args.abort {
        return do_abort();
    }
    if args.skip {
        return do_skip();
    }
    let overrides = option_overrides_from_args(&args);

    if args.r#continue {
        return do_continue(args.interactive, &overrides);
    }
    if args.retry {
        return do_retry(&overrides);
    }

    if args.mbox.is_empty() && !args.stdin {
        return do_am_stdin(args);
    }
    if args.stdin {
        return do_am_stdin(args);
    }

    do_am(args)
}

// ── State directory ─────────────────────────────────────────────────
//
// .git/rebase-apply/  (shared with rebase, as git does)
//   applying          — marker that this is am, not rebase
//   orig-head         — original HEAD OID
//   patches/<N>       — individual parsed patches
//   current           — index (1-based) of current patch being applied
//   last              — total number of patches
//   next              — next patch to apply (1-based)

fn am_dir(git_dir: &Path) -> std::path::PathBuf {
    git_dir.join("rebase-apply")
}

fn write_am_abort_safety(git_dir: &Path) -> Result<()> {
    let state_dir = am_dir(git_dir);
    match resolve_head(git_dir)?.oid() {
        Some(oid) => fs::write(
            state_dir.join("abort-safety"),
            format!("{}\n", oid.to_hex()),
        ),
        None => fs::write(state_dir.join("abort-safety"), b""),
    }
    .map_err(|e| anyhow::anyhow!(e))
}

fn sync_am_orig_head(git_dir: &Path) -> Result<()> {
    match resolve_head(git_dir)?.oid() {
        Some(oid) => write_ref(git_dir, "ORIG_HEAD", oid).map_err(|e| e.into()),
        None => {
            let _ = delete_ref(git_dir, "ORIG_HEAD");
            Ok(())
        }
    }
}

fn index_differs_from_head(repo: &Repository, git_dir: &Path) -> Result<bool> {
    let index = load_index(repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        return Ok(true);
    }
    let head = resolve_head(git_dir)?;
    let Some(head_oid) = head.oid() else {
        return Ok(!index.entries.is_empty());
    };
    let head_tree = {
        let obj = repo.odb.read(head_oid)?;
        let commit = parse_commit(&obj.data)?;
        commit.tree
    };
    let index_tree = write_tree_from_index(&repo.odb, &index, "")?;
    Ok(head_tree != index_tree)
}

fn am_safe_to_abort(git_dir: &Path) -> Result<bool> {
    if am_dir(git_dir).join("dirtyindex").exists() {
        return Ok(false);
    }
    let state_dir = am_dir(git_dir);
    let safety = fs::read_to_string(state_dir.join("abort-safety")).unwrap_or_default();
    let safety_trim = safety.trim();
    let head = resolve_head(git_dir)?;
    match (head.oid(), safety_trim.is_empty()) {
        (None, true) => Ok(true),
        (Some(oid), false) if safety_trim == oid.to_hex() => Ok(true),
        _ => Ok(false),
    }
}

fn is_am_in_progress(git_dir: &Path) -> bool {
    let dir = am_dir(git_dir);
    dir.exists() && dir.join("applying").exists()
}

fn parse_quoted_cr_action(value: &str) -> QuotedCrAction {
    match value.trim().to_ascii_lowercase().as_str() {
        "strip" => QuotedCrAction::Strip,
        "nowarn" => QuotedCrAction::Nowarn,
        "warn" => QuotedCrAction::Warn,
        _ => QuotedCrAction::Warn,
    }
}

fn resolve_quoted_cr_action(cli_value: Option<&str>, config: &ConfigSet) -> QuotedCrAction {
    if let Some(value) = cli_value {
        return parse_quoted_cr_action(value);
    }
    if let Some(value) = config
        .get("mailinfo.quotedCr")
        .or_else(|| config.get("mailinfo.quotedcr"))
    {
        return parse_quoted_cr_action(&value);
    }
    QuotedCrAction::Warn
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let normalized = answer.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

fn select_patches_interactively(patches: Vec<MboxPatch>) -> Result<Vec<MboxPatch>> {
    let mut selected = Vec::new();
    for patch in patches {
        let subject = patch.message.lines().next().unwrap_or("(no subject)");
        if prompt_yes_no(&format!("Apply patch '{}'? [y/N] ", subject))? {
            selected.push(patch);
        }
    }
    Ok(selected)
}

fn merge_option_overrides(base: &mut AmOptions, overrides: AmOptionOverrides) {
    if let Some(value) = overrides.quiet {
        base.quiet = value;
    }
    if let Some(value) = overrides.three_way {
        base.three_way = value;
    }
    if let Some(value) = overrides.keep_cr {
        base.keep_cr = value;
    }
    if let Some(value) = overrides.signoff {
        base.signoff = value;
    }
    if let Some(value) = overrides.reject {
        base.reject = value;
    }
}

fn config_bool(config: &ConfigSet, key: &str) -> Option<bool> {
    config
        .get(key)
        .and_then(|value| parse_bool(value.trim()).ok())
}

fn resolve_keep_cr(args: &Args, config: &ConfigSet) -> bool {
    if args.no_keep_cr {
        return false;
    }
    if args.keep_cr {
        return true;
    }
    config_bool(config, "am.keepcr").unwrap_or(false)
}

fn build_am_options(args: &Args, config: &ConfigSet) -> AmOptions {
    let three_way = if args.no_three_way {
        false
    } else if args.three_way {
        true
    } else {
        config_bool(config, "am.threeWay")
            .or_else(|| config_bool(config, "am.threeway"))
            .unwrap_or(false)
    };
    let message_id = args.message_id || config_bool(config, "am.messageid").unwrap_or(false);
    let keep_cr = resolve_keep_cr(args, config);
    AmOptions {
        quiet: if args.no_quiet { false } else { args.quiet },
        three_way,
        keep_cr,
        no_verify: args.no_verify,
        signoff: if args.no_signoff { false } else { args.signoff },
        reject: if args.no_reject { false } else { args.reject },
        committer_date_is_author_date: args.committer_date_is_author_date,
        ignore_date: args.ignore_date,
        message_id,
        empty: args.empty.clone().unwrap_or_else(|| "stop".to_string()),
        allow_empty: args.allow_empty,
    }
}

fn continue_overrides_from_args(args: &Args) -> AmOptionOverrides {
    let quiet = if args.no_quiet {
        Some(false)
    } else if args.quiet {
        Some(true)
    } else {
        None
    };
    let three_way = if args.no_three_way {
        Some(false)
    } else if args.three_way {
        Some(true)
    } else {
        None
    };
    let keep_cr = if args.no_keep_cr {
        Some(false)
    } else if args.keep_cr {
        Some(true)
    } else {
        None
    };
    let signoff = if args.no_signoff {
        Some(false)
    } else if args.signoff {
        Some(true)
    } else {
        None
    };
    let reject = if args.no_reject {
        Some(false)
    } else if args.reject {
        Some(true)
    } else {
        None
    };
    AmOptionOverrides {
        quiet,
        three_way,
        keep_cr,
        signoff,
        reject,
    }
}

fn option_overrides_from_args(args: &Args) -> AmOptionOverrides {
    continue_overrides_from_args(args)
}

fn merge_options(base: &AmOptions, overrides: &AmOptionOverrides) -> AmOptions {
    let mut merged = base.clone();
    merge_option_overrides(&mut merged, *overrides);
    merged
}

fn do_retry(overrides: &AmOptionOverrides) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    if !is_am_in_progress(git_dir) {
        bail!("operation not in progress");
    }
    let state_dir = am_dir(git_dir);
    let opts = load_am_options(&state_dir);
    apply_remaining(&repo, &opts, Some(overrides))
}

// ── Main flow ───────────────────────────────────────────────────────

fn do_am(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if is_am_in_progress(git_dir) {
        bail!(
            "error: an am session is already in progress\n\
             hint: use \"grit am --continue\" to continue\n\
             hint: or \"grit am --abort\" to abort"
        );
    }

    let keep = args.keep;
    let keep_non_patch = args.keep_non_patch;
    let scissors = args.scissors;
    let no_scissors = args.no_scissors;
    let config = ConfigSet::load(Some(git_dir), true)?;
    let quoted_cr_action = resolve_quoted_cr_action(args.quoted_cr.as_deref(), &config);
    let keep_cr = resolve_keep_cr(&args, &config);

    // Read and parse all mbox/patch files
    let mut all_patches = Vec::new();
    let format_override = args.patch_format.as_deref();
    for mbox_path in &args.mbox {
        let content = fs::read_to_string(mbox_path)
            .with_context(|| format!("cannot read mbox file '{mbox_path}'"))?;
        // Check for stgit series file first (auto-detect or explicit)
        if is_stgit_series(&content) {
            let mut patches = parse_stgit_series(mbox_path)?;
            all_patches.append(&mut patches);
        } else {
            let mut patches = parse_patches(
                &content,
                format_override,
                keep,
                keep_non_patch,
                scissors,
                no_scissors,
                keep_cr,
                quoted_cr_action,
            )?;
            all_patches.append(&mut patches);
        }
    }

    if all_patches.is_empty() {
        eprintln!("Patch format detection failed.");
        std::process::exit(128);
    }

    if args.dry_run {
        for (i, patch) in all_patches.iter().enumerate() {
            let subject = patch.message.lines().next().unwrap_or("(no subject)");
            println!("Patch {}/{}: {}", i + 1, all_patches.len(), subject);
        }
        return Ok(());
    }

    if args.interactive {
        all_patches = select_patches_interactively(all_patches)?;
        if all_patches.is_empty() {
            return Ok(());
        }
    }

    // Save state
    let state_dir = am_dir(git_dir);
    fs::create_dir_all(state_dir.join("patches"))?;
    fs::write(state_dir.join("applying"), "")?;

    let head = resolve_head(git_dir)?;
    let head_oid = head.oid().map(|o| o.to_hex()).unwrap_or_default();
    fs::write(state_dir.join("orig-head"), &head_oid)?;
    // Save the raw HEAD content so abort can restore branch state
    let head_content = fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();
    fs::write(state_dir.join("head-name"), head_content.trim())?;
    fs::write(state_dir.join("last"), all_patches.len().to_string())?;
    fs::write(state_dir.join("next"), "1")?;

    write_am_abort_safety(git_dir)?;
    sync_am_orig_head(git_dir)?;

    // Write individual patches
    for (i, patch) in all_patches.iter().enumerate() {
        let patch_file = state_dir.join("patches").join((i + 1).to_string());
        let serialized = serialize_mbox_patch(patch);
        fs::write(&patch_file, serialized)?;
    }

    // Apply patches
    let opts = build_am_options(&args, &config);
    // Save options to state dir for --continue
    save_am_options(&state_dir, &opts)?;
    apply_remaining(&repo, &opts, None)?;

    Ok(())
}

fn do_am_stdin(args: Args) -> Result<()> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("failed to read from stdin")?;

    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if is_am_in_progress(git_dir) {
        bail!(
            "error: an am session is already in progress\n\
             hint: use \"grit am --continue\" to continue\n\
             hint: or \"grit am --abort\" to abort"
        );
    }

    let config = ConfigSet::load(Some(git_dir), true)?;
    let quoted_cr_action = resolve_quoted_cr_action(args.quoted_cr.as_deref(), &config);
    let keep_cr = resolve_keep_cr(&args, &config);

    let mut all_patches = parse_patches(
        &input,
        args.patch_format.as_deref(),
        args.keep,
        args.keep_non_patch,
        args.scissors,
        args.no_scissors,
        keep_cr,
        quoted_cr_action,
    )?;
    if all_patches.is_empty() {
        eprintln!("Patch format detection failed.");
        std::process::exit(128);
    }

    if args.dry_run {
        for (i, patch) in all_patches.iter().enumerate() {
            let subject = patch.message.lines().next().unwrap_or("(no subject)");
            println!("Patch {}/{}: {}", i + 1, all_patches.len(), subject);
        }
        return Ok(());
    }

    if args.interactive {
        all_patches = select_patches_interactively(all_patches)?;
        if all_patches.is_empty() {
            return Ok(());
        }
    }

    let state_dir = am_dir(git_dir);
    fs::create_dir_all(state_dir.join("patches"))?;
    fs::write(state_dir.join("applying"), "")?;

    let head = resolve_head(git_dir)?;
    let head_oid = head.oid().map(|o| o.to_hex()).unwrap_or_default();
    fs::write(state_dir.join("orig-head"), &head_oid)?;
    let head_content = fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();
    fs::write(state_dir.join("head-name"), head_content.trim())?;
    fs::write(state_dir.join("last"), all_patches.len().to_string())?;
    fs::write(state_dir.join("next"), "1")?;

    write_am_abort_safety(git_dir)?;
    sync_am_orig_head(git_dir)?;

    for (i, patch) in all_patches.iter().enumerate() {
        let patch_file = state_dir.join("patches").join((i + 1).to_string());
        let serialized = serialize_mbox_patch(patch);
        fs::write(&patch_file, serialized)?;
    }

    let opts = build_am_options(&args, &config);
    save_am_options(&state_dir, &opts)?;
    apply_remaining(&repo, &opts, None)?;
    Ok(())
}

/// Apply all remaining patches.
fn apply_remaining(
    repo: &Repository,
    opts: &AmOptions,
    first_patch_overrides: Option<&AmOptionOverrides>,
) -> Result<()> {
    let git_dir = &repo.git_dir;
    let state_dir = am_dir(git_dir);

    let _ = fs::remove_file(state_dir.join("dirtyindex"));
    if index_differs_from_head(repo, git_dir)? {
        fs::write(state_dir.join("dirtyindex"), "")?;
        let mut sb = String::new();
        let index = load_index(repo)?;
        for e in index.entries.iter().filter(|e| e.stage() == 0) {
            let p = String::from_utf8_lossy(&e.path);
            if !sb.is_empty() {
                sb.push(' ');
            }
            sb.push_str(&p);
        }
        eprintln!(
            "error: Dirty index: cannot apply patches (dirty: {sb})\n\
             hint: commit or reset your changes before running \"grit am\" again."
        );
        std::process::exit(128);
    }

    let last: usize = fs::read_to_string(state_dir.join("last"))?.trim().parse()?;
    let mut next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
    let first_next = next;

    while next <= last {
        let patch_file = state_dir.join("patches").join(next.to_string());
        let serialized = fs::read_to_string(&patch_file)?;
        let patch = deserialize_mbox_patch(&serialized)?;

        fs::write(state_dir.join("current"), next.to_string())?;

        // Check if this is an empty patch (no diff)
        let is_empty_patch = patch.diff.trim().is_empty();

        if is_empty_patch {
            match opts.empty.as_str() {
                "drop" => {
                    if !opts.quiet {
                        let subject = patch.message.lines().next().unwrap_or("");
                        eprintln!("Skipping: {}", subject);
                    }
                    next += 1;
                    fs::write(state_dir.join("next"), next.to_string())?;
                    continue;
                }
                "keep" => {
                    // Will be handled in apply_one_patch as empty commit
                }
                _ => {
                    // "stop" is the default - error on empty patch
                    let subject = patch.message.lines().next().unwrap_or("");
                    eprintln!(
                        "error: patch failed: patch does not contain a valid diff\n\
                         Applying: {}\n\
                         hint: Fix the patch and run \"grit am --continue\".\n\
                         hint: To abort, run \"grit am --abort\".",
                        subject
                    );
                    // Save message for --continue
                    fs::write(git_dir.join("MERGE_MSG"), &patch.message)?;
                    std::process::exit(1);
                }
            }
        }

        let effective_opts = if next == first_next {
            first_patch_overrides
                .map(|overrides| merge_options(opts, overrides))
                .unwrap_or_else(|| opts.clone())
        } else {
            opts.clone()
        };

        let subject = patch.message.lines().next().unwrap_or("");
        if !effective_opts.quiet {
            println!("Applying: {}", subject);
        }

        match apply_one_patch(repo, &patch, &effective_opts) {
            Ok(()) => {
                write_am_abort_safety(git_dir)?;
                next += 1;
                fs::write(state_dir.join("next"), next.to_string())?;
            }
            Err(e) => {
                let _ = write_am_abort_safety(git_dir);
                // Invoke rerere to record preimage or replay resolution
                let _ = crate::commands::rerere::auto_rerere_worktree(repo);
                eprintln!(
                    "error: patch failed: {}\n\
                     Applying: {}\n\
                     hint: Fix the patch and run \"grit am --continue\".\n\
                     hint: To abort, run \"grit am --abort\".",
                    e, subject
                );
                std::process::exit(1);
            }
        }
    }

    // All patches applied — cleanup
    cleanup_am_state(git_dir);
    Ok(())
}

/// Apply a single mbox patch: apply the diff, then create a commit.
fn apply_one_patch(repo: &Repository, patch: &MboxPatch, opts: &AmOptions) -> Result<()> {
    let git_dir = &repo.git_dir;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot apply patches in a bare repository"))?;

    let is_empty_patch = patch.diff.trim().is_empty();

    // For non-empty patches, check if the index is dirty
    if !is_empty_patch {
        let index = load_index(repo)?;
        let head = resolve_head(git_dir)?;
        if let Some(head_oid) = head.oid() {
            let obj = repo.odb.read(head_oid)?;
            let commit = parse_commit(&obj.data)?;
            let head_entries = tree_to_index_entries(repo, &commit.tree, "")?;
            if index.entries.len() != head_entries.len()
                || index
                    .entries
                    .iter()
                    .zip(head_entries.iter())
                    .any(|(a, b)| a.oid != b.oid || a.path != b.path)
            {
                bail!(
                    "your local changes would be overwritten by am.\n\
                       Please commit your changes or stash them before you apply patches."
                );
            }
        }
    }

    // Handle empty patches
    if is_empty_patch {
        if opts.empty == "keep" || opts.allow_empty {
            // Run applypatch-msg hook
            if !opts.no_verify {
                let msg_path = git_dir.join("MERGE_MSG");
                fs::write(&msg_path, &patch.message)?;
                if !run_hook(
                    git_dir,
                    "applypatch-msg",
                    &[msg_path.to_str().unwrap_or("")],
                )? {
                    let _ = fs::remove_file(&msg_path);
                    bail!("applypatch-msg hook rejected the patch");
                }
            }

            // Run pre-applypatch hook
            if !opts.no_verify && !run_hook(git_dir, "pre-applypatch", &[])? {
                bail!("pre-applypatch hook rejected the patch");
            }

            // Create empty commit
            let index = load_index(repo)?;
            create_am_commit(repo, &index, patch, opts)?;

            // Run post-applypatch hook
            if !opts.no_verify {
                let _ = run_hook(git_dir, "post-applypatch", &[]);
            }

            let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
            return Ok(());
        } else {
            bail!("patch does not contain a valid diff");
        }
    }

    // Run applypatch-msg hook
    if !opts.no_verify {
        let msg_path = git_dir.join("MERGE_MSG");
        fs::write(&msg_path, &patch.message)?;
        if !run_hook(
            git_dir,
            "applypatch-msg",
            &[msg_path.to_str().unwrap_or("")],
        )? {
            let _ = fs::remove_file(&msg_path);
            bail!("applypatch-msg hook rejected the patch");
        }
    }

    // Try to apply the diff to the working tree. With `--3way`, Git verifies patch index
    // preimages against the work tree even when a fuzzy apply could succeed; mismatch must
    // fall through to the 3-way path (t4151 `changes.mbox`).
    let apply_result =
        apply_patch_to_worktree(work_tree, &patch.diff, opts.keep_cr, opts.three_way);

    match apply_result {
        Ok(affected_paths) => {
            // Stage only the files that the patch touched
            stage_affected_files(repo, &affected_paths)?;
        }
        Err(e) => {
            if opts.three_way {
                // Attempt 3-way merge
                apply_three_way(repo, patch)?;
            } else {
                if opts.reject {
                    let _ = write_reject_files_for_patch(work_tree, &patch.diff);
                }
                // Save message for --continue
                fs::write(git_dir.join("MERGE_MSG"), &patch.message)?;
                return Err(e);
            }
        }
    }

    // Create commit
    let index = load_index(repo)?;

    // Check for conflicts
    if index.entries.iter().any(|e| e.stage() != 0) {
        fs::write(git_dir.join("MERGE_MSG"), &patch.message)?;
        bail!("patch has conflicts");
    }

    // Check if the tree changed (for --allow-empty)
    let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let obj = repo.odb.read(head_oid)?;
        let commit = parse_commit(&obj.data)?;
        if tree_oid == commit.tree && !opts.allow_empty {
            // The patch produced an empty commit - this shouldn't happen for non-empty patches
            // but if it does, error out
            bail!("patch does not apply");
        }
    }

    // Run pre-applypatch hook
    if !opts.no_verify && !run_hook(git_dir, "pre-applypatch", &[])? {
        bail!("pre-applypatch hook rejected the patch");
    }

    create_am_commit(repo, &index, patch, opts)?;

    // Run post-applypatch hook (failure doesn't abort)
    if !opts.no_verify {
        let _ = run_hook(git_dir, "post-applypatch", &[]);
    }

    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    Ok(())
}

/// Result of whole-file 3-way text merge for `git am --3way` (matches Git’s coarse
/// “one side matches base” rules; see `builtin/am.c` / `ll_merge` outcomes).
enum AmThreeWayMerge {
    /// Merged cleanly to a single tree blob (empty string means the path should be absent).
    Clean(String),
    /// Conflict: working tree file contains conflict markers; index should hold stages 1–3.
    Conflict(String),
    /// `HEAD` had no blob (unborn branch or deleted path) but the patch modifies the base — Git
    /// records stages **1** and **3** only (`modify/delete`); work tree keeps **theirs**.
    ModifyDelete(String),
}

/// Merge `base`, `ours` (HEAD), and `theirs` (patch result) using Git’s text rules:
/// - identical `ours`/`theirs` → `ours`
/// - `ours` == `base` → `theirs`
/// - `theirs` == `base` → `ours`
/// - else conflict markers (`patch_label` becomes the `>>>>>>>` label).
fn merge_three_way_text_am(
    base: &str,
    ours: &str,
    theirs: &str,
    patch_label: &str,
) -> AmThreeWayMerge {
    if ours == theirs {
        return AmThreeWayMerge::Clean(ours.to_string());
    }
    if ours == base {
        return AmThreeWayMerge::Clean(theirs.to_string());
    }
    if theirs == base {
        return AmThreeWayMerge::Clean(ours.to_string());
    }

    if ours.is_empty() && !base.is_empty() && theirs != base {
        return AmThreeWayMerge::ModifyDelete(theirs.to_string());
    }

    let mut out = String::new();
    out.push_str("<<<<<<< HEAD\n");
    out.push_str(ours);
    if !ours.is_empty() && !ours.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("=======\n");
    out.push_str(theirs);
    if !theirs.is_empty() && !theirs.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(">>>>>>> {patch_label}\n"));
    AmThreeWayMerge::Conflict(out)
}

/// Attempt a 3-way merge when a patch doesn't apply cleanly.
fn apply_three_way(repo: &Repository, patch: &MboxPatch) -> Result<()> {
    let git_dir = &repo.git_dir;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no work tree"))?;

    // Parse the patch to extract index lines with blob SHAs
    let file_patches = parse_patch(&patch.diff)?;
    let head_tree_oid = match resolve_head(git_dir)?.oid() {
        Some(head_oid) => {
            let head_obj = repo.odb.read(head_oid)?;
            let head_commit = parse_commit(&head_obj.data)?;
            head_commit.tree
        }
        None => "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
            .parse::<ObjectId>()
            .map_err(|e| anyhow::anyhow!(e))?,
    };

    // Build the "base" tree by finding the common ancestor blobs from index lines
    // Then apply the patch to the base, and merge base->patched with HEAD
    //
    // For each file in the patch, we need:
    // 1. The base version (from the patch's index line pre-image hash)
    // 2. The "ours" version (from HEAD tree)
    // 3. The "theirs" version (base + patch applied)

    let mut any_conflict = false;
    let mut affected_paths = Vec::new();
    let mut conflict_stages: Vec<AmConflictStages> = Vec::new();

    for fp in &file_patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let rel_path = strip_components(path_str, 1);
        let mut effective_rel_path = rel_path.clone();
        let mut abs_path = work_tree.join(&effective_rel_path);

        if !abs_path.exists() {
            let preimage = preimage_from_hunks(&fp.hunks);
            if !preimage.is_empty() {
                if let Some(matched_path) =
                    find_tree_path_matching_content(repo, &head_tree_oid, &preimage)?
                {
                    effective_rel_path = matched_path;
                    abs_path = work_tree.join(&effective_rel_path);
                }
            }
        }

        affected_paths.push(effective_rel_path.clone());

        if fp.is_new {
            // New file - just apply directly
            if let Some(parent) = abs_path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let content = apply_hunks("", &fp.hunks)?;
            fs::write(&abs_path, content.as_bytes())?;
            continue;
        }

        if fp.is_deleted {
            if abs_path.exists() {
                fs::remove_file(&abs_path)?;
            }
            continue;
        }

        // "Ours" must match `HEAD` (the tree being patched), not whatever is left on disk after a
        // failed `apply_patch` attempt — using the work tree here can hide real conflicts (t4151).
        let ours =
            get_blob_from_tree(repo, &head_tree_oid, &effective_rel_path).unwrap_or_default();

        // Prefer patch index preimage blob when available.
        let base = if let Some(old_oid_str) = fp.old_oid.as_deref() {
            if let Ok(old_oid) = resolve_revision_for_patch_old_blob(repo, old_oid_str) {
                if let Ok(obj) = repo.odb.read(&old_oid) {
                    String::from_utf8_lossy(&obj.data).into_owned()
                } else {
                    build_preimage_from_hunks(&ours, &fp.hunks).unwrap_or_else(|_| ours.clone())
                }
            } else {
                build_preimage_from_hunks(&ours, &fp.hunks).unwrap_or_else(|_| ours.clone())
            }
        } else {
            // Fall back to deriving preimage from hunks.
            build_preimage_from_hunks(&ours, &fp.hunks).unwrap_or_else(|_| ours.clone())
        };

        // Apply the patch to the base to get "theirs"
        let theirs = match apply_hunks(&base, &fp.hunks) {
            Ok(t) => t,
            Err(_) => {
                // If we can't even apply to base, that's a real failure
                bail!(
                    "Failed to apply patch to {} even in 3-way mode",
                    effective_rel_path
                );
            }
        };

        let patch_subject = patch.message.lines().next().unwrap_or("patch").trim();
        let merged = merge_three_way_text_am(&base, &ours, &theirs, patch_subject);

        let base_oid = if let Some(old_oid_str) = fp.old_oid.as_deref() {
            if let Ok(old_oid) = resolve_revision_for_patch_old_blob(repo, old_oid_str) {
                if repo.odb.read(&old_oid).is_ok() {
                    old_oid
                } else {
                    repo.odb.write(ObjectKind::Blob, base.as_bytes())?
                }
            } else {
                repo.odb.write(ObjectKind::Blob, base.as_bytes())?
            }
        } else {
            repo.odb.write(ObjectKind::Blob, base.as_bytes())?
        };
        let ours_oid = repo.odb.write(ObjectKind::Blob, ours.as_bytes())?;
        let theirs_oid = repo.odb.write(ObjectKind::Blob, theirs.as_bytes())?;
        let mode =
            tree_blob_mode(repo, &head_tree_oid, &effective_rel_path).unwrap_or(MODE_REGULAR);

        match merged {
            AmThreeWayMerge::Clean(content) => {
                if content.is_empty() {
                    let _ = fs::remove_file(&abs_path);
                } else {
                    fs::write(&abs_path, content.as_bytes())?;
                }
            }
            AmThreeWayMerge::Conflict(content) => {
                any_conflict = true;
                fs::write(&abs_path, content.as_bytes())?;
                conflict_stages.push(AmConflictStages::ThreeWay {
                    path: effective_rel_path.clone(),
                    base_oid,
                    ours_oid,
                    theirs_oid,
                    mode,
                });
            }
            AmThreeWayMerge::ModifyDelete(content) => {
                any_conflict = true;
                if let Some(parent) = abs_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                fs::write(&abs_path, content.as_bytes())?;
                conflict_stages.push(AmConflictStages::ModifyDelete {
                    path: effective_rel_path.clone(),
                    base_oid,
                    theirs_oid,
                    mode,
                });
            }
        }
    }

    if any_conflict {
        stage_unmerged_am_conflicts(repo, &conflict_stages)?;
        // Stage non-conflict files from the patch (e.g. new files applied cleanly).
        let conflict_set: std::collections::HashSet<String> = conflict_stages
            .iter()
            .map(|c| match c {
                AmConflictStages::ThreeWay { path, .. }
                | AmConflictStages::ModifyDelete { path, .. } => path.clone(),
            })
            .collect();
        let rest: Vec<String> = affected_paths
            .into_iter()
            .filter(|p| !conflict_set.contains(p))
            .collect();
        if !rest.is_empty() {
            stage_affected_files(repo, &rest)?;
        }
        bail!("3-way merge has conflicts");
    }

    stage_affected_files(repo, &affected_paths)?;
    Ok(())
}

fn tree_blob_mode(repo: &Repository, tree_oid: &ObjectId, rel_path: &str) -> Option<u32> {
    let entries = tree_to_index_entries(repo, tree_oid, "").ok()?;
    let path_bytes = rel_path.as_bytes();
    entries
        .iter()
        .find(|e| e.path == path_bytes)
        .map(|e| e.mode)
}

enum AmConflictStages {
    ThreeWay {
        path: String,
        base_oid: ObjectId,
        ours_oid: ObjectId,
        theirs_oid: ObjectId,
        mode: u32,
    },
    ModifyDelete {
        path: String,
        base_oid: ObjectId,
        theirs_oid: ObjectId,
        mode: u32,
    },
}

fn stage_unmerged_am_conflicts(repo: &Repository, conflicts: &[AmConflictStages]) -> Result<()> {
    let mut index = load_index(repo)?;
    for conflict in conflicts {
        match conflict {
            AmConflictStages::ThreeWay {
                path: rel,
                base_oid,
                ours_oid,
                theirs_oid,
                mode,
            } => {
                let path_bytes = rel.as_bytes().to_vec();
                index.entries.retain(|e| e.path != path_bytes);

                let base_obj = repo.odb.read(base_oid)?;
                let ours_obj = repo.odb.read(ours_oid)?;
                let theirs_obj = repo.odb.read(theirs_oid)?;

                for (stage, oid, size) in [
                    (1u8, *base_oid, base_obj.data.len() as u32),
                    (2u8, *ours_oid, ours_obj.data.len() as u32),
                    (3u8, *theirs_oid, theirs_obj.data.len() as u32),
                ] {
                    let name_len = path_bytes.len().min(0xFFF) as u16;
                    let flags = name_len | ((stage as u16) << 12);
                    index.entries.push(IndexEntry {
                        ctime_sec: 0,
                        ctime_nsec: 0,
                        mtime_sec: 0,
                        mtime_nsec: 0,
                        dev: 0,
                        ino: 0,
                        mode: *mode,
                        uid: 0,
                        gid: 0,
                        size,
                        oid,
                        flags,
                        flags_extended: None,
                        path: path_bytes.clone(),
                    });
                }
            }
            AmConflictStages::ModifyDelete {
                path: rel,
                base_oid,
                theirs_oid,
                mode,
            } => {
                let path_bytes = rel.as_bytes().to_vec();
                index.entries.retain(|e| e.path != path_bytes);

                let base_obj = repo.odb.read(base_oid)?;
                let theirs_obj = repo.odb.read(theirs_oid)?;

                for (stage, oid, size) in [
                    (1u8, *base_oid, base_obj.data.len() as u32),
                    (3u8, *theirs_oid, theirs_obj.data.len() as u32),
                ] {
                    let name_len = path_bytes.len().min(0xFFF) as u16;
                    let flags = name_len | ((stage as u16) << 12);
                    index.entries.push(IndexEntry {
                        ctime_sec: 0,
                        ctime_nsec: 0,
                        mtime_sec: 0,
                        mtime_nsec: 0,
                        dev: 0,
                        ino: 0,
                        mode: *mode,
                        uid: 0,
                        gid: 0,
                        size,
                        oid,
                        flags,
                        flags_extended: None,
                        path: path_bytes.clone(),
                    });
                }
            }
        }
    }
    index.sort();
    repo.write_index(&mut index)?;
    Ok(())
}

fn preimage_from_hunks(hunks: &[Hunk]) -> String {
    let mut out = String::new();
    for hunk in hunks {
        for line in &hunk.lines {
            match line {
                HunkLine::Context(s) | HunkLine::Remove(s) => {
                    out.push_str(s);
                    out.push('\n');
                }
                HunkLine::Add(_) | HunkLine::NoNewline => {}
            }
        }
    }
    out
}

fn find_tree_path_matching_content(
    repo: &Repository,
    tree_oid: &ObjectId,
    content: &str,
) -> Result<Option<String>> {
    let entries = tree_to_index_entries(repo, tree_oid, "")?;
    for entry in entries {
        let obj = repo.odb.read(&entry.oid)?;
        if obj.kind != ObjectKind::Blob {
            continue;
        }
        if obj.data == content.as_bytes() {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if !path.is_empty() {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

fn write_reject_files_for_patch(work_tree: &Path, diff: &str) -> Result<()> {
    let file_patches = parse_patch(diff)?;
    for fp in &file_patches {
        let Some(path_str) = fp.effective_path() else {
            continue;
        };
        let rel_path = strip_components(path_str, 1);
        let reject_path = work_tree.join(format!("{rel_path}.rej"));
        if let Some(parent) = reject_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&reject_path, diff.as_bytes())?;
    }
    Ok(())
}

/// Build a pre-image by reversing the hunk operations on the current content.
fn build_preimage_from_hunks(current: &str, hunks: &[Hunk]) -> Result<String> {
    // The pre-image is what the file looked like before the patch.
    // We can reconstruct it from context + remove lines (those are in the original)
    // while ignoring add lines (those are new)
    let mut pre_lines: Vec<String> = Vec::new();
    let current_lines: Vec<&str> = current.lines().collect();

    let mut cur_idx = 0;
    for hunk in hunks {
        let hunk_start = if hunk.old_start == 0 {
            0
        } else {
            hunk.old_start - 1
        };
        // Copy lines before this hunk from current
        while cur_idx < hunk_start && cur_idx < current_lines.len() {
            pre_lines.push(current_lines[cur_idx].to_string());
            cur_idx += 1;
        }

        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    pre_lines.push(s.clone());
                    cur_idx += 1;
                }
                HunkLine::Remove(s) => {
                    pre_lines.push(s.clone());
                    cur_idx += 1;
                }
                HunkLine::Add(_) => {
                    // Skip add lines - they're not in the pre-image
                }
                HunkLine::NoNewline => {}
            }
        }
    }

    // Copy remaining lines
    while cur_idx < current_lines.len() {
        pre_lines.push(current_lines[cur_idx].to_string());
        cur_idx += 1;
    }

    let mut out = pre_lines.join("\n");
    if !out.is_empty() && (current.ends_with('\n') || current.is_empty()) {
        out.push('\n');
    }
    Ok(out)
}

/// Get a blob from a tree by path.
fn get_blob_from_tree(repo: &Repository, tree_oid: &ObjectId, path: &str) -> Result<String> {
    use grit_lib::objects::parse_tree;
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let name = parts[0];

    let obj = repo.odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;

    for entry in &entries {
        let entry_name = String::from_utf8_lossy(&entry.name);
        if entry_name == name {
            if parts.len() == 1 {
                // This is the file
                let blob = repo.odb.read(&entry.oid)?;
                return Ok(String::from_utf8_lossy(&blob.data).into_owned());
            } else if entry.mode == 0o040000 {
                // Recurse into subdirectory
                return get_blob_from_tree(repo, &entry.oid, parts[1]);
            }
        }
    }

    bail!("path not found in tree: {}", path);
}

fn is_all_zero_oid_hex(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.chars().all(|c| c == '0')
}

fn verify_old_oid_matches_content(expected_oid: &str, content: &str) -> Result<()> {
    let actual_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, content.as_bytes());
    let actual_hex = actual_oid.to_hex();
    if !actual_hex.starts_with(expected_oid) {
        bail!("patch does not apply");
    }
    Ok(())
}

/// Apply a unified diff to the working tree files.
/// Returns the list of affected relative paths.
///
/// When `strict_preimage` is true, every non-zero `index` preimage in the patch must match the
/// current work tree file (Git `git apply` with 3-way fallback semantics).
fn apply_patch_to_worktree(
    work_tree: &Path,
    diff: &str,
    keep_cr: bool,
    strict_preimage: bool,
) -> Result<Vec<String>> {
    // Parse the diff into file patches using the same logic as `grit apply`
    let file_patches = parse_patch(diff)?;
    let mut affected = Vec::new();

    for fp in &file_patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let rel_path = strip_components(path_str, 1);
        let path = work_tree.join(&rel_path);

        if fp.is_rename {
            // Handle rename: old path is removed, new path is added
            if let Some(old) = &fp.old_path {
                let old_rel = strip_components(old, 0);
                let old_abs = work_tree.join(&old_rel);
                if old_abs.exists() {
                    // Read old content, apply hunks if any, write to new path
                    let new_rel = fp
                        .new_path
                        .as_deref()
                        .map(|p| strip_components(p, 0))
                        .unwrap_or_else(|| rel_path.clone());
                    let new_abs = work_tree.join(&new_rel);
                    if let Some(parent) = new_abs.parent() {
                        if !parent.as_os_str().is_empty() && !parent.exists() {
                            fs::create_dir_all(parent)?;
                        }
                    }
                    let old_content = fs::read_to_string(&old_abs)
                        .with_context(|| format!("cannot read {}", old_abs.display()))?;
                    if let Some(expected_oid) = fp.old_oid.as_deref() {
                        verify_old_oid_matches_content(expected_oid, &old_content)?;
                    }
                    let new_content = if fp.hunks.is_empty() {
                        old_content
                    } else {
                        apply_hunks(&old_content, &fp.hunks).with_context(|| {
                            format!("failed to apply patch to {}", old_abs.display())
                        })?
                    };
                    fs::write(&new_abs, new_content.as_bytes())?;
                    fs::remove_file(&old_abs)?;
                    affected.push(old_rel);
                    affected.push(new_rel);
                }
            }
            continue;
        }

        affected.push(rel_path.clone());

        if fp.is_deleted {
            if path.exists() {
                if let Some(expected_oid) = fp.old_oid.as_deref() {
                    if strict_preimage || keep_cr {
                        let old_content = fs::read_to_string(&path)
                            .with_context(|| format!("cannot read {}", path.display()))?;
                        verify_old_oid_matches_content(expected_oid, &old_content)?;
                    }
                }
                fs::remove_file(&path)?;
            }
            continue;
        }

        if fp.is_new {
            if path.exists() || path.is_symlink() {
                bail!("{rel_path}: already exists in index");
            }
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let content = apply_hunks("", &fp.hunks)?;
            fs::write(&path, content.as_bytes())?;
            #[cfg(unix)]
            if fp.new_mode.as_deref() == Some("100755") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            }
            continue;
        }

        // Modify existing file
        let old_content =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        if let Some(expected_oid) = fp.old_oid.as_deref() {
            if (strict_preimage || keep_cr) && !is_all_zero_oid_hex(expected_oid) {
                verify_old_oid_matches_content(expected_oid, &old_content)?;
            }
        }

        if fp.hunks.is_empty() {
            #[cfg(unix)]
            if let Some(mode) = fp.new_mode.as_deref() {
                use std::os::unix::fs::PermissionsExt;
                let perm = if mode == "100755" { 0o755 } else { 0o644 };
                fs::set_permissions(&path, fs::Permissions::from_mode(perm))?;
            }
            continue;
        }

        let new_content = apply_hunks(&old_content, &fp.hunks)
            .with_context(|| format!("failed to apply patch to {}", path.display()))?;
        fs::write(&path, new_content.as_bytes())?;
    }

    Ok(affected)
}

/// Stage only the files affected by the patch into the index.
fn stage_affected_files(repo: &Repository, affected_paths: &[String]) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no work tree"))?;

    let mut index = load_index(repo)?;

    for rel_path in affected_paths {
        let abs = work_tree.join(rel_path);
        if !abs.exists() && !abs.is_symlink() {
            // File was deleted — remove from index
            let path_bytes = rel_path.as_bytes().to_vec();
            index.entries.retain(|e| e.path != path_bytes);
            continue;
        }

        if abs.is_dir() {
            continue;
        }

        let content = fs::read(&abs)?;
        let oid = repo.odb.write(ObjectKind::Blob, &content)?;
        let metadata = fs::metadata(&abs)?;

        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = metadata.permissions().mode();
                if perms & 0o111 != 0 {
                    0o100755u32
                } else {
                    0o100644u32
                }
            }
            #[cfg(not(unix))]
            {
                0o100644u32
            }
        };

        let path_bytes = rel_path.as_bytes().to_vec();
        let size = content.len() as u32;

        let entry = grit_lib::index::IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size,
            oid,
            flags: (path_bytes.len().min(0xFFF)) as u16,
            flags_extended: None,
            path: path_bytes,
        };
        index.add_or_replace(entry);
    }

    index.sort();
    repo.write_index(&mut index)?;
    Ok(())
}

/// Create a commit from the current index using the patch metadata.
fn create_am_commit(
    repo: &Repository,
    index: &Index,
    patch: &MboxPatch,
    opts: &AmOptions,
) -> Result<()> {
    let git_dir = &repo.git_dir;
    let tree_oid = write_tree_from_index(&repo.odb, index, "")?;

    let head = resolve_head(git_dir)?;
    let mut parents = Vec::new();
    if let Some(head_oid) = head.oid() {
        parents.push(*head_oid);
    }

    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    // Build author ident from patch metadata
    let author_ident = if opts.ignore_date {
        // --ignore-date: use author name/email from patch but current time with +0000
        let epoch = now.unix_timestamp();
        if !patch.author.is_empty() {
            format!("{} {} +0000", patch.author, epoch)
        } else {
            let (cname, cemail) = &committer;
            format!("{cname} <{cemail}> {epoch} +0000")
        }
    } else if !patch.author.is_empty() && !patch.date.is_empty() {
        format!("{} {}", patch.author, patch.date)
    } else if !patch.author.is_empty() {
        let epoch = now.unix_timestamp();
        format!("{} {} +0000", patch.author, epoch)
    } else {
        format_ident(&committer, now)
    };

    // Handle --committer-date-is-author-date
    let committer_ident = if opts.committer_date_is_author_date {
        // Extract the date portion from author_ident (everything after the closing >)
        let date_part = if let Some(pos) = author_ident.rfind("> ") {
            &author_ident[pos + 2..]
        } else {
            ""
        };
        let (cname, cemail) = &committer;
        format!("{cname} <{cemail}> {date_part}")
    } else {
        format_ident(&committer, now)
    };

    // Handle --message-id: add Message-Id trailer
    let mut message = patch.message.clone();
    if opts.message_id && !patch.message_id.is_empty() {
        let mid_line = format!("Message-Id: {}", patch.message_id);
        message = add_trailer(&message, &mid_line);
    }

    // Handle --signoff
    if opts.signoff {
        let sob_line = format!("Signed-off-by: {} <{}>", committer.0, committer.1);
        message = add_signoff(&message, &sob_line);
    }

    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author: author_ident,
        committer: committer_ident,
        author_raw: Vec::new(),
        committer_raw: Vec::new(),
        encoding: None,
        message,
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD
    update_head(git_dir, &head, &commit_oid)?;

    Ok(())
}

/// Add a trailer line to a commit message.
fn add_trailer(message: &str, trailer: &str) -> String {
    let trimmed = message.trim_end();
    let lines: Vec<&str> = trimmed.lines().collect();

    // Check if there's already a trailer block
    let has_trailer_block = lines
        .last()
        .is_some_and(|l| l.contains(": ") && !l.starts_with(' ') && !l.starts_with('\t'));

    if has_trailer_block {
        format!("{}\n{}\n", trimmed, trailer)
    } else {
        format!("{}\n\n{}\n", trimmed, trailer)
    }
}

/// Add Signed-off-by line to commit message, following git conventions.
fn add_signoff(message: &str, sob_line: &str) -> String {
    let trimmed = message.trim_end();
    let lines: Vec<&str> = trimmed.lines().collect();

    // Check if the last line is already this exact Signed-off-by
    if let Some(last) = lines.last() {
        if last.trim() == sob_line {
            // Already there as the last trailer — don't add again
            return format!("{}\n", trimmed);
        }
    }

    // Check if there's already a trailer block (lines matching "Key: value")
    let has_trailer_block = lines
        .last()
        .is_some_and(|l| l.contains(": ") && !l.starts_with(' ') && !l.starts_with('\t'));

    if has_trailer_block {
        // Append to existing trailer block
        format!("{}\n{}\n", trimmed, sob_line)
    } else {
        // Add blank line before trailer
        format!("{}\n\n{}\n", trimmed, sob_line)
    }
}

/// Show current patch during an am session.
fn do_show_current_patch(mode: &str) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_am_in_progress(git_dir) {
        bail!("error: no am session in progress");
    }

    let state_dir = am_dir(git_dir);
    let current_str = fs::read_to_string(state_dir.join("current"))
        .or_else(|_| fs::read_to_string(state_dir.join("next")))?;
    let current: usize = current_str.trim().parse()?;
    let patch_file = state_dir.join("patches").join(current.to_string());

    match mode {
        "raw" => {
            let content = fs::read_to_string(&patch_file)?;
            print!("{}", content);
        }
        "diff" => {
            let content = fs::read_to_string(&patch_file)?;
            let patch = deserialize_mbox_patch(&content)?;
            print!("{}", patch.diff);
        }
        _ => {
            bail!("invalid value for --show-current-patch: {}", mode);
        }
    }

    Ok(())
}

// ── --continue ──────────────────────────────────────────────────────

fn do_skip() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_am_in_progress(git_dir) {
        bail!("error: no am session in progress");
    }

    let state_dir = am_dir(git_dir);
    let next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
    let last: usize = fs::read_to_string(state_dir.join("last"))?.trim().parse()?;

    if next > last {
        // Nothing left to skip — just cleanup
        cleanup_am_state(git_dir);
        return Ok(());
    }

    rerere_clear(git_dir).map_err(|e| anyhow::anyhow!(e))?;

    // Match Git `am_skip`: `clean_index(&HEAD, &HEAD)` — reset index/worktree to `HEAD` while
    // preserving stat info; `ORIG_HEAD` is only used by `--abort`.
    let head_commit = resolve_head(git_dir)?.oid().copied();
    crate::commands::read_tree::am_clean_index(&repo, head_commit, head_commit)
        .context("failed to clean index")?;

    // Advance past the skipped patch
    fs::write(state_dir.join("next"), (next + 1).to_string())?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    let opts = load_am_options(&state_dir);
    apply_remaining(&repo, &opts, None)?;

    Ok(())
}

fn do_continue(interactive: bool, overrides: &AmOptionOverrides) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_am_in_progress(git_dir) {
        bail!("error: no am session in progress");
    }

    // The user should have already staged their resolution via 'git add'
    let index = load_index(&repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        bail!(
            "error: you still have unmerged files\n\
             hint: resolve conflicts, stage with 'grit add', then 'grit am --continue'"
        );
    }

    let state_dir = am_dir(git_dir);

    // Check that the index has actually changed compared to HEAD
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let head_tree = {
            let obj = repo.odb.read(head_oid)?;
            let commit = parse_commit(&obj.data)?;
            commit.tree
        };
        let index_tree = write_tree_from_index(&repo.odb, &index, "")?;
        if head_tree == index_tree {
            bail!("error: no changes - did you forget to use 'git add'?");
        }
    }

    let current: usize = fs::read_to_string(state_dir.join("current"))?
        .trim()
        .parse()?;
    let patch_file = state_dir.join("patches").join(current.to_string());
    let serialized = fs::read_to_string(&patch_file)?;
    let patch = deserialize_mbox_patch(&serialized)?;

    // Read message (might have been edited)
    let message = match fs::read_to_string(git_dir.join("MERGE_MSG")) {
        Ok(m) => m,
        Err(_) => patch.message.clone(),
    };

    let patched = MboxPatch { message, ..patch };
    let subject = patched.message.lines().next().unwrap_or("");

    let base_opts = load_am_options(&state_dir);
    let effective_opts = merge_options(&base_opts, overrides);

    if interactive && !prompt_yes_no(&format!("Apply patch '{}'? [y/N] ", subject))? {
        let next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
        fs::write(state_dir.join("next"), (next + 1).to_string())?;
        let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
        apply_remaining(&repo, &base_opts, Some(overrides))?;
        return Ok(());
    }

    // Record rerere postimage before committing
    let _ = crate::commands::rerere::record_postimage(&repo);

    if !effective_opts.quiet {
        println!("Applying: {}", subject);
    }

    create_am_commit(&repo, &index, &patched, &effective_opts)?;

    write_am_abort_safety(git_dir)?;

    // Advance next
    let next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
    fs::write(state_dir.join("next"), (next + 1).to_string())?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    // Continue with remaining
    apply_remaining(&repo, &base_opts, None)?;

    Ok(())
}

// ── --abort ─────────────────────────────────────────────────────────

fn do_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    let state_dir = am_dir(git_dir);
    if !state_dir.exists() {
        bail!("error: no am session in progress");
    }

    // Handle stray directory (no applying marker or no state files)
    if !state_dir.join("applying").exists() || !state_dir.join("orig-head").exists() {
        let _ = fs::remove_dir_all(&state_dir);
        return Ok(());
    }

    if !am_safe_to_abort(git_dir)? {
        eprintln!(
            "warning: You seem to have moved HEAD since the last 'am' failure.\n\
             Not rewinding to ORIG_HEAD"
        );
        cleanup_am_state(git_dir);
        return Ok(());
    }

    rerere_clear(git_dir).map_err(|e| anyhow::anyhow!(e))?;

    let head_commit = resolve_head(git_dir)?.oid().copied();
    let orig_commit = grit_lib::refs::resolve_ref(git_dir, "ORIG_HEAD")
        .ok()
        .or_else(|| {
            let hex = fs::read_to_string(state_dir.join("orig-head")).ok()?;
            let t = hex.trim();
            if t.is_empty() {
                None
            } else {
                ObjectId::from_hex(t).ok()
            }
        });

    if let Err(e) = crate::commands::read_tree::am_clean_index(&repo, head_commit, orig_commit) {
        eprintln!("fatal: failed to clean index\n{e}");
        // Git leaves the am session in place and exits 128 (t4151-am-abort).
        std::process::exit(128);
    }

    let head_name = fs::read_to_string(state_dir.join("head-name")).unwrap_or_default();
    let head_name = head_name.trim();

    if let Some(orig) = orig_commit {
        if let Some(refname) = head_name.strip_prefix("ref: ") {
            fs::write(git_dir.join("HEAD"), format!("{head_name}\n"))?;
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", orig.to_hex()))?;
        } else {
            fs::write(git_dir.join("HEAD"), format!("{}\n", orig.to_hex()))?;
        }
    } else if let Some(refname) = head_name.strip_prefix("ref: ") {
        let _ = delete_ref(git_dir, refname);
    }

    cleanup_am_state(git_dir);
    let _ = delete_ref(git_dir, "ORIG_HEAD");
    eprintln!("am session aborted.");

    Ok(())
}

// ── Save/Load options ───────────────────────────────────────────────

fn save_am_options(state_dir: &Path, opts: &AmOptions) -> Result<()> {
    let mut out = String::new();
    if opts.three_way {
        out.push_str("threeway\n");
    }
    if opts.keep_cr {
        out.push_str("keep-cr\n");
    }
    if opts.no_verify {
        out.push_str("no-verify\n");
    }
    if opts.signoff {
        out.push_str("signoff\n");
    }
    if opts.reject {
        out.push_str("reject\n");
    }
    if opts.quiet {
        out.push_str("quiet\n");
    }
    if opts.message_id {
        out.push_str("message-id\n");
    }
    if opts.allow_empty {
        out.push_str("allow-empty\n");
    }
    if opts.ignore_date {
        out.push_str("ignore-date\n");
    }
    out.push_str(&format!("empty={}\n", opts.empty));
    fs::write(state_dir.join("options"), out)?;
    Ok(())
}

fn load_am_options(state_dir: &Path) -> AmOptions {
    let content = fs::read_to_string(state_dir.join("options")).unwrap_or_default();
    let mut opts = AmOptions {
        quiet: false,
        three_way: false,
        keep_cr: false,
        no_verify: false,
        signoff: false,
        reject: false,
        committer_date_is_author_date: false,
        ignore_date: false,
        message_id: false,
        empty: "stop".to_string(),
        allow_empty: false,
    };
    for line in content.lines() {
        match line.trim() {
            "threeway" => opts.three_way = true,
            "keep-cr" => opts.keep_cr = true,
            "no-verify" => opts.no_verify = true,
            "signoff" => opts.signoff = true,
            "reject" => opts.reject = true,
            "quiet" => opts.quiet = true,
            "message-id" => opts.message_id = true,
            "allow-empty" => opts.allow_empty = true,
            "ignore-date" => opts.ignore_date = true,
            l if l.starts_with("empty=") => opts.empty = l[6..].to_string(),
            _ => {}
        }
    }
    opts
}

// ── Hooks ───────────────────────────────────────────────────────────

fn run_hook(git_dir: &Path, hook_name: &str, args: &[&str]) -> Result<bool> {
    let hook_path = git_dir.join("hooks").join(hook_name);
    if !hook_path.exists() {
        return Ok(true); // No hook = success
    }

    // Determine the work tree (parent of git_dir, unless it's a bare repo)
    let work_dir = git_dir.parent().unwrap_or(git_dir);

    // Build the command - use sh to handle scripts without shebangs
    let mut cmd = std::process::Command::new(&hook_path);
    cmd.args(args).env("GIT_DIR", git_dir).current_dir(work_dir);

    let status = cmd
        .status()
        .or_else(|_| {
            // If direct execution fails, try via /bin/sh
            std::process::Command::new("/bin/sh")
                .arg(&hook_path)
                .args(args)
                .env("GIT_DIR", git_dir)
                .current_dir(work_dir)
                .status()
        })
        .with_context(|| format!("failed to execute hook {}", hook_name))?;

    Ok(status.success())
}

// ── Cleanup ─────────────────────────────────────────────────────────

fn cleanup_am_state(git_dir: &Path) {
    let state_dir = am_dir(git_dir);
    // Only clean up if this is an am session (has "applying" marker)
    if state_dir.join("applying").exists() {
        let _ = fs::remove_dir_all(&state_dir);
    }
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
}

// ── Patch format detection and alternate format parsing ─────────────

/// Detect the patch format from file content.
fn detect_patch_format(input: &str) -> &'static str {
    let trimmed = input.trim_start();
    if trimmed.starts_with("# HG changeset patch") {
        return "hg";
    }
    // stgit format: first non-blank line is the subject (not a header),
    // followed by From:/Date: headers
    let mut lines = trimmed.lines();
    if let Some(first) = lines.next() {
        // Skip blanks after first line
        let mut peeked = lines.clone();
        // Look at lines 2-5 for From:/Date: pattern typical of stgit
        for _ in 0..5 {
            if let Some(l) = peeked.next() {
                let lt = l.trim();
                if lt.is_empty() {
                    continue;
                }
                if lt.starts_with("From:") || lt.starts_with("Date:") {
                    // Looks like stgit if first line isn't a standard mbox header
                    if !first.starts_with("From ")
                        && !first.starts_with("From:")
                        && !first.starts_with("Subject:")
                        && !first.starts_with("Date:")
                        && !first.starts_with("Message-ID:")
                        && !first.starts_with("X-")
                    {
                        return "stgit";
                    }
                }
                break;
            }
        }
    }
    "mbox"
}

/// Detect if a file is an stgit series file.
/// A series file has the specific comment "# This series applies on GIT commit"
/// followed by filenames.
fn is_stgit_series(input: &str) -> bool {
    let mut has_series_header = false;
    let mut has_from_or_date = false;
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("# This series applies on GIT commit") {
            has_series_header = true;
        }
        if trimmed.starts_with("From:") || trimmed.starts_with("Date:") {
            has_from_or_date = true;
        }
    }
    // It's a series file if it has the series header and no From:/Date: headers
    has_series_header && !has_from_or_date
}

/// Parse an stgit-format patch into an MboxPatch.
fn parse_stgit_patch(input: &str) -> Result<Vec<MboxPatch>> {
    let mut lines = input.lines();
    let mut subject = String::new();
    let mut author = String::new();
    let mut date = String::new();
    let mut body_lines = Vec::new();
    let mut diff_lines = Vec::new();
    let mut in_diff = false;
    let mut in_headers;
    let mut past_separator = false;

    // First non-blank line is the subject
    for line in lines.by_ref() {
        if !line.trim().is_empty() {
            subject = line.trim().to_string();
            break;
        }
    }

    // Next lines are headers (From:, Date:) until blank line
    in_headers = true;
    for line in lines.by_ref() {
        if in_headers {
            if line.trim().is_empty() {
                in_headers = false;
                continue;
            }
            if let Some(val) = line.strip_prefix("From:") {
                author = val.trim().to_string();
                continue;
            }
            if let Some(val) = line.strip_prefix("Date:") {
                date = val.trim().to_string();
                continue;
            }
            // Not a header — must be body
            in_headers = false;
            body_lines.push(line);
            continue;
        }

        if !in_diff {
            if line == "---" {
                past_separator = true;
                continue;
            }
            if past_separator && line.starts_with("diff --git ") {
                in_diff = true;
                diff_lines.push(line);
                continue;
            }
            if past_separator {
                // Skip diffstat lines between --- and diff --git
                continue;
            }
            if line.starts_with("diff --git ") {
                in_diff = true;
                diff_lines.push(line);
                continue;
            }
            body_lines.push(line);
        } else {
            if line == "-- " {
                break;
            }
            diff_lines.push(line);
        }
    }

    let author_ident = parse_author_ident(&author, &date);
    let body = body_lines.join("\n").trim().to_string();
    let message = if body.is_empty() {
        format!("{}\n", subject)
    } else {
        format!("{}\n\n{}\n", subject, body)
    };
    let mut diff = diff_lines.join("\n");
    if !diff.is_empty() {
        diff.push('\n');
    }

    Ok(vec![MboxPatch {
        author: author_ident.0,
        date: author_ident.1,
        message,
        diff,
        message_id: String::new(),
    }])
}

/// Parse an stgit series file: read the series, then parse each referenced patch.
fn parse_stgit_series(series_path: &str) -> Result<Vec<MboxPatch>> {
    let content = fs::read_to_string(series_path)
        .with_context(|| format!("cannot read series file '{series_path}'"))?;
    let series_dir = std::path::Path::new(series_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let mut patches = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let patch_path = series_dir.join(trimmed);
        let patch_content = fs::read_to_string(&patch_path)
            .with_context(|| format!("cannot read patch '{}'", patch_path.display()))?;
        let mut parsed = parse_stgit_patch(&patch_content)?;
        patches.append(&mut parsed);
    }
    Ok(patches)
}

/// Parse an hg (Mercurial) format patch into an MboxPatch.
fn parse_hg_patch(input: &str) -> Result<Vec<MboxPatch>> {
    let mut lines = input.lines();
    let mut author = String::new();
    let mut date = String::new();
    let mut body_lines = Vec::new();
    let mut diff_lines = Vec::new();
    let mut in_diff = false;

    // Parse HG headers (lines starting with #)
    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed == "# HG changeset patch" {
            continue;
        }
        if let Some(val) = trimmed.strip_prefix("# User ") {
            author = val.to_string();
            continue;
        }
        if let Some(val) = trimmed.strip_prefix("# Date ") {
            // HG date format: "epoch offset" where offset is seconds west of UTC
            // Convert to git format: "epoch +/-HHMM"
            let parts: Vec<&str> = val.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Ok(epoch), Ok(offset_secs)) =
                    (parts[0].parse::<i64>(), parts[1].parse::<i64>())
                {
                    // HG offset is seconds west of UTC (positive = west)
                    // Git offset is +/-HHMM (positive = east)
                    let git_offset_secs = -offset_secs;
                    let sign = if git_offset_secs >= 0 { '+' } else { '-' };
                    let abs_secs = git_offset_secs.unsigned_abs();
                    let hours = abs_secs / 3600;
                    let mins = (abs_secs % 3600) / 60;
                    date = format!("{} {}{:02}{:02}", epoch, sign, hours, mins);
                } else {
                    date = val.to_string();
                }
            } else {
                date = val.to_string();
            }
            continue;
        }
        if trimmed.starts_with("# ") || trimmed == "#" {
            // Skip other HG headers (Node ID, Parent, etc.)
            continue;
        }
        // First non-header line — this is the start of the body
        body_lines.push(line);
        break;
    }

    // Parse remaining body + diff
    for line in lines {
        if !in_diff {
            if line.starts_with("diff --git ") || line.starts_with("diff -r ") {
                in_diff = true;
                diff_lines.push(line);
                continue;
            }
            body_lines.push(line);
        } else {
            diff_lines.push(line);
        }
    }

    let author_ident = parse_author_ident(&author, &date);
    let body = body_lines.join("\n").trim().to_string();
    // For HG patches, the first line of the body is the subject
    let (subject, rest) = if let Some(idx) = body.find('\n') {
        (body[..idx].to_string(), body[idx + 1..].trim().to_string())
    } else {
        (body.clone(), String::new())
    };

    let message = if rest.is_empty() {
        format!("{}\n", subject)
    } else {
        format!("{}\n\n{}\n", subject, rest)
    };
    let mut diff = diff_lines.join("\n");
    if !diff.is_empty() {
        diff.push('\n');
    }

    Ok(vec![MboxPatch {
        author: author_ident.0,
        date: author_ident.1,
        message,
        diff,
        message_id: String::new(),
    }])
}

/// Parse patches from input, auto-detecting or using the specified format.
fn parse_patches(
    input: &str,
    format: Option<&str>,
    keep: bool,
    keep_non_patch: bool,
    scissors: bool,
    no_scissors: bool,
    keep_cr: bool,
    quoted_cr_action: QuotedCrAction,
) -> Result<Vec<MboxPatch>> {
    let fmt = format.unwrap_or_else(|| detect_patch_format(input));
    match fmt {
        "stgit" => parse_stgit_patch(input),
        "hg" => parse_hg_patch(input),
        _ => parse_mbox_with_opts(
            input,
            keep,
            keep_non_patch,
            scissors,
            no_scissors,
            keep_cr,
            quoted_cr_action,
        ),
    }
}

// ── Mbox parsing ────────────────────────────────────────────────────

/// Unquote mboxrd format: lines starting with >From (or >>From, etc.) are unquoted.
/// In mboxrd, "From " lines inside messages are escaped by prepending ">".
/// Un-flow format=flowed lines (RFC 3676).
/// Lines ending with a trailing space are "flowed" — joined with the next line.
/// Also handles space-unstuffing: one leading space is removed from lines
/// that start with a space (to undo the space-stuffing required by RFC 3676).
fn unflow_format_flowed(lines: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for line in lines {
        // Space-unstuffing: remove one leading space
        let unstuffed = if line.starts_with(' ') {
            &line[1..]
        } else {
            line
        };

        if unstuffed.ends_with(' ') {
            // Flowed line: keep the trailing space (it's content), join with next
            current.push_str(unstuffed);
        } else if !current.is_empty() {
            current.push_str(unstuffed);
            result.push(current.clone());
            current.clear();
        } else {
            result.push(unstuffed.to_string());
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn split_lines_preserve_cr(input: &str) -> Vec<&str> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = input.split('\n').collect();
    if input.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn unquote_mboxrd(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_body = false;

    for line in split_lines_preserve_cr(input) {
        let line_no_cr = line.strip_suffix('\r').unwrap_or(line);
        if line_no_cr.starts_with("From ") && line_no_cr.len() > 5 {
            // mbox separator - reset state
            in_body = false;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if !in_body {
            if line_no_cr.is_empty() {
                in_body = true;
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // In body: unquote >From lines
        if line_no_cr.starts_with(">From ")
            || (line_no_cr.starts_with(">>") && line_no_cr.contains("From "))
        {
            // Strip one leading > if the line matches >+From pattern
            let stripped = line.strip_prefix(">").unwrap_or(line);
            result.push_str(stripped);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing extra newline if input didn't end with one
    if !input.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

fn base64_decode(input: &str) -> Result<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
        if byte == b'=' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let val = TABLE
            .iter()
            .position(|&c| c == byte)
            .ok_or_else(|| anyhow::anyhow!("invalid base64 payload in mbox"))?;
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    Ok(output)
}

fn decode_transfer_payload(
    payload: &str,
    transfer_encoding: &str,
    keep_cr: bool,
    quoted_cr_action: QuotedCrAction,
) -> Result<String> {
    if transfer_encoding != "base64" {
        if keep_cr {
            return Ok(payload.to_string());
        }
        return Ok(payload.replace('\r', ""));
    }

    let decoded = base64_decode(payload)?;
    let mut text = String::from_utf8_lossy(&decoded).into_owned();
    if !keep_cr && text.contains('\r') {
        match quoted_cr_action {
            QuotedCrAction::Strip => {
                text = text.replace('\r', "");
            }
            QuotedCrAction::Warn => {
                eprintln!("warning: quoted CRLF detected");
            }
            QuotedCrAction::Nowarn => {}
        }
    }
    Ok(text)
}

fn split_message_body_and_diff(payload_lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut body_lines = Vec::new();
    let mut diff_lines = Vec::new();
    let mut i = 0usize;
    let mut in_diff = false;

    while i < payload_lines.len() {
        let line = payload_lines[i].as_str();
        let line_no_cr = line.strip_suffix('\r').unwrap_or(line);
        if !in_diff {
            if line_no_cr == "---" {
                i += 1;
                while i < payload_lines.len() {
                    let stat_line = payload_lines[i].as_str();
                    let stat_line_no_cr = stat_line.strip_suffix('\r').unwrap_or(stat_line);
                    if stat_line_no_cr.starts_with("diff --git ") {
                        in_diff = true;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if line_no_cr.starts_with("diff --git ") {
                in_diff = true;
            } else {
                body_lines.push(payload_lines[i].clone());
                i += 1;
                continue;
            }
        }

        if line_no_cr == "-- " {
            break;
        }
        diff_lines.push(payload_lines[i].clone());
        i += 1;
    }

    (body_lines, diff_lines)
}

/// Parse an mbox file into individual patches with options.
fn parse_mbox_with_opts(
    input: &str,
    keep: bool,
    keep_non_patch: bool,
    scissors: bool,
    no_scissors: bool,
    keep_cr: bool,
    quoted_cr_action: QuotedCrAction,
) -> Result<Vec<MboxPatch>> {
    // Handle mboxrd: unquote >From lines
    let input = unquote_mboxrd(input);
    let mut patches = Vec::new();
    let line_storage = split_lines_preserve_cr(&input);
    let mut lines = line_storage.iter().copied().peekable();

    while lines.peek().is_some() {
        // Skip to next "From " line (mbox separator)
        // Or if we're at the start and there's no "From " line, treat as single patch
        let mut _in_headers = false;
        let mut author = String::new();
        let mut date = String::new();
        let mut subject = String::new();
        let mut message_id = String::new();
        let _body = String::new();
        let mut found_from = false;

        // Look for "From " separator line
        while let Some(&line) = lines.peek() {
            let line_no_cr = line.strip_suffix('\r').unwrap_or(line);
            if line_no_cr.starts_with("From ") && line_no_cr.len() > 5 {
                found_from = true;
                lines.next(); // consume "From " line
                break;
            }
            // If we haven't found any "From " line yet and we see headers, treat as raw patch
            if !found_from
                && (line_no_cr.starts_with("From:")
                    || line_no_cr.starts_with("Subject:")
                    || line_no_cr.starts_with("Date:")
                    || line_no_cr.starts_with("Message-ID:")
                    || line_no_cr.starts_with("Message-Id:")
                    || line_no_cr.starts_with("X-"))
            {
                found_from = true;
                break;
            }
            if !found_from {
                lines.next(); // skip non-header lines before first message
                continue;
            }
            break;
        }

        if !found_from && lines.peek().is_none() {
            break;
        }

        // Parse headers
        _in_headers = true;
        let mut last_header = String::new();
        let mut is_format_flowed = false;
        let mut content_transfer_encoding = String::new();

        while let Some(&line) = lines.peek() {
            let line_no_cr = line.strip_suffix('\r').unwrap_or(line);
            if line_no_cr.is_empty() {
                lines.next();
                _in_headers = false;
                break;
            }
            // Continuation line (starts with whitespace)
            if (line_no_cr.starts_with(' ') || line_no_cr.starts_with('\t'))
                && !last_header.is_empty()
            {
                if last_header == "subject" {
                    subject.push(' ');
                    subject.push_str(line_no_cr.trim());
                }
                lines.next();
                continue;
            }

            if let Some(value) = line_no_cr.strip_prefix("From: ") {
                author = value.trim().to_string();
                last_header = "from".to_string();
            } else if let Some(value) = line_no_cr.strip_prefix("Date: ") {
                date = value.trim().to_string();
                last_header = "date".to_string();
            } else if let Some(value) = line_no_cr.strip_prefix("Subject: ") {
                // Strip [PATCH ...] prefix unless --keep
                let subj = if keep {
                    value.trim().to_string()
                } else if keep_non_patch {
                    strip_patch_prefix_keep_non_patch(value.trim())
                } else {
                    strip_patch_prefix(value.trim())
                };
                subject = subj;
                last_header = "subject".to_string();
            } else if let Some(value) = line_no_cr
                .strip_prefix("Message-ID: ")
                .or_else(|| line_no_cr.strip_prefix("Message-Id: "))
                .or_else(|| line_no_cr.strip_prefix("Message-id: "))
            {
                message_id = value.trim().to_string();
                last_header = "message-id".to_string();
            } else if let Some(value) = line_no_cr
                .strip_prefix("Content-Type: ")
                .or_else(|| line_no_cr.strip_prefix("Content-type: "))
            {
                if value.to_lowercase().contains("format=flowed") {
                    is_format_flowed = true;
                }
                last_header = "content-type".to_string();
            } else if let Some(value) = line_no_cr
                .strip_prefix("Content-Transfer-Encoding: ")
                .or_else(|| line_no_cr.strip_prefix("Content-transfer-encoding: "))
            {
                content_transfer_encoding = value.trim().to_ascii_lowercase();
                last_header = "content-transfer-encoding".to_string();
            } else {
                last_header = String::new();
            }
            lines.next();
        }

        let mut raw_payload_lines = Vec::new();
        while let Some(&line) = lines.peek() {
            let line_no_cr = line.strip_suffix('\r').unwrap_or(line);
            if line_no_cr.starts_with("From ") && line_no_cr.len() > 5 {
                break;
            }
            raw_payload_lines.push(line.to_string());
            lines.next();
        }

        let raw_payload = raw_payload_lines.join("\n");
        let decoded_payload = decode_transfer_payload(
            &raw_payload,
            &content_transfer_encoding,
            keep_cr,
            quoted_cr_action,
        )?;
        let mut payload_lines: Vec<String> = decoded_payload
            .split('\n')
            .map(|l| {
                if keep_cr {
                    l.to_string()
                } else {
                    l.strip_suffix('\r').unwrap_or(l).to_string()
                }
            })
            .collect();
        if payload_lines.last().is_some_and(String::is_empty) {
            payload_lines.pop();
        }
        let (body_lines, diff_lines) = split_message_body_and_diff(&payload_lines);

        // Build message from subject + body. Subject continuation lines in
        // mailbox headers are folded in two ways:
        // - default (`git am`): unwrap subject continuations into one line;
        // - keep mode (`git am -k`): preserve continuation line breaks.
        //
        // `Subject:` continuation lines are captured in `body_lines` by this
        // parser, so normalize here before constructing the final message.
        let mut effective_body_lines: Vec<String> = if is_format_flowed {
            let body_refs: Vec<&str> = body_lines.iter().map(String::as_str).collect();
            unflow_format_flowed(&body_refs)
        } else {
            body_lines.clone()
        };
        let mut body_str = effective_body_lines.join("\n").trim().to_string();
        if !body_str.is_empty() && !subject.is_empty() {
            let mut consumed = 0usize;
            let mut continuation = Vec::new();
            for line in &effective_body_lines {
                if line.trim().is_empty() {
                    break;
                }
                continuation.push(line.trim().to_string());
                consumed += 1;
            }
            if !continuation.is_empty() {
                if keep {
                    subject = format!("{subject}\n{}", continuation.join("\n"));
                } else {
                    subject = format!("{subject} {}", continuation.join(" "));
                }
                effective_body_lines.drain(0..consumed);
                body_str = effective_body_lines.join("\n").trim().to_string();
            }
        }

        // Handle --scissors: trim at scissors line, potentially replace subject
        if scissors && !no_scissors {
            let (new_subject, new_body) = apply_scissors_to_message(&subject, &body_str);
            subject = new_subject;
            body_str = new_body;
        }

        let message = if body_str.is_empty() {
            format!("{}\n", subject)
        } else {
            format!("{}\n\n{}\n", subject, body_str)
        };

        // Parse author into "Name <email>" format and extract date
        let author_ident = parse_author_ident(&author, &date);

        // Un-flow format=flowed content
        let effective_diff_lines: Vec<String> = if is_format_flowed {
            eprintln!(
                "warning: Patch sent with format=flowed; space at the end of lines might be lost."
            );
            let diff_refs: Vec<&str> = diff_lines.iter().map(String::as_str).collect();
            unflow_format_flowed(&diff_refs)
        } else {
            diff_lines.clone()
        };

        let mut diff_section = effective_diff_lines.join("\n");
        if !diff_section.is_empty() {
            diff_section.push('\n');
        }

        if !subject.is_empty() || !diff_section.is_empty() {
            patches.push(MboxPatch {
                author: author_ident.0,
                date: author_ident.1,
                message,
                diff: diff_section,
                message_id: message_id.clone(),
            });
        }
    }

    Ok(patches)
}

/// Strip "[PATCH n/m] " or "[PATCH] " prefix from subject.
fn strip_patch_prefix(subject: &str) -> String {
    if subject.starts_with('[') {
        if let Some(end) = subject.find(']') {
            let rest = subject[end + 1..].trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    subject.to_string()
}

/// Strip only PATCH-related bracket content, keep non-patch brackets.
fn strip_patch_prefix_keep_non_patch(subject: &str) -> String {
    if subject.starts_with('[') {
        if let Some(end) = subject.find(']') {
            let bracket_content = &subject[1..end];
            // If it looks like a PATCH prefix, strip it
            if bracket_content.contains("PATCH") {
                let rest = subject[end + 1..].trim();
                if !rest.is_empty() {
                    return rest.to_string();
                }
            }
        }
    }
    subject.to_string()
}

/// Apply scissors to the full message (subject + body), replacing subject if needed.
fn apply_scissors_to_message(subject: &str, body: &str) -> (String, String) {
    // Check if scissors line is in the body
    let mut scissors_idx = None;
    let body_lines: Vec<&str> = body.lines().collect();
    for (i, line) in body_lines.iter().enumerate() {
        if is_scissors_line(line.trim()) {
            scissors_idx = Some(i);
            break;
        }
    }

    if let Some(idx) = scissors_idx {
        // Everything after scissors
        let after: Vec<&str> = body_lines[idx + 1..].to_vec();
        let after_text = after.join("\n");
        let after_trimmed = after_text.trim();

        // Look for Subject: pseudo-header after scissors
        let mut new_subject = String::new();
        let mut new_body_lines = Vec::new();
        let mut in_headers = true;

        for line in after_trimmed.lines() {
            if in_headers {
                if line.is_empty() {
                    in_headers = false;
                    continue;
                }
                if let Some(val) = line.strip_prefix("Subject: ") {
                    new_subject = val.trim().to_string();
                    continue;
                }
                // Non-header line
                in_headers = false;
                new_body_lines.push(line);
            } else {
                new_body_lines.push(line);
            }
        }

        if new_subject.is_empty() {
            new_subject = subject.to_string();
        }

        let new_body = new_body_lines.join("\n").trim().to_string();
        (new_subject, new_body)
    } else {
        (subject.to_string(), body.to_string())
    }
}

/// Check if a line is a scissors line.
/// Git looks for lines containing ">8" or "8<" preceded by dashes/spaces.
/// Examples: "-- >8 --", " - - >8 - - remove everything above"
fn is_scissors_line(line: &str) -> bool {
    // Find ">8" or "8<" in the line
    let scissors_pos = if let Some(pos) = line.find(">8") {
        pos
    } else if let Some(pos) = line.find("8<") {
        pos
    } else {
        return false;
    };

    // Everything before the scissors marker must be only '-' and ' '
    let prefix = &line[..scissors_pos];
    if prefix.is_empty() {
        return false;
    }
    prefix.chars().all(|c| c == '-' || c == ' ')
}

/// Parse "Name <email>" and date string into (author_ident, epoch_offset).
fn parse_author_ident(author: &str, date: &str) -> (String, String) {
    // Try to parse the date into epoch format
    let epoch_date = parse_date_to_epoch(date);
    (author.to_string(), epoch_date)
}

/// Try to parse various date formats into "epoch offset" format.
fn parse_date_to_epoch(date: &str) -> String {
    if date.is_empty() {
        return String::new();
    }

    // Already in "epoch offset" format?
    let parts: Vec<&str> = date.split_whitespace().collect();
    if parts.len() == 2 && parts[0].parse::<i64>().is_ok() {
        return date.to_string();
    }

    // Try RFC 2822-like: "Thu, 07 Apr 2005 22:14:13 -0700"
    if let Some(parsed) = parse_rfc2822_date(date) {
        return parsed;
    }

    // Fall back: just use the date string as-is
    date.to_string()
}

/// Parse an RFC 2822-style date into "epoch offset" format.
fn parse_rfc2822_date(date: &str) -> Option<String> {
    // Format: "Day, DD Mon YYYY HH:MM:SS +/-HHMM" or without the day prefix
    let trimmed = date.trim();

    // Extract the timezone offset (last token)
    let (date_part, tz_str) = {
        let parts: Vec<&str> = trimmed.rsplitn(2, ' ').collect();
        if parts.len() != 2 {
            return None;
        }
        (parts[1], parts[0])
    };

    // Parse timezone offset like +0700 or -0700
    if tz_str.len() != 5 {
        return None;
    }
    let tz_sign = match tz_str.chars().next()? {
        '+' => 1i32,
        '-' => -1i32,
        _ => return None,
    };
    let tz_hours: i32 = tz_str[1..3].parse().ok()?;
    let tz_mins: i32 = tz_str[3..5].parse().ok()?;
    let tz_offset_secs = tz_sign * (tz_hours * 3600 + tz_mins * 60);

    // Strip leading "Day, " if present
    let date_str = if date_part.contains(',') {
        let (_, rest) = date_part.split_once(',')?;
        rest.trim()
    } else {
        date_part.trim()
    };

    // Parse "DD Mon YYYY HH:MM:SS"
    let tokens: Vec<&str> = date_str.split_whitespace().collect();
    if tokens.len() < 4 {
        return None;
    }

    let day: u32 = tokens[0].parse().ok()?;
    let month = match tokens[1].to_lowercase().as_str() {
        "jan" => 1u32,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let year: i32 = tokens[2].parse().ok()?;
    let time_parts: Vec<&str> = tokens[3].split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = if time_parts.len() > 2 {
        time_parts[2].parse().ok()?
    } else {
        0
    };

    // Convert to Unix timestamp
    // Days from year 0 to year, then month/day, then subtract Unix epoch
    let epoch = datetime_to_epoch(year, month, day, hour, min, sec, tz_offset_secs)?;

    Some(format!("{} {}", epoch, tz_str))
}

/// Convert a date to Unix epoch seconds.
fn datetime_to_epoch(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    tz_offset_secs: i32,
) -> Option<i64> {
    // Use a simple calculation
    let m = if month <= 2 { month + 12 } else { month };
    let y = if month <= 2 { year - 1 } else { year };

    // Julian Day Number
    let jdn = (day as i64) + (153 * (m as i64 - 3) + 2) / 5 + 365 * (y as i64) + (y as i64) / 4
        - (y as i64) / 100
        + (y as i64) / 400
        + 1721119;

    // Unix epoch = JDN of 1970-01-01 = 2440588
    let days_since_epoch = jdn - 2440588;
    let secs = days_since_epoch * 86400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64);
    let utc_secs = secs - (tz_offset_secs as i64);

    Some(utc_secs)
}

/// Serialize an MboxPatch for storage in the state directory.
fn serialize_mbox_patch(patch: &MboxPatch) -> String {
    let mut out = String::new();
    out.push_str(&format!("Author: {}\n", patch.author));
    out.push_str(&format!("Date: {}\n", patch.date));
    if !patch.message_id.is_empty() {
        out.push_str(&format!("Message-ID: {}\n", patch.message_id));
    }
    out.push_str(&format!("Message-Length: {}\n", patch.message.len()));
    out.push_str(&format!("Diff-Length: {}\n", patch.diff.len()));
    out.push('\n');
    out.push_str(&patch.message);
    out.push_str(&patch.diff);
    out
}

/// Deserialize an MboxPatch from state directory storage.
fn deserialize_mbox_patch(data: &str) -> Result<MboxPatch> {
    let mut author = String::new();
    let mut date = String::new();
    let mut message_id = String::new();
    let mut msg_len = 0usize;
    let mut diff_len = 0usize;

    let split_at = data.find("\n\n").unwrap_or(data.len());
    let header = &data[..split_at];
    let remaining = if split_at < data.len() {
        &data[split_at + 2..]
    } else {
        ""
    };

    for line in header.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(v) = line.strip_prefix("Author: ") {
            author = v.to_string();
        } else if let Some(v) = line.strip_prefix("Date: ") {
            date = v.to_string();
        } else if let Some(v) = line.strip_prefix("Message-ID: ") {
            message_id = v.to_string();
        } else if let Some(v) = line.strip_prefix("Message-Length: ") {
            msg_len = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Diff-Length: ") {
            diff_len = v.parse().unwrap_or(0);
        }
    }

    let message = if msg_len > 0 && msg_len <= remaining.len() {
        remaining[..msg_len].to_string()
    } else {
        remaining.to_string()
    };

    let diff = if diff_len > 0 && msg_len.saturating_add(diff_len) <= remaining.len() {
        remaining[msg_len..msg_len + diff_len].to_string()
    } else if msg_len < remaining.len() {
        remaining[msg_len..].to_string()
    } else {
        String::new()
    };

    Ok(MboxPatch {
        author,
        date,
        message,
        diff,
        message_id,
    })
}

// ── Patch parsing (subset of apply.rs logic) ────────────────────────

#[derive(Debug, Clone)]
struct FilePatch {
    old_path: Option<String>,
    new_path: Option<String>,
    old_mode: Option<String>,
    new_mode: Option<String>,
    old_oid: Option<String>,
    is_new: bool,
    is_deleted: bool,
    is_rename: bool,
    hunks: Vec<Hunk>,
}

impl FilePatch {
    fn effective_path(&self) -> Option<&str> {
        if self.is_deleted {
            return self
                .old_path
                .as_deref()
                .filter(|p| *p != "/dev/null")
                .or(self.new_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        if self.is_new {
            return self
                .new_path
                .as_deref()
                .filter(|p| *p != "/dev/null")
                .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        self.new_path
            .as_deref()
            .filter(|p| *p != "/dev/null")
            .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"))
    }
}

#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    _old_count: usize,
    _new_start: usize,
    _new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
    NoNewline,
}

fn parse_patch(input: &str) -> Result<Vec<FilePatch>> {
    let mut lines: Vec<&str> = if input.is_empty() {
        Vec::new()
    } else {
        input.split('\n').collect()
    };
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let mut patches = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line_no_cr = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
        if let Some(rest) = line_no_cr.strip_prefix("diff --git ") {
            let mut fp = FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                old_oid: None,
                is_new: false,
                is_deleted: false,
                is_rename: false,
                hunks: Vec::new(),
            };

            if let Some((a, b)) = split_diff_git_paths(rest) {
                fp.old_path = Some(a);
                fp.new_path = Some(b);
            }
            i += 1;

            while i < lines.len()
                && !lines[i]
                    .strip_suffix('\r')
                    .unwrap_or(lines[i])
                    .starts_with("--- ")
                && !lines[i]
                    .strip_suffix('\r')
                    .unwrap_or(lines[i])
                    .starts_with("diff --git ")
                && !lines[i]
                    .strip_suffix('\r')
                    .unwrap_or(lines[i])
                    .starts_with("@@ ")
            {
                let line = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                if let Some(val) = line.strip_prefix("old mode ") {
                    fp.old_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("new mode ") {
                    fp.new_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("new file mode ") {
                    fp.is_new = true;
                    fp.new_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("deleted file mode ") {
                    fp.is_deleted = true;
                    fp.old_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("rename from ") {
                    fp.is_rename = true;
                    fp.old_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("rename to ") {
                    fp.is_rename = true;
                    fp.new_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("index ") {
                    if let Some((old, _rest)) = val.split_once("..") {
                        fp.old_oid = Some(old.trim().to_string());
                    }
                }
                i += 1;
            }

            if i < lines.len()
                && lines[i]
                    .strip_suffix('\r')
                    .unwrap_or(lines[i])
                    .starts_with("--- ")
            {
                let old_line = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                let old_p = &old_line["--- ".len()..];
                fp.old_path = Some(old_p.to_string());
                i += 1;
                if i < lines.len()
                    && lines[i]
                        .strip_suffix('\r')
                        .unwrap_or(lines[i])
                        .starts_with("+++ ")
                {
                    let new_line = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                    let new_p = &new_line["+++ ".len()..];
                    fp.new_path = Some(new_p.to_string());
                    i += 1;
                }
            }

            while i < lines.len()
                && lines[i]
                    .strip_suffix('\r')
                    .unwrap_or(lines[i])
                    .starts_with("@@ ")
            {
                let (hunk, next_i) = parse_hunk(&lines, i)?;
                fp.hunks.push(hunk);
                i = next_i;
            }

            patches.push(fp);
        } else {
            i += 1;
        }
    }

    Ok(patches)
}

fn split_diff_git_paths(s: &str) -> Option<(String, String)> {
    // Keep raw paths (with a/ b/ prefix) so -p<n> stripping works correctly.
    if let Some(pos) = s.find(" b/") {
        let a = &s[..pos];
        let b = &s[pos + 1..];
        return Some((a.to_string(), b.to_string()));
    }
    if s.starts_with("a/") {
        if let Some(pos) = s.find(" /dev/null") {
            let a = &s[..pos];
            return Some((a.to_string(), "/dev/null".to_string()));
        }
    }
    if let Some(b) = s.strip_prefix("/dev/null ") {
        return Some(("/dev/null".to_string(), b.to_string()));
    }
    None
}

fn strip_components(path: &str, n: usize) -> String {
    if n == 0 {
        return path.to_string();
    }
    let mut remaining = path;
    for _ in 0..n {
        if let Some(pos) = remaining.find('/') {
            remaining = &remaining[pos + 1..];
        } else {
            return remaining.to_string();
        }
    }
    remaining.to_string()
}

fn parse_hunk(lines: &[&str], start: usize) -> Result<(Hunk, usize)> {
    let header = lines[start].strip_suffix('\r').unwrap_or(lines[start]);
    let (old_start, old_count, new_start, new_count) =
        parse_hunk_header(header).with_context(|| format!("invalid hunk header: {header}"))?;

    let mut hunk = Hunk {
        old_start,
        _old_count: old_count,
        _new_start: new_start,
        _new_count: new_count,
        lines: Vec::new(),
    };

    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("@@ ") || line.starts_with("diff --git ") {
            break;
        }
        if let Some(rest) = line.strip_prefix('+') {
            hunk.lines.push(HunkLine::Add(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix('-') {
            hunk.lines.push(HunkLine::Remove(rest.to_string()));
        } else if line.is_empty() {
            hunk.lines.push(HunkLine::Context(String::new()));
        } else if let Some(rest) = line.strip_prefix(' ') {
            hunk.lines.push(HunkLine::Context(rest.to_string()));
        } else if line.starts_with('\\') {
            hunk.lines.push(HunkLine::NoNewline);
        } else {
            break;
        }
        i += 1;
    }

    Ok((hunk, i))
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize, usize, usize)> {
    let trimmed = line.trim_start_matches('@').trim_start();
    let end = trimmed.find(" @@").unwrap_or(trimmed.len());
    let range_part = &trimmed[..end];

    let parts: Vec<&str> = range_part.split_whitespace().collect();
    if parts.len() < 2 {
        bail!("expected old and new range in hunk header");
    }

    let (old_start, old_count) = parse_range(parts[0].trim_start_matches('-'))?;
    let (new_start, new_count) = parse_range(parts[1].trim_start_matches('+'))?;

    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(s: &str) -> Result<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        Ok((start_s.parse()?, count_s.parse()?))
    } else {
        let n: usize = s.parse()?;
        Ok((n, 1))
    }
}

fn apply_hunks(old_content: &str, hunks: &[Hunk]) -> Result<String> {
    let has_trailing_newline = old_content.is_empty() || old_content.ends_with('\n');
    let old_lines: Vec<&str> = if old_content.is_empty() {
        Vec::new()
    } else {
        let mut lines: Vec<&str> = old_content.split('\n').collect();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        lines
    };

    let mut result: Vec<String> = Vec::new();
    let mut old_idx: usize = 0;

    for hunk in hunks {
        let hunk_start = if hunk.old_start == 0 {
            0
        } else {
            hunk.old_start - 1
        };

        while old_idx < hunk_start && old_idx < old_lines.len() {
            result.push(old_lines[old_idx].to_string());
            old_idx += 1;
        }

        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    if old_idx >= old_lines.len() {
                        bail!(
                            "context mismatch at line {}: expected {:?}, got EOF",
                            old_idx + 1,
                            s
                        );
                    }
                    if old_lines[old_idx] != s.as_str() {
                        bail!(
                            "context mismatch at line {}: expected {:?}, got {:?}",
                            old_idx + 1,
                            s,
                            old_lines[old_idx]
                        );
                    }
                    old_idx += 1;
                    result.push(s.clone());
                }
                HunkLine::Remove(s) => {
                    if old_idx >= old_lines.len() {
                        bail!(
                            "remove mismatch at line {}: expected {:?}, got EOF",
                            old_idx + 1,
                            s
                        );
                    }
                    if old_lines[old_idx] != s.as_str() {
                        bail!(
                            "remove mismatch at line {}: expected {:?}, got {:?}",
                            old_idx + 1,
                            s,
                            old_lines[old_idx]
                        );
                    }
                    old_idx += 1;
                }
                HunkLine::Add(s) => {
                    result.push(s.clone());
                }
                HunkLine::NoNewline => {}
            }
        }
    }

    while old_idx < old_lines.len() {
        result.push(old_lines[old_idx].to_string());
        old_idx += 1;
    }

    if result.is_empty() {
        return Ok(String::new());
    }

    let ends_no_newline = hunks.last().is_some_and(|h| {
        let mut last_was_add = false;
        let mut saw_no_newline_after_add = false;
        for hl in &h.lines {
            match hl {
                HunkLine::Add(_) => {
                    last_was_add = true;
                    saw_no_newline_after_add = false;
                }
                HunkLine::NoNewline if last_was_add => {
                    saw_no_newline_after_add = true;
                }
                HunkLine::Remove(_) => {
                    last_was_add = false;
                }
                HunkLine::Context(_) => {
                    last_was_add = false;
                    saw_no_newline_after_add = false;
                }
                _ => {}
            }
        }
        saw_no_newline_after_add
    });

    let mut out = result.join("\n");
    if !ends_no_newline && (has_trailing_newline || !hunks.is_empty()) {
        out.push('\n');
    }

    Ok(out)
}

// ── Helpers ─────────────────────────────────────────────────────────

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
    // Respect GIT_COMMITTER_DATE if set
    let timestamp = if let Ok(date) = std::env::var("GIT_COMMITTER_DATE") {
        date
    } else {
        let epoch = now.unix_timestamp();
        let offset = now.offset();
        let hours = offset.whole_hours();
        let minutes = offset.minutes_past_hour().unsigned_abs();
        format!("{epoch} {hours:+03}{minutes:02}")
    };
    format!("{name} <{email}> {timestamp}")
}

fn update_head(git_dir: &Path, head: &HeadState, commit_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
        }
    }
    Ok(())
}

fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<grit_lib::index::IndexEntry>> {
    use grit_lib::objects::parse_tree;
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
            result.push(grit_lib::index::IndexEntry {
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

fn checkout_index_to_worktree(repo: &Repository, work_tree: &Path, index: &Index) -> Result<()> {
    use grit_lib::index::{MODE_EXECUTABLE, MODE_SYMLINK};

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let obj = repo.odb.read(&entry.oid)?;

        if entry.mode == MODE_SYMLINK {
            let target =
                String::from_utf8(obj.data).map_err(|_| anyhow::anyhow!("symlink not UTF-8"))?;
            if abs_path.exists() || abs_path.is_symlink() {
                let _ = fs::remove_file(&abs_path);
            }
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            fs::write(&abs_path, &obj.data)?;
            if entry.mode == MODE_EXECUTABLE {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&abs_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&abs_path, perms)?;
            }
        }
    }

    Ok(())
}
