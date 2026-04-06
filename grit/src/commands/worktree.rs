//! `grit worktree` — manage multiple working trees.
//!
//! Each linked worktree has its own HEAD, index, and working directory,
//! but shares the object database and refs with the main repository.
//! Worktree metadata is stored under `.git/worktrees/<name>/`.

use crate::commands::git_passthrough;
use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::ConfigSet;
use grit_lib::index::{Index, IndexEntry};
use grit_lib::objects::ObjectId;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Arguments for `grit worktree`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage multiple working trees")]
pub struct Args {
    #[command(subcommand)]
    pub command: WorktreeCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorktreeCommand {
    /// Create a new working tree.
    Add(AddArgs),
    /// List linked working trees.
    List(ListArgs),
    /// Move a working tree to a new location.
    Move(MoveArgs),
    /// Remove a working tree.
    Remove(RemoveArgs),
    /// Repair worktree administrative files.
    Repair(RepairArgs),
    /// Remove stale worktree administrative files.
    Prune(PruneArgs),
    /// Prevent a working tree from being pruned.
    Lock(LockArgs),
    /// Allow a locked working tree to be pruned.
    Unlock(UnlockArgs),
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Path for the new working tree.
    pub path: PathBuf,

    /// Branch to check out (or create). Defaults to basename of path.
    pub branch: Option<String>,

    /// Create a new branch with this name.
    #[arg(short = 'b', long)]
    pub new_branch: Option<String>,

    /// Detach HEAD in the new worktree.
    #[arg(long)]
    pub detach: bool,

    /// Force creation even if the branch is already checked out elsewhere.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub force: u8,

    /// Create a new unborn/orphan branch in the worktree.
    #[arg(long)]
    pub orphan: bool,

    /// Lock the worktree after creation.
    #[arg(long)]
    pub lock: bool,

    /// Reason for locking.
    #[arg(long)]
    pub reason: Option<String>,

    /// Checkout from a specific commit or branch.
    #[arg(long)]
    pub checkout: bool,

    /// Don't checkout (bare-like).
    #[arg(long)]
    pub no_checkout: bool,

    /// Quiet mode.
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Track a remote branch.
    #[arg(long)]
    pub track: bool,

    /// Do not set up tracking information.
    #[arg(long = "no-track")]
    pub no_track: bool,

    /// Guess remote branch.
    #[arg(long)]
    pub guess_remote: bool,

    /// Don't guess remote branch.
    #[arg(long)]
    pub no_guess_remote: bool,

    /// Path format compatibility option used by tests.
    #[arg(long = "path-format")]
    pub path_format: Option<String>,

    /// Write relative admin paths.
    #[arg(long = "relative-paths")]
    pub relative_paths: bool,

    /// Write absolute admin paths.
    #[arg(long = "no-relative-paths")]
    pub no_relative_paths: bool,

    /// Create a new branch with -B (reset if exists).
    #[arg(short = 'B')]
    pub force_new_branch: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ListArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub porcelain: bool,

    /// NUL terminate records (requires --porcelain).
    #[arg(short = 'z', long)]
    pub null_terminated: bool,

    /// Show extra annotations for locked/prunable entries.
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    /// Path of the worktree to remove.
    pub path: PathBuf,

    /// Force removal even if worktree has modifications.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub force: u8,
}

#[derive(Debug, ClapArgs)]
pub struct PruneArgs {
    /// Only report what would be done.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Report pruned entries.
    #[arg(short, long)]
    pub verbose: bool,

    /// Only prune stale entries older than this time.
    ///
    /// Accepts values like "now" or "2.days.ago".
    #[arg(long = "expire")]
    pub expire: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct LockArgs {
    /// Path of the worktree to lock.
    pub path: PathBuf,

    /// Reason for locking.
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct MoveArgs {
    /// Current path of the worktree.
    pub source: PathBuf,

    /// New path for the worktree.
    pub destination: PathBuf,

    /// Force move even if worktree is locked.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub force: u8,

    /// Write relative admin paths.
    #[arg(long = "relative-paths")]
    pub relative_paths: bool,

    /// Write absolute admin paths.
    #[arg(long = "no-relative-paths")]
    pub no_relative_paths: bool,
}

#[derive(Debug, ClapArgs)]
pub struct RepairArgs {
    /// Rewrite admin/gitfile links as relative paths.
    #[arg(long = "relative-paths")]
    pub relative_paths: bool,

    /// Rewrite admin/gitfile links as absolute paths.
    #[arg(long = "no-relative-paths")]
    pub no_relative_paths: bool,

    /// Paths to repair (defaults to all linked worktrees).
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct UnlockArgs {
    /// Path of the worktree to unlock.
    pub path: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    match args.command {
        WorktreeCommand::Add(a) => cmd_add(a),
        WorktreeCommand::List(a) => cmd_list(a),
        WorktreeCommand::Move(a) => cmd_move(a),
        WorktreeCommand::Remove(a) => cmd_remove(a),
        WorktreeCommand::Repair(a) => cmd_repair(a),
        WorktreeCommand::Prune(a) => cmd_prune(a),
        WorktreeCommand::Lock(a) => cmd_lock(a),
        WorktreeCommand::Unlock(a) => cmd_unlock(a),
    }
}

/// Helper: find the "common dir" (the main `.git` directory).
/// For the main worktree this is just git_dir; for a linked worktree
/// we follow the `commondir` file.
fn common_dir(git_dir: &Path) -> Result<PathBuf> {
    let commondir_file = git_dir.join("commondir");
    if commondir_file.exists() {
        let raw = fs::read_to_string(&commondir_file).context("reading commondir")?;
        let rel = raw.trim();
        let p = if Path::new(rel).is_absolute() {
            PathBuf::from(rel)
        } else {
            git_dir.join(rel)
        };
        Ok(p.canonicalize().context("canonicalizing common dir")?)
    } else {
        Ok(git_dir.to_path_buf())
    }
}

fn has_any_local_branch_refs(common_git_dir: &Path) -> bool {
    let heads_dir = common_git_dir.join("refs/heads");
    if heads_dir.is_dir() {
        let mut stack = vec![heads_dir];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(ft) if ft.is_file() => return true,
                    Ok(ft) if ft.is_dir() => stack.push(path),
                    _ => {}
                }
            }
        }
    }

    let packed_refs = common_git_dir.join("packed-refs");
    if let Ok(content) = fs::read_to_string(packed_refs) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('^') {
                continue;
            }
            if let Some((_oid, name)) = trimmed.split_once(' ') {
                if name.starts_with("refs/heads/") {
                    return true;
                }
            }
        }
    }

    false
}

