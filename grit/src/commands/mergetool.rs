//! `grit mergetool` — launch an external merge tool for conflicts.
//!
//! Scans the index for unmerged entries (stage > 0) and invokes the
//! configured merge tool on each conflicted file.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::repo::Repository;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

/// Arguments for `grit mergetool`.
#[derive(Debug, ClapArgs)]
#[command(about = "Launch an external merge tool for conflicts")]
pub struct Args {
    /// Specific file(s) to resolve.
    pub file: Vec<String>,

    /// Specify the merge tool to use.
    #[arg(short = 't', long = "tool")]
    pub tool: Option<String>,

    /// Don't prompt before each file.
    #[arg(short = 'y', long = "no-prompt")]
    pub no_prompt: bool,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    // Determine merge tool: --tool flag > merge.tool config > vimdiff
    let tool_name = args
        .tool
        .clone()
        .or_else(|| config.get("merge.tool"))
        .unwrap_or_else(|| "vimdiff".to_string());

    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("No files need merging");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // Find unmerged files (entries with stage > 0)
    let mut unmerged: BTreeSet<String> = BTreeSet::new();
    for entry in &index.entries {
        if entry.stage() > 0 {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            unmerged.insert(path);
        }
    }

    // Filter to requested files if any
    if !args.file.is_empty() {
        unmerged.retain(|p| args.file.iter().any(|f| p == f || p.starts_with(f)));
    }

    if unmerged.is_empty() {
        println!("No files need merging");
        return Ok(());
    }

    let tmp_dir = tempfile::tempdir().context("failed to create temp directory")?;

    for path in &unmerged {
        let path_bytes = path.as_bytes();

        // Extract base (stage 1), ours (stage 2), theirs (stage 3) if available
        let base_path = tmp_dir.path().join(format!("{}.BASE", path.replace('/', "_")));
        let local_path = tmp_dir.path().join(format!("{}.LOCAL", path.replace('/', "_")));
        let remote_path = tmp_dir.path().join(format!("{}.REMOTE", path.replace('/', "_")));

        // Write stage files
        for (stage, dest) in [(1u8, &base_path), (2, &local_path), (3, &remote_path)] {
            if let Some(entry) = index.get(path_bytes, stage) {
                let data = repo
                    .odb
                    .read(&entry.oid)
                    .with_context(|| format!("failed to read object {}", entry.oid))?;
                fs::write(dest, &data.data)?;
            } else {
                fs::write(dest, "")?;
            }
        }

        let merged_path = work_tree.join(path);

        if !args.no_prompt {
            eprint!("Merge file '{}' with {}? [Y/n] ", path, tool_name);
            io::stderr().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let answer = answer.trim().to_lowercase();
            if answer == "n" || answer == "no" {
                continue;
            }
        }

        // Launch tool with LOCAL BASE REMOTE MERGED convention
        let status = Command::new(&tool_name)
            .arg(&local_path)
            .arg(&base_path)
            .arg(&remote_path)
            .arg(&merged_path)
            .status()
            .with_context(|| format!("failed to launch {tool_name}"))?;

        if status.success() {
            println!("{path}: merge resolved");
        } else {
            eprintln!("{path}: merge tool returned non-zero status");
        }
    }

    Ok(())
}
