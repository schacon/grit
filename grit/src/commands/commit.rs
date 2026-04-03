//! `grit commit` — record changes to the repository.
//!
//! Creates a new commit object from the current index state, updates HEAD
//! to point to the new commit, and optionally runs hooks.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::{diff_index_to_tree, diff_index_to_worktree, DiffEntry, DiffStatus};
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::objects::{serialize_commit, CommitData, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use time::OffsetDateTime;
use grit_lib::hooks::{run_hook, HookResult};

/// Arguments for `grit commit`.
#[derive(Debug, ClapArgs)]
#[command(about = "Record changes to the repository")]
pub struct Args {
    /// Use the given message as the commit message.
    #[arg(short = 'm', long = "message")]
    pub message: Vec<String>,

    /// Take the commit message from the given file.
    #[arg(short = 'F', long = "file")]
    pub file: Option<String>,

    /// Commit all changed tracked files (like `git add -u` first).
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Amend the last commit.
    #[arg(long = "amend")]
    pub amend: bool,

    /// Allow an empty commit (no changes).
    #[arg(long = "allow-empty")]
    pub allow_empty: bool,

    /// Allow an empty commit message.
    #[arg(long = "allow-empty-message")]
    pub allow_empty_message: bool,

    /// Show what would be committed without committing.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Suppress commit summary output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Add Signed-off-by trailer.
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// Take the commit message from an existing commit.
    #[arg(long = "reuse-message", value_name = "COMMIT")]
    pub reuse_message: Option<String>,

    /// Override the author.
    #[arg(long = "author")]
    pub author: Option<String>,

    /// Override the date.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Suppress the post-rewrite hook.
    #[arg(long = "no-post-rewrite")]
    pub no_post_rewrite: bool,
}

/// Run the `commit` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_deref();

    // If -a, stage all tracked file changes first
    if args.all {
        if let Some(wt) = work_tree {
            auto_stage_tracked(&repo, wt)?;
        }
    }

    // Load index
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Write tree from index
    let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;

    // Resolve HEAD for parent(s)
    let head = resolve_head(&repo.git_dir)?;
    let mut parents = Vec::new();
    let old_head_oid = head.oid().cloned();

    if args.amend {
        // Amend: use the parent(s) of the current HEAD commit
        if let Some(head_oid) = head.oid() {
            let obj = repo.odb.read(head_oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            parents = commit.parents;
        }
    } else {
        if let Some(head_oid) = head.oid() {
            parents.push(*head_oid);
        }

        // Check for MERGE_HEAD
        let merge_heads = grit_lib::state::read_merge_heads(&repo.git_dir)?;
        parents.extend(merge_heads);
    }

    // Check if tree is the same as parent (empty commit)
    if !args.allow_empty && !parents.is_empty() && !args.amend {
        let parent_obj = repo.odb.read(&parents[0])?;
        let parent_commit = grit_lib::objects::parse_commit(&parent_obj.data)?;
        if parent_commit.tree == tree_oid {
            bail!("nothing to commit, working tree clean");
        }
    }

    // Compute diffs for --dry-run output
    let head_tree = match head.oid() {
        Some(oid) => {
            let obj = repo.odb.read(oid)?;
            let c = grit_lib::objects::parse_commit(&obj.data)?;
            Some(c.tree)
        }
        None => None,
    };
    let staged = diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?;
    let unstaged = if let Some(wt) = work_tree {
        diff_index_to_worktree(&repo.odb, &index, wt)?
    } else {
        Vec::new()
    };
    let untracked = if let Some(wt) = work_tree {
        find_untracked_files(wt, &index)?
    } else {
        Vec::new()
    };

    // --dry-run: show what would be committed and exit
    if args.dry_run {
        print_dry_run(&head, &staged, &unstaged, &untracked)?;
        return Ok(());
    }

    // Run pre-commit hook
    match run_hook(&repo, "pre-commit", &[], None) {
        HookResult::Failed(code) => {
            bail!("pre-commit hook exited with status {code}");
        }
        _ => {}
    }

    // Build commit message
    let mut message = build_message(&args, &repo)?;
    if message.trim().is_empty() && !args.allow_empty_message {
        bail!("Aborting commit due to empty commit message.");
    }

    // Resolve author and committer
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let author = resolve_author(&args, &config, now)?;
    let committer = resolve_committer(&config, now)?;

    // Append Signed-off-by trailer if --signoff
    if args.signoff {
        let trailer = if let Some(angle_end) = committer.find('>') {
            format!("Signed-off-by: {}", &committer[..=angle_end])
        } else {
            format!("Signed-off-by: {committer}")
        };
        if !message.contains(&trailer) {
            let trimmed = message.trim_end();
            message = format!("{trimmed}\n\n{trailer}\n");
        }
    }

    // Run commit-msg hook with temp file containing the message
    {
        let msg_file = repo.git_dir.join("COMMIT_EDITMSG");
        fs::write(&msg_file, &message)?;
        let msg_path_str = msg_file.to_string_lossy().to_string();
        match run_hook(&repo, "commit-msg", &[&msg_path_str], None) {
            HookResult::Failed(code) => {
                bail!("commit-msg hook exited with status {code}");
            }
            HookResult::Success => {
                // Re-read the message in case the hook modified it
                message = fs::read_to_string(&msg_file)?;
            }
            _ => {}
        }
    }

    // Build commit object
    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding: None,
        message,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD
    update_head(&repo.git_dir, &head, &commit_oid)?;

    // Clean up merge state files if present
    cleanup_merge_state(&repo.git_dir);

    // Run post-commit hook (informational, don't abort on failure)
    let _ = run_hook(&repo, "post-commit", &[], None);

    // Run post-rewrite hook for --amend (unless --no-post-rewrite)
    if args.amend && !args.no_post_rewrite {
        if let Some(old_oid) = old_head_oid {
            let stdin_data = format!("{} {}\n", old_oid.to_hex(), commit_oid.to_hex());
            let _ = run_hook(&repo, "post-rewrite", &["amend"], Some(stdin_data.as_bytes()));
        }
    }

    // Output summary
    if !args.quiet {
        let branch = match &head {
            HeadState::Branch { short_name, .. } => short_name.as_str(),
            HeadState::Detached { .. } => "HEAD detached",
            HeadState::Invalid => "unknown",
        };
        let short_oid = &commit_oid.to_hex()[..7];
        let first_line = commit_data.message.lines().next().unwrap_or("");
        if head.is_unborn() {
            eprintln!("[{branch} (root-commit) {short_oid}] {first_line}");
        } else {
            eprintln!("[{branch} {short_oid}] {first_line}");
        }
    }

    Ok(())
}

/// Print dry-run output (like `git commit --dry-run`).
fn print_dry_run(
    head: &HeadState,
    staged: &[DiffEntry],
    unstaged: &[DiffEntry],
    untracked: &[String],
) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match head {
        HeadState::Branch {
            short_name,
            oid: Some(_),
            ..
        } => {
            writeln!(out, "On branch {short_name}")?;
        }
        HeadState::Branch {
            short_name,
            oid: None,
            ..
        } => {
            writeln!(out, "On branch {short_name}")?;
            writeln!(out)?;
            writeln!(out, "No commits yet")?;
        }
        HeadState::Detached { oid } => {
            let short = &oid.to_hex()[..7];
            writeln!(out, "HEAD detached at {short}")?;
        }
        HeadState::Invalid => {
            writeln!(out, "Not currently on any branch.")?;
        }
    }

    if !staged.is_empty() {
        writeln!(out)?;
        writeln!(out, "Changes to be committed:")?;
        writeln!(
            out,
            "  (use \"git restore --staged <file>...\" to unstage)"
        )?;
        for entry in staged {
            let label = status_label_staged(entry.status);
            writeln!(out, "\t{label}:   {}", entry.path())?;
        }
    }

    if !unstaged.is_empty() {
        writeln!(out)?;
        writeln!(out, "Changes not staged for commit:")?;
        writeln!(
            out,
            "  (use \"git add <file>...\" to update what will be committed)"
        )?;
        for entry in unstaged {
            let label = status_label_unstaged(entry.status);
            writeln!(out, "\t{label}:   {}", entry.path())?;
        }
    }

    if !untracked.is_empty() {
        writeln!(out)?;
        writeln!(out, "Untracked files:")?;
        writeln!(
            out,
            "  (use \"git add <file>...\" to include in what will be committed)"
        )?;
        for path in untracked {
            writeln!(out, "\t{path}")?;
        }
    }

    Ok(())
}

