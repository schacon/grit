//! `grit branch` — list, create, or delete branches.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "List, create, or delete branches")]
pub struct Args {
    /// Branch name to create (or pattern to list).
    #[arg()]
    pub name: Option<String>,

    /// Start point for new branch (commit, branch, or tag).
    #[arg()]
    pub start_point: Option<String>,

    /// Delete a branch.
    #[arg(short = 'd', long = "delete")]
    pub delete: bool,

    /// Force delete a branch (even if not merged).
    #[arg(short = 'D')]
    pub force_delete: bool,

    /// Move/rename a branch.
    #[arg(short = 'm', long = "move")]
    pub rename: bool,

    /// Force move/rename.
    #[arg(short = 'M')]
    pub force_rename: bool,

    /// Copy a branch.
    #[arg(short = 'c', long = "copy")]
    pub copy: bool,

    /// List branches (default when no name given).
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// List remote-tracking branches.
    #[arg(short = 'r', long = "remotes")]
    pub remotes: bool,

    /// List both local and remote branches.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Show verbose info (commit subject).
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Show branches containing this commit.
    #[arg(long = "contains")]
    pub contains: Option<String>,

    /// Show branches not containing this commit.
    #[arg(long = "no-contains")]
    pub no_contains: Option<String>,

    /// Show branches merged into this commit.
    #[arg(long = "merged")]
    pub merged: Option<String>,

    /// Show branches not merged into this commit.
    #[arg(long = "no-merged")]
    pub no_merged: Option<String>,

    /// Force creation (overwrite existing branch).
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Set up tracking.
    #[arg(short = 't', long = "track")]
    pub track: Option<Option<String>>,

    /// Do not set up tracking.
    #[arg(long = "no-track")]
    pub no_track: bool,

    /// Show the current branch name.
    #[arg(long = "show-current")]
    pub show_current: bool,
}

/// Run the `branch` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let head = resolve_head(&repo.git_dir)?;

    if args.show_current {
        if let Some(name) = head.branch_name() {
            println!("{name}");
        }
        return Ok(());
    }

    if args.delete || args.force_delete {
        return delete_branch(&repo, &head, &args);
    }

    if args.rename || args.force_rename {
        return rename_branch(&repo, &head, &args);
    }

    // If a name is given and we're not listing, create a branch
    if let Some(ref name) = args.name {
        if !args.list {
            return create_branch(&repo, &head, name, args.start_point.as_deref(), &args);
        }
    }

    // Default: list branches
    list_branches(&repo, &head, &args)
}

/// List branches.
fn list_branches(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let current_branch = head.branch_name().unwrap_or("");

    // Collect local branches
    let mut branches: Vec<(String, ObjectId)> = Vec::new();
    if !args.remotes {
        collect_branches(&repo.git_dir.join("refs/heads"), "", &mut branches)?;
    }

    // Collect remote branches
    if args.remotes || args.all {
        let mut remote_branches = Vec::new();
        collect_branches(&repo.git_dir.join("refs/remotes"), "", &mut remote_branches)?;
        for (name, oid) in remote_branches {
            branches.push((format!("remotes/{name}"), oid));
        }
    }

    branches.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, oid) in &branches {
        let is_current = name == current_branch;
        let prefix = if is_current { "* " } else { "  " };

        if args.verbose > 0 {
            let short_oid = &oid.to_hex()[..7];
            let subject = commit_subject(&repo.odb, oid).unwrap_or_default();
            writeln!(out, "{prefix}{name} {short_oid} {subject}")?;
        } else {
            writeln!(out, "{prefix}{name}")?;
        }
    }

    Ok(())
}

/// Create a new branch.
fn create_branch(
    repo: &Repository,
    head: &HeadState,
    name: &str,
    start_point: Option<&str>,
    args: &Args,
) -> Result<()> {
    let ref_path = repo.git_dir.join("refs/heads").join(name);

    if ref_path.exists() && !args.force {
        bail!("A branch named '{name}' already exists.");
    }

    let oid = match start_point {
        Some(rev) => resolve_rev(repo, rev)?,
        None => *head
            .oid()
            .ok_or_else(|| anyhow::anyhow!("not a valid object name: 'HEAD'"))?,
    };

    if let Some(parent) = ref_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&ref_path, format!("{}\n", oid.to_hex()))?;

    if !args.quiet {
        // Silence by default (git branch is quiet on create)
    }

    Ok(())
}

/// Delete a branch.
fn delete_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let name = args
        .name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("branch name required"))?;

    let current = head.branch_name().unwrap_or("");
    if name == current {
        bail!(
            "Cannot delete branch '{name}' checked out at '{}'",
            repo.work_tree
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );
    }

    let ref_path = repo.git_dir.join("refs/heads").join(name);
    if !ref_path.exists() {
        bail!("branch '{name}' not found.");
    }

    let oid_str = fs::read_to_string(&ref_path)?.trim().to_owned();
    fs::remove_file(&ref_path)?;

    if !args.quiet {
        let short = &oid_str[..7.min(oid_str.len())];
        eprintln!("Deleted branch {name} (was {short}).");
    }

    Ok(())
}

/// Rename a branch.
fn rename_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let old_name = match &args.name {
        Some(n) => n.as_str(),
        None => head
            .branch_name()
            .ok_or_else(|| anyhow::anyhow!("no current branch to rename"))?,
    };

    let new_name = args
        .start_point
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("new branch name required"))?;

    let old_path = repo.git_dir.join("refs/heads").join(old_name);
    let new_path = repo.git_dir.join("refs/heads").join(new_name);

    if !old_path.exists() {
        bail!("branch '{old_name}' not found.");
    }
    if new_path.exists() && !args.force_rename {
        bail!("A branch named '{new_name}' already exists.");
    }

    let content = fs::read_to_string(&old_path)?;
    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&new_path, &content)?;
    fs::remove_file(&old_path)?;

    // Update HEAD if we renamed the current branch
    if head.branch_name() == Some(old_name) {
        let head_content = format!("ref: refs/heads/{new_name}\n");
        fs::write(repo.git_dir.join("HEAD"), head_content)?;
    }

    Ok(())
}

/// Collect branch names from a refs directory.
fn collect_branches(dir: &Path, prefix: &str, out: &mut Vec<(String, ObjectId)>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
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

/// Get the first line of a commit's message.
fn commit_subject(odb: &grit_lib::odb::Odb, oid: &ObjectId) -> Option<String> {
    let obj = odb.read(oid).ok()?;
    let commit = parse_commit(&obj.data).ok()?;
    commit.message.lines().next().map(String::from)
}

/// Resolve a revision to an OID.
fn resolve_rev(repo: &Repository, rev: &str) -> Result<ObjectId> {
    if let Ok(oid) = ObjectId::from_hex(rev) {
        return Ok(oid);
    }

    // Try refs/heads/
    let ref_path = repo.git_dir.join("refs/heads").join(rev);
    if let Ok(content) = fs::read_to_string(&ref_path) {
        if let Ok(oid) = ObjectId::from_hex(content.trim()) {
            return Ok(oid);
        }
    }

    // Try refs/tags/
    let tag_path = repo.git_dir.join("refs/tags").join(rev);
    if let Ok(content) = fs::read_to_string(&tag_path) {
        if let Ok(oid) = ObjectId::from_hex(content.trim()) {
            return Ok(oid);
        }
    }

    if rev == "HEAD" {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            return Ok(*oid);
        }
    }

    bail!("not a valid object name: '{rev}'");
}
