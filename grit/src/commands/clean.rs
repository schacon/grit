//! `grit clean` — remove untracked files from the working tree.
//!
//! Supports dry-run (`-n`/`--dry-run`), force (`-f`/`--force`),
//! removing directories (`-d`), removing ignored files (`-x`),
//! removing *only* ignored files (`-X`), quiet mode (`-q`/`--quiet`),
//! and pathspec filtering.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::Index;
use grit_lib::repo::Repository;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit clean`.
#[derive(Debug, ClapArgs)]
#[command(about = "Remove untracked files from the working tree")]
pub struct Args {
    /// Don't actually remove anything, just show what would be done.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Required to actually remove files (unless clean.requireForce is false).
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Also remove untracked directories.
    #[arg(short = 'd')]
    pub directories: bool,

    /// Also remove ignored files (remove all untracked files).
    #[arg(short = 'x')]
    pub ignored_too: bool,

    /// Remove only ignored files.
    #[arg(short = 'X')]
    pub ignored_only: bool,

    /// Don't print names of removed files.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Paths to limit the clean operation.
    pub pathspec: Vec<String>,
}

/// Run the `clean` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?
        .to_path_buf();

    // Check force requirement: unless dry-run or clean.requireForce=false,
    // -f/--force is mandatory.
    if !args.dry_run && !args.force {
        let require_force = check_require_force(&repo);
        if require_force {
            bail!(
                "clean.requireForce defaults to true and neither -n nor -f given; \
                 refusing to clean"
            );
        }
    }

    if args.ignored_too && args.ignored_only {
        bail!("-x and -X cannot be used together");
    }

    let index = Index::load(&repo.index_path()).context("failed to read index")?;
    let mut matcher =
        IgnoreMatcher::from_repository(&repo).context("failed to load ignore rules")?;

    let cwd = std::env::current_dir().context("failed to resolve current directory")?;

    // Build tracked set from index.
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    // Resolve pathspec filters to worktree-relative prefixes.
    let pathspec_prefixes: Vec<String> = args
        .pathspec
        .iter()
        .map(|p| resolve_pathspec_prefix(&work_tree, &cwd, p))
        .collect::<Result<Vec<_>>>()?;

    // Collect files/directories to remove.
    let mut to_remove: Vec<(String, bool)> = Vec::new(); // (path, is_dir)
    collect_untracked(
        &work_tree,
        &work_tree,
        &tracked,
        &mut matcher,
        &repo,
        Some(&index),
        &args,
        &pathspec_prefixes,
        &mut to_remove,
    )?;

    to_remove.sort_by(|a, b| a.0.cmp(&b.0));

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (path, is_dir) in &to_remove {
        if !args.quiet {
            let prefix = if args.dry_run { "Would remove" } else { "Removing" };
            if *is_dir {
                writeln!(out, "{prefix} {path}/")?;
            } else {
                writeln!(out, "{prefix} {path}")?;
            }
        }

        if !args.dry_run {
            let abs = work_tree.join(path);
            if *is_dir {
                fs::remove_dir_all(&abs)
                    .with_context(|| format!("failed to remove directory '{path}'"))?;
            } else {
                fs::remove_file(&abs)
                    .with_context(|| format!("failed to remove file '{path}'"))?;
                remove_empty_parents(&abs, &work_tree);
            }
        }
    }

    out.flush()?;
    Ok(())
}

/// Check whether clean.requireForce is set. Defaults to true.
fn check_require_force(repo: &Repository) -> bool {
    let config = match ConfigSet::load(Some(&repo.git_dir), true) {
        Ok(c) => c,
        Err(_) => return true,
    };
    match config.get_bool("clean.requireforce") {
        Some(Ok(val)) => val,
        _ => true, // default is true
    }
}

/// Walk the working tree collecting untracked files/directories.
fn collect_untracked(
    dir: &Path,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    matcher: &mut IgnoreMatcher,
    repo: &Repository,
    index: Option<&Index>,
    args: &Args,
    pathspec_prefixes: &[String],
    out: &mut Vec<(String, bool)>,
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

        // Pathspec filtering: if pathspecs given, only consider paths that
        // match at least one prefix.
        if !pathspec_prefixes.is_empty()
            && !pathspec_prefixes
                .iter()
                .any(|prefix| rel.starts_with(prefix) || prefix.starts_with(&rel))
        {
            continue;
        }

        let is_dir = path.is_dir();

        if is_dir {
            // Check if any tracked file is inside this directory.
            let prefix = format!("{rel}/");
            let has_tracked = tracked.iter().any(|t| t.starts_with(&prefix));

            // Also check if a pathspec targets something inside this dir,
            // in which case we must recurse.
            let pathspec_wants_recurse = !pathspec_prefixes.is_empty()
                && pathspec_prefixes.iter().any(|ps| {
                    ps.starts_with(&prefix) || ps == &rel
                });

            if has_tracked || pathspec_wants_recurse {
                // Directory contains tracked files or pathspec targets
                // something inside — recurse into it.
                collect_untracked(
                    &path,
                    work_tree,
                    tracked,
                    matcher,
                    repo,
                    index,
                    args,
                    pathspec_prefixes,
                    out,
                )?;
            } else if args.directories {
                if args.ignored_only || args.ignored_too {
                    // -X/-x with -d: recurse into untracked dirs.
                    // -X finds individual ignored files;
                    // -x removes the whole directory.
                    if args.ignored_too {
                        out.push((rel, true));
                    } else {
                        collect_untracked(
                            &path,
                            work_tree,
                            tracked,
                            matcher,
                            repo,
                            index,
                            args,
                            pathspec_prefixes,
                            out,
                        )?;
                    }
                } else {
                    // Default -d: directory has a mix of ignored and
                    // non-ignored files. Check if all contents are
                    // ignored; if so skip the dir, otherwise recurse
                    // to collect individual non-ignored files.
                    let all_ignored = dir_all_ignored(
                        &path, work_tree, matcher, repo, index,
                    )?;
                    if all_ignored {
                        // Entire directory is ignored — skip it.
                    } else {
                        // Mixed content: recurse to pick out non-ignored files.
                        collect_untracked(
                            &path,
                            work_tree,
                            tracked,
                            matcher,
                            repo,
                            index,
                            args,
                            pathspec_prefixes,
                            out,
                        )?;
                    }
                }
            } else if args.ignored_only {
                // -X without -d: still recurse to find ignored files.
                collect_untracked(
                    &path,
                    work_tree,
                    tracked,
                    matcher,
                    repo,
                    index,
                    args,
                    pathspec_prefixes,
                    out,
                )?;
            }
            // Without -d (and not -X), skip untracked directories entirely.
        } else {
            // File: check if tracked.
            if tracked.contains(&rel) {
                continue;
            }

            let should_include = should_include_path(
                matcher, repo, index, &rel, false, args,
            )?;
            if should_include {
                out.push((rel, false));
            }
        }
    }

    Ok(())
}

/// Determine whether a path should be included in the clean list based on
/// ignore status and the -x/-X flags.
fn should_include_path(
    matcher: &mut IgnoreMatcher,
    repo: &Repository,
    index: Option<&Index>,
    rel_path: &str,
    is_dir: bool,
    args: &Args,
) -> Result<bool> {
    let (ignored, _) = matcher
        .check_path(repo, index, rel_path, is_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if args.ignored_only {
        // -X: only remove ignored files.
        Ok(ignored)
    } else if args.ignored_too {
        // -x: remove everything untracked (ignored or not).
        Ok(true)
    } else {
        // Default: only remove non-ignored untracked files.
        Ok(!ignored)
    }
}

/// Resolve a pathspec to a worktree-relative prefix string.
fn resolve_pathspec_prefix(
    work_tree: &Path,
    cwd: &Path,
    pathspec: &str,
) -> Result<String> {
    let p = Path::new(pathspec);
    if p.is_absolute() {
        let rel = p
            .strip_prefix(work_tree)
            .map_err(|_| anyhow::anyhow!("path '{}' is outside the work tree", pathspec))?;
        return Ok(rel.to_string_lossy().into_owned());
    }

    let abs = cwd.join(pathspec);
    let wt_canon = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.to_path_buf());
    let abs_canon = abs
        .canonicalize()
        .unwrap_or(abs);

    if let Ok(rel) = abs_canon.strip_prefix(&wt_canon) {
        return Ok(rel.to_string_lossy().into_owned());
    }

    // Fallback: treat as relative to worktree root.
    Ok(pathspec.to_owned())
}

/// Check whether all files in a directory are ignored.
fn dir_all_ignored(
    dir: &Path,
    work_tree: &Path,
    matcher: &mut IgnoreMatcher,
    repo: &Repository,
    index: Option<&Index>,
) -> Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(true),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }

        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(name);

        let is_dir = path.is_dir();
        let (ignored, _) = matcher
            .check_path(repo, index, &rel, is_dir)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if !ignored {
            if is_dir {
                let sub_all = dir_all_ignored(&path, work_tree, matcher, repo, index)?;
                if !sub_all {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Remove empty parent directories up to (but not including) the worktree root.
fn remove_empty_parents(file: &Path, work_tree: &Path) {
    let mut current = file.parent();
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
