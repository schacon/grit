//! `grit commit` — record changes to the repository.
//!
//! Creates a new commit object from the current index state, updates HEAD
//! to point to the new commit, and optionally runs hooks.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::{diff_index_to_tree, diff_index_to_worktree, DiffEntry, DiffStatus};
use grit_lib::error::Error;
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::index::Index;
use grit_lib::objects::{serialize_commit, CommitData, ObjectId, ObjectKind};
use grit_lib::refs::append_reflog;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

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
    #[arg(short = 'C', long = "reuse-message", value_name = "COMMIT")]
    pub reuse_message: Option<String>,

    /// Like -C, but open editor to modify the message.
    #[arg(short = 'c', long = "reedit-message", value_name = "COMMIT")]
    pub reedit_message: Option<String>,

    /// Override the author.
    #[arg(long = "author")]
    pub author: Option<String>,

    /// Override the date.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Suppress the post-rewrite hook.
    #[arg(long = "no-post-rewrite")]
    pub no_post_rewrite: bool,

    /// Give output in short format (for dry-run).
    #[arg(long = "short")]
    pub short: bool,

    /// Give output in porcelain format (for dry-run).
    #[arg(long = "porcelain")]
    pub porcelain: bool,

    /// Give output in long format (default for dry-run).
    #[arg(long = "long")]
    pub long: bool,

    /// Include staged changes when given pathspec (with -i).
    #[arg(short = 'i', long = "include")]
    pub include: bool,

    /// Only commit specified paths (with -o or --only).
    #[arg(short = 'o', long = "only")]
    pub only: bool,

    /// Interactively add changes.
    #[arg(long = "interactive")]
    pub interactive: bool,

    /// Untracked files mode.
    #[arg(short = 'u', long = "untracked-files", value_name = "MODE", num_args = 0..=1, default_missing_value = "all")]
    pub untracked_files: Option<String>,

    /// Verbose - show diff in commit message editor.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress verbose output.
    #[arg(long = "no-verbose")]
    pub no_verbose: bool,

    /// Override cleanup mode.
    #[arg(long = "cleanup", value_name = "MODE")]
    pub cleanup: Option<String>,

    /// Use specified template file.
    #[arg(short = 't', long = "template", value_name = "FILE")]
    pub template: Option<String>,

    /// Edit the commit message (used with -C).
    #[arg(short = 'e', long = "edit")]
    pub edit: bool,

    /// Suppress editing the commit message.
    #[arg(long = "no-edit")]
    pub no_edit: bool,

    /// Set the commit status (accepted but not used).
    #[arg(long = "status")]
    pub status: bool,

    /// Suppress commit status in editor template.
    #[arg(long = "no-status")]
    pub no_status: bool,

    /// Add a Signed-off-by trailer with specific value.
    #[arg(long = "trailer", value_name = "TOKEN:VALUE")]
    pub trailer: Vec<String>,

    /// Override gpg sign.
    #[arg(short = 'S', long = "gpg-sign", value_name = "KEYID", num_args = 0..=1, default_missing_value = "")]
    pub gpg_sign: Option<String>,

    /// Don't sign the commit.
    #[arg(long = "no-gpg-sign")]
    pub no_gpg_sign: bool,

    /// Don't verify the commit message.
    #[arg(long = "no-verify", short = 'n')]
    pub no_verify: bool,

    /// Fixup commit.
    #[arg(long = "fixup", value_name = "COMMIT")]
    pub fixup: Option<String>,

    /// Squash commit.
    #[arg(long = "squash", value_name = "COMMIT")]
    pub squash: Option<String>,

    /// Reset author.
    #[arg(long = "reset-author")]
    pub reset_author: bool,

    /// Pathspec — files to include in the commit (stages them first).
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub pathspec: Vec<String>,
}

