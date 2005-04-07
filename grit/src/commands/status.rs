//! `grit status` — show the working tree status.
//!
//! Displays staged changes, unstaged changes, and untracked files.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{parse_bool, ConfigFile, ConfigScope, ConfigSet};
use grit_lib::diff::{detect_renames, diff_index_to_tree, diff_index_to_worktree, DiffStatus};
use grit_lib::error::Error;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::Repository;
use grit_lib::state::{detect_in_progress, resolve_head, HeadState};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit status`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show the working tree status")]
pub struct Args {
    /// Give output in short format.
    #[arg(short = 's', long = "short", overrides_with = "no_short")]
    pub short: bool,

    /// Disable short format (override status.short=true).
    #[arg(long = "no-short", overrides_with = "short")]
    pub no_short: bool,

    /// Give output in the porcelain format (v1 or v2).
    #[arg(long = "porcelain", value_name = "VERSION", num_args = 0..=1, default_missing_value = "v1")]
    pub porcelain: Option<String>,

    /// Show the branch name.
    #[arg(short = 'b', long = "branch", overrides_with = "no_branch")]
    pub branch: bool,

    /// Don't show branch name.
    #[arg(long = "no-branch", overrides_with = "branch")]
    pub no_branch: bool,

    /// Show untracked files.
    #[arg(short = 'u', long = "untracked-files", default_value = "normal")]
    pub untracked: String,

    /// Show ignored files.
    #[arg(long = "ignored")]
    pub ignored: bool,

    /// Terminate entries with NUL.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Show ahead/behind counts relative to upstream tracking branch (default).
    #[arg(long = "ahead-behind", overrides_with = "no_ahead_behind")]
    pub ahead_behind: bool,

    /// Suppress ahead/behind counts.
    #[arg(long = "no-ahead-behind")]
    pub no_ahead_behind: bool,

    /// Display untracked files in columns (accepted, not fully implemented).
    #[arg(long = "column", value_name = "STYLE", num_args = 0..=1, default_missing_value = "always")]
    pub column: Option<String>,

    /// Disable columnar output.
    #[arg(long = "no-column")]
    pub no_column: bool,

    /// Use v2 porcelain format.
    #[arg(long = "porcelain=v2", hide = true)]
    pub _porcelain_v2_hidden: bool,

    /// Renames detection mode.
    #[arg(short = 'M', long = "find-renames", value_name = "N", num_args = 0..=1, default_missing_value = "true")]
    pub find_renames: Option<String>,

    /// Do not detect renames.
    #[arg(long = "no-find-renames")]
    pub no_find_renames: bool,

    /// Suppress optional lock on the index.
    #[arg(long = "no-optional-locks")]
    pub no_optional_locks: bool,

    /// Show staged diff (use twice for unstaged diff too).
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Show stash info.
    #[arg(long = "show-stash")]
    pub show_stash: bool,

    /// Don't show stash info.
    #[arg(long = "no-show-stash")]
    pub no_show_stash: bool,

    /// Ignore submodule changes.
    #[arg(long = "ignore-submodules", value_name = "WHEN", num_args = 0..=1, default_missing_value = "all")]
    pub ignore_submodules: Option<String>,

    /// NUL-terminated output (implies porcelain).
    #[arg(long = "no-renames")]
    pub no_renames: bool,

    /// Pathspec arguments.
    #[arg(last = true)]
    pub pathspec: Vec<String>,
}

/// Run the `status` command.
pub fn run(mut args: Args) -> Result<()> {
    // -z implies porcelain
    if args.null_terminated && args.porcelain.is_none() {
        args.porcelain = Some("v1".to_string());
    }
    // In porcelain v2 mode, always show branch headers.
    // In porcelain v1, the branch header is only shown with --branch.
    if args.porcelain.as_deref() == Some("v2") {
        args.branch = true;
    }
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let head = resolve_head(&repo.git_dir)?;
    let in_progress = detect_in_progress(&repo.git_dir);

    // Load full config for status.displayCommentPrefix and advice.statusHints
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_else(|_| ConfigSet::new());

    // Apply config-based overrides for status options
    if let Some(val) = config.get("status.showUntrackedFiles") {
        // Config only applies when the user didn't explicitly pass -u
        if args.untracked == "normal" {
            args.untracked = val;
        }
    }
    // status.short config: only apply if user didn't pass --short or --no-short
    if !args.no_short {
        if let Some(val) = config.get("status.short") {
            if !args.short && (val == "true" || val == "yes" || val == "on" || val == "1") {
                args.short = true;
            }
        }
    }
    // --no-short overrides both config and -s
    if args.no_short {
        args.short = false;
    }
    // status.branch config: only apply if user didn't pass --branch or --no-branch
    if !args.no_branch {
        if let Some(val) = config.get("status.branch") {
            if !args.branch && (val == "true" || val == "yes" || val == "on" || val == "1") {
                args.branch = true;
            }
        }
    }
    // --no-branch overrides both config and -b
    if args.no_branch {
        args.branch = false;
    }

    // Normalize untracked-files values: "false"/"0" → "no", "true"/"1" → "normal"
    let untracked_mode = match args.untracked.as_str() {
        "no" | "false" | "0" => "no",
        "all" => "all",
        _ => "normal",
    };

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
    let staged_raw = diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?;
    // Detect renames among staged entries (delete+add → rename at 50% threshold)
    let staged = detect_renames(&repo.odb, staged_raw, 50);

    // Diff: unstaged (worktree vs index)
    let unstaged = diff_index_to_worktree(&repo.odb, &index, work_tree)?;

    // Untracked and ignored files
    let show_all_untracked = untracked_mode == "all";
    let hide_untracked = untracked_mode == "no";
    let (untracked, ignored_files) = if !hide_untracked {
        collect_untracked_and_ignored(&repo, &index, work_tree, args.ignored, show_all_untracked)?
    } else if args.ignored {
        // Even with -u no, --ignored should show ignored files
        let (_, ignored) = collect_untracked_and_ignored(&repo, &index, work_tree, true, false)?;
        (Vec::new(), ignored)
    } else {
        (Vec::new(), Vec::new())
    };

    // Compute the cwd prefix relative to work_tree so paths are displayed
    // relative to the user's current directory (matching git behavior).
    let cwd = std::env::current_dir().unwrap_or_default();
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd);
    let wt_canon = work_tree.canonicalize().unwrap_or_else(|_| work_tree.to_path_buf());
    let prefix = cwd_canon.strip_prefix(&wt_canon).ok().and_then(|p| {
        if p.as_os_str().is_empty() {
            None
        } else {
            Some(p.to_path_buf())
        }
    });

    // Re-map paths from worktree-relative to cwd-relative when prefix is set.
    let relativize = |wt_rel: &str| -> String {
        if let Some(ref pfx) = prefix {
            let target = Path::new(wt_rel);
            // Compute ".." components to go up from prefix, then append target
            let mut result = PathBuf::new();
            for _ in pfx.components() {
                result.push("..");
            }
            result.push(target);
            result.to_string_lossy().to_string()
        } else {
            wt_rel.to_string()
        }
    };

    let staged = remap_diff_paths(&staged, &relativize);
    let unstaged = remap_diff_paths(&unstaged, &relativize);
    let untracked: Vec<String> = untracked.into_iter().map(|p| relativize(&p)).collect();
    let ignored_files: Vec<String> = ignored_files.into_iter().map(|p| relativize(&p)).collect();

    // Apply optional pathspec filters (including exclusion forms like :!path).
    let (staged, unstaged, untracked, ignored_files) =
        apply_pathspec_filters(staged, unstaged, untracked, ignored_files, &args.pathspec);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.short || args.porcelain.is_some() {
        format_short(
            &mut out,
            &args,
            &head,
            &repo,
            &staged,
            &unstaged,
            &untracked,
            &ignored_files,
        )?;
    } else {
        format_long(
            &mut out,
            &head,
            &repo,
            &config,
            &args,
            &in_progress,
            &staged,
            &unstaged,
            &untracked,
            &ignored_files,
            hide_untracked,
        )?;

        // -v: append cached diff; -vv: also append working tree diff
        if args.verbose >= 1 {
            drop(out);
            let exe = std::env::current_exe().unwrap_or_else(|_| "grit".into());
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("diff").arg("--cached");
            let output = cmd.output();
            if let Ok(o) = output {
                let stdout2 = io::stdout();
                let mut out2 = stdout2.lock();
                out2.write_all(&o.stdout)?;
            }

            if args.verbose >= 2 {
                let stdout3 = io::stdout();
                let mut out3 = stdout3.lock();
                writeln!(out3, "--------------------------------------------------")?;
                writeln!(out3, "Changes not staged for commit:")?;
                let mut cmd2 = std::process::Command::new(&exe);
                cmd2.arg("diff");
                let output2 = cmd2.output();
                if let Ok(o) = output2 {
                    out3.write_all(&o.stdout)?;
                }
            }
        }
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
        } else if is_dir {
            // For untracked directories, check if all files inside are ignored.
            // Git hides directories whose entire contents are ignored.
            let dir_path = work_tree.join(check_path);
            let mut sub_files = Vec::new();
            walk_for_untracked(&dir_path, work_tree, &tracked, &mut sub_files, true)?;
            let all_ignored = !sub_files.is_empty()
                && sub_files.iter().all(|f| {
                    let f_is_dir = f.ends_with('/');
                    let f_check = if f_is_dir {
                        &f[..f.len() - 1]
                    } else {
                        f.as_str()
                    };
                    matcher
                        .check_path(repo, Some(index), f_check, f_is_dir)
                        .map(|(ig, _)| ig)
                        .unwrap_or(false)
                });
            if all_ignored {
                if collect_ignored {
                    ignored_files.push(path);
                }
            } else {
                untracked.push(path);
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
    repo: &Repository,
    staged: &[grit_lib::diff::DiffEntry],
    unstaged: &[grit_lib::diff::DiffEntry],
    untracked: &[String],
    ignored_files: &[String],
) -> Result<()> {
    let terminator = if args.null_terminated { '\0' } else { '\n' };

    if args.branch {
        let branch = head.branch_name().unwrap_or("HEAD (no branch)");
        write!(out, "## {branch}")?;
        if !args.no_ahead_behind {
            if let Some(branch_name) = head.branch_name() {
                if let Ok(Some((upstream, ahead, behind))) = compute_ahead_behind(repo, branch_name)
                {
                    write!(out, "...{upstream}")?;
                    if ahead > 0 || behind > 0 {
                        let mut parts = Vec::new();
                        if ahead > 0 {
                            parts.push(format!("ahead {ahead}"));
                        }
                        if behind > 0 {
                            parts.push(format!("behind {behind}"));
                        }
                        write!(out, " [{}]", parts.join(", "))?;
                    }
                }
            }
        }
        write!(out, "{terminator}")?;
    }

    // Build a merged view: XY path
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut staged_map: std::collections::HashMap<String, char> = std::collections::HashMap::new();
    let mut unstaged_map: std::collections::HashMap<String, char> =
        std::collections::HashMap::new();

    // Track rename pairs: old_path -> (status_char, display_string)
    let mut staged_renames: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();

    for entry in staged {
        if entry.status == DiffStatus::Renamed {
            let old = entry.old_path.as_deref().unwrap_or("").to_owned();
            let new = entry.new_path.as_deref().unwrap_or("").to_owned();
            let display = format!("{old} -> {new}");
            staged_map.insert(new.clone(), 'R');
            staged_renames.insert(new.clone(), (old, display));
            paths.insert(new);
        } else {
            let path = entry.path().to_owned();
            staged_map.insert(path.clone(), entry.status.letter());
            paths.insert(path);
        }
    }

    for entry in unstaged {
        let path = entry.path().to_owned();
        unstaged_map.insert(path.clone(), entry.status.letter());
        paths.insert(path);
    }

    for path in &paths {
        let x = staged_map.get(path).copied().unwrap_or(' ');
        let y = unstaged_map.get(path).copied().unwrap_or(' ');
        if let Some((_old, display)) = staged_renames.get(path) {
            write!(out, "{x}{y} {display}{terminator}")?;
        } else {
            write!(out, "{x}{y} {path}{terminator}")?;
        }
    }

    for path in untracked {
        write!(out, "?? {path}{terminator}")?;
    }

    for path in ignored_files {
        write!(out, "!! {path}{terminator}")?;
    }

    Ok(())
}

/// Helper: write a line with optional comment prefix.
/// Git's comment prefix behavior:
///   "# text" for normal text, "#" for empty lines, "#\tfile" for tab-indented lines.
fn cpw(out: &mut impl Write, prefix: &str, line: &str) -> Result<()> {
    if prefix.is_empty() {
        writeln!(out, "{line}")?;
    } else if line.is_empty() {
        // Empty line: just "#" with no trailing space
        writeln!(out, "{}", prefix.trim_end())?;
    } else if line.starts_with('\t') {
        // Tab-indented: "#\tfile" (no space between # and tab)
        writeln!(out, "{}{line}", prefix.trim_end())?;
    } else {
        writeln!(out, "{prefix}{line}")?;
    }
    Ok(())
}

/// Long format (default).
fn format_long(
    out: &mut impl Write,
    head: &HeadState,
    repo: &Repository,
    config: &ConfigSet,
    args: &Args,
    in_progress: &[grit_lib::state::InProgressOperation],
    staged: &[grit_lib::diff::DiffEntry],
    unstaged: &[grit_lib::diff::DiffEntry],
    untracked: &[String],
    ignored_files: &[String],
    hide_untracked: bool,
) -> Result<()> {
    // Determine comment prefix
    let comment_prefix = match config.get("status.displayCommentPrefix") {
        Some(v) if v == "true" || v == "yes" || v == "on" || v == "1" => "# ",
        _ => "",
    };
    let cp = comment_prefix;

    // Determine if hints should be shown
    let show_hints = if let Ok(v) = std::env::var("GIT_ADVICE") {
        parse_bool(&v).unwrap_or(true)
    } else {
        match config.get("advice.statusHints") {
            Some(v) if v == "false" || v == "no" || v == "off" || v == "0" => false,
            _ => true,
        }
    };

    // Branch info
    match head {
        HeadState::Branch {
            short_name,
            oid: Some(_),
            ..
        } => {
            cpw(out, cp, &format!("On branch {short_name}"))?;
            if !args.no_ahead_behind {
                if let Ok(Some((upstream, ahead, behind))) = compute_ahead_behind(repo, short_name)
                {
                    if ahead > 0 && behind > 0 {
                        cpw(
                            out,
                            cp,
                            &format!("Your branch and '{}' have diverged,", upstream),
                        )?;
                        cpw(
                            out,
                            cp,
                            &format!(
                                "and have {} and {} different commits each, respectively.",
                                ahead, behind
                            ),
                        )?;
                        if show_hints {
                            cpw(out, cp, "  (use \"git pull\" if you want to integrate the remote branch with yours)")?;
                        }
                        cpw(out, cp, "")?;
                    } else if ahead > 0 {
                        cpw(
                            out,
                            cp,
                            &format!(
                                "Your branch is ahead of '{}' by {} commit{}.",
                                upstream,
                                ahead,
                                if ahead == 1 { "" } else { "s" }
                            ),
                        )?;
                        if show_hints {
                            cpw(
                                out,
                                cp,
                                "  (use \"git push\" to publish your local commits)",
                            )?;
                        }
                        cpw(out, cp, "")?;
                    } else if behind > 0 {
                        cpw(out, cp, &format!("Your branch is behind '{}' by {} commit{}, and can be fast-forwarded.", upstream, behind, if behind == 1 { "" } else { "s" }))?;
                        if show_hints {
                            cpw(out, cp, "  (use \"git pull\" to update your local branch)")?;
                        }
                        cpw(out, cp, "")?;
                    } else {
                        cpw(
                            out,
                            cp,
                            &format!("Your branch is up to date with '{}'.", upstream),
                        )?;
                        cpw(out, cp, "")?;
                    }
                }
            }
        }
        HeadState::Branch {
            short_name,
            oid: None,
            ..
        } => {
            cpw(out, cp, &format!("On branch {short_name}"))?;
            cpw(out, cp, "")?;
            cpw(out, cp, "No commits yet")?;
            cpw(out, cp, "")?;
        }
        HeadState::Detached { oid } => {
            let short = &oid.to_hex()[..7];
            cpw(out, cp, &format!("HEAD detached at {short}"))?;
        }
        HeadState::Invalid => {
            cpw(out, cp, "Not currently on any branch.")?;
        }
    }

    // In-progress operations
    for op in in_progress {
        cpw(out, cp, "")?;
        cpw(out, cp, &format!("You are currently {}.", op.description()))?;
        cpw(out, cp, &format!("  ({})", op.hint()))?;
    }

    // Track whether we've printed any section (to know if we need a separator)
    let mut has_section = false;

    // Staged changes
    if !staged.is_empty() {
        has_section = true;
        cpw(out, cp, "Changes to be committed:")?;
        if show_hints {
            if head.oid().is_some() {
                cpw(
                    out,
                    cp,
                    "  (use \"git restore --staged <file>...\" to unstage)",
                )?;
            } else {
                cpw(out, cp, "  (use \"git rm --cached <file>...\" to unstage)")?;
            }
        }
        for entry in staged {
            let label = match entry.status {
                DiffStatus::Added => "new file",
                DiffStatus::Deleted => "deleted",
                DiffStatus::Modified => "modified",
                DiffStatus::Renamed => "renamed",
                DiffStatus::TypeChanged => "typechange",
                _ => "changed",
            };
            cpw(out, cp, &format!("\t{label}:   {}", entry.path()))?;
        }
        cpw(out, cp, "")?;
    }

    // Unstaged changes
    if !unstaged.is_empty() {
        if has_section {
            // blank line already printed after previous section
        } else {
            has_section = true;
        }
        cpw(out, cp, "Changes not staged for commit:")?;
        if show_hints {
            cpw(
                out,
                cp,
                "  (use \"git add <file>...\" to update what will be committed)",
            )?;
            cpw(
                out,
                cp,
                "  (use \"git restore <file>...\" to discard changes in working directory)",
            )?;
        }
        for entry in unstaged {
            let label = match entry.status {
                DiffStatus::Deleted => "deleted",
                DiffStatus::Modified => "modified",
                DiffStatus::TypeChanged => "typechange",
                _ => "changed",
            };
            cpw(out, cp, &format!("\t{label}:   {}", entry.path()))?;
        }
        cpw(out, cp, "")?;
    }

    // Untracked files
    if !untracked.is_empty() {
        if has_section {
            // blank line already printed after previous section
        } else {
            has_section = true;
        }
        cpw(out, cp, "Untracked files:")?;
        if show_hints {
            cpw(
                out,
                cp,
                "  (use \"git add <file>...\" to include in what will be committed)",
            )?;
        }
        for path in untracked {
            cpw(out, cp, &format!("\t{path}"))?;
        }
        cpw(out, cp, "")?;
    }

    // Ignored files
    if !ignored_files.is_empty() {
        if has_section {
            // blank line already printed after previous section
        }
        cpw(out, cp, "Ignored files:")?;
        if show_hints {
            cpw(
                out,
                cp,
                "  (use \"git add -f <file>...\" to include in what will be committed)",
            )?;
        }
        for path in ignored_files {
            cpw(out, cp, &format!("\t{path}"))?;
        }
        cpw(out, cp, "")?;
    }

    // "Untracked files not listed" message when -uno is used
    if hide_untracked {
        if show_hints {
            cpw(
                out,
                cp,
                "Untracked files not listed (use -u option to show untracked files)",
            )?;
        } else {
            cpw(out, cp, "Untracked files not listed")?;
        }
    }

    // Footer messages
    if staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
        if hide_untracked {
            // When hiding untracked, don't say "working tree clean"
        } else if !ignored_files.is_empty() {
            if show_hints {
                cpw(
                    out,
                    cp,
                    "nothing to commit but untracked files present (use \"git add\" to track)",
                )?;
            } else {
                cpw(out, cp, "nothing to commit but untracked files present")?;
            }
        } else {
            cpw(out, cp, "nothing to commit, working tree clean")?;
        }
    } else if !staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
        // Only staged changes — no footer needed (git doesn't print one)
    } else if staged.is_empty() && !unstaged.is_empty() && untracked.is_empty() {
        cpw(
            out,
            cp,
            "no changes added to commit (use \"git add\" and/or \"git commit -a\")",
        )?;
    } else if staged.is_empty() && unstaged.is_empty() && !untracked.is_empty() {
        if show_hints {
            cpw(
                out,
                cp,
                "nothing added to commit but untracked files present (use \"git add\" to track)",
            )?;
        } else {
            cpw(out, cp, "nothing added to commit but untracked files present")?;
        }
    } else if staged.is_empty() {
        cpw(
            out,
            cp,
            "no changes added to commit (use \"git add\" and/or \"git commit -a\")",
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
        if name == ".test_tick" || name == ".gitconfig" {
            // Test-harness artifacts that may be created in the working tree
            // when HOME points at the repo root.
            continue;
        }

        let path = entry.path();
        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name);

        if rel == ".test_tick" || rel == ".gitconfig" {
            continue;
        }

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
                    .is_some_and(|t| t.starts_with(&prefix));
                if has_tracked {
                    walk_for_untracked(&path, work_tree, tracked, out, show_all)?;
                } else {
                    // Check if dir has any files (recursively);
                    // empty directories are not shown by git.
                    let mut sub = Vec::new();
                    walk_for_untracked(&path, work_tree, tracked, &mut sub, false)?;
                    if !sub.is_empty() {
                        out.push(format!("{rel}/"));
                    }
                }
            }
        } else if !tracked.contains(&rel) {
            out.push(rel);
        }
    }

    Ok(())
}

/// Split pathspecs into include and exclude lists.
fn split_pathspecs(pathspecs: &[String]) -> (Vec<String>, Vec<String>) {
    let mut includes = Vec::new();
    let mut excludes = Vec::new();

    for spec in pathspecs {
        if let Some(rest) = spec.strip_prefix(":!") {
            excludes.push(rest.to_string());
        } else if let Some(rest) = spec.strip_prefix(":(exclude)") {
            excludes.push(rest.to_string());
        } else {
            includes.push(spec.clone());
        }
    }

    (includes, excludes)
}

fn path_matches_any(path: &str, specs: &[String]) -> bool {
    specs
        .iter()
        .any(|spec| crate::pathspec::pathspec_matches(spec, path))
}

fn path_in_pathspec(path: &str, includes: &[String], excludes: &[String]) -> bool {
    let included = if includes.is_empty() {
        true
    } else {
        path_matches_any(path, includes)
    };
    included && !path_matches_any(path, excludes)
}

fn apply_pathspec_filters(
    staged: Vec<grit_lib::diff::DiffEntry>,
    unstaged: Vec<grit_lib::diff::DiffEntry>,
    untracked: Vec<String>,
    ignored_files: Vec<String>,
    pathspecs: &[String],
) -> (
    Vec<grit_lib::diff::DiffEntry>,
    Vec<grit_lib::diff::DiffEntry>,
    Vec<String>,
    Vec<String>,
) {
    if pathspecs.is_empty() {
        return (staged, unstaged, untracked, ignored_files);
    }

    let (includes, excludes) = split_pathspecs(pathspecs);

    let staged = staged
        .into_iter()
        .filter(|e| path_in_pathspec(e.path(), &includes, &excludes))
        .collect();
    let unstaged = unstaged
        .into_iter()
        .filter(|e| path_in_pathspec(e.path(), &includes, &excludes))
        .collect();
    let untracked = untracked
        .into_iter()
        .filter(|p| path_in_pathspec(p, &includes, &excludes))
        .collect();
    let ignored_files = ignored_files
        .into_iter()
        .filter(|p| path_in_pathspec(p, &includes, &excludes))
        .collect();

    (staged, unstaged, untracked, ignored_files)
}

/// Compute ahead/behind counts for the current branch relative to its upstream.
fn compute_ahead_behind(
    repo: &Repository,
    branch_name: &str,
) -> Result<Option<(String, usize, usize)>> {
    let config_path = repo.git_dir.join("config");
    let config_file = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => return Ok(None),
    };
    let mut config = ConfigSet::new();
    config.merge(&config_file);

    let merge_key = format!("branch.{branch_name}.merge");
    let remote_key = format!("branch.{branch_name}.remote");

    let merge = match config.get(&merge_key) {
        Some(m) => m,
        None => return Ok(None),
    };
    let remote = config
        .get(&remote_key)
        .unwrap_or_else(|| "origin".to_string());

    let upstream_branch = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    let upstream_display = if remote == "." {
        upstream_branch.to_string()
    } else {
        format!("{remote}/{upstream_branch}")
    };

    // Resolve upstream OID — for remote="." it's a local branch, otherwise a remote ref
    let upstream_ref = if remote == "." {
        format!("refs/heads/{upstream_branch}")
    } else {
        format!("refs/remotes/{remote}/{upstream_branch}")
    };
    let upstream_oid = match resolve_ref_to_oid(&repo.git_dir, &upstream_ref) {
        Some(oid) => oid,
        None => return Ok(Some((upstream_display, 0, 0))), // gone
    };

    // Resolve local OID
    let local_ref = format!("refs/heads/{branch_name}");
    let local_oid = match resolve_ref_to_oid(&repo.git_dir, &local_ref) {
        Some(oid) => oid,
        None => return Ok(None),
    };

    if local_oid == upstream_oid {
        return Ok(Some((upstream_display, 0, 0)));
    }

    // Count ahead/behind using ancestor closure
    let local_ancestors = collect_ancestors_set(repo, local_oid)?;
    let upstream_ancestors = collect_ancestors_set(repo, upstream_oid)?;

    let ahead = local_ancestors
        .iter()
        .filter(|oid| !upstream_ancestors.contains(oid))
        .count();
    let behind = upstream_ancestors
        .iter()
        .filter(|oid| !local_ancestors.contains(oid))
        .count();

    Ok(Some((upstream_display, ahead, behind)))
}

fn resolve_ref_to_oid(git_dir: &Path, refname: &str) -> Option<ObjectId> {
    grit_lib::refs::resolve_ref(git_dir, refname).ok()
}

fn collect_ancestors_set(
    repo: &Repository,
    start: ObjectId,
) -> Result<std::collections::HashSet<ObjectId>> {
    use std::collections::HashSet;
    let mut visited = HashSet::new();
    let mut queue = vec![start];
    while let Some(oid) = queue.pop() {
        if !visited.insert(oid) {
            continue;
        }
        if let Ok(obj) = repo.odb.read(&oid) {
            if let Ok(commit) = parse_commit(&obj.data) {
                for parent in &commit.parents {
                    if !visited.contains(parent) {
                        queue.push(*parent);
                    }
                }
            }
        }
    }
    Ok(visited)
}

/// Remap worktree-relative paths in diff entries using the given function.
fn remap_diff_paths(
    entries: &[grit_lib::diff::DiffEntry],
    f: &dyn Fn(&str) -> String,
) -> Vec<grit_lib::diff::DiffEntry> {
    entries
        .iter()
        .map(|e| {
            let mut new_entry = e.clone();
            if let Some(ref p) = e.old_path {
                new_entry.old_path = Some(f(p));
            }
            if let Some(ref p) = e.new_path {
                new_entry.new_path = Some(f(p));
            }
            new_entry
        })
        .collect()
}
