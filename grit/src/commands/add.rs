//! `grit add` — add file contents to the index.
//!
//! Stages files from the working tree into the index so they will be
//! included in the next commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::error::Error;
use grit_lib::diff::stat_matches;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{entry_from_metadata, normalize_mode, Index, IndexEntry};
#[allow(unused_imports)]
use grit_lib::objects::ObjectId;
use grit_lib::objects::ObjectKind;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Arguments for `grit add`.
#[derive(Debug, ClapArgs)]
#[command(about = "Add file contents to the index")]
pub struct Args {
    /// Files to add. Use '.' to add everything.
    pub pathspec: Vec<String>,

    /// Update tracked files (don't add new files).
    #[arg(short = 'u', long = "update")]
    pub update: bool,

    /// Add, modify, and remove index entries to match the working tree.
    #[arg(short = 'A', long = "all", alias = "no-ignore-removal")]
    pub all: bool,

    /// Record only the intent to add a path (placeholder entry).
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Dry run — show what would be added.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Be verbose.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Allow adding otherwise ignored files.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Interactive patch mode.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Interactive add mode.
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Edit the diff vs. the index before staging.
    #[arg(short = 'e', long = "edit")]
    pub edit: bool,

    /// Override the file mode for the added files (+x or -x).
    #[arg(long = "chmod")]
    pub chmod: Option<String>,

    /// Renormalize tracked files (apply clean/smudge filters).
    #[arg(long = "renormalize")]
    pub renormalize: bool,

    /// Refresh stat info in the index without changing content.
    #[arg(long = "refresh")]
    pub refresh: bool,

    /// Continue adding files when some cannot be added.
    #[arg(long = "ignore-errors")]
    pub ignore_errors: bool,

    /// Suppress warning for non-existent pathspecs (with --refresh).
    #[arg(long = "ignore-missing")]
    pub ignore_missing: bool,
}