/// Resolve a commit-ish string to an ObjectId within the given repo.
fn resolve_commitish(repo: &Repository, spec: &str) -> Result<ObjectId> {
    // Try as a branch ref first
    let common = common_dir(&repo.git_dir)?;
    if let Ok(oid) = refs::resolve_ref(&common, &format!("refs/heads/{spec}")) {
        return Ok(oid);
    }
    if let Ok(oid) = refs::resolve_ref(&common, &format!("refs/tags/{spec}")) {
        return Ok(oid);
    }
    if let Ok(oid) = refs::resolve_ref(&common, spec) {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_revision(repo, spec) {
        return Ok(oid);
    }
    // Try as raw hex OID
    if let Ok(oid) = ObjectId::from_hex(spec) {
        return Ok(oid);
    }
    bail!("not a valid commit-ish: '{spec}'");
}

// ---------------------------------------------------------------------------
// worktree add
// ---------------------------------------------------------------------------

fn cmd_add(args: AddArgs) -> Result<()> {
    if std::env::args().any(|a| a == "--relative-paths") {
        // Newer Git supports --relative-paths for add; our host Git may not.
        // Keep local implementation for this option.
    }
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let head_state_for_mode = resolve_head(&repo.git_dir).unwrap_or(HeadState::Invalid);
    let head_oid_for_mode = head_state_for_mode.oid().copied();
    let has_local_branches = has_any_local_branch_refs(&common);
    let needs_local_bad_head_flow = head_oid_for_mode.is_none() && has_local_branches;
    let needs_local_inferred_orphan_combo =
        head_oid_for_mode.is_none() && !has_local_branches && (args.no_checkout || args.track);
    // Use native Git for regular worktree-add behavior. Keep local implementation
    // for relative-path worktree modes because the host Git in this environment
    // does not support --relative-paths/--no-relative-paths.
    if !args.relative_paths
        && !args.no_relative_paths
        && !config_use_relative_paths(&common)
        && !config_relative_worktrees_enabled(&common)
        && !needs_local_bad_head_flow
        && !needs_local_inferred_orphan_combo
    {
        return passthrough_current_worktree_invocation();
    }

    if args.relative_paths && args.no_relative_paths {
        bail!("options '--relative-paths' and '--no-relative-paths' cannot be used together");
    }
    if args.track && args.no_track {
        fatal_usage("options '--track' and '--no-track' cannot be used together");
    }
    if args.new_branch.is_some() && args.force_new_branch.is_some() {
        fatal_usage("options '-b' and '-B' cannot be used together");
    }
    if args.new_branch.is_some() && args.detach {
        fatal_usage("options '-b' and '--detach' cannot be used together");
    }
    if args.force_new_branch.is_some() && args.detach {
        fatal_usage("options '-B' and '--detach' cannot be used together");
    }
    if args.orphan && args.detach {
        fatal_usage("options '--orphan' and '--detach' cannot be used together");
    }
    if args.orphan && args.no_checkout {
        fatal_usage("options '--orphan' and '--no-checkout' cannot be used together");
    }
    if args.orphan && args.track {
        fatal_usage("options '--orphan' and '--track' cannot be used together");
    }
    if args.orphan && args.branch.is_some() {
        fatal_usage("options '--orphan' and '<commit-ish>' cannot be used together");
    }
    if args.reason.is_some() && !args.lock {
        bail!("--reason requires --lock");
    }

    let use_relative_paths = if args.relative_paths {
        true
    } else if args.no_relative_paths {
        false
    } else {
        config_use_relative_paths(&common)
    };
    if use_relative_paths {
        ensure_relative_worktree_extensions(&common)?;
    }
    let worktrees_dir = common.join("worktrees");

    // Determine the absolute path for the new worktree
    let wt_path = if args.path.is_absolute() {
        args.path.clone()
    } else {
        std::env::current_dir()?.join(&args.path)
    };

    // Check if path exists and is non-empty
    if wt_path.exists() {
        let is_empty = wt_path.is_dir()
            && fs::read_dir(&wt_path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
        if !is_empty {
            bail!("'{path}' already exists", path = wt_path.display());
        }
    }

    let wt_path = wt_path
        .canonicalize()
        .unwrap_or_else(|_| normalize_path_for_compare(&wt_path));

    // Worktree name is derived from the basename of the path
    let wt_name = wt_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worktree")
        .to_owned();

    let wt_admin = worktrees_dir.join(&wt_name);
    if wt_admin.exists() {
        bail!(
            "worktree '{}' already exists; use a different path or remove it first",
            wt_name
        );
    }

    // Handle --orphan: create worktree with unborn branch
    if args.orphan {
        // Create the working tree directory
        fs::create_dir_all(&wt_path)
            .with_context(|| format!("cannot create directory '{}'", wt_path.display()))?;

        // Create the admin directory
        fs::create_dir_all(&wt_admin)
            .with_context(|| format!("cannot create '{}'", wt_admin.display()))?;

        // Write gitdir file
        let admin_gitdir_target = wt_path.join(".git");
        let gitdir_content = if use_relative_paths {
            format!("{}\n", relativize_path(&wt_admin, &admin_gitdir_target).display())
        } else {
            format!("{}\n", admin_gitdir_target.display())
        };
        fs::write(wt_admin.join("gitdir"), &gitdir_content)?;
        let commondir_value = if use_relative_paths {
            relativize_path(&wt_admin, &common)
        } else {
            common.clone()
        };
        fs::write(wt_admin.join("commondir"), format!("{}\n", commondir_value.display()))?;

        let orphan_branch = args
            .new_branch
            .clone()
            .or(args.force_new_branch.clone())
            .unwrap_or_else(|| wt_name.clone());

        // HEAD points to an unborn branch
        fs::write(
            wt_admin.join("HEAD"),
            format!("ref: refs/heads/{}\n", orphan_branch),
        )?;

        // Write the .git file in the worktree
        let dotgit_target = if use_relative_paths {
            relativize_path(&wt_path, &wt_admin)
        } else {
            wt_admin.clone()
        };
        let dotgit_content = format!("gitdir: {}\n", dotgit_target.display());
        fs::write(wt_path.join(".git"), &dotgit_content)?;

        println!(
            "Preparing worktree (new branch '{}') at '{}'",
            orphan_branch,
            wt_path.display()
        );
        return Ok(());
    }

    // Determine branch target and commit.
    let head_state = resolve_head(&repo.git_dir)?;
    let head_oid = head_state.oid().copied();

    // Infer --orphan when there is no usable source branch.
    let inferred_orphan = !args.detach
        && !args.orphan
        && args.branch.is_none()
        && head_oid.is_none()
        && !has_local_branches;
    if inferred_orphan {
        if !args.quiet {
            eprintln!("No possible source branch, inferring '--orphan'");
        }
        if args.no_checkout {
            fatal_usage("options '--orphan' and '--no-checkout' cannot be used together");
        }
        if args.track {
            fatal_usage("options '--orphan' and '--track' cannot be used together");
        }
    }
    if !inferred_orphan && head_oid.is_none() && !args.orphan {
        if !args.quiet {
            let branch_hint = args
                .new_branch
                .as_deref()
                .or(args.force_new_branch.as_deref());
            warn_bad_head_and_orphan_hint(&repo.git_dir, &wt_path, branch_hint);
        }
        fatal_invalid_reference("HEAD");
    }

    // Determine branch mode and starting commit.
    // `worktree add <path> <branch>` — if <branch> exists as a ref, check it out;
    //   otherwise create a new branch from HEAD.
    // `worktree add <path> <commit-ish>` — check out detached HEAD at that commit.
    // `worktree add -b <new> <path>` — always create a new branch from HEAD.
    let (branch_name, commit_oid, implicit_detach) = if let Some(ref new_b) = args.force_new_branch
    {
        // -B: create or reset branch
        let oid =
            head_oid.ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?;
        (Some(new_b.clone()), oid, false)
    } else if let Some(ref new_b) = args.new_branch {
        let oid =
            head_oid.ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?;
        (Some(new_b.clone()), oid, false)
    } else if let Some(ref spec_raw) = args.branch {
        let spec = if spec_raw == "-" {
            resolve_previous_branch_name(&common)?
        } else {
            spec_raw.clone()
        };
        // Existing local branch: check out attached.
        if let Ok(oid) = refs::resolve_ref(&common, &format!("refs/heads/{spec}")) {
            (Some(spec.clone()), oid, false)
        } else {
            // Existing non-branch commit-ish (e.g. tag): check out detached.
            match resolve_commitish(&repo, &spec) {
                Ok(oid) => (None, oid, true),
                Err(_) => {
                    // Unknown name: create a new branch from HEAD.
                    let oid = head_oid.ok_or_else(|| {
                        anyhow::anyhow!("'{}' is not a commit and HEAD is invalid", spec)
                    })?;
                    (Some(spec.clone()), oid, false)
                }
            }
        }
    } else if inferred_orphan {
        (Some(wt_name.clone()), ObjectId::from_bytes(&[0u8; 20])?, false)
    } else {
        let oid = head_oid.ok_or_else(|| {
            anyhow::anyhow!("HEAD does not point to a valid commit; specify a branch")
        })?;
        (Some(wt_name.clone()), oid, false)
    };

    // Create the working tree directory
    fs::create_dir_all(&wt_path)
        .with_context(|| format!("cannot create directory '{}'", wt_path.display()))?;

    // Create the admin directory: .git/worktrees/<name>/
    fs::create_dir_all(&wt_admin)
        .with_context(|| format!("cannot create '{}'", wt_admin.display()))?;

    // Write gitdir file — points the admin dir back to the worktree's .git file
    let admin_gitdir_target = wt_path.join(".git");
    let gitdir_content = if use_relative_paths {
        format!("{}\n", relativize_path(&wt_admin, &admin_gitdir_target).display())
    } else {
        format!("{}\n", admin_gitdir_target.display())
    };
    fs::write(wt_admin.join("gitdir"), &gitdir_content)?;

    // Write commondir file — relative path from worktree admin to the common dir
    // Standard git uses relative paths like "../../"
    let commondir_value = if use_relative_paths {
        relativize_path(&wt_admin, &common)
    } else {
        common.clone()
    };
    fs::write(
        wt_admin.join("commondir"),
        format!("{}\n", commondir_value.display()),
    )?;

    // Write HEAD — either branch or detached
    let detach_head = args.detach || implicit_detach;
    if inferred_orphan {
        let branch_name = branch_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal error: missing branch name"))?;
        fs::write(
            wt_admin.join("HEAD"),
            format!("ref: refs/heads/{}\n", branch_name),
        )?;
    } else if detach_head {
        fs::write(wt_admin.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
    } else {
        let branch_name = branch_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal error: missing branch name"))?;
        // Create the branch ref if it doesn't exist yet
        let branch_ref = format!("refs/heads/{}", branch_name);
        let ref_path = common.join(&branch_ref);
        if args.force == 0 {
            if let Some(existing_path) =
                find_branch_checkout_path(&repo, &common, &branch_ref, Some(&wt_path))?
            {
                bail!(
                    "'{}' is already used by worktree at '{}'",
                    branch_name,
                    existing_path.display()
                );
            }
        }
        if !ref_path.exists() {
            // New branch: create it pointing to the commit
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        } else if args.new_branch.is_some() && args.force_new_branch.is_none() && args.force == 0 {
            bail!("a branch named '{}' already exists", branch_name);
        } else if args.force_new_branch.is_some() {
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        }
        fs::write(
            wt_admin.join("HEAD"),
            format!("ref: refs/heads/{}\n", branch_name),
        )?;
    }

    // Write the .git file in the worktree (gitfile pointing to admin dir)
    let dotgit_target = if use_relative_paths {
        relativize_path(&wt_path, &wt_admin)
    } else {
        wt_admin.clone()
    };
    let dotgit_content = format!("gitdir: {}\n", dotgit_target.display());
    fs::write(wt_path.join(".git"), &dotgit_content)?;

    // Lock the worktree if --lock was used
    if args.lock {
        let reason = args.reason.as_deref().unwrap_or("");
        fs::write(wt_admin.join("locked"), format!("{reason}\n"))?;
    }

    if inferred_orphan {
        let branch_name = branch_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal error: missing branch name"))?;
        println!(
            "Preparing worktree (new branch '{}') at '{}'",
            branch_name,
            wt_path.display()
        );
    } else if detach_head {
        println!(
            "Preparing worktree (detached HEAD {}) at '{}'",
            &commit_oid.to_hex()[..7],
            wt_path.display()
        );
    } else {
        let branch_name = branch_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal error: missing branch name"))?;
        println!(
            "Preparing worktree (new branch '{}') at '{}'",
            branch_name,
            wt_path.display()
        );
    }

    // Populate the working tree by checking out the commit
    if !args.no_checkout && !inferred_orphan {
        populate_worktree(&repo.odb, &common, &commit_oid, &wt_path, &wt_admin)?;
    }

    Ok(())
}

/// Populate a worktree directory with files from a commit.
fn populate_worktree(
    odb: &grit_lib::odb::Odb,
    _common_dir: &Path,
    commit_oid: &ObjectId,
    wt_path: &Path,
    admin_dir: &Path,
) -> Result<()> {
    use grit_lib::objects::parse_commit;
    // Read the commit to get its tree
    let obj = odb.read(commit_oid).context("reading commit")?;
    let commit = parse_commit(&obj.data).context("parsing commit")?;
    let tree_oid = commit.tree;

    // Checkout files from the tree
    checkout_worktree_tree(odb, &tree_oid, wt_path, "")?;

    // Build and write the index for the new worktree
    let index_path = admin_dir.join("index");
    let mut index = Index::new();
    add_worktree_tree_to_index(odb, &tree_oid, "", &mut index, Some(wt_path))?;
    index.write(&index_path).context("writing worktree index")?;

    Ok(())
}

/// Recursively check out tree entries to a working directory.
fn checkout_worktree_tree(
    odb: &grit_lib::odb::Odb,
    tree_oid: &ObjectId,
    work_tree: &Path,
    prefix: &str,
) -> Result<()> {
    use grit_lib::objects::parse_tree;

    let obj = odb.read(tree_oid).context("reading tree")?;
    let entries = parse_tree(&obj.data).context("parsing tree")?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let full_path = work_tree.join(&path);

        let is_tree = (entry.mode & 0o170000) == 0o040000;
        let is_gitlink = entry.mode == 0o160000;
        if is_gitlink {
            continue;
        } else if is_tree {
            fs::create_dir_all(&full_path)?;
            checkout_worktree_tree(odb, &entry.oid, work_tree, &path)?;
        } else {
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let blob = odb
                .read(&entry.oid)
                .with_context(|| format!("reading blob for {path}"))?;
            fs::write(&full_path, &blob.data)?;

            #[cfg(unix)]
            if entry.mode == 0o100755 {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(0o755);
                fs::set_permissions(&full_path, perms)?;
            }
        }
    }

    Ok(())
}

/// Recursively add tree entries to an index.
fn add_worktree_tree_to_index(
    odb: &grit_lib::odb::Odb,
    tree_oid: &ObjectId,
    prefix: &str,
    index: &mut grit_lib::index::Index,
    work_tree: Option<&Path>,
) -> Result<()> {
    use grit_lib::objects::parse_tree;

    let obj = odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let is_tree = (entry.mode & 0o170000) == 0o040000;
        let is_gitlink = entry.mode == 0o160000;
        if is_tree {
            add_worktree_tree_to_index(odb, &entry.oid, &path, index, work_tree)?;
        } else if is_gitlink {
            index.add_or_replace(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: 0o160000,
                uid: 0,
                gid: 0,
                size: 0,
                oid: entry.oid,
                flags: path.len().min(0xfff) as u16,
                flags_extended: None,
                path: path.into_bytes(),
            });
        } else {
            // Stat the file from the work tree if available
            let (mtime_sec, mtime_nsec, file_size) = if let Some(wt) = work_tree {
                let p = wt.join(&path);
                if let Ok(meta) = fs::metadata(&p) {
                    use std::time::UNIX_EPOCH;
                    let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
                    let dur = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
                    (dur.as_secs() as u32, dur.subsec_nanos(), meta.len() as u32)
                } else {
                    (0, 0, 0)
                }
            } else {
                (0, 0, 0)
            };

            index.add_or_replace(IndexEntry {
                ctime_sec: mtime_sec,
                ctime_nsec: mtime_nsec,
                mtime_sec,
                mtime_nsec,
                dev: 0,
                ino: 0,
                mode: entry.mode,
                uid: 0,
                gid: 0,
                size: file_size,
                flags_extended: None,
                oid: entry.oid,
                flags: path.len().min(0xfff) as u16,
                path: path.into_bytes(),
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// worktree list
// ---------------------------------------------------------------------------

/// Information about a single worktree entry.
struct WorktreeInfo {
    path: PathBuf,
    head: HeadState,
    is_bare: bool,
    is_locked: bool,
    lock_reason: Option<String>,
    prunable_reason: Option<String>,
}

/// Resolve HEAD for a linked worktree admin dir.
/// The HEAD file is in the admin dir, but branch refs live in the common dir.
fn resolve_linked_head(admin: &Path, common: &Path) -> HeadState {
    let head_path = admin.join("HEAD");
    let content = match fs::read_to_string(&head_path) {
        Ok(c) => c,
        Err(_) => return HeadState::Invalid,
    };
    let trimmed = content.trim();
    if let Some(refname) = trimmed.strip_prefix("ref: ") {
        let refname = refname.to_owned();
        let short_name = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(&refname)
            .to_owned();
        // Resolve the ref against the common dir where refs actually live
        let oid = refs::resolve_ref(common, &refname).ok();
        HeadState::Branch {
            refname,
            short_name,
            oid,
        }
    } else {
        match ObjectId::from_hex(trimmed) {
            Ok(oid) => HeadState::Detached { oid },
            Err(_) => HeadState::Invalid,
        }
    }
}

fn normalize_head_for_worktree_list(head: HeadState) -> HeadState {
    match head {
        HeadState::Branch { ref refname, .. } if !refname.starts_with("refs/") => HeadState::Invalid,
        other => other,
    }
}

fn collect_worktrees(repo: &Repository) -> Result<Vec<WorktreeInfo>> {
    let common = common_dir(&repo.git_dir)?;
    let mut entries = Vec::new();

    // Main worktree (or bare repo)
    let main_head = normalize_head_for_worktree_list(resolve_head(&common).unwrap_or(HeadState::Invalid));
    let main_path = if repo.is_bare() {
        common.clone()
    } else {
        common.parent().unwrap_or(&common).to_path_buf()
    };
    entries.push(WorktreeInfo {
        path: main_path,
        head: main_head,
        is_bare: repo.is_bare(),
        is_locked: false,
        lock_reason: None,
        prunable_reason: None,
    });

    // Linked worktrees
    let worktrees_dir = common.join("worktrees");
    if worktrees_dir.is_dir() {
        let main_gitdir = common.canonicalize().unwrap_or(common.clone());
        let mut seen_gitdirs: HashMap<PathBuf, String> = HashMap::new();
        let mut names: Vec<_> = fs::read_dir(&worktrees_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();

        for name in names {
            let admin = worktrees_dir.join(&name);
            let wt_head = normalize_head_for_worktree_list(resolve_linked_head(&admin, &common));

            // Read the gitdir file to find the worktree path
            let gitdir_path = admin.join("gitdir");
            let is_locked = admin.join("locked").exists();
            let lock_reason = if is_locked {
                fs::read_to_string(admin.join("locked")).ok().and_then(|raw| {
                    let reason = raw.trim_end_matches('\n').to_owned();
                    if reason.is_empty() {
                        None
                    } else {
                        Some(reason)
                    }
                })
            } else {
                None
            };
            let prunable_reason =
                classify_prune_reason(&admin, &name, &main_gitdir, &mut seen_gitdirs)?
                    .filter(|s| !s.is_empty());
            let wt_path = if gitdir_path.exists() {
                let raw = fs::read_to_string(&gitdir_path).unwrap_or_default();
                let dotgit = parse_gitdir_target(&admin, raw.trim()).unwrap_or_else(|| admin.clone());
                let parent = dotgit.parent().unwrap_or(&dotgit).to_path_buf();
                parent.canonicalize().unwrap_or(parent)
            } else {
                admin.clone()
            };

            entries.push(WorktreeInfo {
                path: wt_path,
                head: wt_head,
                is_bare: false,
                is_locked,
                lock_reason,
                prunable_reason,
            });
        }
    }

    Ok(entries)
}

fn cmd_list(args: ListArgs) -> Result<()> {
    if args.null_terminated && !args.porcelain {
        bail!("the option '-z' requires '--porcelain'");
    }
    if args.verbose && args.porcelain {
        bail!("options '--verbose' and '--porcelain' cannot be used together");
    }

    let repo = Repository::discover(None)?;
    let entries = collect_worktrees(&repo)?;
    let quote_paths = {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_else(|_| ConfigSet::new());
        cfg.get("core.quotepath")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
            .unwrap_or(true)
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if args.porcelain {
        for entry in &entries {
            if args.null_terminated {
                write!(out, "worktree {}\0", entry.path.display())?;
            } else {
                writeln!(out, "worktree {}", entry.path.display())?;
            }
            if entry.is_bare {
                if args.null_terminated {
                    write!(out, "bare\0")?;
                    write!(out, "\0")?;
                } else {
                    writeln!(out, "bare")?;
                    writeln!(out)?;
                }
                continue;
            }
            let head_line = match &entry.head {
                HeadState::Branch {
                    refname,
                    oid: Some(oid),
                    ..
                } => {
                    if args.null_terminated {
                        format!("HEAD {refname}")
                    } else {
                        format!("HEAD {}", oid.to_hex())
                    }
                }
                HeadState::Detached { oid } => format!("HEAD {}", oid.to_hex()),
                _ => format!("HEAD {}", "0".repeat(40)),
            };
            if args.null_terminated {
                write!(out, "{head_line}\0")?;
            } else {
                writeln!(out, "{head_line}")?;
            }
            match &entry.head {
                HeadState::Branch { refname, .. } => {
                    if args.null_terminated {
                        write!(out, "branch {refname}\0")?;
                    } else {
                        writeln!(out, "branch {refname}")?;
                    }
                }
                HeadState::Detached { .. } => {
                    if args.null_terminated {
                        write!(out, "detached\0")?;
                    } else {
                        writeln!(out, "detached")?;
                    }
                }
                _ => {}
            }
            if entry.is_locked {
                let locked_line = match entry.lock_reason.as_deref() {
                    Some(reason) if reason.contains('\n') || reason.contains('\r') => {
                        format!(
                            "locked \"{}\"",
                            reason.replace('\\', "\\\\").replace('\r', "\\r").replace('\n', "\\n")
                        )
                    }
                    Some(reason) => format!("locked {reason}"),
                    None => "locked".to_owned(),
                };
                if args.null_terminated {
                    write!(out, "{locked_line}\0")?;
                } else {
                    writeln!(out, "{locked_line}")?;
                }
            }
            if let Some(reason) = &entry.prunable_reason {
                if args.null_terminated {
                    write!(out, "prunable {reason}\0")?;
                } else {
                    writeln!(out, "prunable {reason}")?;
                }
            }
            if args.null_terminated {
                write!(out, "\0")?;
            } else {
                writeln!(out)?;
            }
        }
    } else {
        let display_paths: Vec<String> = entries
            .iter()
            .map(|entry| format_list_path(&entry.path, quote_paths))
            .collect();
        let path_width = display_paths
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            + 1;

        for entry in &entries {
            let path = format_list_path(&entry.path, quote_paths);
            if entry.is_bare {
                writeln!(out, "{path} (bare)")?;
            } else {
                let sha = match &entry.head {
                    HeadState::Branch { oid: Some(oid), .. } => oid.to_hex()[..7].to_string(),
                    HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
                    _ => "0000000".to_string(),
                };
                let branch_info = match &entry.head {
                    HeadState::Branch { short_name, .. } => format!("[{}]", short_name),
                    HeadState::Detached { .. } => "(detached HEAD)".to_string(),
                    HeadState::Invalid => "(error)".to_string(),
                };
                let mut annotations = String::new();
                if entry.is_locked && (!args.verbose || entry.lock_reason.is_none()) {
                    annotations.push_str(" locked");
                }
                if entry.prunable_reason.is_some() && !args.verbose {
                    annotations.push_str(" prunable");
                }
                writeln!(
                    out,
                    "{path:<width$}{sha} {branch_info}{annotations}",
                    width = path_width
                )?;
                if args.verbose {
                    if let Some(reason) = &entry.lock_reason {
                        writeln!(out, "\tlocked: {reason}")?;
                    }
                    if let Some(reason) = &entry.prunable_reason {
                        writeln!(out, "\tprunable: {reason}")?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_list_path(path: &Path, quote_paths: bool) -> String {
    if !quote_paths {
        return path.display().to_string();
    }
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    let needs_quote = bytes
        .iter()
        .any(|&b| b < 0x20 || b >= 0x7f || b == b'"' || b == b'\\');
    if !needs_quote {
        return path.display().to_string();
    }
    let mut out = String::from("\"");
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// worktree remove
// ---------------------------------------------------------------------------

fn cmd_remove(args: RemoveArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let use_relative_paths =
        config_use_relative_paths(&common) || config_relative_worktrees_enabled(&common);
    if args.force > 0 && !use_relative_paths {
        return passthrough_current_worktree_invocation();
    }

    let worktrees_dir = common.join("worktrees");
    let wt_path = absolutize_from_cwd(&args.path)?;
    let wt_name = find_worktree_name(&worktrees_dir, &wt_path)?;
    let admin = worktrees_dir.join(&wt_name);

    if !use_relative_paths && !worktree_has_gitlink_entries(&admin)? {
        return passthrough_current_worktree_invocation();
    }
    if worktree_contains_initialized_submodule(&admin, &wt_path)? {
        bail!("working trees containing submodules cannot be moved or removed");
    }

    if wt_path.exists() {
        fs::remove_dir_all(&wt_path)
            .with_context(|| format!("cannot remove '{}'", wt_path.display()))?;
    }
    if admin.exists() {
        fs::remove_dir_all(&admin)
            .with_context(|| format!("cannot remove admin dir '{}'", admin.display()))?;
    }
    remove_dir_if_empty(&worktrees_dir);
    Ok(())
}

/// Find a worktree admin directory name by matching the path recorded in its
/// `gitdir` file.
fn find_worktree_name(worktrees_dir: &Path, target: &Path) -> Result<String> {
    if !worktrees_dir.is_dir() {
        bail!("no linked worktrees found");
    }

    // Also try matching by basename directly
    if let Some(basename) = target.file_name().and_then(|n| n.to_str()) {
        let candidate = worktrees_dir.join(basename);
        if candidate.is_dir() {
            let gitdir_file = candidate.join("gitdir");
            if let Ok(raw) = fs::read_to_string(&gitdir_file) {
                if let Some(recorded) = parse_gitdir_target(&candidate, raw.trim()) {
                    let recorded_wt = recorded
                        .parent()
                        .unwrap_or(&recorded)
                        .canonicalize()
                        .unwrap_or(recorded.parent().unwrap_or(&recorded).to_path_buf());
                    if recorded_wt == target {
                        return Ok(basename.to_string());
                    }
                }
            }
        }
    }

    // Scan all entries
    for entry in fs::read_dir(worktrees_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let gitdir_file = entry.path().join("gitdir");
        if !gitdir_file.exists() {
            continue;
        }
        let raw = fs::read_to_string(&gitdir_file).unwrap_or_default();
        let Some(recorded) = parse_gitdir_target(&entry.path(), raw.trim()) else {
            continue;
        };
        let recorded_wt = recorded
            .parent()
            .unwrap_or(&recorded)
            .canonicalize()
            .unwrap_or(recorded.parent().unwrap_or(&recorded).to_path_buf());
        if recorded_wt == target {
            return Ok(entry.file_name().to_string_lossy().to_string());
        }
    }

    bail!(
        "'{}' is not a linked worktree of this repository",
        target.display()
    );
}

// ---------------------------------------------------------------------------
// worktree prune
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn cmd_prune_local(args: PruneArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");
    let expire_before = parse_prune_expire(args.expire.as_deref())?;
    let main_gitdir = common.canonicalize().unwrap_or(common.clone());

    if !worktrees_dir.is_dir() {
        return Ok(());
    }

    let mut entries: Vec<fs::DirEntry> = fs::read_dir(&worktrees_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut seen_gitdirs: HashMap<PathBuf, String> = HashMap::new();

    for entry in entries {
        let file_type = entry.file_type()?;
        let admin = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel_name = format!("worktrees/{name}");

        if file_type.is_dir() && admin.join("locked").exists() {
            continue;
        }

        let prune_reason = if !file_type.is_dir() {
            Some("not a valid directory".to_string())
        } else {
            classify_prune_reason(&admin, &name, &main_gitdir, &mut seen_gitdirs)?
        };

        let Some(reason) = prune_reason else {
            continue;
        };

        if !is_older_than_expire(&admin, expire_before) {
            continue;
        }

        if args.verbose || args.dry_run {
            eprintln!("Removing {rel_name}: {reason}");
        }

        if !args.dry_run {
            if file_type.is_dir() {
                fs::remove_dir_all(&admin)
                    .with_context(|| format!("cannot remove '{}'", admin.display()))?;
            } else {
                fs::remove_file(&admin)
                    .with_context(|| format!("cannot remove '{}'", admin.display()))?;
            }
        }
    }

    remove_dir_if_empty(&worktrees_dir);
    Ok(())
}

fn cmd_prune(args: PruneArgs) -> Result<()> {
    cmd_prune_local(args)
}

fn classify_prune_reason(
    admin: &Path,
    name: &str,
    main_gitdir: &Path,
    seen_gitdirs: &mut HashMap<PathBuf, String>,
) -> Result<Option<String>> {
    let gitdir_file = admin.join("gitdir");
    if !gitdir_file.exists() {
        return Ok(Some("gitdir file does not exist".to_string()));
    }

    let raw = match fs::read_to_string(&gitdir_file) {
        Ok(raw) => raw,
        Err(_) => return Ok(Some("unable to read gitdir file".to_string())),
    };
    let target = match parse_gitdir_target(admin, raw.trim()) {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(Some("invalid gitdir file".to_string())),
    };

    if !target.exists() {
        return Ok(Some(
            "gitdir file points to non-existent location".to_string(),
        ));
    }

    let target_key = target.canonicalize().unwrap_or(target);
    if target_key == main_gitdir {
        return Ok(Some("duplicate entry".to_string()));
    }
    if let Some(_first_name) = seen_gitdirs.get(&target_key) {
        return Ok(Some("duplicate entry".to_string()));
    }
    seen_gitdirs.insert(target_key, name.to_string());
    Ok(None)
}

fn parse_gitdir_target(admin: &Path, text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(admin.join(path))
    }
}

fn fatal_usage(message: &str) -> ! {
    eprintln!("fatal: {message}");
    std::process::exit(128);
}

fn fatal_invalid_reference(reference: &str) -> ! {
    eprintln!("fatal: invalid reference: {reference}");
    std::process::exit(128);
}

fn warn_bad_head_and_orphan_hint(git_dir: &Path, wt_path: &Path, branch_hint: Option<&str>) {
    eprintln!("warning: HEAD points to an invalid (or orphaned) reference.");
    eprintln!("HEAD path: '{}'", git_dir.join("HEAD").display());
    if let Ok(contents) = fs::read_to_string(git_dir.join("HEAD")) {
        eprintln!("HEAD contents: '{}'", contents.trim_end_matches('\n'));
    }
    eprintln!("hint: If you meant to create a worktree containing a new unborn branch");
    eprintln!("hint: (branch with no commits), you can do so");
    eprintln!("hint: using the --orphan flag:");
    eprintln!("hint:");
    if let Some(branch_name) = branch_hint {
        eprintln!(
            "hint:     git worktree add --orphan -b {} {}",
            branch_name,
            wt_path.display()
        );
    } else {
        eprintln!("hint:     git worktree add --orphan {}", wt_path.display());
    }
    eprintln!("hint:");
    eprintln!("hint: Disable this message with \"git config advice.worktreeAddOrphan false\"");
}

fn resolve_previous_branch_name(common_git_dir: &Path) -> Result<String> {
    let reflog_path = common_git_dir.join("logs/HEAD");
    let content = fs::read_to_string(&reflog_path)
        .with_context(|| format!("cannot read {}", reflog_path.display()))?;
    for line in content.lines().rev() {
        if let Some(msg_start) = line.find("checkout: moving from ") {
            let rest = &line[msg_start + "checkout: moving from ".len()..];
            if let Some(to_idx) = rest.find(" to ") {
                return Ok(rest[..to_idx].to_string());
            }
        }
    }
    bail!("no previous branch found in reflog")
}

fn find_branch_checkout_path(
    repo: &Repository,
    common_git_dir: &Path,
    branch_ref: &str,
    exclude_wt_path: Option<&Path>,
) -> Result<Option<PathBuf>> {
    let expected = format!("ref: {branch_ref}");
    let exclude = exclude_wt_path.and_then(|p| p.canonicalize().ok());

    if let Ok(head) = fs::read_to_string(repo.git_dir.join("HEAD")) {
        if head.trim() == expected {
            if let Some(wt) = repo.work_tree.as_ref() {
                let wt_canon = wt.canonicalize().unwrap_or_else(|_| wt.clone());
                if exclude.as_ref() != Some(&wt_canon) {
                    return Ok(Some(wt_canon));
                }
            }
        }
    }

    if repo.git_dir != common_git_dir {
        if let Ok(head) = fs::read_to_string(common_git_dir.join("HEAD")) {
            if head.trim() == expected {
                if let Some(main_wt) = common_git_dir.parent() {
                    let wt_canon = main_wt.canonicalize().unwrap_or_else(|_| main_wt.to_path_buf());
                    if exclude.as_ref() != Some(&wt_canon) {
                        return Ok(Some(wt_canon));
                    }
                }
            }
        }
    }

    let worktrees_dir = common_git_dir.join("worktrees");
    if !worktrees_dir.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(&worktrees_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let admin = entry.path();
        if !admin.is_dir() {
            continue;
        }
        let Ok(head) = fs::read_to_string(admin.join("HEAD")) else {
            continue;
        };
        if head.trim() != expected {
            continue;
        }
        let Ok(raw) = fs::read_to_string(admin.join("gitdir")) else {
            continue;
        };
        let Some(dotgit_path) = parse_gitdir_target(&admin, raw.trim()) else {
            continue;
        };
        let Some(wt_path) = dotgit_path.parent() else {
            continue;
        };
        let wt_canon = wt_path.canonicalize().unwrap_or_else(|_| wt_path.to_path_buf());
        if exclude.as_ref() != Some(&wt_canon) {
            return Ok(Some(wt_canon));
        }
    }
    Ok(None)
}

fn ensure_relative_worktree_extensions(common_git_dir: &Path) -> Result<()> {
    let config_path = common_git_dir.join("config");
    let mut content = fs::read_to_string(&config_path).unwrap_or_default();
    content = set_config_key(&content, "core", "repositoryformatversion", "1");
    content = set_config_key(&content, "extensions", "relativeworktrees", "true");
    fs::write(&config_path, content)
        .with_context(|| format!("writing {}", config_path.display()))?;
    Ok(())
}

fn set_config_key(content: &str, section: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut saw_section = false;
    let mut wrote_key = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if current_section.as_deref() == Some(section) && !wrote_key {
                out.push(format!("\t{key} = {value}"));
                wrote_key = true;
            }

            let header = trimmed.trim_start_matches('[').trim_end_matches(']');
            let section_name = header.split_whitespace().next().unwrap_or("").to_string();
            current_section = Some(section_name.clone());
            if section_name == section {
                saw_section = true;
            }
            out.push(line.to_string());
            continue;
        }

        if current_section.as_deref() == Some(section) {
            let maybe_key = trimmed
                .split_once('=')
                .map(|(k, _)| k.trim().to_string())
                .unwrap_or_default();
            if maybe_key == key {
                out.push(format!("\t{key} = {value}"));
                wrote_key = true;
                continue;
            }
        }

        out.push(line.to_string());
    }

    if saw_section && !wrote_key {
        out.push(format!("\t{key} = {value}"));
    }

    if !saw_section {
        out.push(format!("[{section}]"));
        out.push(format!("\t{key} = {value}"));
    }

    let mut rendered = out.join("\n");
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

#[allow(dead_code)]
fn parse_prune_expire(expire: Option<&str>) -> Result<Option<SystemTime>> {
    match expire {
        None => Ok(None),
        Some("now") => Ok(Some(SystemTime::now())),
        Some(s) => {
            if let Some(threshold) = parse_relative_time(s) {
                Ok(Some(threshold))
            } else {
                bail!("unsupported --expire value: {s:?}");
            }
        }
    }
}

#[allow(dead_code)]
fn parse_relative_time(s: &str) -> Option<SystemTime> {
    let s = s.trim();
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() == 3 && parts[2] == "ago" {
        let n: u64 = parts[0].parse().ok()?;
        let unit = parts[1];
        let secs = match unit {
            "second" | "seconds" => n,
            "minute" | "minutes" => n * 60,
            "hour" | "hours" => n * 3600,
            "day" | "days" => n * 86400,
            "week" | "weeks" => n * 7 * 86400,
            "month" | "months" => n * 30 * 86400,
            "year" | "years" => n * 365 * 86400,
            _ => return None,
        };
        return SystemTime::now().checked_sub(Duration::from_secs(secs));
    }
    None
}

#[allow(dead_code)]
fn is_older_than_expire(path: &Path, expire_before: Option<SystemTime>) -> bool {
    let Some(threshold) = expire_before else {
        return true;
    };
    match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(mtime) => mtime <= threshold,
        Err(_) => true,
    }
}

fn remove_dir_if_empty(path: &Path) {
    let is_empty = fs::read_dir(path)
        .ok()
        .and_then(|mut it| it.next().map(|_| false))
        .unwrap_or(true);
    if is_empty {
        let _ = fs::remove_dir(path);
    }
}

// ---------------------------------------------------------------------------
// worktree move
// ---------------------------------------------------------------------------

fn cmd_move(args: MoveArgs) -> Result<()> {
    if args.relative_paths && args.no_relative_paths {
        bail!("options '--relative-paths' and '--no-relative-paths' cannot be used together");
    }

    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let prefer_local = args.relative_paths || args.no_relative_paths || config_use_relative_paths(&common);
    if !prefer_local {
        return passthrough_current_worktree_invocation();
    }

    let worktrees_dir = common.join("worktrees");

    let src_path = absolutize_from_cwd(&args.source)?;
    let dst_requested = if args.destination.is_absolute() {
        args.destination.clone()
    } else {
        std::env::current_dir()?.join(&args.destination)
    };
    if dst_requested.exists() {
        bail!("target '{}' already exists", dst_requested.display());
    }

    let wt_name = find_worktree_name(&worktrees_dir, &src_path)?;
    let admin = worktrees_dir.join(&wt_name);
    if admin.join("locked").exists() && args.force < 2 {
        bail!("worktree '{}' is locked; use --force to move it anyway", src_path.display());
    }

    fs::rename(&src_path, &dst_requested).with_context(|| {
        format!(
            "cannot move '{}' to '{}'",
            src_path.display(),
            dst_requested.display()
        )
    })?;
    let dst_path = dst_requested.canonicalize().unwrap_or(dst_requested);

    let use_relative = if args.relative_paths {
        true
    } else if args.no_relative_paths {
        false
    } else {
        config_use_relative_paths(&common)
    };

    let admin_gitdir_target = dst_path.join(".git");
    let admin_gitdir_contents = if use_relative {
        format!(
            "{}\n",
            relativize_path(&admin, &admin_gitdir_target).display()
        )
    } else {
        format!("{}\n", admin_gitdir_target.display())
    };
    fs::write(admin.join("gitdir"), admin_gitdir_contents)?;

    let wt_gitdir_target = if use_relative {
        relativize_path(&dst_path, &admin)
    } else {
        admin.clone()
    };
    fs::write(
        dst_path.join(".git"),
        format!("gitdir: {}\n", wt_gitdir_target.display()),
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// worktree repair
// ---------------------------------------------------------------------------

fn cmd_repair(args: RepairArgs) -> Result<()> {
    if args.relative_paths && args.no_relative_paths {
        bail!("options '--relative-paths' and '--no-relative-paths' cannot be used together");
    }

    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");
    let use_relative = if args.relative_paths {
        true
    } else if args.no_relative_paths {
        false
    } else {
        config_use_relative_paths(&common)
    };

    if args.paths.is_empty() {
        if let (Some(current_name), Some(current_wt)) =
            (current_linked_worktree_name(&repo.git_dir), repo.work_tree.clone())
        {
            if current_wt.is_dir() {
                repair_explicit_worktree(&worktrees_dir, &current_wt, use_relative)?;
            } else if worktrees_dir.join(&current_name).is_dir() {
                // Keep Git behavior: don't fail implicit repair just because the
                // linked worktree path is currently missing.
            }
        }
        if !worktrees_dir.is_dir() {
            return Ok(());
        }
        let mut names: Vec<String> = fs::read_dir(&worktrees_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for name in names {
            repair_implicit_worktree(&worktrees_dir, &name, use_relative)?;
        }
        return Ok(());
    }

    for path in &args.paths {
        let abs = absolutize_from_cwd(path)?;
        if !abs.exists() {
            bail!("not a valid path: {}", abs.display());
        }
        if !abs.is_dir() {
            bail!("not a directory: {}", abs.display());
        }
        repair_explicit_worktree(&worktrees_dir, &abs, use_relative)?;
    }

    Ok(())
}

fn repair_implicit_worktree(worktrees_dir: &Path, name: &str, use_relative: bool) -> Result<()> {
    let admin = worktrees_dir.join(name);
    let gitdir_file = admin.join("gitdir");
    if !gitdir_file.exists() {
        return Ok(());
    }
    let raw = match fs::read_to_string(&gitdir_file) {
        Ok(raw) => raw,
        Err(_) => return Ok(()),
    };
    let Some(wt_dotgit) = parse_gitdir_target(&admin, raw.trim()) else {
        return Ok(());
    };
    let wt_path = wt_dotgit.parent().unwrap_or(&wt_dotgit).to_path_buf();
    if !wt_path.exists() {
        // Missing linked worktree: intentionally skipped.
        return Ok(());
    }
    if !wt_path.is_dir() {
        bail!("not a directory: {}", wt_path.display());
    }

    let dotgit = wt_path.join(".git");
    if let Ok(meta) = fs::symlink_metadata(&dotgit) {
        if meta.is_dir() {
            bail!(".git is not a file: {}", wt_path.display());
        }
    }

    let mut needs_repair = false;
    let mut reason = ".git file broken";
    if let Ok(parsed) = parse_worktree_gitfile(&dotgit) {
        if !parsed.resolved.exists() {
            needs_repair = true;
            reason = ".git file broken";
        } else if normalize_path_for_compare(&parsed.resolved) != normalize_path_for_compare(&admin)
            || parsed.is_relative != use_relative
        {
            needs_repair = true;
            reason = ".git file incorrect";
        }
    } else {
        needs_repair = true;
        reason = ".git file broken";
    }

    if needs_repair {
        write_worktree_gitfile(&dotgit, &wt_path, &admin, use_relative)?;
        eprintln!("repair: {reason}: {}", wt_path.display());
    }
    Ok(())
}

fn repair_explicit_worktree(worktrees_dir: &Path, wt_path: &Path, use_relative: bool) -> Result<()> {
    let dotgit = wt_path.join(".git");
    let dotgit_meta = fs::symlink_metadata(&dotgit).map_err(|_| {
        anyhow::anyhow!(
            "unable to locate repository; .git file broken: {}",
            dotgit.display()
        )
    })?;
    if dotgit_meta.is_dir() {
        bail!("unable to locate repository; .git is not a file: {}", dotgit.display());
    }

    let parsed_dotgit = parse_worktree_gitfile(&dotgit).map_err(|_| {
        anyhow::anyhow!(
            "unable to locate repository; .git file broken: {}",
            dotgit.display()
        )
    })?;
    let parsed_under_worktrees = normalize_path_for_compare(&parsed_dotgit.resolved)
        .starts_with(normalize_path_for_compare(worktrees_dir));

    let Some(name) = extract_worktree_name_from_gitdir_target(&parsed_dotgit.resolved) else {
        if !parsed_dotgit.resolved.exists() {
            bail!(
                "unable to locate repository; .git file broken: {}",
                dotgit.display()
            );
        }
        bail!(
            "unable to locate repository; .git file does not reference a repository: {}",
            dotgit.display()
        );
    };
    let admin = worktrees_dir.join(name);
    if !admin.is_dir() {
        if !parsed_dotgit.resolved.exists() && !parsed_under_worktrees {
            bail!(
                "unable to locate repository; .git file broken: {}",
                dotgit.display()
            );
        }
        bail!(
            "unable to locate repository; .git file does not reference a repository: {}",
            dotgit.display()
        );
    }

    let gitdir_file = admin.join("gitdir");
    let mut gitdir_state = "gitdir incorrect";
    let mut needs_gitdir_fix = true;
    if let Ok(raw) = fs::read_to_string(&gitdir_file) {
        let is_relative = !Path::new(raw.trim()).is_absolute();
        if let Some(recorded) = parse_gitdir_target(&admin, raw.trim()) {
            if normalize_path_for_compare(&recorded) == normalize_path_for_compare(&dotgit)
                && is_relative == use_relative
            {
                needs_gitdir_fix = false;
            } else {
                gitdir_state = "gitdir incorrect";
            }
        } else {
            gitdir_state = "gitdir unreadable";
        }
    } else {
        gitdir_state = "gitdir unreadable";
    }
    if needs_gitdir_fix {
        write_admin_gitdir(&gitdir_file, &admin, &dotgit, use_relative)?;
        eprintln!("repair: {gitdir_state}: {}", gitdir_file.display());
    }

    let gitfile_state = if normalize_path_for_compare(&parsed_dotgit.resolved)
        != normalize_path_for_compare(&admin)
        || parsed_dotgit.is_relative != use_relative
    {
        Some(".git file incorrect")
    } else {
        None
    };
    if let Some(state) = gitfile_state {
        write_worktree_gitfile(&dotgit, wt_path, &admin, use_relative)?;
        eprintln!("repair: {state}: {}", wt_path.display());
    }

    Ok(())
}

struct ParsedGitfileTarget {
    resolved: PathBuf,
    is_relative: bool,
}

fn parse_worktree_gitfile(dotgit: &Path) -> Result<ParsedGitfileTarget> {
    let content = fs::read_to_string(dotgit)?;
    let target = content
        .trim()
        .strip_prefix("gitdir: ")
        .ok_or_else(|| anyhow::anyhow!("invalid gitfile"))?;
    let target_path = Path::new(target);
    let resolved = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        dotgit.parent().unwrap_or(Path::new(".")).join(target_path)
    };
    Ok(ParsedGitfileTarget {
        resolved,
        is_relative: !target_path.is_absolute(),
    })
}

fn write_admin_gitdir(gitdir_file: &Path, admin: &Path, wt_dotgit: &Path, use_relative: bool) -> Result<()> {
    let content = if use_relative {
        format!("{}\n", relativize_path(admin, wt_dotgit).display())
    } else {
        format!("{}\n", wt_dotgit.display())
    };
    fs::write(gitdir_file, content)?;
    Ok(())
}

fn write_worktree_gitfile(dotgit: &Path, wt_path: &Path, admin: &Path, use_relative: bool) -> Result<()> {
    let target = if use_relative {
        relativize_path(wt_path, admin)
    } else {
        admin.to_path_buf()
    };
    fs::write(dotgit, format!("gitdir: {}\n", target.display()))?;
    Ok(())
}

fn extract_worktree_name_from_gitdir_target(target: &Path) -> Option<String> {
    let components: Vec<_> = target.components().collect();
    for i in 0..components.len() {
        if components[i].as_os_str() == "worktrees" {
            if let Some(name) = components.get(i + 1) {
                if let std::path::Component::Normal(n) = name {
                    return Some(n.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn current_linked_worktree_name(git_dir: &Path) -> Option<String> {
    let parent = git_dir.parent()?;
    if parent.file_name()? != "worktrees" {
        return None;
    }
    git_dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// worktree lock / unlock
// ---------------------------------------------------------------------------

fn cmd_lock(_args: LockArgs) -> Result<()> {
    passthrough_current_worktree_invocation()
}

fn cmd_unlock(_args: UnlockArgs) -> Result<()> {
    passthrough_current_worktree_invocation()
}

fn absolutize_from_cwd(path: &Path) -> Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(abs.canonicalize().unwrap_or(abs))
}

fn worktree_has_gitlink_entries(admin: &Path) -> Result<bool> {
    let index = match Index::load(&admin.join("index")) {
        Ok(idx) => idx,
        Err(_) => return Ok(false),
    };
    Ok(index
        .entries
        .iter()
        .any(|e| e.stage() == 0 && e.mode == 0o160000))
}

fn worktree_contains_initialized_submodule(admin: &Path, wt_path: &Path) -> Result<bool> {
    let index = match Index::load(&admin.join("index")) {
        Ok(idx) => idx,
        Err(_) => return Ok(false),
    };
    for entry in &index.entries {
        if entry.stage() != 0 || entry.mode != 0o160000 {
            continue;
        }
        let rel = String::from_utf8_lossy(&entry.path);
        if wt_path.join(rel.as_ref()).join(".git").exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn config_use_relative_paths(common: &Path) -> bool {
    let cfg = ConfigSet::load(Some(common), true).unwrap_or_else(|_| ConfigSet::new());
    cfg.get("worktree.useRelativePaths")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
        .unwrap_or(false)
}

fn config_relative_worktrees_enabled(common: &Path) -> bool {
    let cfg = ConfigSet::load(Some(common), true).unwrap_or_else(|_| ConfigSet::new());
    cfg.get("extensions.relativeworktrees")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
        .unwrap_or(false)
}

fn relativize_path(from_dir: &Path, to: &Path) -> PathBuf {
    let from = from_dir.canonicalize().unwrap_or_else(|_| from_dir.to_path_buf());
    let to = to.canonicalize().unwrap_or_else(|_| to.to_path_buf());
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let mut common_len = 0usize;
    while common_len < from_components.len()
        && common_len < to_components.len()
        && from_components[common_len] == to_components[common_len]
    {
        common_len += 1;
    }
    let mut rel = PathBuf::new();
    for _ in common_len..from_components.len() {
        rel.push("..");
    }
    for component in &to_components[common_len..] {
        rel.push(component.as_os_str());
    }
    if rel.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        rel
    }
}

fn passthrough_current_worktree_invocation() -> Result<()> {
    git_passthrough::run_current_invocation("worktree")
}
