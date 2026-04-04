//! `grit sparse-checkout` — manage sparse checkout patterns.
//!
//! Sparse checkout allows only a subset of files to be checked out
//! in the working tree. Patterns are stored in `.git/info/sparse-checkout`
//! and controlled by `core.sparseCheckout` config.

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Write};

/// Arguments for `grit sparse-checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage sparse checkout patterns")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: SparseCheckoutSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SparseCheckoutSubcommand {
    /// Initialize sparse checkout.
    Init,
    /// Set sparse checkout patterns.
    Set(SetArgs),
    /// List current sparse checkout patterns.
    List,
    /// Disable sparse checkout.
    Disable,
}

#[derive(Debug, ClapArgs)]
pub struct SetArgs {
    /// Patterns to include in sparse checkout.
    #[arg(required = true)]
    pub patterns: Vec<String>,
}

/// Run `grit sparse-checkout`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.subcommand {
        SparseCheckoutSubcommand::Init => cmd_init(&repo),
        SparseCheckoutSubcommand::Set(set_args) => cmd_set(&repo, &set_args.patterns),
        SparseCheckoutSubcommand::List => cmd_list(&repo),
        SparseCheckoutSubcommand::Disable => cmd_disable(&repo),
    }
}

/// Enable sparse checkout by setting `core.sparseCheckout = true`
/// and creating the sparse-checkout file with a default `/*` pattern.
fn cmd_init(repo: &Repository) -> Result<()> {
    set_sparse_config(repo, true)?;

    let sc_path = sparse_checkout_path(repo);
    // Create parent directory if needed
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    // Only write defaults if the file doesn't exist yet
    if !sc_path.exists() {
        fs::write(&sc_path, "/*\n").context("writing sparse-checkout file")?;
    }

    Ok(())
}

/// Set the sparse checkout patterns, replacing any existing ones.
fn cmd_set(repo: &Repository, patterns: &[String]) -> Result<()> {
    // Ensure sparse checkout is enabled
    set_sparse_config(repo, true)?;

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    let mut content = String::new();
    for pat in patterns {
        content.push_str(pat);
        content.push('\n');
    }
    fs::write(&sc_path, &content).context("writing sparse-checkout file")?;

    // Apply sparse checkout patterns to the working tree
    apply_sparse_patterns(repo, patterns)?;

    Ok(())
}

/// Apply sparse checkout patterns: remove files from the working tree that
/// don't match any pattern, and set skip-worktree bit in the index.
fn apply_sparse_patterns(repo: &Repository, patterns: &[String]) -> Result<()> {
    use grit_lib::index::Index;

    let work_tree = repo.work_tree.as_deref()
        .ok_or_else(|| anyhow::anyhow!("bare repository cannot use sparse checkout"))?;

    let index_path = repo.git_dir.join("index");
    let mut index = Index::load(&index_path)
        .context("reading index")?;

    // Promote to version 3 if needed (skip-worktree requires extended flags)
    if index.version < 3 {
        index.version = 3;
    }

    for entry in &mut index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        let matches = path_matches_sparse_patterns(&path_str, patterns);

        if matches {
            // File should be in the working tree
            if entry.skip_worktree() {
                entry.set_skip_worktree(false);
                // Restore the file if missing
                let full_path = work_tree.join(&path_str);
                if !full_path.exists() {
                    if let Some(parent) = full_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Ok(obj) = repo.odb.read(&entry.oid) {
                        let _ = fs::write(&full_path, &obj.data);
                    }
                }
            }
        } else {
            // File should NOT be in the working tree — remove it
            entry.set_skip_worktree(true);
            let full_path = work_tree.join(&path_str);
            if full_path.exists() {
                let _ = fs::remove_file(&full_path);
                // Clean up empty parent directories
                if let Some(parent) = full_path.parent() {
                    remove_empty_dirs_up_to(parent, work_tree);
                }
            }
        }
    }

    index.write(&index_path).context("writing index")?;
    Ok(())
}

/// Check if a file path matches any of the sparse checkout patterns.
/// Patterns are treated as directory prefixes (like git's cone mode).
fn path_matches_sparse_patterns(path: &str, patterns: &[String]) -> bool {
    // Always include top-level files (not in subdirectories)
    if !path.contains('/') {
        return true;
    }

    for pattern in patterns {
        // Treat pattern as a directory prefix
        let prefix = pattern.trim_end_matches('/');
        if path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/') {
            return true;
        }
        // Also check if path IS the pattern (exact match)
        if path == prefix {
            return true;
        }
    }
    false
}

/// Remove empty directories walking up from `dir` to `stop` (exclusive).
fn remove_empty_dirs_up_to(dir: &std::path::Path, stop: &std::path::Path) {
    let mut current = dir.to_path_buf();
    while current != stop {
        if let Ok(mut entries) = fs::read_dir(&current) {
            if entries.next().is_some() {
                break; // Not empty
            }
            let _ = fs::remove_dir(&current);
        } else {
            break;
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }
}

/// List the current sparse checkout patterns.
fn cmd_list(repo: &Repository) -> Result<()> {
    // Check if sparse checkout is actually enabled
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        anyhow::bail!("this worktree is not sparse");
    }

    let sc_path = sparse_checkout_path(repo);
    if !sc_path.exists() {
        // No sparse checkout file — nothing to list
        return Ok(());
    }

    let content = fs::read_to_string(&sc_path).context("reading sparse-checkout file")?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            writeln!(out, "{trimmed}")?;
        }
    }
    Ok(())
}

/// Disable sparse checkout by setting `core.sparseCheckout = false`
/// and restoring all files to the working tree.
fn cmd_disable(repo: &Repository) -> Result<()> {
    use grit_lib::index::Index;
    set_sparse_config(repo, false)?;

    // Restore all skip-worktree files to the working tree
    let work_tree = match repo.work_tree.as_deref() {
        Some(wt) => wt,
        None => return Ok(()),
    };

    let index_path = repo.git_dir.join("index");
    let mut index = Index::load(&index_path).context("reading index")?;

    // Ensure version 3 for extended flags
    if index.version < 3 {
        index.version = 3;
    }

    for entry in &mut index.entries {
        if entry.skip_worktree() {
            entry.set_skip_worktree(false);
            let path_str = String::from_utf8_lossy(&entry.path).to_string();
            let full_path = work_tree.join(&path_str);
            if !full_path.exists() {
                // Restore file from the ODB
                if let Some(parent) = full_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if entry.oid != grit_lib::diff::zero_oid() {
                    if let Ok(obj) = repo.odb.read(&entry.oid) {
                        let _ = fs::write(&full_path, &obj.data);
                    }
                }
            }
        }
    }

    index.write(&index_path).context("writing index")?;

    // Remove sparse-checkout file
    let sc_path = sparse_checkout_path(repo);
    let _ = fs::remove_file(&sc_path);

    Ok(())
}

fn sparse_checkout_path(repo: &Repository) -> std::path::PathBuf {
    repo.git_dir.join("info").join("sparse-checkout")
}

/// Set `core.sparseCheckout` in the local repo config.
fn set_sparse_config(repo: &Repository, enable: bool) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content =
            fs::read_to_string(&config_path).context("reading repo config")?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    let value = if enable { "true" } else { "false" };
    config.set("core.sparseCheckout", value)?;
    config.write().context("writing repo config")?;
    Ok(())
}
