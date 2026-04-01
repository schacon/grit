//! `grit rm` — remove files from the index and working tree.
//!
//! Supports removing files from the index only (`--cached`), recursive
//! removal (`-r`), forced removal of modified files (`-f`/`--force`),
//! dry-run mode (`-n`/`--dry-run`), and quiet mode (`-q`/`--quiet`).

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::zero_oid;
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::repo::Repository;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Arguments for `grit rm`.
#[derive(Debug, ClapArgs)]
#[command(about = "Remove files from the working tree and from the index")]
pub struct Args {
    /// Files to remove.
    #[arg(required = true)]
    pub pathspec: Vec<String>,

    /// Only remove from the index; keep the working tree file.
    #[arg(long = "cached")]
    pub cached: bool,

    /// Override the up-to-date check; allow removing files with local changes.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Allow recursive removal when a leading directory name is given.
    #[arg(short = 'r')]
    pub recursive: bool,

    /// Dry run — show what would be removed without doing it.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Suppress the `rm 'file'` output message.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Exit with zero status even if no files matched.
    #[arg(long = "ignore-unmatch")]
    pub ignore_unmatch: bool,
}

/// Run the `rm` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Build a map of path → HEAD OID for safety checks.
    let head_tree_map = build_head_map(&repo)?;

    // Phase 1: collect all index paths to remove and check safety.
    let mut to_remove: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for pathspec in &args.pathspec {
        let rel = resolve_rel(pathspec, work_tree)?;
        let abs_path = work_tree.join(&rel);

        // Collect matching index entries (by prefix for directories).
        let matches: Vec<String> = index
            .entries
            .iter()
            .filter(|e| {
                let p = String::from_utf8_lossy(&e.path);
                p == rel || p.starts_with(&format!("{rel}/"))
            })
            .map(|e| String::from_utf8_lossy(&e.path).into_owned())
            .collect();

        if matches.is_empty() {
            if args.ignore_unmatch {
                continue;
            }
            bail!("pathspec '{}' did not match any files", pathspec);
        }

        // Require -r for directories.
        if !args.recursive {
            for m in &matches {
                if Path::new(m) != Path::new(&rel) {
                    bail!("not removing '{}' recursively without -r", pathspec);
                }
            }
            if abs_path.is_dir() && !matches.is_empty() {
                bail!("not removing '{}' recursively without -r", pathspec);
            }
        }

        for path_str in matches {
            match safety_check(
                &index,
                &repo.odb,
                work_tree,
                &path_str,
                &head_tree_map,
                &args,
            ) {
                Ok(()) => to_remove.push(path_str),
                Err(msg) => errors.push(msg),
            }
        }
    }

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("error: {e}");
        }
        bail!("some files could not be removed");
    }

    // Phase 2: perform all removals (only reached when all checks passed).
    for path_str in &to_remove {
        if args.dry_run {
            if !args.quiet {
                println!("rm '{path_str}'");
            }
            continue;
        }

        if !args.cached {
            let abs_path = work_tree.join(path_str);
            if abs_path.exists() || abs_path.symlink_metadata().is_ok() {
                if let Err(e) = fs::remove_file(&abs_path) {
                    bail!("cannot remove '{path_str}': {e}");
                }
                remove_empty_parents(&abs_path, work_tree);
            }
        }

        index.remove(path_str.as_bytes());

        if !args.quiet {
            println!("rm '{path_str}'");
        }
    }

    if !args.dry_run && !to_remove.is_empty() {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

/// Check whether a single file can be safely removed.
///
/// Returns `Err(message)` when removal is refused without `--force`.
fn safety_check(
    index: &Index,
    odb: &grit_lib::odb::Odb,
    work_tree: &Path,
    path_str: &str,
    head_map: &HashMap<String, grit_lib::objects::ObjectId>,
    args: &Args,
) -> std::result::Result<(), String> {
    if args.force {
        return Ok(());
    }

    let path_bytes = path_str.as_bytes();
    let entry = match index.get(path_bytes, 0) {
        Some(e) => e,
        None => return Ok(()),
    };

    let index_oid = entry.oid;
    let is_intent_to_add = index_oid == zero_oid();

    if is_intent_to_add {
        // Intent-to-add entries: only allow removal with --cached.
        if !args.cached {
            return Err(format!(
                "'{path_str}' has changes staged in the index\n\
                 (use --cached to keep the file, or -f to force removal)"
            ));
        }
        return Ok(());
    }

    let head_oid = head_map.get(path_str);

    // index differs from HEAD.
    let staged_differs = match head_oid {
        None => true,
        Some(h) => h != &index_oid,
    };

    // working tree differs from index.
    let abs_path = work_tree.join(path_str);
    let worktree_differs = if abs_path.exists() {
        worktree_differs_from_index(odb, &abs_path, &index_oid).unwrap_or(false)
    } else {
        false
    };

    if args.cached {
        // --cached: refuse only when index matches neither HEAD nor worktree file.
        if staged_differs && worktree_differs {
            return Err(format!(
                "'{path_str}' has staged content different from both \
                 the file and the HEAD\n\
                 (use -f to force removal)"
            ));
        }
    } else {
        // Full removal: refuse if index differs from HEAD or file differs from index.
        if staged_differs && worktree_differs {
            return Err(format!(
                "'{path_str}' has staged content different from both \
                 the file and the HEAD\n\
                 (use -f to force removal)"
            ));
        }
        if staged_differs {
            return Err(format!(
                "'{path_str}' has changes staged in the index\n\
                 (use --cached to keep the file, or -f to force removal)"
            ));
        }
        if worktree_differs {
            return Err(format!(
                "'{path_str}' has local modifications\n\
                 (use --cached to keep the file, or -f to force removal)"
            ));
        }
    }

    Ok(())
}

/// Returns `true` if the working tree file content differs from the index OID.
fn worktree_differs_from_index(
    _odb: &grit_lib::odb::Odb,
    abs_path: &Path,
    index_oid: &grit_lib::objects::ObjectId,
) -> Result<bool> {
    let data = fs::read(abs_path)?;
    let wt_oid = grit_lib::odb::Odb::hash_object_data(ObjectKind::Blob, &data);
    Ok(wt_oid != *index_oid)
}

/// Build a map from repo-relative path string to HEAD tree OID.
fn build_head_map(repo: &Repository) -> Result<HashMap<String, grit_lib::objects::ObjectId>> {
    let head = grit_lib::state::resolve_head(&repo.git_dir)?;
    let commit_oid = match head.oid() {
        Some(o) => o,
        None => return Ok(HashMap::new()),
    };
    let commit_obj = repo.odb.read(commit_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    flatten_tree_to_map(&repo.odb, &commit.tree, "")
}

/// Recursively flatten a tree into a path→OID map.
fn flatten_tree_to_map(
    odb: &grit_lib::odb::Odb,
    tree_oid: &grit_lib::objects::ObjectId,
    prefix: &str,
) -> Result<HashMap<String, grit_lib::objects::ObjectId>> {
    let obj = odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;
    let mut map = HashMap::new();

    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };

        if entry.mode == 0o040000 {
            let nested = flatten_tree_to_map(odb, &entry.oid, &path)?;
            map.extend(nested);
        } else {
            map.insert(path, entry.oid);
        }
    }

    Ok(map)
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

/// Resolve a user-supplied pathspec to a worktree-relative path string.
///
/// Handles paths supplied from outside the worktree by stripping the
/// worktree prefix when present.
fn resolve_rel(pathspec: &str, work_tree: &Path) -> Result<String> {
    let p = Path::new(pathspec);
    if p.is_absolute() {
        let rel = p
            .strip_prefix(work_tree)
            .map_err(|_| anyhow::anyhow!("path '{}' is outside the work tree", pathspec))?;
        return Ok(rel.to_string_lossy().into_owned());
    }

    let cwd = std::env::current_dir()?;
    let abs = cwd.join(pathspec);
    let wt_canon = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.to_path_buf());

    if let Ok(rel) = abs.strip_prefix(&wt_canon) {
        return Ok(rel.to_string_lossy().into_owned());
    }

    // Fallback: already relative to worktree root.
    Ok(pathspec.to_owned())
}
