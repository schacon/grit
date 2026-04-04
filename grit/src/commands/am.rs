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
use std::io::{self, Read};
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::error::Error as GritError;
use grit_lib::index::Index;
use grit_lib::objects::{
    parse_commit, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::repo::Repository;
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

    /// Do not apply the patch, just show what would be applied.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Read patches from stdin (default if no files given).
    #[arg(long = "stdin")]
    pub stdin: bool,

    /// Add Signed-off-by trailer.
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// Keep the [PATCH] prefix in the subject.
    #[arg(short = 'k', long = "keep")]
    pub keep: bool,

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
struct AmOptions {
    quiet: bool,
    three_way: bool,
    no_verify: bool,
    signoff: bool,
    committer_date_is_author_date: bool,
    ignore_date: bool,
    message_id: bool,
    empty: String,
    allow_empty: bool,
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
    if args.r#continue {
        return do_continue(args.quiet);
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

fn is_am_in_progress(git_dir: &Path) -> bool {
    let dir = am_dir(git_dir);
    dir.exists() && dir.join("applying").exists()
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
            let mut patches = parse_patches(&content, format_override, keep, keep_non_patch, scissors, no_scissors)?;
            all_patches.append(&mut patches);
        }
    }

    if all_patches.is_empty() {
        eprintln!("Patch format detection failed."); std::process::exit(128);
    }

    if args.dry_run {
        for (i, patch) in all_patches.iter().enumerate() {
            let subject = patch.message.lines().next().unwrap_or("(no subject)");
            println!("Patch {}/{}: {}", i + 1, all_patches.len(), subject);
        }
        return Ok(());
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

    // Write individual patches
    for (i, patch) in all_patches.iter().enumerate() {
        let patch_file = state_dir.join("patches").join((i + 1).to_string());
        let serialized = serialize_mbox_patch(patch);
        fs::write(&patch_file, serialized)?;
    }

    // Apply patches
    let config = ConfigSet::load(Some(git_dir), true)?;
    let three_way = if args.no_three_way {
        false
    } else if args.three_way {
        true
    } else {
        config.get("am.threeWay").or_else(|| config.get("am.threeway")).map(|v| v == "true").unwrap_or(false)
    };
    let message_id = args.message_id || config.get("am.messageid").map(|v| v == "true").unwrap_or(false);
    let opts = AmOptions {
        quiet: args.quiet,
        three_way,
        no_verify: args.no_verify,
        signoff: args.signoff,
        committer_date_is_author_date: args.committer_date_is_author_date,
        ignore_date: args.ignore_date,
        message_id,
        empty: args.empty.unwrap_or_else(|| "stop".to_string()),
        allow_empty: args.allow_empty,
    };
    // Save options to state dir for --continue
    save_am_options(&state_dir, &opts)?;
    apply_remaining(&repo, &opts)?;

    Ok(())
}

fn do_am_stdin(args: Args) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)
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

    let all_patches = parse_patches(&input, args.patch_format.as_deref(), args.keep, args.keep_non_patch, args.scissors, args.no_scissors)?;
    if all_patches.is_empty() {
        eprintln!("Patch format detection failed."); std::process::exit(128);
    }

    if args.dry_run {
        for (i, patch) in all_patches.iter().enumerate() {
            let subject = patch.message.lines().next().unwrap_or("(no subject)");
            println!("Patch {}/{}: {}", i + 1, all_patches.len(), subject);
        }
        return Ok(());
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

    for (i, patch) in all_patches.iter().enumerate() {
        let patch_file = state_dir.join("patches").join((i + 1).to_string());
        let serialized = serialize_mbox_patch(patch);
        fs::write(&patch_file, serialized)?;
    }

    let config = ConfigSet::load(Some(git_dir), true)?;
    let three_way = if args.no_three_way {
        false
    } else if args.three_way {
        true
    } else {
        config.get("am.threeWay").or_else(|| config.get("am.threeway")).map(|v| v == "true").unwrap_or(false)
    };
    let message_id = args.message_id || config.get("am.messageid").map(|v| v == "true").unwrap_or(false);
    let opts = AmOptions {
        quiet: args.quiet,
        three_way,
        no_verify: args.no_verify,
        signoff: args.signoff,
        committer_date_is_author_date: args.committer_date_is_author_date,
        ignore_date: args.ignore_date,
        message_id,
        empty: args.empty.unwrap_or_else(|| "stop".to_string()),
        allow_empty: args.allow_empty,
    };
    save_am_options(&state_dir, &opts)?;
    apply_remaining(&repo, &opts)?;
    Ok(())
}

