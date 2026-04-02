//! `grit add` — add file contents to the index.
//!
//! Stages files from the working tree into the index so they will be
//! included in the next commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::error::Error;
use grit_lib::diff::stat_matches;
use grit_lib::index::{entry_from_stat, normalize_mode, Index, IndexEntry};
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
    #[arg(required_unless_present_any = ["update", "all"])]
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
}

/// Run the `add` command.
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

    let odb = &repo.odb;

    // Resolve the current working directory relative to the worktree
    let cwd = std::env::current_dir()?;
    let prefix = pathdiff(&cwd, work_tree);

    if args.all || args.pathspec.iter().any(|p| p == ".") {
        add_all(odb, &mut index, work_tree, prefix.as_deref(), &args)?;
    } else if args.update {
        update_tracked(odb, &mut index, work_tree, prefix.as_deref(), &args)?;
    } else {
        for pathspec in &args.pathspec {
            let resolved = resolve_pathspec(pathspec, work_tree, prefix.as_deref());
            add_path(odb, &mut index, work_tree, &resolved, &args)?;
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
) -> Result<()> {
    let scan_root = match prefix {
        Some(p) if !p.is_empty() => work_tree.join(p),
        _ => work_tree.to_path_buf(),
    };

    let mut paths = Vec::new();
    walk_directory(&scan_root, work_tree, &mut paths)?;

    for rel_path in &paths {
        let abs_path = work_tree.join(rel_path);
        stage_file(odb, index, work_tree, rel_path, &abs_path, args)?;
    }

    // Handle deletions: index entries whose files no longer exist
    let removed: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path);
            let abs = work_tree.join(path_str.as_ref());
            !abs.exists() && prefix.map(|p| path_str.starts_with(p)).unwrap_or(true)
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
            stage_file(odb, index, work_tree, path_str, &abs_path, args)?;
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
fn add_path(odb: &Odb, index: &mut Index, work_tree: &Path, path: &str, args: &Args) -> Result<()> {
    let abs_path = work_tree.join(path);

    if !abs_path.exists() {
        let path_bytes = path.as_bytes();
        if index.get(path_bytes, 0).is_some() {
            if !args.dry_run {
                index.remove(path_bytes);
            }
            if args.verbose {
                eprintln!("remove '{path}'");
            }
            return Ok(());
        }
        bail!("pathspec '{}' did not match any files", path);
    }

    if abs_path.is_dir() {
        let mut paths = Vec::new();
        walk_directory(&abs_path, work_tree, &mut paths)?;
        for rel_path in &paths {
            let file_abs = work_tree.join(rel_path);
            stage_file(odb, index, work_tree, rel_path, &file_abs, args)?;
        }
    } else {
        stage_file(odb, index, work_tree, path, &abs_path, args)?;
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
) -> Result<()> {
    if args.dry_run {
        eprintln!("add '{rel_path}'");
        return Ok(());
    }

    let meta = fs::symlink_metadata(abs_path)?;

    if args.intent_to_add {
        let mode = if meta.file_type().is_symlink() {
            0o120000
        } else {
            normalize_mode(meta.mode())
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

    // Skip if index already has this file with matching stat data
    if let Some(existing) = index.get(rel_path.as_bytes(), 0) {
        if stat_matches(existing, &meta) && existing.mode == normalize_mode(meta.mode()) {
            return Ok(());
        }
    }

    // Read file content and hash it
    let data = if meta.file_type().is_symlink() {
        let target = fs::read_link(abs_path)?;
        target.to_string_lossy().into_owned().into_bytes()
    } else {
        fs::read(abs_path)?
    };

    let oid = odb.write(ObjectKind::Blob, &data)?;
    let mode = if meta.file_type().is_symlink() {
        0o120000
    } else {
        normalize_mode(meta.mode())
    };
    let entry = entry_from_stat(abs_path, rel_path.as_bytes(), oid, mode)?;
    index.add_or_replace(entry);

    if args.verbose {
        eprintln!("add '{rel_path}'");
    }

    Ok(())
}

/// Recursively walk a directory, collecting relative paths (skipping .git).
fn walk_directory(dir: &Path, work_tree: &Path, out: &mut Vec<String>) -> Result<()> {
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

        if path.is_dir() {
            walk_directory(&path, work_tree, out)?;
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
