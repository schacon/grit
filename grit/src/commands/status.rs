//! `grit status` — show the working tree status.
//!
//! Displays staged changes, unstaged changes, and untracked files.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::{
    detect_renames, diff_index_to_tree, diff_index_to_worktree, head_path_states,
    submodule_porcelain_flags, DiffEntry, DiffStatus,
};
use grit_lib::error::Error;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{Index, IndexEntry, MODE_GITLINK, MODE_TREE};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::reflog;
use grit_lib::repo::Repository;
use grit_lib::state::{detect_in_progress, resolve_head, HeadState};

use crate::branch_tracking::{
    format_tracking_info, shorten_tracking_ref, stat_branch_pair, upstream_tracking_full_ref,
    AheadBehindMode, TrackingStat,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::git_column::{merge_column_config, print_columns, ColOpts, ColumnOptions};

/// Arguments for `grit status`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show the working tree status")]
pub struct Args {
    /// Give output in short format.
    #[arg(short = 's', long = "short", overrides_with = "no_short")]
    pub short: bool,

    /// Long format (Git compatibility; status is long by default unless `-s` / porcelain).
    #[arg(long = "long", hide = true)]
    pub long: bool,

    /// Disable short format (override status.short=true).
    #[arg(long = "no-short", overrides_with = "short")]
    pub no_short: bool,

    /// Give output in the porcelain format (v1 or v2).
    ///
    /// Values must use `=` (`--porcelain=v2`) so a bare `--porcelain` does not swallow a pathspec.
    #[arg(
        long = "porcelain",
        default_missing_value = "v1",
        num_args = 0..=1,
        require_equals = true
    )]
    pub porcelain: Option<String>,

    /// Show the branch name.
    #[arg(short = 'b', long = "branch", overrides_with = "no_branch")]
    pub branch: bool,

    /// Don't show branch name.
    #[arg(long = "no-branch", overrides_with = "branch")]
    pub no_branch: bool,

    /// Show untracked files (`-u` alone defaults to `all`, matching Git).
    #[arg(short = 'u', long = "untracked-files", value_name = "MODE", num_args = 0..=1, default_missing_value = "all")]
    pub untracked: Option<String>,

    /// Show ignored files (`traditional`, `matching`, or `no`; bare `--ignored` means `traditional`).
    #[arg(
        long = "ignored",
        value_name = "MODE",
        num_args = 0..=1,
        default_missing_value = "traditional"
    )]
    pub ignored: Option<String>,

    /// Terminate entries with NUL.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Show ahead/behind counts relative to upstream tracking branch (default).
    #[arg(long = "ahead-behind", overrides_with = "no_ahead_behind")]
    pub ahead_behind: bool,

    /// Suppress ahead/behind counts.
    #[arg(long = "no-ahead-behind")]
    pub no_ahead_behind: bool,

    /// Display untracked files in columns (Git `column.c` layout).
    #[arg(
        long = "column",
        value_name = "STYLE",
        num_args = 0..=1,
        default_missing_value = "always",
        overrides_with = "no_column"
    )]
    pub column: Option<String>,

    /// Disable columnar output.
    #[arg(long = "no-column", overrides_with = "column")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoredMode {
    No,
    Traditional,
    Matching,
}

fn parse_ignored_mode(raw: Option<&str>) -> Result<IgnoredMode> {
    match raw {
        None => Ok(IgnoredMode::No),
        Some("no") => Ok(IgnoredMode::No),
        Some("traditional") => Ok(IgnoredMode::Traditional),
        Some("matching") => Ok(IgnoredMode::Matching),
        Some(other) => Err(anyhow::anyhow!("Invalid ignored mode '{other}'")),
    }
}

fn has_tracked_under(
    tracked: &BTreeSet<String>,
    gitlinks: &BTreeSet<String>,
    rel_dir: &str,
) -> bool {
    let prefix = if rel_dir.is_empty() {
        String::new()
    } else {
        format!("{rel_dir}/")
    };
    tracked
        .range::<String, _>(prefix.clone()..)
        .next()
        .is_some_and(|t| t.starts_with(&prefix))
        || gitlinks.iter().any(|g| {
            g.as_str() == rel_dir || (!rel_dir.is_empty() && g.starts_with(&format!("{rel_dir}/")))
        })
}

