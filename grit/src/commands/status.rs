//! `grit status` — show the working tree status.
//!
//! Displays staged changes, unstaged changes, and untracked files.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{diff_index_to_tree, diff_index_to_worktree, DiffStatus};
use grit_lib::error::Error;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::Index;
use grit_lib::objects::parse_commit;
use grit_lib::repo::Repository;
use grit_lib::state::{detect_in_progress, resolve_head, HeadState};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit status`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show the working tree status")]
pub struct Args {
    /// Give output in short format.
    #[arg(short = 's', long = "short")]
    pub short: bool,

    /// Give output in the porcelain v1 format.
    #[arg(long = "porcelain")]
    pub porcelain: bool,

    /// Show the branch name.
    #[arg(short = 'b', long = "branch")]
    pub branch: bool,

    /// Show untracked files.
    #[arg(short = 'u', long = "untracked-files", default_value = "normal")]
    pub untracked: String,

    /// Show ignored files.
    #[arg(long = "ignored")]
    pub ignored: bool,

    /// Terminate entries with NUL.
    #[arg(short = 'z')]
    pub null_terminated: bool,
}

/// Run the `status` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let head = resolve_head(&repo.git_dir)?;
    let in_progress = detect_in_progress(&repo.git_dir);

    // Load index
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Get HEAD tree OID
    let head_tree = match head.oid() {
        Some(oid) => {
            let obj = repo.odb.read(oid)?;
            let commit = parse_commit(&obj.data)?;
            Some(commit.tree)
        }
        None => None,
    };

    // Diff: staged (index vs HEAD tree)
    let staged = diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?;

    // Diff: unstaged (worktree vs index)
    let unstaged = diff_index_to_worktree(&repo.odb, &index, work_tree)?;

    // Untracked and ignored files
    let show_all_untracked = args.untracked == "all";
    let (untracked, ignored_files) = if args.untracked != "no" {
        collect_untracked_and_ignored(&repo, &index, work_tree, args.ignored, show_all_untracked)?
    } else if args.ignored {
        // Even with -u no, --ignored should show ignored files
        let (_, ignored) = collect_untracked_and_ignored(&repo, &index, work_tree, true, false)?;
        (Vec::new(), ignored)
    } else {
        (Vec::new(), Vec::new())
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.short || args.porcelain {
        format_short(
            &mut out,
            &args,
            &head,
            &staged,
            &unstaged,
            &untracked,
            &ignored_files,
        )?;
    } else {
        format_long(
            &mut out,
            &head,
            &in_progress,
            &staged,
            &unstaged,
            &untracked,
            &ignored_files,
        )?;
    }

    Ok(())
}

/// Collect untracked files, filtering out ignored ones.
/// If `collect_ignored` is true, also return the ignored file list.
fn collect_untracked_and_ignored(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    collect_ignored: bool,
    show_all: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    let mut all_untracked = Vec::new();
    walk_for_untracked(work_tree, work_tree, &tracked, &mut all_untracked, show_all)?;
    all_untracked.sort();

    // Build ignore matcher
    let mut matcher = IgnoreMatcher::from_repository(repo)?;

    let mut untracked = Vec::new();
    let mut ignored_files = Vec::new();

    for path in all_untracked {
        let is_dir = path.ends_with('/');
        let check_path = if is_dir {
            &path[..path.len() - 1]
        } else {
            &path
        };
        let (is_ignored, _) = matcher.check_path(repo, Some(index), check_path, is_dir)?;
        if is_ignored {
            if collect_ignored {
                ignored_files.push(path);
            }
        } else {
            untracked.push(path);
        }
    }

    Ok((untracked, ignored_files))
}

/// Short/porcelain format.
fn format_short(
    out: &mut impl Write,
    args: &Args,
    head: &HeadState,
    staged: &[grit_lib::diff::DiffEntry],
    unstaged: &[grit_lib::diff::DiffEntry],
    untracked: &[String],
    ignored_files: &[String],
) -> Result<()> {
    let terminator = if args.null_terminated { '\0' } else { '\n' };

    if args.branch || args.porcelain {
        let branch = head.branch_name().unwrap_or("HEAD (no branch)");
        write!(out, "## {branch}")?;
        write!(out, "{terminator}")?;
    }

    // Build a merged view: XY path
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut staged_map: std::collections::HashMap<String, char> = std::collections::HashMap::new();
    let mut unstaged_map: std::collections::HashMap<String, char> =
        std::collections::HashMap::new();

    for entry in staged {
        let path = entry.path().to_owned();
        staged_map.insert(path.clone(), entry.status.letter());
        paths.insert(path);
    }

    for entry in unstaged {
        let path = entry.path().to_owned();
        unstaged_map.insert(path.clone(), entry.status.letter());
        paths.insert(path);
    }

    for path in &paths {
        let x = staged_map.get(path).copied().unwrap_or(' ');
        let y = unstaged_map.get(path).copied().unwrap_or(' ');
        write!(out, "{x}{y} {path}{terminator}")?;
    }

    for path in untracked {
        write!(out, "?? {path}{terminator}")?;
    }

    for path in ignored_files {
        write!(out, "!! {path}{terminator}")?;
    }

    Ok(())
}

/// Long format (default).
fn format_long(
    out: &mut impl Write,
    head: &HeadState,
    in_progress: &[grit_lib::state::InProgressOperation],
    staged: &[grit_lib::diff::DiffEntry],
    unstaged: &[grit_lib::diff::DiffEntry],
    untracked: &[String],
    ignored_files: &[String],
) -> Result<()> {
    // Branch info
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

    // In-progress operations
    for op in in_progress {
        writeln!(out)?;
        writeln!(out, "You are currently {}.", op.description())?;
        writeln!(out, "  ({})", op.hint())?;
    }

    // Track whether we've printed any section (to know if we need a separator)
    let mut has_section = false;

    // Staged changes
    if !staged.is_empty() {
        has_section = true;
        writeln!(out, "Changes to be committed:")?;
        writeln!(out, "  (use \"git restore --staged <file>...\" to unstage)")?;
        for entry in staged {
            let label = match entry.status {
                DiffStatus::Added => "new file",
                DiffStatus::Deleted => "deleted",
                DiffStatus::Modified => "modified",
                DiffStatus::Renamed => "renamed",
                DiffStatus::TypeChanged => "typechange",
                _ => "changed",
            };
            writeln!(out, "\t{label}:   {}", entry.path())?;
        }
        writeln!(out)?;
    }

    // Unstaged changes
    if !unstaged.is_empty() {
        if has_section {
            // blank line already printed after previous section
        } else {
            has_section = true;
        }
        writeln!(out, "Changes not staged for commit:")?;
        writeln!(
            out,
            "  (use \"git add <file>...\" to update what will be committed)"
        )?;
        writeln!(
            out,
            "  (use \"git restore <file>...\" to discard changes in working directory)"
        )?;
        for entry in unstaged {
            let label = match entry.status {
                DiffStatus::Deleted => "deleted",
                DiffStatus::Modified => "modified",
                DiffStatus::TypeChanged => "typechange",
                _ => "changed",
            };
            writeln!(out, "\t{label}:   {}", entry.path())?;
        }
        writeln!(out)?;
    }

    // Untracked files
    if !untracked.is_empty() {
        if has_section {
            // blank line already printed after previous section
        } else {
            has_section = true;
        }
        writeln!(out, "Untracked files:")?;
        writeln!(
            out,
            "  (use \"git add <file>...\" to include in what will be committed)"
        )?;
        for path in untracked {
            writeln!(out, "\t{path}")?;
        }
        writeln!(out)?;
    }

    // Ignored files
    if !ignored_files.is_empty() {
        if has_section {
            // blank line already printed after previous section
        } else {
            has_section = true;
        }
        writeln!(out, "Ignored files:")?;
        writeln!(
            out,
            "  (use \"git add -f <file>...\" to include in what will be committed)"
        )?;
        for path in ignored_files {
            writeln!(out, "\t{path}")?;
        }
        writeln!(out)?;
    }

    // Footer messages
    if staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
        if !ignored_files.is_empty() {
            writeln!(
                out,
                "nothing to commit but untracked files present (use \"git add\" to track)"
            )?;
        } else {
            writeln!(out, "nothing to commit, working tree clean")?;
        }
    } else if !staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
        // Only staged changes — no footer needed (git doesn't print one)
    } else if staged.is_empty() && !unstaged.is_empty() && untracked.is_empty() {
        writeln!(
            out,
            "no changes added to commit (use \"git add\" and/or \"git commit -a\")"
        )?;
    } else if staged.is_empty() && unstaged.is_empty() && !untracked.is_empty() {
        writeln!(
            out,
            "nothing added to commit but untracked files present (use \"git add\" to track)"
        )?;
    } else if staged.is_empty() {
        writeln!(
            out,
            "no changes added to commit (use \"git add\" and/or \"git commit -a\")"
        )?;
    }

    Ok(())
}

/// Find untracked files in the working tree (raw, before ignore filtering).
#[allow(dead_code)]
fn find_untracked(work_tree: &Path, index: &Index) -> Result<Vec<String>> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    let mut untracked = Vec::new();
    walk_for_untracked(work_tree, work_tree, &tracked, &mut untracked, false)?;
    untracked.sort();
    Ok(untracked)
}

/// Walk directories finding files not in the tracked set.
fn walk_for_untracked(
    dir: &Path,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    out: &mut Vec<String>,
    show_all: bool,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let name = entry.file_name().to_string_lossy().to_string();

        if name == ".git" {
            continue;
        }

        let path = entry.path();
        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name);

        // Use file_type() from DirEntry — avoids extra stat syscall on Linux
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        if is_dir {
            if show_all {
                walk_for_untracked(&path, work_tree, tracked, out, show_all)?;
            } else {
                let prefix = format!("{rel}/");
                let has_tracked = tracked
                    .range::<String, _>(&prefix..)
                    .next()
                    .map_or(false, |t| t.starts_with(&prefix));
                if has_tracked {
                    walk_for_untracked(&path, work_tree, tracked, out, show_all)?;
                } else {
                    out.push(format!("{rel}/"));
                }
            }
        } else if !tracked.contains(&rel) {
            out.push(rel);
        }
    }

    Ok(())
}