/// Run the `add` command.
pub fn run(args: Args) -> Result<()> {
    // Stubs for unsupported interactive modes
    if args.patch {
        bail!("patch mode (add -p) is not yet supported");
    }
    if args.interactive {
        bail!("interactive mode (add -i) is not yet supported");
    }
    if args.edit {
        bail!("edit mode (add -e) is not yet supported");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let core_filemode = config
        .get_bool("core.filemode")
        .and_then(|r| r.ok())
        .unwrap_or(true);

    let mut index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    let odb = &repo.odb;

    // Resolve the current working directory relative to the worktree
    let cwd = std::env::current_dir()?;
    let prefix = pathdiff(&cwd, work_tree);

    // Validate empty string pathspecs
    for ps in &args.pathspec {
        if ps.is_empty() {
            bail!("invalid path ''");
        }
    }

    // "git add" with no pathspecs and no flags: give advice
    if args.pathspec.is_empty() && !args.all && !args.update && !args.refresh && args.chmod.is_none() {
        eprintln!("Nothing specified, nothing added.");
        eprintln!("hint: Maybe you wanted to say 'git add .'?");
        eprintln!("hint: Disable this message with \"git config set advice.addEmptyPathspec false\"");
        return Ok(());
    }

    // --refresh mode
    if args.refresh {
        return run_refresh(&repo, &mut index, work_tree, prefix.as_deref(), &args);
    }

    // --chmod with no pathspecs: do nothing (don't error, just return)
    if args.chmod.is_some() && args.pathspec.is_empty() {
        if !args.dry_run {
            index.write(&repo.index_path())?;
        }
        return Ok(());
    }

    // Build ignore matcher if needed (not needed with --force)
    let mut ignore_matcher = if !args.force {
        Some(IgnoreMatcher::from_repository(&repo)?)
    } else {
        None
    };

    let add_cfg = AddConfig {
        core_filemode,
        ignore_errors: args.ignore_errors || config.get_bool("add.ignore-errors").and_then(|r| r.ok()).unwrap_or(false),
    };

    if args.all || args.pathspec.iter().any(|p| p == ".") {
        add_all(odb, &mut index, work_tree, prefix.as_deref(), &args, &repo, &mut ignore_matcher, &add_cfg)?;
    } else if args.update {
        update_tracked(odb, &mut index, work_tree, prefix.as_deref(), &args, &add_cfg)?;
    } else {
        let mut had_errors = false;
        let mut had_ignored = false;
        for pathspec in &args.pathspec {
            let resolved = resolve_pathspec(pathspec, work_tree, prefix.as_deref());
            match add_path(odb, &mut index, work_tree, &resolved, &args, &repo, &mut ignore_matcher, &add_cfg) {
                Ok(()) => {}
                Err(AddPathError::Ignored(msg)) => {
                    eprintln!("{msg}");
                    had_ignored = true;
                    had_errors = true;
                }
                Err(AddPathError::IoError(e)) => {
                    if add_cfg.ignore_errors {
                        eprintln!("warning: {e}");
                        had_errors = true;
                    } else {
                        // Write index even on error if we've done partial work
                        return Err(e);
                    }
                }
                Err(AddPathError::Other(e)) => {
                    if add_cfg.ignore_errors {
                        eprintln!("warning: {e}");
                        had_errors = true;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        if had_ignored {
            if !args.dry_run {
                index.write(&repo.index_path())?;
            }
            bail!("some ignored files could not be added");
        }
        if had_errors && !add_cfg.ignore_errors {
            if !args.dry_run {
                index.write(&repo.index_path())?;
            }
            bail!("adding files failed");
        }
    }

    if !args.dry_run {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

struct AddConfig {
    core_filemode: bool,
    ignore_errors: bool,
}

enum AddPathError {
    Ignored(String),
    IoError(anyhow::Error),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for AddPathError {
    fn from(e: anyhow::Error) -> Self {
        AddPathError::Other(e)
    }
}

/// Run --refresh: update stat info in the index.
fn run_refresh(
    repo: &Repository,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
) -> Result<()> {
    if args.pathspec.is_empty() {
        // Refresh all entries
        for ie in &mut index.entries {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            if let Some(p) = prefix {
                if !path_str.starts_with(p) {
                    continue;
                }
            }
            let abs_path = work_tree.join(&path_str);
            if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                // Update stat fields but keep oid/mode
                ie.ctime_sec = meta.ctime() as u32;
                ie.ctime_nsec = meta.ctime_nsec() as u32;
                ie.mtime_sec = meta.mtime() as u32;
                ie.mtime_nsec = meta.mtime_nsec() as u32;
                ie.dev = meta.dev() as u32;
                ie.ino = meta.ino() as u32;
                ie.uid = meta.uid();
                ie.gid = meta.gid();
                ie.size = meta.len() as u32;
            }
        }
    } else {
        for pathspec in &args.pathspec {
            let resolved = resolve_pathspec(pathspec, work_tree, prefix);
            let found = index.entries.iter_mut().any(|ie| {
                let path_str = String::from_utf8_lossy(&ie.path);
                if path_str == resolved || path_str.starts_with(&format!("{resolved}/")) {
                    let abs_path = work_tree.join(path_str.as_ref());
                    if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                        ie.ctime_sec = meta.ctime() as u32;
                        ie.ctime_nsec = meta.ctime_nsec() as u32;
                        ie.mtime_sec = meta.mtime() as u32;
                        ie.mtime_nsec = meta.mtime_nsec() as u32;
                        ie.dev = meta.dev() as u32;
                        ie.ino = meta.ino() as u32;
                        ie.uid = meta.uid();
                        ie.gid = meta.gid();
                        ie.size = meta.len() as u32;
                    }
                    true
                } else {
                    false
                }
            });
            if !found && !args.ignore_missing {
                bail!("pathspec '{}' did not match any files", pathspec);
            }
        }
    }

    if !args.dry_run {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

/// Add all files under the working tree (or a prefix) to the index.
fn add_all(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> Result<()> {
    let scan_root = match prefix {
        Some(p) if !p.is_empty() => work_tree.join(p),
        _ => work_tree.to_path_buf(),
    };

    let mut paths = Vec::new();
    walk_directory(&scan_root, work_tree, &mut paths, repo, ignore_matcher, args.force)?;

    // Build a set of worktree paths for fast deletion detection
    let worktree_paths: std::collections::HashSet<&str> =
        paths.iter().map(|s| s.as_str()).collect();

    for rel_path in &paths {
        let abs_path = work_tree.join(rel_path);
        if let Err(e) = stage_file(odb, index, work_tree, rel_path, &abs_path, args, add_cfg) {
            if add_cfg.ignore_errors {
                eprintln!("warning: {e}");
            } else {
                return Err(e);
            }
        }
    }

    // Handle deletions: index entries whose files are not in the worktree scan
    let prefix_bytes = prefix.map(|p| p.as_bytes());
    let removed: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|ie| {
            if let Some(pb) = prefix_bytes {
                if !ie.path.starts_with(pb) {
                    return false;
                }
            }
            let path_str = std::str::from_utf8(&ie.path).unwrap_or("");
            !worktree_paths.contains(path_str)
        })
        .map(|ie| ie.path.clone())
        .collect();

    for path in removed {
        if args.verbose {
            let path_str = String::from_utf8_lossy(&path);
            eprintln!("remove '{path_str}'");
        }
        if !args.dry_run {
            index.remove(&path);
        }
    }

    Ok(())
}

/// Update only already-tracked files.
fn update_tracked(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
    add_cfg: &AddConfig,
) -> Result<()> {
    let tracked: Vec<(Vec<u8>, String)> = index
        .entries
        .iter()
        .filter(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path);
            prefix.map(|p| path_str.starts_with(p)).unwrap_or(true)
        })
        .map(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            (ie.path.clone(), path_str)
        })
        .collect();

    for (raw_path, path_str) in &tracked {
        let abs_path = work_tree.join(path_str);
        if abs_path.exists() {
            stage_file(odb, index, work_tree, path_str, &abs_path, args, add_cfg)?;
        } else {
            if args.verbose {
                eprintln!("remove '{path_str}'");
            }
            if !args.dry_run {
                index.remove(raw_path);
            }
        }
    }

    Ok(())
}

/// Add a single pathspec (which may be a file or directory).
fn add_path(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    path: &str,
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> std::result::Result<(), AddPathError> {
    let abs_path = work_tree.join(path);

    if !abs_path.exists() {
        let path_bytes = path.as_bytes();
        // Check if it's an index entry that needs to be removed
        if index.get(path_bytes, 0).is_some() {
            if !args.dry_run {
                index.remove(path_bytes);
            }
            if args.verbose {
                eprintln!("remove '{path}'");
            }
            return Ok(());
        }
        // Check unmerged entries (stages 1, 2, 3)
        let has_unmerged = (1..=3).any(|stage| index.get(path_bytes, stage).is_some());
        if has_unmerged {
            // Can't resolve a conflict if file doesn't exist
            return Err(AddPathError::Other(anyhow::anyhow!("pathspec '{}' did not match any files", path)));
        }
        return Err(AddPathError::Other(anyhow::anyhow!("pathspec '{}' did not match any files", path)));
    }

    // Use symlink_metadata so symlinks to directories are staged as
    // symlinks, not traversed.
    let is_real_dir = fs::symlink_metadata(&abs_path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if is_real_dir {
        let mut paths = Vec::new();
        walk_directory(&abs_path, work_tree, &mut paths, repo, ignore_matcher, args.force)?;
        for rel_path in &paths {
            let file_abs = work_tree.join(rel_path);
            if let Err(e) = stage_file(odb, index, work_tree, rel_path, &file_abs, args, add_cfg) {
                if add_cfg.ignore_errors {
                    eprintln!("warning: {e}");
                } else {
                    return Err(AddPathError::IoError(e));
                }
            }
        }
    } else {
        // Check if file is ignored
        if let Some(matcher) = ignore_matcher.as_mut() {
            let (ignored, _match_info) = matcher.check_path(repo, Some(&*index), path, false)
                .map_err(|e| AddPathError::Other(e.into()))?;
            if ignored {
                // Check if there are unmerged entries for this path - allow adding to resolve conflicts
                let has_unmerged = (1..=3).any(|stage| index.get(path.as_bytes(), stage).is_some());
                if !has_unmerged {
                    return Err(AddPathError::Ignored(format!(
                        "The following paths are ignored by one of your .gitignore files:\n{path}\nhint: Use -f if you really want to add them.\nhint: Disable this message with \"git config set advice.addIgnoredFile false\""
                    )));
                }
            }
        }
        stage_file(odb, index, work_tree, path, &abs_path, args, add_cfg)
            .map_err(AddPathError::IoError)?;
    }

    Ok(())
}

/// Stage a single file into the index.
fn stage_file(
    odb: &Odb,
    index: &mut Index,
    _work_tree: &Path,
    rel_path: &str,
    abs_path: &Path,
    args: &Args,
    add_cfg: &AddConfig,
) -> Result<()> {
    if args.dry_run {
        if args.chmod.is_some() {
            // Don't actually stage, just check if the file exists
            return Ok(());
        }
        eprintln!("add '{rel_path}'");
        return Ok(());
    }

    let meta = fs::symlink_metadata(abs_path)?;

    if args.intent_to_add {
        let mode = if meta.file_type().is_symlink() {
            0o120000
        } else if add_cfg.core_filemode {
            normalize_mode(meta.mode())
        } else {
            0o100644 // When core.filemode=false, default to regular
        };
        let entry = IndexEntry {
            ctime_sec: meta.ctime() as u32,
            ctime_nsec: meta.ctime_nsec() as u32,
            mtime_sec: meta.mtime() as u32,
            mtime_nsec: meta.mtime_nsec() as u32,
            dev: meta.dev() as u32,
            ino: meta.ino() as u32,
            mode,
            uid: meta.uid(),
            gid: meta.gid(),
            size: 0,
            oid: grit_lib::diff::zero_oid(),
            flags: rel_path.len().min(0xFFF) as u16,
            flags_extended: None,
            path: rel_path.as_bytes().to_vec(),
        };
        index.add_or_replace(entry);
        if args.verbose {
            eprintln!("add '{rel_path}'");
        }
        return Ok(());
    }

    // Determine mode
    let is_symlink = meta.file_type().is_symlink();
    let mode = if is_symlink {
        0o120000
    } else if add_cfg.core_filemode {
        normalize_mode(meta.mode())
    } else {
        // core.filemode=false: preserve existing mode from index if any,
        // otherwise default to 100644
        // Check for unmerged entries: prefer higher stages for mode
        let existing_mode = index.get(rel_path.as_bytes(), 0)
            .or_else(|| index.get(rel_path.as_bytes(), 2))
            .or_else(|| index.get(rel_path.as_bytes(), 1))
            .map(|e| e.mode);
        existing_mode.unwrap_or(0o100644)
    };

    // Handle --chmod flag
    let final_mode = if let Some(ref chmod_val) = args.chmod {
        if is_symlink {
            let display_path = rel_path;
            eprintln!("warning: cannot chmod {} '{}'", chmod_val, display_path);
            return Err(anyhow::anyhow!("cannot chmod {} '{}'", chmod_val, display_path));
        }
        match chmod_val.as_str() {
            "+x" => 0o100755,
            "-x" => 0o100644,
            other => bail!("unrecognized --chmod value: {}", other),
        }
    } else {
        mode
    };

    // Skip if index already has this file with matching stat data and no chmod override
    if args.chmod.is_none() {
        if let Some(existing) = index.get(rel_path.as_bytes(), 0) {
            if stat_matches(existing, &meta) && existing.mode == final_mode {
                return Ok(());
            }
        }
    }

    // Read file content and hash it
    let data = if is_symlink {
        let target = fs::read_link(abs_path)?;
        target.to_string_lossy().into_owned().into_bytes()
    } else {
        fs::read(abs_path)?
    };

    let oid = odb.write(ObjectKind::Blob, &data)?;
    let mut entry = entry_from_metadata(&meta, rel_path.as_bytes(), oid, final_mode);
    entry.mode = final_mode; // Ensure mode override sticks
    index.add_or_replace(entry);

    if args.verbose {
        eprintln!("add '{rel_path}'");
    }

    Ok(())
}

/// Recursively walk a directory, collecting relative paths (skipping .git and ignored files).
fn walk_directory(
    dir: &Path,
    work_tree: &Path,
    out: &mut Vec<String>,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    force: bool,
) -> Result<()> {
    let entries = fs::read_dir(dir)?;
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str == ".git" {
            continue;
        }

        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());

        // Use symlink_metadata to detect symlinks *before* following them.
        // A symlink to a directory should be stored as a symlink blob,
        // not traversed into.
        let ft = match fs::symlink_metadata(&path) {
            Ok(m) => m.file_type(),
            Err(_) => continue,
        };
        let is_symlink = ft.is_symlink();
        let is_dir = !is_symlink && ft.is_dir();

        // Check if ignored
        if !force {
            if let Some(matcher) = ignore_matcher.as_mut() {
                if let Ok((ignored, _)) = matcher.check_path(repo, None, &rel, is_dir) {
                    if ignored {
                        continue;
                    }
                }
            }
        }

        if is_dir {
            walk_directory(&path, work_tree, out, repo, ignore_matcher, force)?;
        } else {
            out.push(rel);
        }
    }

    Ok(())
}

/// Compute path relative to work tree from cwd.
fn pathdiff(cwd: &Path, work_tree: &Path) -> Option<String> {
    let cwd_canon = cwd.canonicalize().ok()?;
    let wt_canon = work_tree.canonicalize().ok()?;

    if cwd_canon == wt_canon {
        return None;
    }

    cwd_canon
        .strip_prefix(&wt_canon)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Resolve a pathspec relative to the prefix (cwd within worktree).
fn resolve_pathspec(pathspec: &str, _work_tree: &Path, prefix: Option<&str>) -> String {
    if pathspec == "." {
        return prefix.unwrap_or("").to_owned();
    }

    match prefix {
        Some(p) if !p.is_empty() => {
            let combined = PathBuf::from(p).join(pathspec);
            combined.to_string_lossy().to_string()
        }
        _ => pathspec.to_owned(),
    }
}
