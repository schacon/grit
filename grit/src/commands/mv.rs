//! `grit mv` — move or rename files in the index and working tree.
//!
//! Renames files (or directories) both on disk and in the index so the change
//! is automatically staged for the next commit.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::repo::Repository;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Arguments for `grit mv`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Move or rename a file, a directory, or a symlink",
    override_usage = "grit mv [-v] [-f] [-n] [-k] <source> <destination>\n       \
                      grit mv [-v] [-f] [-n] [-k] <source>... <destination-directory>"
)]
pub struct Args {
    /// Source(s) and destination — last element is always the destination.
    /// At least two values are required.
    #[arg(required = true, num_args = 2..)]
    pub paths: Vec<String>,

    /// Force move/rename even if target exists.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Dry run — show what would be moved without doing it.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Skip move/rename errors instead of aborting.
    #[arg(short = 'k')]
    pub skip_errors: bool,

    /// Be verbose.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

/// A planned rename operation.
struct MoveOp {
    src: String,
    dst: String,
    /// True when this move covers only the index (no on-disk rename needed,
    /// e.g. when the whole directory expansion is already handled by a parent).
    index_only: bool,
}

/// Run the `mv` command.
pub fn run(args: Args) -> Result<()> {
    // The last path is the destination; everything before is a source.
    let (raw_sources, raw_dest) = {
        let mut all = args.paths;
        // clap guarantees num_args >= 2, so this is always Some.
        let dest = all
            .pop()
            .ok_or_else(|| anyhow::anyhow!("usage: grit mv <source> ... <destination>"))?;
        (all, dest)
    };

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

    // Resolve the current working directory relative to the worktree so that
    // relative paths typed by the user work correctly when they `cd` into a
    // subdirectory.
    let cwd = std::env::current_dir()?;
    let prefix = compute_prefix(&cwd, work_tree);

    // Canonicalise sources relative to worktree.
    let sources: Vec<String> = raw_sources
        .iter()
        .map(|s| resolve_path(s, prefix.as_deref(), work_tree))
        .collect();

    // Validate that all sources are inside the worktree.
    for (raw, resolved) in raw_sources.iter().zip(sources.iter()) {
        if Path::new(resolved).is_absolute() {
            bail!("source '{}' is outside the work tree", raw);
        }
    }

    // Destination: strip trailing slashes for stat purposes, but remember
    // whether the user supplied one (it forces "must be a directory" check).
    let dest_has_trailing_slash = raw_dest.ends_with('/') || raw_dest.ends_with('\\');
    let dest_trimmed = raw_dest.trim_end_matches('/').trim_end_matches('\\');
    let dest_rel = resolve_path(dest_trimmed, prefix.as_deref(), work_tree);

    // Validate that destination is inside the worktree (reject absolute
    // paths that point outside).
    if Path::new(&dest_rel).is_absolute() {
        bail!("destination '{}' is outside the work tree", raw_dest);
    }

    let dest_abs = work_tree.join(&dest_rel);

    // Build the list of (src, dst) pairs.
    let mut ops: Vec<MoveOp> = Vec::new();

    // Determine if the destination is a directory.
    let dest_is_dir = dest_abs.is_dir()
        || dest_rel.is_empty() // "." normalises to ""
        || is_index_dir(&dest_rel, &index);

    if !dest_is_dir && sources.len() > 1 {
        bail!("destination '{}' is not a directory", dest_trimmed);
    }

    // Destination must exist if user gave trailing slash and it doesn't
    // resolve to a directory (unless src is a dir being moved there).
    if dest_has_trailing_slash && !dest_abs.is_dir() && !dest_abs.exists() {
        // Allow "git mv dir/ no-such-dir/" only when src is a dir.
        let single_src_is_dir = sources.len() == 1 && {
            let sabs = work_tree.join(&sources[0]);
            sabs.is_dir() || is_index_dir(&sources[0], &index)
        };
        if !single_src_is_dir {
            bail!("destination directory '{}' does not exist", dest_trimmed);
        }
    }

    // Detect conflicting moves: a file and its parent directory cannot both
    // be moved at the same time.
    if sources.len() > 1 {
        for (i, src_a) in sources.iter().enumerate() {
            let src_a_clean = src_a.trim_end_matches('/').trim_end_matches('\\');
            let prefix_a = format!("{}/", src_a_clean);
            for (j, src_b) in sources.iter().enumerate() {
                if i == j {
                    continue;
                }
                let src_b_clean = src_b.trim_end_matches('/').trim_end_matches('\\');
                if src_b_clean.starts_with(&prefix_a) {
                    bail!(
                        "cannot move both '{}' and its parent directory '{}'",
                        src_b_clean,
                        src_a_clean
                    );
                }
            }
        }
    }

    for src_rel in &sources {
        // Strip trailing slashes from source for stat.
        let src_rel = src_rel
            .trim_end_matches('/')
            .trim_end_matches('\\')
            .to_owned();
        let src_abs = work_tree.join(&src_rel);

        // Compute the destination path for this source.
        let dst_rel: String = if dest_is_dir {
            // Move into directory: basename of source goes under dest.
            let basename = Path::new(&src_rel)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| src_rel.clone());
            if dest_rel.is_empty() {
                basename
            } else {
                format!("{}/{}", dest_rel.trim_end_matches('/'), basename)
            }
        } else {
            dest_rel.clone()
        };

        let dst_abs = work_tree.join(&dst_rel);

        // Validate source.
        if src_abs.exists() {
            if src_abs.is_dir() {
                // A submodule path is represented as a gitlink entry (mode
                // 160000) at the directory root. Treat it like a regular
                // path move instead of expanding tracked files beneath it.
                if index.get(src_rel.as_bytes(), 0).is_some() {
                    ops.push(MoveOp {
                        src: src_rel.clone(),
                        dst: dst_rel.clone(),
                        index_only: false,
                    });
                    continue;
                }
                // Expand directory to individual index entries.
                let expanded = expand_dir_sources(&src_rel, &dst_rel, &index);
                if expanded.is_empty() {
                    let msg = format!("source directory is empty or not tracked: '{src_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                // Check destination directory does not already exist.
                if dst_abs.is_dir() {
                    let msg = format!("destination already exists: '{dst_rel}'");
                    if args.skip_errors {
                        continue;
                    }
                    bail!("{msg}");
                }
                // Add the directory-level op (triggers the on-disk rename).
                ops.push(MoveOp {
                    src: src_rel.clone(),
                    dst: dst_rel.clone(),
                    index_only: false,
                });
                // Add index-only ops for every file inside.
                for (fsrc, fdst) in expanded {
                    ops.push(MoveOp {
                        src: fsrc,
                        dst: fdst,
                        index_only: true,
                    });
                }
                continue;
            }
        } else {
            // Source doesn't exist on disk — must be tracked in the index.
            let any_entry = index.entries.iter().any(|e| e.path == src_rel.as_bytes());
            if !any_entry {
                let msg = format!(
                    "not under version control, source='{src_rel}', destination='{dst_rel}'"
                );
                if args.skip_errors {
                    continue;
                }
                bail!("{msg}");
            }
        }

        // Source must be tracked; check for conflicts (stage > 0 entries, no stage-0 entry).
        let stage0 = index.get(src_rel.as_bytes(), 0);
        let has_conflict = index
            .entries
            .iter()
            .any(|e| e.path == src_rel.as_bytes() && e.stage() > 0);

        if has_conflict {
            let msg = format!("conflicted, source='{src_rel}', destination='{dst_rel}'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        if stage0.is_none() && !src_abs.is_dir() {
            let msg =
                format!("not under version control, source='{src_rel}', destination='{dst_rel}'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        // Check destination doesn't already exist (unless -f).
        if dst_abs.exists()
            && !(args.force && (dst_abs.is_file() || dst_abs.is_symlink()) && !dst_abs.is_dir())
        {
            if !args.force {
                let msg =
                    format!("destination exists, source='{src_rel}', destination='{dst_rel}'");
                if args.skip_errors {
                    continue;
                }
                bail!("{msg}");
            }
            // force but dst is a directory: cannot overwrite directory
            if dst_abs.is_dir() {
                let msg = format!("Cannot overwrite, source='{src_rel}', destination='{dst_rel}'");
                if args.skip_errors {
                    continue;
                }
                bail!("{msg}");
            }
        }

        // Destination ends with / but doesn't exist as a dir.
        if dest_has_trailing_slash && !dest_abs.exists() && sources.len() == 1 {
            // Not reaching here for single dir src (handled above).
            let msg = format!("destination directory does not exist: '{dest_trimmed}/'");
            if args.skip_errors {
                continue;
            }
            bail!("{msg}");
        }

        ops.push(MoveOp {
            src: src_rel.clone(),
            dst: dst_rel.clone(),
            index_only: false,
        });
    }

    // Execute operations.
    for op in &ops {
        if args.verbose || args.dry_run {
            println!("Renaming {} to {}", op.src, op.dst);
        }
        if args.dry_run {
            continue;
        }

        let src_abs = work_tree.join(&op.src);
        let dst_abs = work_tree.join(&op.dst);

        if !op.index_only {
            // Create parent directories if needed.
            if let Some(parent) = dst_abs.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            // Rename on disk.
            if src_abs.exists() {
                fs::rename(&src_abs, &dst_abs)
                    .with_context(|| format!("renaming '{}' failed", op.src))?;
            }
        }

        // Update index: clone the old entry, change its path, re-insert.
        if let Some(old_entry) = index.get(op.src.as_bytes(), 0).cloned() {
            let new_path = op.dst.as_bytes().to_vec();
            let path_len = new_path.len().min(0x0FFF);
            let mut new_entry = old_entry;
            // Preserve flags except path length bits (low 12 bits of flags).
            new_entry.flags = (new_entry.flags & !0x0FFF) | path_len as u16;
            new_entry.path = new_path;

            // Refresh stat info from the destination file so git considers
            // the index entry up-to-date after the rename.
            if let Ok(meta) = fs::symlink_metadata(&dst_abs) {
                new_entry.ctime_sec = meta.ctime() as u32;
                new_entry.ctime_nsec = meta.ctime_nsec() as u32;
                new_entry.mtime_sec = meta.mtime() as u32;
                new_entry.mtime_nsec = meta.mtime_nsec() as u32;
                new_entry.dev = meta.dev() as u32;
                new_entry.ino = meta.ino() as u32;
                new_entry.uid = meta.uid();
                new_entry.gid = meta.gid();
                new_entry.size = meta.size() as u32;
            }

            index.remove(op.src.as_bytes());
            index.add_or_replace(new_entry);
        }
    }

    if !args.dry_run {
        index.write(&repo.index_path())?;
    }

    Ok(())
}

/// Expand all index entries under `src_dir/` to their new paths under `dst_dir/`.
///
/// Returns a list of `(old_index_path, new_index_path)` pairs for every file
/// inside the directory.
fn expand_dir_sources(src_dir: &str, dst_dir: &str, index: &Index) -> Vec<(String, String)> {
    let prefix = format!("{}/", src_dir);
    index
        .entries
        .iter()
        .filter(|e| {
            let p = String::from_utf8_lossy(&e.path);
            p.starts_with(&prefix)
        })
        .map(|e| {
            let p = String::from_utf8_lossy(&e.path).to_string();
            let suffix = &p[prefix.len()..];
            let new_path = format!("{}/{}", dst_dir, suffix);
            (p, new_path)
        })
        .collect()
}

/// Returns `true` when `path` acts as a virtual directory in the index
/// (i.e. there are index entries whose path starts with `path/`).
fn is_index_dir(path: &str, index: &Index) -> bool {
    let prefix = format!("{}/", path);
    index
        .entries
        .iter()
        .any(|e| String::from_utf8_lossy(&e.path).starts_with(&prefix))
}

/// Compute the path of `cwd` relative to `work_tree`, if `cwd` is inside
/// `work_tree`.  Returns `None` (meaning: no prefix) when they are the same.
fn compute_prefix(cwd: &Path, work_tree: &Path) -> Option<String> {
    let cwd_c = cwd.canonicalize().ok()?;
    let wt_c = work_tree.canonicalize().ok()?;
    if cwd_c == wt_c {
        return None;
    }
    cwd_c
        .strip_prefix(&wt_c)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Resolve a user-provided path (possibly relative to the subdirectory they
/// are in) to a worktree-relative path.
fn resolve_path(path: &str, prefix: Option<&str>, work_tree: &Path) -> String {
    let p = Path::new(path);

    // Handle absolute paths by stripping the worktree prefix.
    if p.is_absolute() {
        let wt_canon = work_tree
            .canonicalize()
            .unwrap_or_else(|_| work_tree.to_path_buf());
        if let Ok(rel) = p.strip_prefix(&wt_canon) {
            return normalise_path(&rel.to_string_lossy());
        }
        // Also try without canonicalising (in case worktree is already canonical).
        if let Ok(rel) = p.strip_prefix(work_tree) {
            return normalise_path(&rel.to_string_lossy());
        }
        // Absolute path outside the worktree — return as-is so the caller
        // can detect the error.
        return path.to_owned();
    }

    match prefix {
        Some(pfx) if !pfx.is_empty() => {
            let combined = PathBuf::from(pfx).join(path);
            normalise_path(&combined.to_string_lossy())
        }
        _ => normalise_path(path),
    }
}

/// Collapse `/./` and `foo/../` sequences in a slash-separated path string.
fn normalise_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
