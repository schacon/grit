//! `grit difftool` — launch an external diff tool.
//!
//! Opens an external diff viewer for each changed file.  Reads the
//! `diff.tool` config key (falling back to `vimdiff`), generates
//! temporary copies of the old versions, and invokes the tool.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::{diff_index_to_worktree, DiffEntry, DiffStatus};
use grit_lib::error::Error;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

/// Arguments for `grit difftool`.
#[derive(Debug, ClapArgs)]
#[command(about = "Launch an external diff tool")]
pub struct Args {
    /// Commit or ref to diff against (default: index vs worktree).
    pub commit: Option<String>,

    /// Restrict diff to these paths.
    #[arg(last = true)]
    pub paths: Vec<String>,

    /// Don't prompt before each file.
    #[arg(short = 'y', long = "no-prompt")]
    pub no_prompt: bool,

    /// Specify the diff tool to use.
    #[arg(short = 't', long = "tool")]
    pub tool: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    // Determine which tool to use: --tool flag > diff.tool config > vimdiff
    let tool_name = args
        .tool
        .clone()
        .or_else(|| config.get("diff.tool"))
        .unwrap_or_else(|| "vimdiff".to_string());

    let index = match repo.load_index() {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // Get diff entries (index vs worktree)
    let entries = diff_index_to_worktree(&repo.odb, &index, work_tree, false, false)?;

    // Filter by paths if specified
    let entries: Vec<&DiffEntry> = if args.paths.is_empty() {
        entries.iter().collect()
    } else {
        entries
            .iter()
            .filter(|e| {
                let p = e.path();
                args.paths.iter().any(|filter| p.starts_with(filter))
            })
            .collect()
    };

    if entries.is_empty() {
        return Ok(());
    }

    let tmp_dir = tempfile::tempdir().context("failed to create temp directory")?;

    for entry in &entries {
        if entry.status == DiffStatus::Added {
            // New file — diff /dev/null against worktree
            if !args.no_prompt {
                eprint!("View diff for '{}' in {}? [Y/n] ", entry.path(), tool_name);
                io::stderr().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                let answer = answer.trim().to_lowercase();
                if answer == "n" || answer == "no" {
                    continue;
                }
            }
            let wt_path = work_tree.join(entry.path());
            Command::new(&tool_name)
                .arg("/dev/null")
                .arg(&wt_path)
                .status()
                .with_context(|| format!("failed to launch {tool_name}"))?;
            continue;
        }

        // Write old version to a temp file
        {
            let data = repo
                .odb
                .read(&entry.old_oid)
                .with_context(|| format!("failed to read object {}", entry.old_oid))?;
            let old_path = tmp_dir
                .path()
                .join(format!("a_{}", entry.path().replace('/', "_")));
            fs::write(&old_path, &data.data)?;

            let new_path = if entry.status == DiffStatus::Deleted {
                PathBuf::from("/dev/null")
            } else {
                work_tree.join(entry.path())
            };

            if !args.no_prompt {
                eprint!("View diff for '{}' in {}? [Y/n] ", entry.path(), tool_name);
                io::stderr().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                let answer = answer.trim().to_lowercase();
                if answer == "n" || answer == "no" {
                    continue;
                }
            }

            Command::new(&tool_name)
                .arg(&old_path)
                .arg(&new_path)
                .status()
                .with_context(|| format!("failed to launch {tool_name}"))?;
        }
    }

    Ok(())
}