/// Apply all remaining patches.
fn apply_remaining(repo: &Repository, opts: &AmOptions) -> Result<()> {
    let git_dir = &repo.git_dir;
    let state_dir = am_dir(git_dir);

    let last: usize = fs::read_to_string(state_dir.join("last"))?.trim().parse()?;
    let mut next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;

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

        match apply_one_patch(repo, &patch, opts) {
            Ok(()) => {
                let subject = patch.message.lines().next().unwrap_or("");
                if !opts.quiet {
                    eprintln!("Applying: {}", subject);
                }
                next += 1;
                fs::write(state_dir.join("next"), next.to_string())?;
            }
            Err(e) => {
                let subject = patch.message.lines().next().unwrap_or("");
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
            if index.entries.len() != head_entries.len() ||
                index.entries.iter().zip(head_entries.iter()).any(|(a, b)| a.oid != b.oid || a.path != b.path) {
                bail!("your local changes would be overwritten by am.\n\
                       Please commit your changes or stash them before you apply patches.");
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
                if !run_hook(git_dir, "applypatch-msg", &[msg_path.to_str().unwrap_or("")])? {
                    let _ = fs::remove_file(&msg_path);
                    bail!("applypatch-msg hook rejected the patch");
                }
            }

            // Run pre-applypatch hook
            if !opts.no_verify {
                if !run_hook(git_dir, "pre-applypatch", &[])? {
                    bail!("pre-applypatch hook rejected the patch");
                }
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
        if !run_hook(git_dir, "applypatch-msg", &[msg_path.to_str().unwrap_or("")])? {
            let _ = fs::remove_file(&msg_path);
            bail!("applypatch-msg hook rejected the patch");
        }
    }

    // Try to apply the diff to the working tree
    let apply_result = apply_patch_to_worktree(work_tree, &patch.diff);

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
    if !opts.no_verify {
        if !run_hook(git_dir, "pre-applypatch", &[])? {
            bail!("pre-applypatch hook rejected the patch");
        }
    }

    create_am_commit(repo, &index, patch, opts)?;

    // Run post-applypatch hook (failure doesn't abort)
    if !opts.no_verify {
        let _ = run_hook(git_dir, "post-applypatch", &[]);
    }

    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    Ok(())
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
    let head = resolve_head(git_dir)?;
    let head_oid = head.oid().ok_or_else(|| anyhow::anyhow!("no HEAD for 3-way merge"))?;
    let head_obj = repo.odb.read(head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;

    // Build the "base" tree by finding the common ancestor blobs from index lines
    // Then apply the patch to the base, and merge base->patched with HEAD
    //
    // For each file in the patch, we need:
    // 1. The base version (from the patch's index line pre-image hash)
    // 2. The "ours" version (from HEAD tree)
    // 3. The "theirs" version (base + patch applied)

    let mut any_conflict = false;
    let mut affected_paths = Vec::new();

    for fp in &file_patches {
        let path_str = fp.effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let rel_path = strip_components(path_str, 1);
        let abs_path = work_tree.join(&rel_path);
        affected_paths.push(rel_path.clone());

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

        // Get "ours" from working tree / HEAD
        let ours = if abs_path.exists() {
            fs::read_to_string(&abs_path).unwrap_or_default()
        } else {
            // Try from HEAD tree
            get_blob_from_tree(repo, &head_commit.tree, &rel_path)
                .unwrap_or_default()
        };

        // Try to find the base blob from the index line in the diff
        // The patch's context lines tell us what the pre-image looks like
        // Build the pre-image from hunks
        let base = build_preimage_from_hunks(&ours, &fp.hunks).unwrap_or_else(|_| ours.clone());

        // Apply the patch to the base to get "theirs"
        let theirs = match apply_hunks(&base, &fp.hunks) {
            Ok(t) => t,
            Err(_) => {
                // If we can't even apply to base, that's a real failure
                bail!("Failed to apply patch to {} even in 3-way mode", rel_path);
            }
        };

        // Now do a 3-way merge: base, ours, theirs
        let merged = three_way_merge(&base, &ours, &theirs);
        if merged.has_conflicts {
            any_conflict = true;
        }
        fs::write(&abs_path, &merged.content)?;
    }

    // Stage affected files
    stage_affected_files(repo, &affected_paths)?;

    if any_conflict {
        bail!("3-way merge has conflicts");
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
        let hunk_start = if hunk.old_start == 0 { 0 } else { hunk.old_start - 1 };
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

struct MergeResult {
    content: String,
    has_conflicts: bool,
}

/// Simple line-based 3-way merge.
fn three_way_merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
    let base_lines: Vec<&str> = base.lines().collect();
    let ours_lines: Vec<&str> = ours.lines().collect();
    let theirs_lines: Vec<&str> = theirs.lines().collect();

    let mut result = Vec::new();
    let mut has_conflicts = false;
    let max_len = base_lines.len().max(ours_lines.len()).max(theirs_lines.len());

    for i in 0..max_len {
        let b = base_lines.get(i).copied().unwrap_or("");
        let o = ours_lines.get(i).copied().unwrap_or("");
        let t = theirs_lines.get(i).copied().unwrap_or("");

        if o == t {
            result.push(o.to_string());
        } else if b == o {
            // Only theirs changed
            result.push(t.to_string());
        } else if b == t {
            // Only ours changed
            result.push(o.to_string());
        } else {
            // Both changed differently - conflict
            has_conflicts = true;
            result.push(format!("<<<<<<< HEAD"));
            result.push(o.to_string());
            result.push(format!("======="));
            result.push(t.to_string());
            result.push(format!(">>>>>>> patch"));
        }
    }

    // Handle length differences
    let mut content = result.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }

    MergeResult { content, has_conflicts }
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

/// Apply a unified diff to the working tree files.
/// Returns the list of affected relative paths.
fn apply_patch_to_worktree(work_tree: &Path, diff: &str) -> Result<Vec<String>> {
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
                    let new_rel = fp.new_path.as_deref().map(|p| strip_components(p, 0)).unwrap_or_else(|| rel_path.clone());
                    let new_abs = work_tree.join(&new_rel);
                    if let Some(parent) = new_abs.parent() {
                        if !parent.as_os_str().is_empty() && !parent.exists() {
                            fs::create_dir_all(parent)?;
                        }
                    }
                    let old_content = fs::read_to_string(&old_abs)
                        .with_context(|| format!("cannot read {}", old_abs.display()))?;
                    let new_content = if fp.hunks.is_empty() {
                        old_content
                    } else {
                        apply_hunks(&old_content, &fp.hunks)
                            .with_context(|| format!("failed to apply patch to {}", old_abs.display()))?
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
                fs::remove_file(&path)?;
            }
            continue;
        }

        if fp.is_new {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let content = apply_hunks("", &fp.hunks)?;
            fs::write(&path, content.as_bytes())?;
            #[cfg(unix)]
            if fp.new_mode.as_deref().map_or(false, |m| m == "100755") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            }
            continue;
        }

        // Modify existing file
        let old_content = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;

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
    index.write(&repo.index_path())?;
    Ok(())
}

/// Create a commit from the current index using the patch metadata.
fn create_am_commit(repo: &Repository, index: &Index, patch: &MboxPatch, opts: &AmOptions) -> Result<()> {
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
    let has_trailer_block = lines.last().map_or(false, |l| {
        l.contains(": ") && !l.starts_with(' ') && !l.starts_with('\t')
    });

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
    let has_trailer_block = lines.last().map_or(false, |l| {
        l.contains(": ") && !l.starts_with(' ') && !l.starts_with('\t')
    });

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

    // Reset working tree to HEAD state (undo partial apply)
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let obj = repo.odb.read(head_oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let mut index = Index::new();
        index.entries = entries;
        index.sort();
        index.write(&repo.index_path())?;

        if let Some(wt) = &repo.work_tree {
            checkout_index_to_worktree(&repo, wt, &index)?;
        }
    }

    // Advance past the skipped patch
    fs::write(state_dir.join("next"), (next + 1).to_string())?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    let opts = load_am_options(&state_dir);
    apply_remaining(&repo, &opts)?;

    Ok(())
}

fn do_continue(quiet: bool) -> Result<()> {
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

    let current: usize = fs::read_to_string(state_dir.join("current"))?.trim().parse()?;
    let patch_file = state_dir.join("patches").join(current.to_string());
    let serialized = fs::read_to_string(&patch_file)?;
    let patch = deserialize_mbox_patch(&serialized)?;

    // Read message (might have been edited)
    let message = match fs::read_to_string(git_dir.join("MERGE_MSG")) {
        Ok(m) => m,
        Err(_) => patch.message.clone(),
    };

    let patched = MboxPatch {
        message,
        ..patch
    };

    // Load saved options
    let mut opts = load_am_options(&state_dir);
    opts.quiet = quiet;

    // Record rerere postimage before committing
    let _ = crate::commands::rerere::record_postimage(&repo);

    create_am_commit(&repo, &index, &patched, &opts)?;

    let subject = patched.message.lines().next().unwrap_or("");
    if !quiet {
        eprintln!("Applying: {}", subject);
    }

    // Advance next
    let next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
    fs::write(state_dir.join("next"), (next + 1).to_string())?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    // Continue with remaining
    apply_remaining(&repo, &opts)?;

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

    let state_dir = am_dir(git_dir);
    let orig_head_hex = fs::read_to_string(state_dir.join("orig-head"))?;
    let orig_head_hex = orig_head_hex.trim();

    if !orig_head_hex.is_empty() {
        let orig_oid = ObjectId::from_hex(orig_head_hex)?;

        // Restore to original HEAD
        let obj = repo.odb.read(&orig_oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let mut index = Index::new();
        index.entries = entries;
        index.sort();
        index.write(&repo.index_path())?;

        if let Some(wt) = &repo.work_tree {
            checkout_index_to_worktree(&repo, wt, &index)?;
        }

        // Restore HEAD — use saved head-name to restore branch state
        let head_name = fs::read_to_string(state_dir.join("head-name"))
            .unwrap_or_default();
        let head_name = head_name.trim();
        if let Some(refname) = head_name.strip_prefix("ref: ") {
            // Was on a branch — restore the ref
            fs::write(git_dir.join("HEAD"), format!("{}\n", head_name))?;
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", orig_oid.to_hex()))?;
        } else {
            // Was detached
            fs::write(git_dir.join("HEAD"), format!("{}\n", orig_oid.to_hex()))?;
        }
    }

    cleanup_am_state(git_dir);
    eprintln!("am session aborted.");

    Ok(())
}

// ── Save/Load options ───────────────────────────────────────────────

fn save_am_options(state_dir: &Path, opts: &AmOptions) -> Result<()> {
    let mut out = String::new();
    if opts.three_way { out.push_str("threeway\n"); }
    if opts.no_verify { out.push_str("no-verify\n"); }
    if opts.signoff { out.push_str("signoff\n"); }
    if opts.quiet { out.push_str("quiet\n"); }
    if opts.message_id { out.push_str("message-id\n"); }
    if opts.allow_empty { out.push_str("allow-empty\n"); }
    if opts.ignore_date { out.push_str("ignore-date\n"); }
    out.push_str(&format!("empty={}\n", opts.empty));
    fs::write(state_dir.join("options"), out)?;
    Ok(())
}

fn load_am_options(state_dir: &Path) -> AmOptions {
    let content = fs::read_to_string(state_dir.join("options")).unwrap_or_default();
    let mut opts = AmOptions {
        quiet: false,
        three_way: false,
        no_verify: false,
        signoff: false,
        committer_date_is_author_date: false,
        ignore_date: false,
        message_id: false,
        empty: "stop".to_string(),
        allow_empty: false,
    };
    for line in content.lines() {
        match line.trim() {
            "threeway" => opts.three_way = true,
            "no-verify" => opts.no_verify = true,
            "signoff" => opts.signoff = true,
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
    cmd.args(args)
        .env("GIT_DIR", git_dir)
        .current_dir(work_dir);

    let status = cmd.status()
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
                    if !first.starts_with("From ") && !first.starts_with("From:") &&
                       !first.starts_with("Subject:") && !first.starts_with("Date:") &&
                       !first.starts_with("Message-ID:") && !first.starts_with("X-") {
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
                if let (Ok(epoch), Ok(offset_secs)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
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
        (body[..idx].to_string(), body[idx+1..].trim().to_string())
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
fn parse_patches(input: &str, format: Option<&str>, keep: bool, keep_non_patch: bool, scissors: bool, no_scissors: bool) -> Result<Vec<MboxPatch>> {
    let fmt = format.unwrap_or_else(|| detect_patch_format(input));
    match fmt {
        "stgit" => parse_stgit_patch(input),
        "hg" => parse_hg_patch(input),
        _ => parse_mbox_with_opts(input, keep, keep_non_patch, scissors, no_scissors),
    }
}

// ── Mbox parsing ────────────────────────────────────────────────────

/// Unquote mboxrd format: lines starting with >From (or >>From, etc.) are unquoted.
/// In mboxrd, "From " lines inside messages are escaped by prepending ">".
fn unquote_mboxrd(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_body = false;

    for line in input.lines() {
        if line.starts_with("From ") && line.len() > 5 {
            // mbox separator - reset state
            in_body = false;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if !in_body {
            if line.is_empty() {
                in_body = true;
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // In body: unquote >From lines
        if line.starts_with(">From ") || line.starts_with(">>") && line.contains("From ") {
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

/// Parse an mbox file into individual patches with options.
fn parse_mbox_with_opts(input: &str, keep: bool, keep_non_patch: bool, scissors: bool, no_scissors: bool) -> Result<Vec<MboxPatch>> {
    // Handle mboxrd: unquote >From lines
    let input = unquote_mboxrd(input);
    let mut patches = Vec::new();
    let mut lines = input.lines().peekable();

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
            if line.starts_with("From ") && line.len() > 5 {
                found_from = true;
                lines.next(); // consume "From " line
                break;
            }
            // If we haven't found any "From " line yet and we see headers, treat as raw patch
            if !found_from && (line.starts_with("From:") || line.starts_with("Subject:") || line.starts_with("Date:") || line.starts_with("Message-ID:") || line.starts_with("Message-Id:") || line.starts_with("X-")) {
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

        while let Some(&line) = lines.peek() {
            if line.is_empty() {
                lines.next();
                _in_headers = false;
                break;
            }
            // Continuation line (starts with whitespace)
            if (line.starts_with(' ') || line.starts_with('\t')) && !last_header.is_empty() {
                if last_header == "subject" {
                    subject.push(' ');
                    subject.push_str(line.trim());
                }
                lines.next();
                continue;
            }

            if let Some(value) = line.strip_prefix("From: ") {
                author = value.trim().to_string();
                last_header = "from".to_string();
            } else if let Some(value) = line.strip_prefix("Date: ") {
                date = value.trim().to_string();
                last_header = "date".to_string();
            } else if let Some(value) = line.strip_prefix("Subject: ") {
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
            } else if let Some(value) = line.strip_prefix("Message-ID: ")
                .or_else(|| line.strip_prefix("Message-Id: "))
                .or_else(|| line.strip_prefix("Message-id: ")) {
                message_id = value.trim().to_string();
                last_header = "message-id".to_string();
            } else {
                last_header = String::new();
            }
            lines.next();
        }

        // Parse body (everything until "---" separator or diff start)
        let mut in_diff = false;
        let mut body_lines = Vec::new();
        let mut diff_lines = Vec::new();

        while let Some(&line) = lines.peek() {
            // Check for next mbox message
            if line.starts_with("From ") && line.len() > 5 && !diff_lines.is_empty() {
                break;
            }

            if !in_diff {
                if line == "---" {
                    // Separator between message body and diffstat/diff
                    lines.next();
                    // Now skip diffstat lines until we hit "diff --git"
                    while let Some(&l) = lines.peek() {
                        if l.starts_with("diff --git ") {
                            in_diff = true;
                            break;
                        }
                        if l.starts_with("From ") && l.len() > 5 {
                            break;
                        }
                        lines.next();
                    }
                    continue;
                }
                if line.starts_with("diff --git ") {
                    in_diff = true;
                    // Don't consume — fall through to diff section
                } else {
                    body_lines.push(line);
                    lines.next();
                    continue;
                }
            }

            if in_diff {
                // Collect diff lines until "-- " (signature separator) or next message
                if line == "-- " || line == "-- \n" {
                    lines.next();
                    // Skip remaining signature lines
                    while let Some(&l) = lines.peek() {
                        if l.starts_with("From ") && l.len() > 5 {
                            break;
                        }
                        lines.next();
                    }
                    break;
                }
                if line.starts_with("From ") && line.len() > 5 {
                    break;
                }
                diff_lines.push(line);
                lines.next();
            }
        }

        // Build message from subject + body
        let mut body_str = body_lines.join("\n").trim().to_string();

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

        let mut diff_section = diff_lines.join("\n");
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
    if parts.len() == 2 {
        if parts[0].parse::<i64>().is_ok() {
            return date.to_string();
        }
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
        "jan" => 1u32, "feb" => 2, "mar" => 3, "apr" => 4,
        "may" => 5, "jun" => 6, "jul" => 7, "aug" => 8,
        "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => return None,
    };
    let year: i32 = tokens[2].parse().ok()?;
    let time_parts: Vec<&str> = tokens[3].split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = if time_parts.len() > 2 { time_parts[2].parse().ok()? } else { 0 };

    // Convert to Unix timestamp
    // Days from year 0 to year, then month/day, then subtract Unix epoch
    let epoch = datetime_to_epoch(year, month, day, hour, min, sec, tz_offset_secs)?;

    Some(format!("{} {}", epoch, tz_str))
}

/// Convert a date to Unix epoch seconds.
fn datetime_to_epoch(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32, tz_offset_secs: i32) -> Option<i64> {
    // Use a simple calculation
    let m = if month <= 2 { month + 12 } else { month };
    let y = if month <= 2 { year - 1 } else { year };

    // Julian Day Number
    let jdn = (day as i64) + (153 * (m as i64 - 3) + 2) / 5
        + 365 * (y as i64) + (y as i64) / 4 - (y as i64) / 100 + (y as i64) / 400 + 1721119;

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

    let mut lines = data.lines();
    for line in &mut lines {
        if line.is_empty() {
            break;
        }
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

    // Remaining content is message + diff
    let remaining: String = lines.collect::<Vec<&str>>().join("\n");
    // Add back the newline that .lines() stripped
    let remaining = if data.ends_with('\n') && !remaining.ends_with('\n') {
        format!("{remaining}\n")
    } else {
        remaining
    };

    let message = if msg_len > 0 && msg_len <= remaining.len() {
        remaining[..msg_len].to_string()
    } else {
        remaining.clone()
    };

    let diff = if diff_len > 0 && msg_len + diff_len <= remaining.len() {
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
    is_new: bool,
    is_deleted: bool,
    is_rename: bool,
    hunks: Vec<Hunk>,
}

impl FilePatch {
    fn effective_path(&self) -> Option<&str> {
        if self.is_deleted {
            return self.old_path.as_deref().filter(|p| *p != "/dev/null")
                .or(self.new_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        if self.is_new {
            return self.new_path.as_deref().filter(|p| *p != "/dev/null")
                .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        self.new_path.as_deref().filter(|p| *p != "/dev/null")
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
    let lines: Vec<&str> = input.lines().collect();
    let mut patches = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("diff --git ") {
            let mut fp = FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_deleted: false,
                is_rename: false,
                hunks: Vec::new(),
            };

            let rest = &lines[i]["diff --git ".len()..];
            if let Some((a, b)) = split_diff_git_paths(rest) {
                fp.old_path = Some(a);
                fp.new_path = Some(b);
            }
            i += 1;

            while i < lines.len()
                && !lines[i].starts_with("--- ")
                && !lines[i].starts_with("diff --git ")
                && !lines[i].starts_with("@@ ")
            {
                let line = lines[i];
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
                }
                i += 1;
            }

            if i < lines.len() && lines[i].starts_with("--- ") {
                let old_p = &lines[i]["--- ".len()..];
                fp.old_path = Some(old_p.to_string());
                i += 1;
                if i < lines.len() && lines[i].starts_with("+++ ") {
                    let new_p = &lines[i]["+++ ".len()..];
                    fp.new_path = Some(new_p.to_string());
                    i += 1;
                }
            }

            while i < lines.len() && lines[i].starts_with("@@ ") {
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
    let header = lines[start];
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)
        .with_context(|| format!("invalid hunk header: {header}"))?;

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
        old_content.lines().collect()
    };

    let mut result: Vec<String> = Vec::new();
    let mut old_idx: usize = 0;

    for hunk in hunks {
        let hunk_start = if hunk.old_start == 0 { 0 } else { hunk.old_start - 1 };

        while old_idx < hunk_start && old_idx < old_lines.len() {
            result.push(old_lines[old_idx].to_string());
            old_idx += 1;
        }

        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    if old_idx < old_lines.len() {
                        if old_lines[old_idx] != s.as_str() {
                            bail!(
                                "context mismatch at line {}: expected {:?}, got {:?}",
                                old_idx + 1, s, old_lines[old_idx]
                            );
                        }
                        old_idx += 1;
                    }
                    result.push(s.clone());
                }
                HunkLine::Remove(s) => {
                    if old_idx < old_lines.len() {
                        if old_lines[old_idx] != s.as_str() {
                            bail!(
                                "remove mismatch at line {}: expected {:?}, got {:?}",
                                old_idx + 1, s, old_lines[old_idx]
                            );
                        }
                        old_idx += 1;
                    }
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

    let ends_no_newline = hunks.last().map_or(false, |h| {
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
    let index_path = repo.index_path();
    match Index::load(&index_path) {
        Ok(idx) => Ok(idx),
        Err(GritError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::new()),
        Err(e) => Err(e.into()),
    }
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

fn checkout_index_to_worktree(
    repo: &Repository,
    work_tree: &Path,
    index: &Index,
) -> Result<()> {
    use grit_lib::index::{MODE_EXECUTABLE, MODE_SYMLINK};

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let obj = repo.odb.read(&entry.oid)?;

        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink not UTF-8"))?;
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