/// Run the `commit` command.
pub fn run(args: Args) -> Result<()> {
    // Validate conflicting options
    let msg_source_count = [
        !args.message.is_empty(),
        args.file.is_some(),
        args.reuse_message.is_some(),
        args.reedit_message.is_some(),
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    if msg_source_count > 1 {
        bail!("Only one of -m, -F, -C, -c can be used.");
    }

    // -a and explicit pathspec don't mix
    if args.all && !args.pathspec.is_empty() {
        bail!(
            "paths '{}' with -a does not make sense",
            args.pathspec.join(" ")
        );
    }

    // --include and --only don't mix
    if args.include && args.only {
        bail!("--include and --only are mutually exclusive");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo.work_tree.as_deref();

    // If -a, stage all tracked file changes first
    if args.all {
        if let Some(wt) = work_tree {
            auto_stage_tracked(&repo, wt)?;
        }
    }

    // If pathspec given, stage those specific files first
    if !args.pathspec.is_empty() {
        if let Some(wt) = work_tree {
            stage_pathspec_files(&repo, wt, &args.pathspec)?;
        }
    }

    // Load index
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Write tree from index
    let tree_oid = match write_tree_from_index(&repo.odb, &index, "") {
        Ok(oid) => oid,
        Err(err) => {
            if is_permission_denied_error(&err) {
                eprintln!(
                    "error: insufficient permission for adding an object to repository database .git/objects"
                );
                eprintln!("error: Error building trees");
                std::process::exit(128);
            }
            return Err(err.into());
        }
    };

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
    if let HookResult::Failed(code) = run_hook(&repo, "pre-commit", &[], None) {
        bail!("pre-commit hook exited with status {code}");
    }

    // Build commit message
    let msg_result = build_message(&args, &repo)?;
    let mut message = msg_result.message;
    let mut raw_message = msg_result.raw_bytes;
    if message.trim().is_empty() && !args.allow_empty_message {
        bail!("Aborting commit due to empty commit message.");
    }

    // Resolve author and committer
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    // Check i18n.commitEncoding for non-UTF-8 commit messages
    let commit_encoding = config
        .get("i18n.commitEncoding")
        .or_else(|| config.get("i18n.commitencoding"));
    let now = OffsetDateTime::now_utc();

    // When amending, preserve original author unless explicitly overridden
    let amend_author = if args.amend
        && args.author.is_none()
        && args.reuse_message.is_none()
        && args.reedit_message.is_none()
        && args.date.is_none()
    {
        if let Some(head_oid) = head.oid() {
            let obj = repo.odb.read(head_oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            Some(commit.author)
        } else {
            None
        }
    } else {
        None
    };
    let author = if let Some(preserved) = amend_author {
        preserved
    } else {
        resolve_author(&args, &config, now)?
    };
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
            // Also update raw_message if present
            if let Some(ref raw) = raw_message {
                let trimmed_raw = {
                    let mut end = raw.len();
                    while end > 0
                        && (raw[end - 1] == b'\n' || raw[end - 1] == b' ' || raw[end - 1] == b'\r')
                    {
                        end -= 1;
                    }
                    &raw[..end]
                };
                let mut new_raw = trimmed_raw.to_vec();
                new_raw.extend_from_slice(format!("\n\n{trailer}\n").as_bytes());
                raw_message = Some(new_raw);
            }
        }
    }

    // Run commit-msg hook with temp file containing the message
    {
        let msg_file = repo.git_dir.join("COMMIT_EDITMSG");
        // Write raw bytes when available so the hook sees the original encoding
        if let Some(ref raw) = raw_message {
            fs::write(&msg_file, raw)?;
        } else {
            fs::write(&msg_file, &message)?;
        }
        let msg_path_str = msg_file.to_string_lossy().to_string();
        match run_hook(&repo, "commit-msg", &[&msg_path_str], None) {
            HookResult::Failed(code) => {
                bail!("commit-msg hook exited with status {code}");
            }
            HookResult::Success => {
                // Re-read the message in case the hook modified it
                let new_raw = fs::read(&msg_file)?;
                match String::from_utf8(new_raw.clone()) {
                    Ok(s) => {
                        message = s;
                        raw_message = None;
                    }
                    Err(_) => {
                        message = String::from_utf8_lossy(&new_raw).to_string();
                        raw_message = Some(new_raw);
                    }
                }
            }
            _ => {}
        }
    }

    // Build commit object — set encoding header when i18n.commitEncoding is configured
    // and differs from UTF-8.
    let encoding = match &commit_encoding {
        Some(enc) if !enc.eq_ignore_ascii_case("utf-8") && !enc.eq_ignore_ascii_case("utf8") => {
            Some(enc.clone())
        }
        _ => None,
    };
    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding,
        message,
        raw_message,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    update_head(&repo.git_dir, &head, &commit_oid)?;

    // Write reflog entries
    {
        let msg = if head.is_unborn() {
            format!(
                "commit (initial): {}",
                commit_data.message.lines().next().unwrap_or("")
            )
        } else if args.amend {
            format!(
                "commit (amend): {}",
                commit_data.message.lines().next().unwrap_or("")
            )
        } else {
            format!(
                "commit: {}",
                commit_data.message.lines().next().unwrap_or("")
            )
        };
        // Write to HEAD reflog
        let _ = append_reflog(
            &repo.git_dir,
            "HEAD",
            &old_oid,
            &commit_oid,
            &commit_data.committer,
            &msg,
        );
        // Write to branch reflog if on a branch
        if let HeadState::Branch { refname, .. } = &head {
            let _ = append_reflog(
                &repo.git_dir,
                refname,
                &old_oid,
                &commit_oid,
                &commit_data.committer,
                &msg,
            );
        }
    }

    // Clean up merge state files if present
    cleanup_merge_state(&repo.git_dir);

    // Run post-commit hook (informational, don't abort on failure)
    let _ = run_hook(&repo, "post-commit", &[], None);

    // Run post-rewrite hook for --amend (unless --no-post-rewrite)
    if args.amend && !args.no_post_rewrite {
        if let Some(old_oid) = old_head_oid {
            let stdin_data = format!("{} {}\n", old_oid.to_hex(), commit_oid.to_hex());
            let _ = run_hook(
                &repo,
                "post-rewrite",
                &["amend"],
                Some(stdin_data.as_bytes()),
            );
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
            println!("[{branch} (root-commit) {short_oid}] {first_line}");
        } else {
            println!("[{branch} {short_oid}] {first_line}");
        }

        // Print diff stat summary line
        let parent_tree = if commit_data.parents.is_empty() {
            None
        } else {
            let parent_obj = repo.odb.read(&commit_data.parents[0])?;
            let parent_commit = grit_lib::objects::parse_commit(&parent_obj.data)?;
            Some(parent_commit.tree)
        };
        if let Ok(diff_entries) =
            grit_lib::diff::diff_trees(&repo.odb, parent_tree.as_ref(), Some(&commit_data.tree), "")
        {
            let zero_oid = ObjectId::from_bytes(&[0u8; 20]).unwrap();
            let mut total_files = 0usize;
            let mut total_ins = 0usize;
            let mut total_del = 0usize;
            for entry in &diff_entries {
                total_files += 1;
                let old_content = if entry.old_oid == zero_oid {
                    String::new()
                } else {
                    repo.odb
                        .read(&entry.old_oid)
                        .map(|o| String::from_utf8_lossy(&o.data).into_owned())
                        .unwrap_or_default()
                };
                let new_content = if entry.new_oid == zero_oid {
                    String::new()
                } else {
                    repo.odb
                        .read(&entry.new_oid)
                        .map(|o| String::from_utf8_lossy(&o.data).into_owned())
                        .unwrap_or_default()
                };
                let (a, d) = grit_lib::diff::count_changes(&old_content, &new_content);
                total_ins += a;
                total_del += d;
            }
            if total_files > 0 {
                let mut summary = format!(
                    " {} file{} changed",
                    total_files,
                    if total_files == 1 { "" } else { "s" }
                );
                if total_ins > 0 {
                    summary.push_str(&format!(
                        ", {} insertion{}(+)",
                        total_ins,
                        if total_ins == 1 { "" } else { "s" }
                    ));
                }
                if total_del > 0 {
                    summary.push_str(&format!(
                        ", {} deletion{}(-)",
                        total_del,
                        if total_del == 1 { "" } else { "s" }
                    ));
                }
                println!("{summary}");
            }
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
        writeln!(out, "  (use \"git restore --staged <file>...\" to unstage)")?;
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

/// Stage specific files given as pathspec arguments to `commit`.
fn stage_pathspec_files(repo: &Repository, work_tree: &Path, pathspecs: &[String]) -> Result<()> {
    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Resolve pathspecs relative to the current directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| work_tree.to_path_buf());
    // Canonicalize work_tree to resolve symlinks for proper prefix stripping
    let canon_work_tree = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.to_path_buf());

    for spec in pathspecs {
        // Resolve the pathspec relative to cwd
        let abs_path = if std::path::Path::new(spec).is_absolute() {
            PathBuf::from(spec)
        } else {
            cwd.join(spec)
        };
        // Canonicalize to resolve symlinks, then compute relative path to work_tree
        let canon_path = abs_path.canonicalize().unwrap_or(abs_path.clone());
        let rel_path = canon_path
            .strip_prefix(&canon_work_tree)
            .unwrap_or_else(|_| {
                // Fallback: try stripping non-canonical work_tree
                abs_path
                    .strip_prefix(work_tree)
                    .unwrap_or(std::path::Path::new(spec))
            });
        let rel_str = rel_path.to_string_lossy().to_string();
        if canon_path.exists() {
            use std::os::unix::fs::MetadataExt;
            let meta = fs::symlink_metadata(&canon_path)?;
            let data = if meta.file_type().is_symlink() {
                let target = fs::read_link(&canon_path)?;
                target.to_string_lossy().into_owned().into_bytes()
            } else {
                fs::read(&canon_path)?
            };
            let oid = repo.odb.write(ObjectKind::Blob, &data)?;
            let mode = grit_lib::index::normalize_mode(meta.mode());
            let raw_path = rel_str.as_bytes().to_vec();
            let entry = grit_lib::index::entry_from_stat(&canon_path, &raw_path, oid, mode)?;
            index.add_or_replace(entry);
        } else {
            // File deleted — remove from index
            index.remove(rel_str.as_bytes());
        }
    }

    index.write(&repo.index_path())?;
    Ok(())
}

/// Auto-stage tracked files (for `commit -a`).
fn auto_stage_tracked(repo: &Repository, work_tree: &Path) -> Result<()> {
    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let tracked: Vec<(Vec<u8>, String, u32)> = index
        .entries
        .iter()
        .map(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            (ie.path.clone(), path_str, ie.mode)
        })
        .collect();

    let mut changed = false;
    for (raw_path, path_str, idx_mode) in &tracked {
        let abs_path = work_tree.join(path_str);
        if abs_path.exists() {
            // Gitlink (submodule) entries: read the embedded repo's HEAD to
            // get the current commit OID instead of trying to read the
            // directory as a file.
            if *idx_mode == 0o160000 {
                let head_path = abs_path.join(".git/HEAD");
                if let Ok(head_content) = fs::read_to_string(&head_path) {
                    let head_trimmed = head_content.trim();
                    let oid_hex = if let Some(r) = head_trimmed.strip_prefix("ref: ") {
                        let ref_path = abs_path.join(".git").join(r);
                        match fs::read_to_string(&ref_path) {
                            Ok(s) => s.trim().to_string(),
                            Err(_) => continue,
                        }
                    } else {
                        head_trimmed.to_string()
                    };
                    if let Ok(oid) = oid_hex.parse::<ObjectId>() {
                        use std::os::unix::fs::MetadataExt;
                        let meta = fs::symlink_metadata(&abs_path)?;
                        let entry = grit_lib::index::IndexEntry {
                            ctime_sec: meta.ctime() as u32,
                            ctime_nsec: meta.ctime_nsec() as u32,
                            mtime_sec: meta.mtime() as u32,
                            mtime_nsec: meta.mtime_nsec() as u32,
                            dev: meta.dev() as u32,
                            ino: meta.ino() as u32,
                            mode: 0o160000,
                            uid: meta.uid(),
                            gid: meta.gid(),
                            size: 0,
                            oid,
                            flags: path_str.len().min(0xFFF) as u16,
                            flags_extended: None,
                            path: raw_path.clone(),
                        };
                        index.add_or_replace(entry);
                        changed = true;
                    }
                }
                continue;
            }
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

/// Result of building a commit message — may be UTF-8 or raw bytes.
struct MessageResult {
    /// UTF-8 message (always set; lossy if raw_bytes is Some).
    message: String,
    /// Raw bytes when the message is not valid UTF-8.
    raw_bytes: Option<Vec<u8>>,
}

/// Build the commit message from --reuse-message, -m, -F, MERGE_MSG, or editor.
fn build_message(args: &Args, repo: &Repository) -> Result<MessageResult> {
    // --reuse-message / --reedit-message: take message (and author) from an existing commit
    let reuse_rev = args.reuse_message.as_ref().or(args.reedit_message.as_ref());
    if let Some(rev) = reuse_rev {
        let oid = resolve_revision(repo, rev)?;
        let obj = repo.odb.read(&oid)?;
        let commit = grit_lib::objects::parse_commit(&obj.data)?;
        return Ok(MessageResult {
            message: commit.message,
            raw_bytes: None,
        });
    }

    // -m flags
    if !args.message.is_empty() {
        let msg = args.message.join("\n\n");
        return Ok(MessageResult {
            message: ensure_trailing_newline(&msg),
            raw_bytes: None,
        });
    }

    // -F file
    if let Some(ref file_path) = args.file {
        let raw = if file_path == "-" {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            buf
        } else {
            fs::read(file_path)?
        };
        match String::from_utf8(raw.clone()) {
            Ok(s) => {
                return Ok(MessageResult {
                    message: ensure_trailing_newline(&s),
                    raw_bytes: None,
                });
            }
            Err(_) => {
                // Non-UTF-8 message — store raw bytes.
                let lossy = String::from_utf8_lossy(&raw).to_string();
                let mut raw_nl = raw;
                if !raw_nl.ends_with(b"\n") {
                    raw_nl.push(b'\n');
                }
                return Ok(MessageResult {
                    message: ensure_trailing_newline(&lossy),
                    raw_bytes: Some(raw_nl),
                });
            }
        }
    }

    // Check for MERGE_MSG
    if let Some(msg) = grit_lib::state::read_merge_msg(&repo.git_dir)? {
        return Ok(MessageResult {
            message: ensure_trailing_newline(&msg),
            raw_bytes: None,
        });
    }

    // Check for SQUASH_MSG
    let squash_msg_path = repo.git_dir.join("SQUASH_MSG");
    if let Ok(msg) = std::fs::read_to_string(&squash_msg_path) {
        if !msg.is_empty() {
            return Ok(MessageResult {
                message: ensure_trailing_newline(&msg),
                raw_bytes: None,
            });
        }
    }

    // If amend, use the previous commit message as default
    if args.amend {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            let obj = repo.odb.read(oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            return Ok(MessageResult {
                message: commit.message,
                raw_bytes: None,
            });
        }
    }

    // If --allow-empty-message, return empty message
    if args.allow_empty_message {
        return Ok(MessageResult {
            message: String::new(),
            raw_bytes: None,
        });
    }

    // TODO: Launch editor
    bail!("no commit message provided (use -m or -F)");
}

/// Resolve the author identity from args, env, and config.

/// Check if an ident name is valid (not empty and not all special characters).
fn validate_ident_name(name: &str, kind: &str) -> Result<()> {
    let cleaned: String = name
        .chars()
        .filter(|&c| {
            c != '.'
                && c != ','
                && c != ';'
                && c != '<'
                && c != '>'
                && c != '\''
                && c != '"'
                && c != ' '
        })
        .collect();
    if cleaned.is_empty() {
        if name.is_empty() {
            bail!("empty ident name (for <{}>) not allowed", kind);
        } else {
            bail!("invalid ident name: '{}'", name);
        }
    }
    Ok(())
}

fn resolve_author(args: &Args, config: &ConfigSet, now: OffsetDateTime) -> Result<String> {
    // --reuse-message / --reedit-message: reuse the original commit's author
    let reuse_rev = args.reuse_message.as_ref().or(args.reedit_message.as_ref());
    if let Some(rev) = reuse_rev {
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
        .or_else(|| config.get("user.name"));
    let name = match name {
        Some(n) if n.is_empty() => {
            eprintln!("Author identity unknown");
            bail!("empty ident name (for <author>) not allowed");
        }
        Some(n) => n,
        None => {
            eprintln!("Author identity unknown");
            bail!("Author identity unknown\n\nPlease tell me who you are.\n\n\
                 Run\n\n  git config user.email \"you@example.com\"\n  git config user.name \"Your Name\"");
        }
    };
    validate_ident_name(&name, "author")?;

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
        Some(d) => parse_date_to_git_timestamp(&d).unwrap_or(d),
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Resolve the committer identity from env and config.
fn resolve_committer(config: &ConfigSet, now: OffsetDateTime) -> Result<String> {
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"));
    let name = match name {
        Some(n) if n.is_empty() => {
            eprintln!("Committer identity unknown");
            bail!("empty ident name (for <committer>) not allowed");
        }
        Some(n) => n,
        None => "Unknown".to_owned(),
    };
    validate_ident_name(&name, "committer")?;

    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let date_str = std::env::var("GIT_COMMITTER_DATE").ok();
    let timestamp = match date_str {
        Some(d) => parse_date_to_git_timestamp(&d).unwrap_or(d),
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Parse a date string (like "2006-06-26 00:04:00 +0000") into git's
/// `<epoch> <offset>` format. Returns None if already in epoch format.
pub fn parse_date_to_git_timestamp(date_str: &str) -> Option<String> {
    let trimmed = date_str.trim();

    // Already in `<epoch> <offset>` format? (epoch is all digits)
    let parts: Vec<&str> = trimmed.rsplitn(2, ' ').collect();
    if parts.len() == 2 {
        let maybe_epoch = parts[1];
        if maybe_epoch.chars().all(|c| c.is_ascii_digit()) {
            // Already epoch + offset
            return None;
        }
    }

    // Try parsing "YYYY-MM-DD HH:MM:SS <tz>" format
    if parts.len() == 2 {
        let tz = parts[0];
        let datetime = parts[1];

        // Parse tz offset
        let tz_bytes = tz.as_bytes();
        if tz_bytes.len() >= 5 {
            let sign: i64 = if tz_bytes[0] == b'-' { -1 } else { 1 };
            let h: i64 = tz[1..3].parse().unwrap_or(0);
            let m: i64 = tz[3..5].parse().unwrap_or(0);
            let tz_secs = sign * (h * 3600 + m * 60);

            // Try YYYY-MM-DD HH:MM:SS
            if let Ok(offset) = time::UtcOffset::from_whole_seconds(tz_secs as i32) {
                let fmt = time::format_description::parse(
                    "[year]-[month]-[day] [hour]:[minute]:[second]",
                )
                .ok()?;
                if let Ok(naive) = time::PrimitiveDateTime::parse(datetime, &fmt) {
                    let dt = naive.assume_offset(offset);
                    let epoch = dt.unix_timestamp();
                    return Some(format!("{epoch} {tz}"));
                }
            }
        }
    }

    // Try "@<epoch>" format (git uses this for testing)
    if let Some(epoch_str) = trimmed.strip_prefix('@') {
        // @<epoch> <tz>
        let ep_parts: Vec<&str> = epoch_str.splitn(2, ' ').collect();
        if ep_parts.len() == 2 {
            if let Ok(_epoch) = ep_parts[0].parse::<i64>() {
                return Some(format!("{} {}", ep_parts[0], ep_parts[1]));
            }
        }
    }

    None
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
            if grit_lib::reftable::is_reftable_repo(git_dir) {
                grit_lib::reftable::reftable_write_ref(git_dir, refname, commit_oid, None, None)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            } else {
                let ref_path = git_dir.join(refname);
                if let Some(parent) = ref_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
            }
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

fn is_permission_denied_error(err: &grit_lib::error::Error) -> bool {
    err.to_string().contains("Permission denied") || err.to_string().contains("permission denied")
}