fn status_label_staged(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Added => "new file",
        DiffStatus::Deleted => "deleted",
        DiffStatus::Modified => "modified",
        DiffStatus::Renamed => "renamed",
        DiffStatus::TypeChanged => "typechange",
        _ => "changed",
    }
}

fn status_label_unstaged(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Deleted => "deleted",
        DiffStatus::Modified => "modified",
        DiffStatus::TypeChanged => "typechange",
        _ => "changed",
    }
}

/// Find untracked files in the working tree.
fn find_untracked_files(work_tree: &Path, index: &Index) -> Result<Vec<String>> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    let mut untracked = Vec::new();
    walk_untracked(work_tree, work_tree, &tracked, &mut untracked)?;
    untracked.sort();
    Ok(untracked)
}

fn walk_untracked(
    dir: &Path,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    out: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());
    for entry in sorted {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name);
        if path.is_dir() {
            let prefix = format!("{rel}/");
            let has_tracked = tracked.iter().any(|t| t.starts_with(&prefix));
            if has_tracked {
                walk_untracked(&path, work_tree, tracked, out)?;
            } else {
                out.push(format!("{rel}/"));
            }
        } else if !tracked.contains(&rel) {
            out.push(rel);
        }
    }
    Ok(())
}

/// Auto-stage tracked files (for `commit -a`).
fn auto_stage_tracked(repo: &Repository, work_tree: &Path) -> Result<()> {
    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let tracked: Vec<(Vec<u8>, String)> = index
        .entries
        .iter()
        .map(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            (ie.path.clone(), path_str)
        })
        .collect();

    let mut changed = false;
    for (raw_path, path_str) in &tracked {
        let abs_path = work_tree.join(path_str);
        if abs_path.exists() {
            use std::os::unix::fs::MetadataExt;
            let meta = fs::symlink_metadata(&abs_path)?;
            let data = if meta.file_type().is_symlink() {
                let target = fs::read_link(&abs_path)?;
                target.to_string_lossy().into_owned().into_bytes()
            } else {
                fs::read(&abs_path)?
            };
            let oid = repo.odb.write(ObjectKind::Blob, &data)?;
            let mode = grit_lib::index::normalize_mode(meta.mode());
            let entry = grit_lib::index::entry_from_stat(&abs_path, raw_path, oid, mode)?;
            index.add_or_replace(entry);
            changed = true;
        } else {
            index.remove(raw_path);
            changed = true;
        }
    }

    if changed {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

/// Build the commit message from --reuse-message, -m, -F, MERGE_MSG, or editor.
fn build_message(args: &Args, repo: &Repository) -> Result<String> {
    // --reuse-message: take message (and author) from an existing commit
    if let Some(ref rev) = args.reuse_message {
        let oid = resolve_revision(repo, rev)?;
        let obj = repo.odb.read(&oid)?;
        let commit = grit_lib::objects::parse_commit(&obj.data)?;
        return Ok(commit.message);
    }

    // -m flags
    if !args.message.is_empty() {
        let msg = args.message.join("\n\n");
        return Ok(ensure_trailing_newline(&msg));
    }

    // -F file
    if let Some(ref file_path) = args.file {
        let content = if file_path == "-" {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            fs::read_to_string(file_path)?
        };
        return Ok(ensure_trailing_newline(&content));
    }

    // Check for MERGE_MSG
    if let Some(msg) = grit_lib::state::read_merge_msg(&repo.git_dir)? {
        return Ok(ensure_trailing_newline(&msg));
    }

    // If amend, use the previous commit message as default
    if args.amend {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            let obj = repo.odb.read(oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            return Ok(commit.message);
        }
    }

    // TODO: Launch editor
    bail!("no commit message provided (use -m or -F)");
}

/// Resolve the author identity from args, env, and config.
fn resolve_author(args: &Args, config: &ConfigSet, now: OffsetDateTime) -> Result<String> {
    // --reuse-message: reuse the original commit's author
    if let Some(ref rev) = args.reuse_message {
        let repo = Repository::discover(None)?;
        let oid = resolve_revision(&repo, rev)?;
        let obj = repo.odb.read(&oid)?;
        let commit = grit_lib::objects::parse_commit(&obj.data)?;
        return Ok(commit.author);
    }

    if let Some(ref author) = args.author {
        return Ok(author.clone());
    }

    let name = std::env::var("GIT_AUTHOR_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .ok_or_else(|| anyhow::anyhow!(
            "Author identity unknown\n\nPlease tell me who you are.\n\n\
             Run\n\n  git config user.email \"you@example.com\"\n  git config user.name \"Your Name\""
        ))?;

    let email = std::env::var("GIT_AUTHOR_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let date_str = args
        .date
        .as_deref()
        .map(String::from)
        .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok());

    let timestamp = match date_str {
        Some(d) => d,
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Resolve the committer identity from env and config.
fn resolve_committer(config: &ConfigSet, now: OffsetDateTime) -> Result<String> {
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());

    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let date_str = std::env::var("GIT_COMMITTER_DATE").ok();
    let timestamp = match date_str {
        Some(d) => d,
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Format a timestamp in Git's format: `<epoch> <offset>`.
fn format_git_timestamp(dt: OffsetDateTime) -> String {
    let epoch = dt.unix_timestamp();
    let offset = dt.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{epoch} {hours:+03}{minutes:02}")
}

/// Update HEAD to point to the new commit.
fn update_head(git_dir: &Path, head: &HeadState, commit_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            // Update the ref that HEAD points to
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            // Write directly to HEAD
            fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
        }
    }
    Ok(())
}

/// Clean up merge-related state files after a successful commit.
fn cleanup_merge_state(git_dir: &Path) {
    let _ = fs::remove_file(git_dir.join("MERGE_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(git_dir.join("MERGE_MODE"));
    let _ = fs::remove_file(git_dir.join("SQUASH_MSG"));
    let _ = fs::remove_file(git_dir.join("CHERRY_PICK_HEAD"));
    let _ = fs::remove_file(git_dir.join("REVERT_HEAD"));
}

/// Ensure a string ends with a newline.
fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}
