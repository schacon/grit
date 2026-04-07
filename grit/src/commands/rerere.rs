//! `grit rerere` — reuse recorded resolution of conflicted merges.
//!
//! Subcommands:
//! - (no args) — record current conflict resolutions or replay recorded ones
//! - `forget <path>` — forget recorded resolution for a path
//! - `status` — show files with recorded resolutions
//! - `diff` — show diff between current conflicts and recorded resolution

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::repo::Repository;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit rerere`.
#[derive(Debug, ClapArgs)]
#[command(about = "Reuse recorded resolution of conflicted merges")]
pub struct Args {
    #[command(subcommand)]
    pub subcmd: Option<RerereSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum RerereSubcommand {
    /// Forget recorded resolution for a path.
    Forget {
        /// Path to forget resolution for.
        pathspec: String,
    },
    /// Show files with recorded resolutions.
    Status,
    /// Show current conflicts vs recorded resolution.
    Diff,
    /// Reset recorded resolutions to current conflict state.
    Clear,
    /// Explicitly record current resolutions (also done by default).
    #[command(name = "remaining")]
    Remaining,
    /// Show merge conflicts that have not been resolved.
    #[command(name = "gc")]
    Gc,
}

/// Run the `rerere` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)?;
    let rerere_dir = repo.git_dir.join("rr-cache");

    match args.subcmd {
        None => cmd_default(&repo, &rerere_dir),
        Some(RerereSubcommand::Forget { pathspec }) => cmd_forget(&repo, &rerere_dir, &pathspec),
        Some(RerereSubcommand::Status) => cmd_status(&repo, &rerere_dir),
        Some(RerereSubcommand::Diff) => cmd_diff(&repo, &rerere_dir),
        Some(RerereSubcommand::Clear) => cmd_clear(&rerere_dir),
        Some(RerereSubcommand::Remaining) => cmd_remaining(&repo, &rerere_dir),
        Some(RerereSubcommand::Gc) => cmd_gc(&rerere_dir),
    }
}

/// Check if rerere is enabled in config.
fn is_rerere_enabled(repo: &Repository) -> bool {
    if let Ok(config) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), false) {
        if let Some(val) = config.get("rerere.enabled") {
            return val == "true" || val == "1";
        }
    }
    // Also enabled if rr-cache dir exists.
    repo.git_dir.join("rr-cache").is_dir()
}

/// Default: record/replay conflict resolutions.
fn cmd_default(repo: &Repository, rerere_dir: &Path) -> Result<()> {
    if !is_rerere_enabled(repo) {
        return Ok(());
    }

    let work_tree = repo
        .work_tree
        .as_ref()
        .context("rerere requires a working tree")?;

    // Find conflicted files from the index.
    let conflicts = find_conflict_files(repo)?;

    if conflicts.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(rerere_dir)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &conflicts {
        let file_path = work_tree.join(path);
        if !file_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&file_path)
            .with_context(|| format!("reading {}", file_path.display()))?;

        // Extract conflict and compute hash.
        if let Some(conflict_id) = compute_conflict_hash(&content) {
            let cache_dir = rerere_dir.join(&conflict_id);
            let preimage = cache_dir.join("preimage");
            let postimage = cache_dir.join("postimage");

            if postimage.exists() {
                // Replay recorded resolution.
                let resolved = fs::read_to_string(&postimage)?;
                fs::write(&file_path, &resolved)?;
                writeln!(out, "Resolved '{path}' using previous resolution.")?;
            } else {
                // Record the conflict preimage.
                fs::create_dir_all(&cache_dir)?;
                fs::write(&preimage, &content)?;
                writeln!(out, "Recorded preimage for '{path}'")?;
            }
        }
    }

    Ok(())
}

/// `rerere forget <path>` — forget recorded resolution.
fn cmd_forget(repo: &Repository, rerere_dir: &Path, pathspec: &str) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_ref()
        .context("rerere requires a working tree")?;

    let file_path = work_tree.join(pathspec);
    if !file_path.exists() {
        bail!("error: no such path '{}' in the working tree", pathspec);
    }

    let content = fs::read_to_string(&file_path)?;
    let mut forgot_any = false;
    if let Some(conflict_id) = compute_conflict_hash(&content) {
        let cache_dir = rerere_dir.join(&conflict_id);
        if cache_dir.exists() {
            fs::remove_dir_all(&cache_dir)?;
            println!("Updated prereg for '{pathspec}'");
            forgot_any = true;
        }
    }

    // Also check all cached dirs for a thisimage matching this path.
    if rerere_dir.is_dir() {
        for entry in fs::read_dir(rerere_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let thisimage = entry.path().join("thisimage");
            if thisimage.exists() {
                if let Ok(stored_path) = fs::read_to_string(&thisimage) {
                    if stored_path.trim() == pathspec {
                        fs::remove_dir_all(entry.path())?;
                        println!("Forgot resolution for '{pathspec}'");
                        forgot_any = true;
                    }
                }
            }
        }
    }

    if !forgot_any {
        bail!("error: no remembered resolution for '{}'", pathspec);
    }

    Ok(())
}

/// `rerere status` — show files with recorded resolutions.
fn cmd_status(repo: &Repository, rerere_dir: &Path) -> Result<()> {
    let conflicts = find_conflict_files(repo)?;
    let work_tree = repo
        .work_tree
        .as_ref()
        .context("rerere requires a working tree")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &conflicts {
        let file_path = work_tree.join(path);
        if !file_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&file_path).unwrap_or_default();
        if let Some(conflict_id) = compute_conflict_hash(&content) {
            let cache_dir = rerere_dir.join(&conflict_id);
            if cache_dir.join("preimage").exists() {
                writeln!(out, "{path}")?;
            }
        }
    }

    Ok(())
}

/// `rerere diff` — show diff between current and recorded.
fn cmd_diff(repo: &Repository, rerere_dir: &Path) -> Result<()> {
    let conflicts = find_conflict_files(repo)?;
    let work_tree = repo
        .work_tree
        .as_ref()
        .context("rerere requires a working tree")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &conflicts {
        let file_path = work_tree.join(path);
        if !file_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&file_path).unwrap_or_default();
        if let Some(conflict_id) = compute_conflict_hash(&content) {
            let cache_dir = rerere_dir.join(&conflict_id);
            let preimage = cache_dir.join("preimage");
            if preimage.exists() {
                let recorded = fs::read_to_string(&preimage)?;
                // Simple line-by-line diff.
                writeln!(out, "--- a/{path}")?;
                writeln!(out, "+++ b/{path}")?;

                // Emit a basic unified diff.
                for diff in similar::TextDiff::from_lines(&recorded, &content)
                    .unified_diff()
                    .iter_hunks()
                {
                    write!(out, "{diff}")?;
                }
            }
        }
    }

    Ok(())
}

/// `rerere clear` — remove all rr-cache entries.
fn cmd_clear(rerere_dir: &Path) -> Result<()> {
    if rerere_dir.is_dir() {
        for entry in fs::read_dir(rerere_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                // Only remove dirs that have a "thisimage" — active conflicts.
                let thisimage = entry.path().join("thisimage");
                if thisimage.exists() {
                    fs::remove_dir_all(entry.path())?;
                }
            }
        }
    }
    Ok(())
}

/// `rerere remaining` — show unresolved conflicts.
fn cmd_remaining(repo: &Repository, _rerere_dir: &Path) -> Result<()> {
    let conflicts = find_conflict_files(repo)?;
    let work_tree = repo
        .work_tree
        .as_ref()
        .context("rerere requires a working tree")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &conflicts {
        let file_path = work_tree.join(path);
        if file_path.exists() {
            let content = fs::read_to_string(&file_path).unwrap_or_default();
            if has_conflict_markers(&content) {
                writeln!(out, "{path}")?;
            }
        }
    }

    Ok(())
}

/// `rerere gc` — garbage-collect old rr-cache entries.
fn cmd_gc(rerere_dir: &Path) -> Result<()> {
    if !rerere_dir.is_dir() {
        return Ok(());
    }

    let now = std::time::SystemTime::now();
    let max_age_resolved = std::time::Duration::from_secs(60 * 24 * 3600); // 60 days
    let max_age_unresolved = std::time::Duration::from_secs(15 * 24 * 3600); // 15 days

    for entry in fs::read_dir(rerere_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let postimage = entry.path().join("postimage");
        let preimage = entry.path().join("preimage");

        let max_age = if postimage.exists() {
            max_age_resolved
        } else if preimage.exists() {
            max_age_unresolved
        } else {
            continue;
        };

        let check_file = if postimage.exists() {
            &postimage
        } else {
            &preimage
        };

        if let Ok(meta) = fs::metadata(check_file) {
            if let Ok(modified) = meta.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age > max_age {
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }

    Ok(())
}

/// Find files in conflict state from the index.
fn find_conflict_files(repo: &Repository) -> Result<Vec<String>> {
    let index_path = repo.git_dir.join("index");
    if !index_path.exists() {
        return Ok(Vec::new());
    }

    let index = repo.load_index_at(&index_path)?;
    let mut conflict_paths = Vec::new();

    for entry in &index.entries {
        // Stage != 0 means conflicted.
        let stage = (entry.flags >> 12) & 0x3;
        if stage != 0 {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if !conflict_paths.contains(&path) {
                conflict_paths.push(path);
            }
        }
    }

    Ok(conflict_paths)
}

/// Compute a hash from the conflict markers in a file.
/// Returns None if no conflict markers found.
fn compute_conflict_hash(content: &str) -> Option<String> {
    let mut hasher = Sha1::new();
    let mut in_conflict = false;
    let mut found_conflict = false;

    for line in content.lines() {
        if line.starts_with("<<<<<<<") {
            in_conflict = true;
            found_conflict = true;
            hasher.update(b"<<<<<<<\n");
        } else if line.starts_with("=======") && in_conflict {
            hasher.update(b"=======\n");
        } else if line.starts_with(">>>>>>>") && in_conflict {
            hasher.update(b">>>>>>>\n");
            in_conflict = false;
        } else if in_conflict {
            hasher.update(line.as_bytes());
            hasher.update(b"\n");
        }
    }

    if found_conflict {
        let hash = hasher.finalize();
        Some(hex::encode(hash))
    } else {
        None
    }
}

/// Check if content has conflict markers.
fn has_conflict_markers(content: &str) -> bool {
    content.lines().any(|l| l.starts_with("<<<<<<<"))
}

/// Public entry point for other commands (am, merge, rebase) to invoke
/// rerere record/replay after a conflict.
///
/// Returns `Ok(true)` if rerere replayed a recorded resolution.
#[allow(dead_code)]
pub fn auto_rerere(repo: &Repository) -> Result<bool> {
    if !is_rerere_enabled(repo) {
        return Ok(false);
    }
    let rerere_dir = repo.git_dir.join("rr-cache");
    let work_tree = match repo.work_tree.as_ref() {
        Some(wt) => wt,
        None => return Ok(false),
    };
    let conflicts = find_conflict_files(repo)?;
    if conflicts.is_empty() {
        return Ok(false);
    }

    fs::create_dir_all(&rerere_dir)?;
    let mut replayed = false;

    for path in &conflicts {
        let file_path = work_tree.join(path);
        if !file_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&file_path)?;
        if let Some(conflict_id) = compute_conflict_hash(&content) {
            let cache_dir = rerere_dir.join(&conflict_id);
            let postimage = cache_dir.join("postimage");
            if postimage.exists() {
                let resolved = fs::read_to_string(&postimage)?;
                fs::write(&file_path, &resolved)?;
                eprintln!("Resolved '{}' using previous resolution.", path);
                replayed = true;
            } else {
                fs::create_dir_all(&cache_dir)?;
                fs::write(cache_dir.join("preimage"), &content)?;
                // Store the path so we can find it later for postimage recording
                fs::write(cache_dir.join("thisimage"), path.as_bytes())?;
                eprintln!("Recorded preimage for '{}'", path);
            }
        }
    }
    Ok(replayed)
}

/// Public entry point to record a postimage (after a conflict is resolved
/// by the user and `am --continue` / `merge --continue` is called).
pub fn record_postimage(repo: &Repository) -> Result<()> {
    if !is_rerere_enabled(repo) {
        return Ok(());
    }
    let rerere_dir = repo.git_dir.join("rr-cache");
    if !rerere_dir.is_dir() {
        return Ok(());
    }
    let work_tree = match repo.work_tree.as_ref() {
        Some(wt) => wt,
        None => return Ok(()),
    };

    // Walk the rr-cache looking for dirs that have a preimage but no postimage.
    for entry in fs::read_dir(&rerere_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let pre = entry.path().join("preimage");
        let post = entry.path().join("postimage");
        if pre.exists() && !post.exists() {
            let thisimage_path = entry.path().join("thisimage");
            if let Ok(path_str) = fs::read_to_string(&thisimage_path) {
                let file_path = work_tree.join(path_str.trim());
                if file_path.exists() {
                    let content = fs::read_to_string(&file_path)?;
                    if !has_conflict_markers(&content) {
                        fs::write(&post, &content)?;
                        eprintln!("Recorded resolution for '{}'.", path_str.trim());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Worktree-based rerere for commands that don't set up index conflict
/// stages (e.g. `am`).  Scans tracked files for conflict markers instead
/// of relying on index stage entries.
///
/// Returns `Ok(true)` if a recorded resolution was replayed.
pub fn auto_rerere_worktree(repo: &Repository) -> Result<bool> {
    if !is_rerere_enabled(repo) {
        return Ok(false);
    }
    let rerere_dir = repo.git_dir.join("rr-cache");
    let work_tree = match repo.work_tree.as_ref() {
        Some(wt) => wt,
        None => return Ok(false),
    };

    // Scan worktree for files with conflict markers
    let index_path = repo.git_dir.join("index");
    let index = if index_path.exists() {
        repo.load_index_at(&index_path)?
    } else {
        return Ok(false);
    };

    fs::create_dir_all(&rerere_dir)?;
    let mut replayed = false;

    for entry in &index.entries {
        let path = String::from_utf8_lossy(&entry.path).to_string();
        let file_path = work_tree.join(&path);
        if !file_path.exists() {
            continue;
        }
        let content = match fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !has_conflict_markers(&content) {
            continue;
        }

        if let Some(conflict_id) = compute_conflict_hash(&content) {
            let cache_dir = rerere_dir.join(&conflict_id);
            let postimage = cache_dir.join("postimage");
            if postimage.exists() {
                let resolved = fs::read_to_string(&postimage)?;
                fs::write(&file_path, &resolved)?;
                eprintln!("Resolved '{}' using previous resolution.", path);
                replayed = true;
            } else {
                fs::create_dir_all(&cache_dir)?;
                fs::write(cache_dir.join("preimage"), &content)?;
                fs::write(cache_dir.join("thisimage"), path.as_bytes())?;
                eprintln!("Recorded preimage for '{}'", path);
            }
        }
    }
    Ok(replayed)
}