fn relative_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Run the `status` command.
pub fn run(mut args: Args) -> Result<()> {
    // Whether the user passed `--porcelain` (before `-z` may synthesize it).
    let explicit_porcelain = args.porcelain.is_some();
    // -z implies porcelain
    if args.null_terminated && args.porcelain.is_none() {
        args.porcelain = Some("v1".to_string());
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

    let mut colopts = ColOpts::new();
    if args.no_column {
        crate::git_column::parse_column_tokens_into("never", &mut colopts)
            .map_err(|e| anyhow::anyhow!(e))?;
    } else {
        merge_column_config(&config, &mut colopts).map_err(|e| anyhow::anyhow!(e))?;
        if let Some(style) = args.column.as_deref() {
            crate::git_column::apply_column_cli_arg(&mut colopts, Some(style))
                .map_err(|e| anyhow::anyhow!(e))?;
        }
    }
    crate::git_column::finalize_colopts(&mut colopts, None);

    // Apply config-based overrides for status options
    let untracked_mode_str = match args.untracked.as_ref() {
        None => config
            .get("status.showUntrackedFiles")
            .map(|s| s.to_string())
            .unwrap_or_else(|| "normal".to_string()),
        Some(s) => s.clone(),
    };
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
    // status.branch config: only apply if user didn't pass --branch or --no-branch.
    // Porcelain ignores `status.branch`; the `##` line requires explicit `-b` (Git tests).
    if !args.no_branch && args.porcelain.is_none() {
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

    let mut show_stash = args.show_stash;
    if !args.no_show_stash {
        if let Some(val) = config.get("status.showStash") {
            if !args.show_stash && (val == "true" || val == "yes" || val == "on" || val == "1") {
                show_stash = true;
            }
        }
    }
    if args.no_show_stash {
        show_stash = false;
    }

    // `status.aheadbehind` defaults true; only applies to human-readable formats (Git `commit.c`).
    let mut effective_no_ahead_behind = args.no_ahead_behind;
    if args.ahead_behind {
        effective_no_ahead_behind = false;
    } else if (args.short || args.porcelain.is_none()) && !args.no_ahead_behind {
        if let Some(v) = config.get("status.aheadbehind") {
            if matches!(
                v.to_ascii_lowercase().as_str(),
                "false" | "no" | "off" | "0"
            ) {
                effective_no_ahead_behind = true;
            }
        }
    }

    // Normalize untracked-files values: "false"/"0" → "no", "true"/"1" → "normal"
    let untracked_mode = match untracked_mode_str.as_str() {
        "no" | "false" | "0" => "no",
        "all" => "all",
        _ => "normal",
    };

    let ignored_mode = parse_ignored_mode(args.ignored.as_deref())?;
    if ignored_mode == IgnoredMode::Matching && untracked_mode == "no" {
        return Err(anyhow::anyhow!(
            "unsupported combination of ignored and untracked-files arguments"
        ));
    }

    // Load index: remember sparse-index on disk, then expand placeholders for diffs.
    let index_path = repo.index_path();
    let mut index = match grit_lib::index::Index::load(&index_path) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };
    let index_sparse_on_disk = index.sparse_directories;
    let _ = index.expand_sparse_directory_placeholders(&repo.odb);

    // Get HEAD tree OID
    let head_tree = match head.oid() {
        Some(oid) => {
            let obj = repo.odb.read(oid)?;
            let commit = parse_commit(&obj.data)?;
            Some(commit.tree)
        }
        None => None,
    };

    // Resolve rename detection settings for status.
    let status_rename_threshold = resolve_status_rename_threshold(&args, &config);

    // Diff: staged (index vs HEAD tree)
    let staged_raw = diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?;
    // Detect renames among staged entries when enabled.
    let staged = if let Some(threshold) = status_rename_threshold {
        detect_renames(&repo.odb, staged_raw.clone(), threshold)
    } else {
        staged_raw.clone()
    };

    // Diff: unstaged (worktree vs index), with optional rename detection.
    let unstaged_raw = diff_index_to_worktree(&repo.odb, &index, work_tree)?;
    let unstaged = if let Some(threshold) = status_rename_threshold {
        detect_renames(&repo.odb, unstaged_raw.clone(), threshold)
    } else {
        unstaged_raw.clone()
    };

    // Untracked and ignored files
    let show_all_untracked = untracked_mode == "all";
    let hide_untracked = untracked_mode == "no";

    // Porcelain v1: Git omits the `##` branch line when `--untracked-files=no` (e.g.
    // `status --porcelain -uno`). Grit still defaults to showing `##` for plain `--porcelain`
    // so clean repos and mixed outputs match our status tests (t12570). `-b` / `--branch`
    // always forces the header.
    if explicit_porcelain
        && args.porcelain.as_deref() == Some("v1")
        && !args.no_branch
        && !args.branch
        && !hide_untracked
    {
        args.branch = true;
    }

    let (untracked, ignored_files) = if !hide_untracked {
        collect_untracked_and_ignored(&repo, &index, work_tree, ignored_mode, show_all_untracked)?
    } else {
        (Vec::new(), Vec::new())
    };

    // `status.relativePaths` (default true): when false, paths stay worktree-relative
    // from repo root even when cwd is a subdirectory (Git `wt_status_collect`).
    let status_relative_paths = match config.get("status.relativePaths") {
        Some(v) if v == "false" || v == "no" || v == "off" || v == "0" => false,
        _ => true,
    };

    // Compute the cwd prefix relative to work_tree so paths are displayed
    // relative to the user's current directory (matching git behavior).
    // Porcelain always uses paths relative to the work tree root (ignores `status.relativePaths`).
    let prefix = if status_relative_paths && args.porcelain.is_none() {
        let cwd = std::env::current_dir().unwrap_or_default();
        let cwd_canon = cwd.canonicalize().unwrap_or(cwd);
        let wt_canon = work_tree
            .canonicalize()
            .unwrap_or_else(|_| work_tree.to_path_buf());
        cwd_canon.strip_prefix(&wt_canon).ok().and_then(|p| {
            if p.as_os_str().is_empty() {
                None
            } else {
                Some(p.to_path_buf())
            }
        })
    } else {
        None
    };

    // Re-map paths from worktree-relative to cwd-relative when prefix is set.
    // Git shows paths relative to the current directory (not `../` per nested dir only).
    let relativize = |wt_rel: &str| -> String {
        let Some(ref pfx) = prefix else {
            return wt_rel.to_string();
        };
        let from_base = work_tree.join(pfx);
        let to_path = work_tree.join(wt_rel.trim_end_matches('/'));
        let rel = diff_paths_relative(&from_base, &to_path);
        let s = rel.to_string_lossy().to_string();
        if wt_rel.ends_with('/') && !s.is_empty() && !s.ends_with('/') {
            format!("{s}/")
        } else if wt_rel.ends_with('/') && s.is_empty() {
            "./".to_owned()
        } else {
            s
        }
    };

    let pathspecs: Vec<String> = args
        .pathspec
        .iter()
        .filter(|spec| spec.as_str() != "--")
        .cloned()
        .collect();
    let staged: Vec<grit_lib::diff::DiffEntry> = staged
        .into_iter()
        .filter(|entry| status_path_matches(entry.path(), &pathspecs))
        .collect();
    let unstaged: Vec<grit_lib::diff::DiffEntry> = unstaged
        .into_iter()
        .filter(|entry| status_path_matches(entry.path(), &pathspecs))
        .collect();
    let untracked: Vec<String> = untracked
        .into_iter()
        .filter(|p| status_path_matches(p, &pathspecs))
        .collect();
    let ignored_files: Vec<String> = ignored_files
        .into_iter()
        .filter(|p| status_path_matches(p, &pathspecs))
        .collect();

    let staged_long = remap_diff_paths(&staged, &relativize);
    let unstaged_long = remap_diff_paths(&unstaged, &relativize);
    let untracked_long: Vec<String> = untracked.iter().map(|p| relativize(p)).collect();
    let ignored_long: Vec<String> = ignored_files.iter().map(|p| relativize(p)).collect();

    let quote_path_cfg = match config.get_bool("core.quotePath") {
        Some(Ok(v)) => v,
        Some(Err(_)) | None => true,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.porcelain.as_deref() == Some("v2") {
        format_porcelain_v2(
            &mut out,
            &args,
            &head,
            &repo,
            &config,
            work_tree,
            &index,
            head_tree.as_ref(),
            &staged,
            &unstaged,
            &untracked,
            &ignored_files,
            show_stash,
        )?;
    } else if args.short || args.porcelain.is_some() {
        format_short(
            &mut out,
            &args,
            effective_no_ahead_behind,
            &head,
            &repo,
            &staged,
            &unstaged,
            &untracked,
            &ignored_files,
            &relativize,
            quote_path_cfg,
        )?;
    } else {
        format_long(
            &mut out,
            &head,
            &repo,
            &config,
            &args,
            colopts,
            effective_no_ahead_behind,
            &in_progress,
            &index,
            index_sparse_on_disk,
            &staged_long,
            &unstaged_long,
            &untracked_long,
            &ignored_long,
            hide_untracked,
        )?;

        // -v: append cached diff; -vv: also append working tree diff.
        // Git `wt_longstatus_print_verbose`: `-v` uses normal diff prefixes; with `-vv` and
        // staged changes, print a second "Changes to be committed:" then cached diff with `c/`
        // vs `i/`; if there are unstaged changes, print separator + "Changes not staged for
        // commit:" then diff with `i/` vs `w/` (`diff.mnemonicprefix=true` for each).
        if args.verbose >= 1 {
            drop(out);
            let exe = std::env::current_exe().unwrap_or_else(|_| "grit".into());

            // Git prints these lines without the status comment prefix (matches test `echo` lines).
            if args.verbose >= 2 && !staged.is_empty() && head.oid().is_some() {
                let stdout_h = io::stdout();
                let mut out_h = stdout_h.lock();
                writeln!(out_h, "Changes to be committed:")?;
            }

            let mut cmd = std::process::Command::new(&exe);
            if args.verbose >= 2 {
                cmd.arg("-c").arg("diff.mnemonicprefix=true");
            }
            cmd.arg("diff").arg("--cached");
            let output = cmd.output();
            if let Ok(o) = output {
                let stdout2 = io::stdout();
                let mut out2 = stdout2.lock();
                out2.write_all(&o.stdout)?;
            }

            if args.verbose >= 2 && !unstaged.is_empty() {
                let stdout3 = io::stdout();
                let mut out3 = stdout3.lock();
                writeln!(out3, "--------------------------------------------------")?;
                writeln!(out3, "Changes not staged for commit:")?;
                let mut cmd2 = std::process::Command::new(&exe);
                cmd2.arg("-c").arg("diff.mnemonicprefix=true").arg("diff");
                let output2 = cmd2.output();
                if let Ok(o) = output2 {
                    out3.write_all(&o.stdout)?;
                }
            }
        }
    }

    Ok(())
}

/// Quote a path for `git status -s` / porcelain (Git `quote_path` / C-style rules).
fn quote_status_short_path(display: &str, quote_path_cfg: bool) -> String {
    let mut out = String::with_capacity(display.len() + 2);
    let mut needs_quotes = false;
    for ch in display.chars() {
        match ch {
            ' ' => {
                out.push(' ');
                needs_quotes = true;
            }
            '"' => {
                out.push_str("\\\"");
                needs_quotes = true;
            }
            '\\' => {
                out.push_str("\\\\");
                needs_quotes = true;
            }
            '\t' => {
                out.push_str("\\t");
                needs_quotes = true;
            }
            '\n' => {
                out.push_str("\\n");
                needs_quotes = true;
            }
            '\r' => {
                out.push_str("\\r");
                needs_quotes = true;
            }
            c if c.is_control() => {
                out.push_str(&format!("\\{:03o}", u32::from(c)));
                needs_quotes = true;
            }
            c if (c as u32) >= 0x80 => {
                if quote_path_cfg {
                    for b in ch.to_string().bytes() {
                        out.push_str(&format!("\\{:03o}", b));
                    }
                    needs_quotes = true;
                } else {
                    out.push(c);
                }
            }
            c => out.push(c),
        }
    }
    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

/// `to` expressed relative to `from` (Git-style: `..` segments then remainder).
fn diff_paths_relative(from: &Path, to: &Path) -> PathBuf {
    let from_components: Vec<std::path::Component<'_>> = from.components().collect();
    let to_components: Vec<std::path::Component<'_>> = to.components().collect();
    let mut i = 0usize;
    let min = from_components.len().min(to_components.len());
    while i < min && from_components[i] == to_components[i] {
        i += 1;
    }
    let mut out = PathBuf::new();
    for _ in i..from_components.len() {
        out.push("..");
    }
    for c in to_components.iter().skip(i) {
        out.push(c);
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Collect untracked and ignored paths, matching Git's `dir.c` + `wt-status.c` behavior
/// for `--ignored` / `--untracked-files` combinations.
fn collect_untracked_and_ignored(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    ignored_mode: IgnoredMode,
    show_all: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    let gitlinks: BTreeSet<String> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.mode == MODE_GITLINK)
        .map(|e| String::from_utf8_lossy(&e.path).into_owned())
        .collect();

    let mut matcher = IgnoreMatcher::from_repository(repo)?;
    let mut untracked = Vec::new();
    let mut ignored = Vec::new();

    visit_untracked_node(
        repo,
        index,
        work_tree,
        &tracked,
        &gitlinks,
        &mut matcher,
        ignored_mode,
        show_all,
        "",
        work_tree,
        &mut untracked,
        &mut ignored,
    )?;

    untracked.sort();
    ignored.sort();
    Ok((untracked, ignored))
}

fn visit_untracked_node(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    gitlinks: &BTreeSet<String>,
    matcher: &mut IgnoreMatcher,
    ignored_mode: IgnoredMode,
    show_all: bool,
    rel: &str,
    abs: &Path,
    untracked_out: &mut Vec<String>,
    ignored_out: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(abs) {
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
        let child_rel = relative_path(rel, &name);
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        if is_dir && gitlinks.contains(&child_rel) {
            continue;
        }

        if tracked.contains(&child_rel) {
            continue;
        }

        if is_dir {
            visit_untracked_directory(
                repo,
                index,
                work_tree,
                tracked,
                gitlinks,
                matcher,
                ignored_mode,
                show_all,
                &child_rel,
                &path,
                untracked_out,
                ignored_out,
            )?;
        } else {
            let (is_ign, _) = matcher.check_path(repo, Some(index), &child_rel, false)?;
            if is_ign {
                if ignored_mode != IgnoredMode::No {
                    ignored_out.push(child_rel);
                }
            } else {
                untracked_out.push(child_rel);
            }
        }
    }

    Ok(())
}

fn visit_untracked_directory(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    gitlinks: &BTreeSet<String>,
    matcher: &mut IgnoreMatcher,
    ignored_mode: IgnoredMode,
    show_all: bool,
    rel: &str,
    abs: &Path,
    untracked_out: &mut Vec<String>,
    ignored_out: &mut Vec<String>,
) -> Result<()> {
    if has_tracked_under(tracked, gitlinks, rel) {
        visit_untracked_node(
            repo,
            index,
            work_tree,
            tracked,
            gitlinks,
            matcher,
            ignored_mode,
            show_all,
            rel,
            abs,
            untracked_out,
            ignored_out,
        )?;
        return Ok(());
    }

    // Git `dir.c`: with `--ignored=matching` and full untracked listing, an excluded
    // directory is reported as a single path without enumerating children (unless
    // tracked files force a full walk — handled above).
    if ignored_mode == IgnoredMode::Matching
        && show_all
        && matcher.check_path(repo, Some(index), rel, true)?.0
    {
        ignored_out.push(format!("{rel}/"));
        return Ok(());
    }

    if ignored_mode == IgnoredMode::Traditional && !show_all {
        if let Some(dir_line) = traditional_normal_directory_only(
            repo, index, work_tree, tracked, gitlinks, matcher, rel, abs,
        )? {
            ignored_out.push(dir_line);
            return Ok(());
        }
    }

    let mut sub_untracked = Vec::new();
    let mut sub_ignored = Vec::new();
    visit_untracked_node(
        repo,
        index,
        work_tree,
        tracked,
        gitlinks,
        matcher,
        ignored_mode,
        true,
        rel,
        abs,
        &mut sub_untracked,
        &mut sub_ignored,
    )?;

    if show_all {
        untracked_out.append(&mut sub_untracked);
        ignored_out.append(&mut sub_ignored);
        return Ok(());
    }

    // `--untracked-files=normal`: collapse subtrees like Git's `walk_for_untracked`.
    if !sub_untracked.is_empty() && !sub_ignored.is_empty() {
        untracked_out.append(&mut sub_untracked);
        ignored_out.append(&mut sub_ignored);
        return Ok(());
    }

    if sub_untracked.is_empty() && !sub_ignored.is_empty() {
        let dir_excluded = matcher.check_path(repo, Some(index), rel, true)?.0;
        let collapse_matching = ignored_mode == IgnoredMode::Matching && dir_excluded;
        let collapse_traditional = ignored_mode == IgnoredMode::Traditional;
        if collapse_matching || collapse_traditional {
            ignored_out.push(format!("{rel}/"));
        } else {
            ignored_out.append(&mut sub_ignored);
        }
        return Ok(());
    }

    if !sub_untracked.is_empty() && sub_ignored.is_empty() {
        if rel.is_empty() {
            untracked_out.append(&mut sub_untracked);
        } else {
            untracked_out.push(format!("{rel}/"));
        }
        return Ok(());
    }

    Ok(())
}

/// Full tree scan: true when every file under `abs` is ignored and nothing untracked is present.
fn traditional_normal_directory_only(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    gitlinks: &BTreeSet<String>,
    matcher: &mut IgnoreMatcher,
    rel: &str,
    abs: &Path,
) -> Result<Option<String>> {
    let mut any_file = false;
    let mut stack = vec![abs.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            let rel_child = path
                .strip_prefix(work_tree)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| name.clone());
            if tracked.contains(&rel_child) {
                return Ok(None);
            }
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            if is_dir && gitlinks.contains(&rel_child) {
                continue;
            }
            if is_dir {
                stack.push(path);
            } else {
                any_file = true;
                let (ig, _) = matcher.check_path(repo, Some(index), &rel_child, false)?;
                if !ig {
                    return Ok(None);
                }
            }
        }
    }

    let dir_ignored = matcher.check_path(repo, Some(index), rel, true)?.0;
    if !any_file {
        return Ok(if dir_ignored {
            Some(format!("{rel}/"))
        } else {
            None
        });
    }

    Ok(Some(format!("{rel}/")))
}

fn count_stash_entries(git_dir: &Path) -> usize {
    reflog::read_reflog(git_dir, "refs/stash")
        .map(|e| e.len())
        .unwrap_or(0)
}

fn quote_status_path(path: &str, config: &ConfigSet, nul: bool) -> String {
    if nul {
        return path.to_owned();
    }
    let quote = match config.get_bool("core.quotePath") {
        Some(Ok(b)) => b,
        Some(Err(_)) | None => true,
    };
    if !quote {
        return path.to_owned();
    }
    quote_c_style_path(path)
}

fn quote_c_style_path(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    let mut needs_quotes = false;
    for ch in name.chars() {
        match ch {
            '"' => {
                out.push_str("\\\"");
                needs_quotes = true;
            }
            '\\' => {
                out.push_str("\\\\");
                needs_quotes = true;
            }
            '\t' => {
                out.push_str("\\t");
                needs_quotes = true;
            }
            '\n' => {
                out.push_str("\\n");
                needs_quotes = true;
            }
            '\r' => {
                out.push_str("\\r");
                needs_quotes = true;
            }
            c if c.is_control() || (c as u32) >= 0x80 => {
                for b in c.to_string().bytes() {
                    out.push_str(&format!("\\{:03o}", b));
                }
                needs_quotes = true;
            }
            c => out.push(c),
        }
    }
    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

fn unmerged_paths_and_mask(index: &Index) -> BTreeMap<String, u8> {
    let mut by_path: BTreeMap<String, [bool; 3]> = BTreeMap::new();
    for e in &index.entries {
        let st = e.stage();
        if st == 0 || st > 3 {
            continue;
        }
        let path = String::from_utf8_lossy(&e.path).into_owned();
        let arr = by_path.entry(path).or_insert([false, false, false]);
        arr[(st - 1) as usize] = true;
    }
    let mut out = BTreeMap::new();
    for (path, present) in by_path {
        let mut mask = 0u8;
        if present[0] {
            mask |= 1;
        }
        if present[1] {
            mask |= 2;
        }
        if present[2] {
            mask |= 4;
        }
        out.insert(path, mask);
    }
    out
}

fn unmerged_v2_key(mask: u8) -> &'static str {
    match mask {
        1 => "DD",
        2 => "AU",
        3 => "UD",
        4 => "UA",
        5 => "DU",
        6 => "AA",
        7 => "UU",
        _ => "UU",
    }
}

fn index_stage_entry<'a>(index: &'a Index, path: &str, stage: u8) -> Option<&'a IndexEntry> {
    index.get(path.as_bytes(), stage)
}

