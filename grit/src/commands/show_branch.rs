//! `grit show-branch` — show branches and their commits.
//!
//! Lists branch heads with abbreviated commit hash and subject line,
//! similar to `git show-branch` in its basic mode.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit show-branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show branches and their commits")]
pub struct Args {
    /// Branch names to show (defaults to all local branches).
    #[arg()]
    pub branches: Vec<String>,
}

/// Run the `show-branch` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let head = resolve_head(&repo.git_dir)?;
    let current_branch = match &head {
        HeadState::Branch { short_name, .. } => Some(short_name.clone()),
        _ => None,
    };

    let mut branches: Vec<(String, ObjectId)> = Vec::new();

    if args.branches.is_empty() {
        // List all local branches
        let heads_dir = repo.git_dir.join("refs/heads");
        collect_branches(&heads_dir, "", &mut branches)?;
        branches.sort_by(|a, b| a.0.cmp(&b.0));
    } else {
        for name in &args.branches {
            let ref_path = repo.git_dir.join("refs/heads").join(name);
            if let Ok(content) = fs::read_to_string(&ref_path) {
                if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                    branches.push((name.clone(), oid));
                }
            }
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (name, oid) in &branches {
        let hex = oid.to_hex();
        let abbrev = &hex[..7.min(hex.len())];

        let subject = match repo.odb.read(oid) {
            Ok(obj) => match parse_commit(&obj.data) {
                Ok(commit) => commit
                    .message
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        };

        let marker = if current_branch.as_deref() == Some(name.as_str()) {
            "* "
        } else {
            "  "
        };

        writeln!(out, "{marker}[{name}] {abbrev} {subject}")?;
    }

    Ok(())
}

/// Recursively collect branches from the heads directory.
fn collect_branches(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, ObjectId)>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            file_name
        } else {
            format!("{prefix}/{file_name}")
        };

        if path.is_dir() {
            collect_branches(&path, &full_name, out)?;
        } else if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                out.push((full_name, oid));
            }
        }
    }

    Ok(())
}