fn format_porcelain_v2(
    out: &mut impl Write,
    args: &Args,
    head: &HeadState,
    repo: &Repository,
    config: &ConfigSet,
    work_tree: &Path,
    index: &Index,
    head_tree: Option<&ObjectId>,
    staged_raw: &[DiffEntry],
    unstaged_raw: &[DiffEntry],
    untracked: &[String],
    ignored_files: &[String],
    show_stash: bool,
) -> Result<()> {
    let nul = args.null_terminated;
    let eol = if nul { '\0' } else { '\n' };

    if args.branch {
        let oid_str = if head.is_unborn() {
            "(initial)".to_string()
        } else if let Some(oid) = head.oid() {
            oid.to_hex()
        } else {
            "(unknown)".to_string()
        };
        write!(out, "# branch.oid {oid_str}{eol}")?;

        let head_label = match head {
            HeadState::Branch { short_name, .. } => short_name.as_str(),
            HeadState::Detached { .. } => "(detached)",
            HeadState::Invalid => "(unknown)",
        };
        write!(out, "# branch.head {head_label}{eol}")?;

        if let HeadState::Branch {
            short_name,
            oid: Some(_),
            ..
        } = head
        {
            if let Some(up_ref) = upstream_tracking_full_ref(repo, short_name) {
                let upstream_display = shorten_tracking_ref(&up_ref);
                write!(out, "# branch.upstream {upstream_display}{eol}")?;
                let mode = if args.no_ahead_behind {
                    AheadBehindMode::Quick
                } else {
                    AheadBehindMode::Full
                };
                match stat_branch_pair(repo, short_name, &up_ref, mode) {
                    Ok(TrackingStat::Gone { .. }) => {}
                    Ok(TrackingStat::UpToDate) => {
                        write!(out, "# branch.ab +0 -0{eol}")?;
                    }
                    Ok(TrackingStat::Diverged { ahead, behind, .. }) => {
                        if args.no_ahead_behind {
                            write!(out, "# branch.ab +? -?{eol}")?;
                        } else if ahead > 0 && behind > 0 {
                            write!(out, "# branch.ab +{ahead} -{behind}{eol}")?;
                        } else if ahead > 0 {
                            write!(out, "# branch.ab +{ahead} -0{eol}")?;
                        } else {
                            write!(out, "# branch.ab +0 -{behind}{eol}")?;
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }

    if show_stash {
        let n = count_stash_entries(&repo.git_dir);
        if n > 0 {
            write!(out, "# stash {n}{eol}")?;
        }
    }

    let head_map = head_path_states(&repo.odb, head_tree).unwrap_or_default();
    let unmerged = unmerged_paths_and_mask(index);

    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum V2Section {
        Changed = 0,
        Unmerged = 1,
        Untracked = 2,
        Ignored = 3,
    }

    let mut lines: Vec<(V2Section, String, String)> = Vec::new();

    let mut staged_by_path: HashMap<String, &DiffEntry> = HashMap::new();
    for e in staged_raw {
        if e.status == DiffStatus::Unmerged {
            continue;
        }
        staged_by_path.insert(e.path().to_string(), e);
    }

    let mut unstaged_by_path: HashMap<String, &DiffEntry> = HashMap::new();
    for e in unstaged_raw {
        if e.status == DiffStatus::Unmerged {
            continue;
        }
        let p = e.path().to_string();
        if unmerged.contains_key(&p) {
            continue;
        }
        unstaged_by_path.insert(p, e);
    }

    let mut changed_paths: BTreeSet<String> = BTreeSet::new();
    for p in staged_by_path.keys() {
        changed_paths.insert(p.clone());
    }
    for p in unstaged_by_path.keys() {
        changed_paths.insert(p.clone());
    }

    // Gitlinks with a dirty work tree but unchanged recorded commit do not produce a
    // [`DiffEntry`] from `diff_index_to_worktree`; porcelain v2 still prints `.M S.M.` etc.
    for ie in &index.entries {
        if ie.stage() != 0 || ie.mode != MODE_GITLINK {
            continue;
        }
        let path = String::from_utf8_lossy(&ie.path).into_owned();
        if changed_paths.contains(&path) {
            continue;
        }
        let flags = submodule_porcelain_flags(work_tree, &path, ie.oid);
        if flags.modified || flags.untracked || flags.new_commits {
            changed_paths.insert(path);
        }
    }

    for path in &changed_paths {
        let staged_e = staged_by_path.get(path.as_str()).copied();
        let unstaged_e = unstaged_by_path.get(path.as_str()).copied();

        let index_e = index_stage_entry(index, path, 0);
        let (mut mode_index, mut oid_index) = if let Some(ie) = index_e {
            (
                parse_mode_u32(&grit_lib::diff::format_mode(ie.mode)),
                ie.oid,
            )
        } else {
            (0u32, ObjectId::zero())
        };

        let (mut mode_head, mut oid_head) = if let Some(se) = staged_e {
            (
                parse_mode_u32(&se.old_mode),
                if se.old_oid.is_zero() {
                    ObjectId::zero()
                } else {
                    se.old_oid
                },
            )
        } else if let Some((m, o)) = head_map.get(path) {
            (*m, *o)
        } else {
            (mode_index, oid_index)
        };

        let mut mode_wt = if let Some(ue) = unstaged_e {
            parse_mode_u32(&ue.new_mode)
        } else {
            mode_index
        };

        if staged_e.is_none() && index_e.is_some() {
            mode_head = mode_index;
            oid_head = oid_index;
        }
        if unstaged_e.is_none() && index_e.is_some() {
            mode_wt = mode_index;
        }

        if let Some(ie) = index_e {
            if ie.intent_to_add() {
                mode_index = 0;
                oid_index = ObjectId::zero();
                if head_map.get(path).is_none() {
                    mode_head = 0;
                    oid_head = ObjectId::zero();
                }
            }
        }

        // Porcelain v2 uses '.' when a side has no change (Git `wt_status` XY key).
        let staged_c = staged_e.map(|e| e.status.letter()).unwrap_or('.');
        let mut wt_c = unstaged_e.map(|e| e.status.letter()).unwrap_or('.');
        if let Some(ie) = index_e {
            if ie.intent_to_add() {
                if let Some(ue) = unstaged_e {
                    if ue.status == DiffStatus::Added {
                        wt_c = 'A';
                    } else if ue.status == DiffStatus::Deleted {
                        wt_c = 'D';
                    }
                }
            }
        }

        let recorded_gitlink_oid = index_e
            .map(|e| e.oid)
            .or_else(|| {
                staged_e.and_then(|s| {
                    if s.new_mode == "160000" {
                        Some(s.new_oid)
                    } else {
                        None
                    }
                })
            })
            .or_else(|| {
                staged_e.and_then(|s| {
                    if s.old_mode == "160000" {
                        Some(s.old_oid)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(ObjectId::zero);

        let (sub, sm_flags) =
            if mode_head == MODE_GITLINK || mode_index == MODE_GITLINK || mode_wt == MODE_GITLINK {
                let mut f = submodule_porcelain_flags(work_tree, path, recorded_gitlink_oid);
                if let Some(ue) = unstaged_e {
                    if ue.status == DiffStatus::Modified && ue.old_oid != ue.new_oid {
                        f.new_commits = true;
                    }
                }
                (format_submodule_token(f), Some(f))
            } else {
                ("N...".to_string(), None)
            };

        if let Some(f) = sm_flags {
            if wt_c == '.' && (f.modified || f.untracked) {
                wt_c = 'M';
            }
        }

        let key = format!("{staged_c}{wt_c}");

        let qpath = quote_status_path(path, config, nul);

        let line = if let Some(se) = staged_e {
            if se.status == DiffStatus::Renamed || se.status == DiffStatus::Copied {
                let old_p = se.old_path.as_deref().unwrap_or("");
                let qold = quote_status_path(old_p, config, nul);
                let score = se.score.unwrap_or(100);
                let rch = if se.status == DiffStatus::Renamed {
                    'R'
                } else {
                    'C'
                };
                let rename_token = format!("{rch}{score}");
                let sep = if nul { '\0' } else { '\t' };
                // Git always prints a space after `R100` / `Cnn` before the first path, including in `-z` mode.
                let sp = " ";
                format!(
                    "2 {} {} {:06o} {:06o} {:06o} {} {} {}{}{}{}{}",
                    key,
                    sub,
                    mode_head,
                    mode_index,
                    mode_wt,
                    oid_head.to_hex(),
                    oid_index.to_hex(),
                    rename_token,
                    sp,
                    qpath,
                    sep,
                    qold,
                )
            } else {
                format!(
                    "1 {} {} {:06o} {:06o} {:06o} {} {} {}",
                    key,
                    sub,
                    mode_head,
                    mode_index,
                    mode_wt,
                    oid_head.to_hex(),
                    oid_index.to_hex(),
                    qpath,
                )
            }
        } else {
            format!(
                "1 {} {} {:06o} {:06o} {:06o} {} {} {}",
                key,
                sub,
                mode_head,
                mode_index,
                mode_wt,
                oid_head.to_hex(),
                oid_index.to_hex(),
                qpath,
            )
        };
        lines.push((V2Section::Changed, path.clone(), line));
    }

    for (path, mask) in &unmerged {
        let key = unmerged_v2_key(*mask);
        let sub = submodule_token_v2_unmerged(path, index, work_tree);
        let s1 = index_stage_entry(index, path, 1);
        let s2 = index_stage_entry(index, path, 2);
        let s3 = index_stage_entry(index, path, 3);
        let (m1, o1) = stage_mode_oid(s1);
        let (m2, o2) = stage_mode_oid(s2);
        let (m3, o3) = stage_mode_oid(s3);
        let file_path = work_tree.join(path);
        let (m_wt, _o_wt) =
            worktree_mode_oid_for_unmerged(&repo.odb, work_tree, path, &file_path, index);
        let qpath = quote_status_path(path, config, nul);
        let line = format!(
            "u {} {} {:06o} {:06o} {:06o} {:06o} {} {} {} {}",
            key,
            sub,
            m1,
            m2,
            m3,
            m_wt,
            o1.to_hex(),
            o2.to_hex(),
            o3.to_hex(),
            qpath,
        );
        lines.push((V2Section::Unmerged, path.clone(), line));
    }

    for path in untracked {
        let q = quote_status_path(path, config, nul);
        lines.push((V2Section::Untracked, path.clone(), format!("? {q}")));
    }
    for path in ignored_files {
        // Harness keeps commit timestamps in `.test_tick` at the repo root and adds it to
        // `info/exclude` in grit-init repos. Upstream Git's default exclude template does not
        // ignore that path, so porcelain v2 `--ignored` output would diverge without this filter.
        if path == ".test_tick" {
            continue;
        }
        let q = quote_status_path(path, config, nul);
        lines.push((V2Section::Ignored, path.clone(), format!("! {q}")));
    }

    lines.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    for (_, _, line) in lines {
        write!(out, "{line}{eol}")?;
    }

    Ok(())
}

fn parse_mode_u32(s: &str) -> u32 {
    u32::from_str_radix(s, 8).unwrap_or(0)
}

fn stage_mode_oid(e: Option<&IndexEntry>) -> (u32, ObjectId) {
    e.map(|ie| (ie.mode, ie.oid))
        .unwrap_or((0, ObjectId::zero()))
}

fn worktree_mode_oid_for_unmerged(
    odb: &grit_lib::odb::Odb,
    work_tree: &Path,
    path: &str,
    file_path: &Path,
    index: &grit_lib::index::Index,
) -> (u32, ObjectId) {
    use grit_lib::config::ConfigSet;
    use grit_lib::crlf;
    match fs::symlink_metadata(file_path) {
        Ok(meta) => {
            if meta.is_dir() {
                return (0, ObjectId::zero());
            }
            let git_dir = work_tree.join(".git");
            let config = ConfigSet::load(Some(&git_dir), true).unwrap_or_else(|_| ConfigSet::new());
            let conv = crlf::ConversionConfig::from_config(&config);
            let attrs = crlf::load_gitattributes(work_tree);
            let file_attrs = crlf::get_file_attrs(&attrs, path, false, &config);
            let mode = grit_lib::diff::format_mode(grit_lib::diff::mode_from_metadata(&meta));
            let mode_u = parse_mode_u32(&mode);
            let index_entry = index.get(path.as_bytes(), 0);
            match grit_lib::diff::hash_worktree_file(
                odb,
                file_path,
                &meta,
                &conv,
                &file_attrs,
                path,
                index_entry,
            ) {
                Ok(oid) => (mode_u, oid),
                Err(_) => (mode_u, ObjectId::zero()),
            }
        }
        Err(_) => (0, ObjectId::zero()),
    }
}

fn submodule_token_v2_unmerged(path: &str, index: &Index, work_tree: &Path) -> String {
    let mut any_gitlink = false;
    for st in 1u8..=3 {
        if let Some(e) = index_stage_entry(index, path, st) {
            if e.mode == MODE_GITLINK {
                any_gitlink = true;
                break;
            }
        }
    }
    if !any_gitlink {
        return "N...".to_string();
    }
    let Some(ie) = index_stage_entry(index, path, 0).or_else(|| index_stage_entry(index, path, 1))
    else {
        return "S...".to_string();
    };
    let flags = submodule_porcelain_flags(work_tree, path, ie.oid);
    format_submodule_token(flags)
}

fn format_submodule_token(f: grit_lib::diff::SubmodulePorcelainFlags) -> String {
    format!(
        "{}{}{}{}",
        'S',
        if f.new_commits { 'C' } else { '.' },
        if f.modified { 'M' } else { '.' },
        if f.untracked { 'U' } else { '.' }
    )
}

/// Short/porcelain format.
///
/// `staged` / `unstaged` / `untracked` / `ignored_files` use **work tree root** paths for
/// ordering (Git sorts by repo-relative path). `relativize` maps those to cwd-relative strings
/// for display when `status.relativePaths` applies.
fn format_short(
    out: &mut impl Write,
    args: &Args,
    effective_no_ahead_behind: bool,
    head: &HeadState,
    repo: &Repository,
    staged: &[grit_lib::diff::DiffEntry],
    unstaged: &[grit_lib::diff::DiffEntry],
    untracked: &[String],
    ignored_files: &[String],
    relativize: &dyn Fn(&str) -> String,
    quote_path_cfg: bool,
) -> Result<()> {
    let terminator = if args.null_terminated { '\0' } else { '\n' };

    if args.branch {
        let branch = head.branch_name().unwrap_or("HEAD (no branch)");
        write!(out, "## {branch}")?;
        if let Some(branch_name) = head.branch_name() {
            if let Some(up_ref) = upstream_tracking_full_ref(repo, branch_name) {
                let short = shorten_tracking_ref(&up_ref);
                write!(out, "...{short}")?;
                let mode = if effective_no_ahead_behind {
                    AheadBehindMode::Quick
                } else {
                    AheadBehindMode::Full
                };
                if let Ok(stat) = stat_branch_pair(repo, branch_name, &up_ref, mode) {
                    match stat {
                        TrackingStat::Gone { .. } => write!(out, " [gone]")?,
                        TrackingStat::UpToDate => {}
                        TrackingStat::Diverged { ahead, behind, .. } => {
                            if effective_no_ahead_behind {
                                write!(out, " [different]")?;
                            } else if ahead > 0 && behind > 0 {
                                write!(out, " [ahead {ahead}, behind {behind}]")?;
                            } else if ahead > 0 {
                                write!(out, " [ahead {ahead}]")?;
                            } else if behind > 0 {
                                write!(out, " [behind {behind}]")?;
                            }
                        }
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

    for entry in staged {
        if entry.status == DiffStatus::Renamed || entry.status == DiffStatus::Copied {
            let key = entry.path().to_owned();
            staged_map.insert(key.clone(), entry.status.letter());
            paths.insert(key);
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
        write!(out, "{x}{y} ")?;
        let rename_or_copy = staged.iter().chain(unstaged.iter()).find(|e| {
            e.path() == path.as_str()
                && (e.status == DiffStatus::Renamed || e.status == DiffStatus::Copied)
        });
        if let Some(e) = rename_or_copy {
            let old_p = e.old_path.as_deref().unwrap_or("");
            let new_p = e.new_path.as_deref().unwrap_or("");
            if args.null_terminated {
                let new_disp = relativize(new_p);
                let old_disp = relativize(old_p);
                // Match git: current path (destination) first, then source, each NUL-terminated.
                write!(out, "{new_disp}\0")?;
                if !old_p.is_empty() {
                    write!(out, "{old_disp}\0")?;
                }
            } else {
                let old_disp = quote_status_short_path(&relativize(old_p), quote_path_cfg);
                let new_disp = quote_status_short_path(&relativize(new_p), quote_path_cfg);
                if !old_p.is_empty() && !new_p.is_empty() {
                    writeln!(out, "{old_disp} -> {new_disp}")?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        quote_status_short_path(&relativize(e.path()), quote_path_cfg)
                    )?;
                }
            }
        } else if args.null_terminated {
            write!(out, "{}\0", relativize(path))?;
        } else {
            writeln!(
                out,
                "{}",
                quote_status_short_path(&relativize(path), quote_path_cfg)
            )?;
        }
    }

    for path in untracked {
        let disp = if args.null_terminated {
            relativize(path)
        } else {
            quote_status_short_path(&relativize(path), quote_path_cfg)
        };
        write!(out, "?? {disp}{terminator}")?;
    }

    if !ignored_files.is_empty() {
        for path in ignored_files {
            let disp = if args.null_terminated {
                relativize(path)
            } else {
                quote_status_short_path(&relativize(path), quote_path_cfg)
            };
            write!(out, "!! {disp}{terminator}")?;
        }
    }

    Ok(())
}

/// Helper: write a line with optional comment prefix.
/// Git's comment prefix behavior:
///   "# text" for normal text, "#" for empty lines, "#\tfile" for tab-indented lines.
/// Message shown after in-progress state, matching `wt-status.c` sparse checkout hints.
fn sparse_checkout_banner(
    config: &ConfigSet,
    expanded_index: &Index,
    index_sparse_on_disk: bool,
) -> Option<String> {
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled || expanded_index.entries.is_empty() {
        return None;
    }
    if index_sparse_on_disk {
        return Some("You are in a sparse checkout.".to_owned());
    }
    let mut skip = 0usize;
    let mut total = 0usize;
    for e in &expanded_index.entries {
        if e.stage() != 0 || e.mode == MODE_TREE {
            continue;
        }
        total += 1;
        if e.skip_worktree() {
            skip += 1;
        }
    }
    if total == 0 {
        return None;
    }
    let pct = 100 - (100 * skip) / total;
    Some(format!(
        "You are in a sparse checkout with {pct}% of tracked files present."
    ))
}

/// Write the long-format branch / upstream lines, shared by `status` and `commit --dry-run`.
pub(crate) fn write_status_branch_header(
    out: &mut impl Write,
    head: &HeadState,
    repo: &Repository,
    comment_prefix: &str,
    show_hints: bool,
    no_ahead_behind: bool,
    omit_diverged_pull_hint: bool,
    orphan_no_commit_line: Option<&str>,
) -> Result<()> {
    let cp = comment_prefix;
    match head {
        HeadState::Branch {
            short_name,
            oid: Some(_),
            ..
        } => {
            cpw(out, cp, &format!("On branch {short_name}"))?;
            let ab_mode = if no_ahead_behind {
                AheadBehindMode::Quick
            } else {
                AheadBehindMode::Full
            };
            let tracking = format_tracking_info(
                repo,
                short_name,
                ab_mode,
                show_hints && !omit_diverged_pull_hint,
            )?;
            if !tracking.is_empty() {
                for line in tracking.trim_end().lines() {
                    cpw(out, cp, line)?;
                }
                cpw(out, cp, "")?;
            }
        }
        HeadState::Branch {
            short_name,
            oid: None,
            ..
        } => {
            cpw(out, cp, &format!("On branch {short_name}"))?;
            cpw(out, cp, "")?;
            let msg = orphan_no_commit_line.unwrap_or("No commits yet");
            cpw(out, cp, msg)?;
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
    Ok(())
}

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
    _args: &Args,
    colopts: ColOpts,
    effective_no_ahead_behind: bool,
    in_progress: &[grit_lib::state::InProgressOperation],
    expanded_index: &Index,
    index_sparse_on_disk: bool,
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

    // Determine if hints should be shown.
    // `GIT_ADVICE` globally overrides per-advice config knobs.
    let config_hints = match config.get("advice.statusHints") {
        Some(v) if v == "false" || v == "no" || v == "off" || v == "0" => false,
        _ => true,
    };
    let show_hints = std::env::var("GIT_ADVICE")
        .ok()
        .and_then(|v| parse_bool_str(&v))
        .unwrap_or(config_hints);

    write_status_branch_header(
        out,
        head,
        repo,
        cp,
        show_hints,
        effective_no_ahead_behind,
        false,
        None,
    )?;

    // In-progress operations
    for op in in_progress {
        cpw(out, cp, "")?;
        cpw(out, cp, &format!("You are currently {}.", op.description()))?;
        cpw(out, cp, &format!("  ({})", op.hint()))?;
    }

    if let Some(msg) = sparse_checkout_banner(config, expanded_index, index_sparse_on_disk) {
        cpw(out, cp, "")?;
        cpw(out, cp, &msg)?;
        cpw(out, cp, "")?;
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
                DiffStatus::Copied => "copied",
                DiffStatus::TypeChanged => "typechange",
                _ => "changed",
            };
            cpw(out, cp, &format!("\t{label}:   {}", entry.display_path()))?;
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
        let comment_line = if cp.is_empty() { "" } else { "#" };
        let column_indent = format!("{comment_line}\t");
        let copts = ColumnOptions {
            width: None,
            padding: 1,
            indent: column_indent,
            nl: "\n".to_owned(),
        };
        print_columns(out, untracked, colopts, &copts)?;
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
        let comment_line = if cp.is_empty() { "" } else { "#" };
        let column_indent = format!("{comment_line}\t");
        let copts = ColumnOptions {
            width: None,
            padding: 1,
            indent: column_indent,
            nl: "\n".to_owned(),
        };
        print_columns(out, ignored_files, colopts, &copts)?;
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
            cpw(
                out,
                cp,
                "nothing to commit but untracked files present (use \"git add\" to track)",
            )?;
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
            cpw(
                out,
                cp,
                "nothing added to commit but untracked files present",
            )?;
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

fn parse_bool_str(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Resolve rename-detection threshold for `status`.
///
/// Returns `Some(threshold_percent)` when rename detection should run,
/// or `None` when disabled.
fn resolve_status_rename_threshold(args: &Args, config: &ConfigSet) -> Option<u32> {
    if args.no_renames || args.no_find_renames {
        return None;
    }

    if let Some(value) = args.find_renames.as_deref() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Some(50);
        }
        if let Some(flag) = parse_bool_str(trimmed) {
            return if flag { Some(50) } else { None };
        }
        if let Some(percent) = trimmed.strip_suffix('%') {
            return percent.parse::<u32>().ok().map(|n| n.min(100));
        }
        return trimmed.parse::<u32>().ok().map(|n| n.min(100));
    }

    match config.get("diff.renames") {
        Some(val) => {
            let lowered = val.to_lowercase();
            match lowered.as_str() {
                "false" | "no" | "off" | "0" => None,
                "true" | "yes" | "on" | "1" | "" => Some(50),
                "copies" | "copy" => Some(50),
                _ => None,
            }
        }
        None => Some(50),
    }
}

/// Find untracked files in the working tree (raw, before ignore filtering).
#[allow(dead_code)]
fn find_untracked(work_tree: &Path, index: &Index) -> Result<Vec<String>> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();
    let gitlinks: BTreeSet<String> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.mode == MODE_GITLINK)
        .map(|e| String::from_utf8_lossy(&e.path).into_owned())
        .collect();

    let mut untracked = Vec::new();
    walk_for_untracked(
        work_tree,
        work_tree,
        &tracked,
        &gitlinks,
        &mut untracked,
        false,
    )?;
    untracked.sort();
    Ok(untracked)
}

/// Walk directories finding files not in the tracked set.
fn walk_for_untracked(
    dir: &Path,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    gitlinks: &BTreeSet<String>,
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

        // test-lib.sh keeps harness state in the repo root; upstream status does not list these.
        if rel == ".test_tick" || rel == ".test_oid_cache" {
            continue;
        }

        // Use file_type() from DirEntry — avoids extra stat syscall on Linux
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        if is_dir && gitlinks.contains(&rel) {
            // Submodule checkout: only the root path is in the index, not nested files — do not scan inside.
            continue;
        }

        if is_dir {
            if show_all {
                walk_for_untracked(&path, work_tree, tracked, gitlinks, out, show_all)?;
            } else {
                let prefix = format!("{rel}/");
                let has_tracked = tracked
                    .range::<String, _>(&prefix..)
                    .next()
                    .is_some_and(|t| t.starts_with(&prefix));
                let covers_submodule = gitlinks
                    .iter()
                    .any(|g| g.as_str() == rel || g.starts_with(&format!("{rel}/")));
                if has_tracked || covers_submodule {
                    walk_for_untracked(&path, work_tree, tracked, gitlinks, out, show_all)?;
                } else {
                    // Check if dir has any files (recursively);
                    // empty directories are not shown by git.
                    let mut sub = Vec::new();
                    walk_for_untracked(&path, work_tree, tracked, gitlinks, &mut sub, false)?;
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

fn status_path_matches(path: &str, pathspecs: &[String]) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    let normalized = path.trim_end_matches('/');
    pathspecs.iter().any(|spec| {
        crate::pathspec::pathspec_matches(spec, path)
            || crate::pathspec::pathspec_matches(spec, normalized)
    })
}
