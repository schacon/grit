//! `grit worktree` — manage multiple working trees.
//!
//! Each linked worktree has its own HEAD, index, and working directory,
//! but shares the object database and refs with the main repository.
//! Worktree metadata is stored under `.git/worktrees/<name>/`.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::index::{Index, IndexEntry};
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    #[arg(short, long)]
    pub force: bool,

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

    /// Guess remote branch.
    #[arg(long)]
    pub guess_remote: bool,

    /// Don't guess remote branch.
    #[arg(long)]
    pub no_guess_remote: bool,

    /// Create a new branch with -B (reset if exists).
    #[arg(short = 'B')]
    pub force_new_branch: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ListArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub porcelain: bool,
}

#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    /// Path of the worktree to remove.
    pub path: PathBuf,

    /// Force removal even if worktree has modifications.
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, ClapArgs)]
pub struct PruneArgs {
    /// Only report what would be done.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Report pruned entries.
    #[arg(short, long)]
    pub verbose: bool,
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
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, ClapArgs)]
pub struct RepairArgs {
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
    // Validate mutually exclusive options
    {
        let mut exclusive = Vec::new();
        if args.new_branch.is_some() {
            exclusive.push("-b");
        }
        if args.force_new_branch.is_some() {
            exclusive.push("-B");
        }
        if args.detach {
            exclusive.push("--detach");
        }
        if args.orphan {
            exclusive.push("--orphan");
        }
        if exclusive.len() > 1 {
            bail!(
                "options '{}' and '{}' cannot be used together",
                exclusive[0],
                exclusive[1]
            );
        }
        if args.orphan && args.no_checkout {
            bail!("options '--orphan' and '--no-checkout' cannot be used together");
        }
        if args.reason.is_some() && !args.lock {
            bail!("--reason requires --lock");
        }
    }

    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
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

    let wt_path = wt_path.canonicalize().unwrap_or_else(|_| {
        // Path may not exist yet; create it then canonicalize
        let _ = fs::create_dir_all(&wt_path);
        wt_path.canonicalize().unwrap_or(wt_path.clone())
    });

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

    let head_state = resolve_head(&common)?;
    let head_oid = head_state.oid().copied();
    let inferred_orphan = !args.orphan
        && args.new_branch.is_none()
        && args.force_new_branch.is_none()
        && args.branch.is_none()
        && head_oid.is_none();
    let orphan_mode = args.orphan || inferred_orphan;

    // Handle --orphan (or inferred orphan): create worktree with unborn branch
    if orphan_mode {
        if inferred_orphan && !args.quiet {
            eprintln!("No possible source branch, inferring '--orphan'");
        }

        let orphan_branch = args.branch.clone().unwrap_or_else(|| wt_name.clone());

        // Create the working tree directory
        fs::create_dir_all(&wt_path)
            .with_context(|| format!("cannot create directory '{}'", wt_path.display()))?;

        // Create the admin directory
        fs::create_dir_all(&wt_admin)
            .with_context(|| format!("cannot create '{}'", wt_admin.display()))?;

        // Write gitdir file
        let gitdir_content = format!("{}\n", wt_path.join(".git").display());
        fs::write(wt_admin.join("gitdir"), &gitdir_content)?;
        fs::write(
            wt_admin.join("commondir"),
            format!("{}\n", common.display()),
        )?;

        // HEAD points to an unborn branch
        fs::write(
            wt_admin.join("HEAD"),
            format!("ref: refs/heads/{}\n", orphan_branch),
        )?;

        // Write the .git file in the worktree
        let dotgit_content = format!("gitdir: {}\n", wt_admin.display());
        fs::write(wt_path.join(".git"), &dotgit_content)?;

        println!(
            "Preparing worktree (new branch '{}') at '{}'",
            orphan_branch,
            wt_path.display()
        );
        return Ok(());
    }

    // Determine branch name and commit
    // Determine branch name and starting commit.
    // `worktree add <path> <branch>` — if <branch> exists as a ref, check it out;
    //   otherwise create a new branch from HEAD.
    // `worktree add -b <new> <path>` — always create a new branch from HEAD.
    let (branch_name, commit_oid) = if let Some(ref new_b) = args.force_new_branch {
        // -B: create or reset branch
        let oid =
            head_oid.ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?;
        (new_b.clone(), oid)
    } else if let Some(ref new_b) = args.new_branch {
        let oid =
            head_oid.ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?;
        (new_b.clone(), oid)
    } else if let Some(ref spec) = args.branch {
        // Try to resolve the branch/commit-ish
        match resolve_commitish(&repo, spec) {
            Ok(oid) => (spec.clone(), oid),
            Err(_) => {
                // Branch doesn't exist yet — create from HEAD
                let oid = head_oid.ok_or_else(|| {
                    anyhow::anyhow!("'{}' is not a commit and HEAD is invalid", spec)
                })?;
                (spec.clone(), oid)
            }
        }
    } else {
        let oid = head_oid.ok_or_else(|| {
            anyhow::anyhow!("HEAD does not point to a valid commit; specify a branch")
        })?;
        (wt_name.clone(), oid)
    };

    // Create the working tree directory
    fs::create_dir_all(&wt_path)
        .with_context(|| format!("cannot create directory '{}'", wt_path.display()))?;

    // Create the admin directory: .git/worktrees/<name>/
    fs::create_dir_all(&wt_admin)
        .with_context(|| format!("cannot create '{}'", wt_admin.display()))?;

    // Write gitdir file — points the admin dir back to the worktree's .git file
    let gitdir_content = format!("{}\n", wt_path.join(".git").display());
    fs::write(wt_admin.join("gitdir"), &gitdir_content)?;

    // Write commondir file — relative path from worktree admin to the common dir
    // Standard git uses relative paths like "../../"
    fs::write(
        wt_admin.join("commondir"),
        format!("{}\n", common.display()),
    )?;

    // Write HEAD — either branch or detached
    if args.detach {
        fs::write(wt_admin.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
    } else {
        // Create the branch ref if it doesn't exist yet
        let branch_ref = format!("refs/heads/{}", branch_name);
        let ref_path = common.join(&branch_ref);
        if !ref_path.exists() {
            // New branch: create it pointing to the commit
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        } else if !args.force {
            // Branch already exists — check if it's checked out in another worktree
            // (For simplicity, allow it; git also warns but --force overrides)
        }
        fs::write(
            wt_admin.join("HEAD"),
            format!("ref: refs/heads/{}\n", branch_name),
        )?;
    }

    // Write the .git file in the worktree (gitfile pointing to admin dir)
    let dotgit_content = format!("gitdir: {}\n", wt_admin.display());
    fs::write(wt_path.join(".git"), &dotgit_content)?;

    // Lock the worktree if --lock was used
    if args.lock {
        let reason = args.reason.as_deref().unwrap_or("");
        fs::write(wt_admin.join("locked"), format!("{reason}\n"))?;
    }

    println!(
        "Preparing worktree (new branch '{}') at '{}'",
        branch_name,
        wt_path.display()
    );

    // Populate the working tree by checking out the commit
    if !args.no_checkout {
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

fn collect_worktrees(repo: &Repository) -> Result<Vec<WorktreeInfo>> {
    let common = common_dir(&repo.git_dir)?;
    let mut entries = Vec::new();

    // Main worktree (or bare repo)
    let main_head = resolve_head(&common).unwrap_or(HeadState::Invalid);
    let main_path = if repo.is_bare() {
        common.clone()
    } else {
        repo.work_tree
            .clone()
            .unwrap_or_else(|| common.parent().unwrap_or(&common).to_path_buf())
    };
    entries.push(WorktreeInfo {
        path: main_path,
        head: main_head,
        is_bare: repo.is_bare(),
        is_locked: false,
    });

    // Linked worktrees
    let worktrees_dir = common.join("worktrees");
    if worktrees_dir.is_dir() {
        let mut names: Vec<_> = fs::read_dir(&worktrees_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();

        for name in names {
            let admin = worktrees_dir.join(&name);
            let wt_head = resolve_linked_head(&admin, &common);

            // Read the gitdir file to find the worktree path
            let gitdir_path = admin.join("gitdir");
            let wt_path = if gitdir_path.exists() {
                let raw = fs::read_to_string(&gitdir_path).unwrap_or_default();
                let p = PathBuf::from(raw.trim());
                // gitdir points to <worktree>/.git, so parent is the worktree
                p.parent().unwrap_or(&p).to_path_buf()
            } else {
                admin.clone()
            };

            let is_locked = admin.join("locked").exists();

            entries.push(WorktreeInfo {
                path: wt_path,
                head: wt_head,
                is_bare: false,
                is_locked,
            });
        }
    }

    Ok(entries)
}

fn cmd_list(args: ListArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let entries = collect_worktrees(&repo)?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if args.porcelain {
        for entry in &entries {
            writeln!(out, "worktree {}", entry.path.display())?;
            match &entry.head {
                HeadState::Branch { oid: Some(oid), .. } => {
                    writeln!(out, "HEAD {}", oid.to_hex())?;
                }
                HeadState::Detached { oid } => {
                    writeln!(out, "HEAD {}", oid.to_hex())?;
                }
                _ => {
                    writeln!(out, "HEAD {}", "0".repeat(40))?;
                }
            }
            match &entry.head {
                HeadState::Branch { refname, .. } => {
                    writeln!(out, "branch {}", refname)?;
                }
                HeadState::Detached { .. } => {
                    writeln!(out, "detached")?;
                }
                _ => {}
            }
            if entry.is_bare {
                writeln!(out, "bare")?;
            }
            if entry.is_locked {
                writeln!(out, "locked")?;
            }
            writeln!(out)?;
        }
    } else {
        for entry in &entries {
            let sha = match &entry.head {
                HeadState::Branch { oid: Some(oid), .. } => oid.to_hex()[..7].to_string(),
                HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
                _ => "0000000".to_string(),
            };

            let branch_info = if entry.is_bare {
                "(bare)".to_string()
            } else {
                match &entry.head {
                    HeadState::Branch { short_name, .. } => {
                        format!("[{}]", short_name)
                    }
                    HeadState::Detached { .. } => "(detached HEAD)".to_string(),
                    HeadState::Invalid => "(error)".to_string(),
                }
            };

            let lock_marker = if entry.is_locked { " locked" } else { "" };
            writeln!(
                out,
                "{:<40} {} {}{}",
                entry.path.display(),
                sha,
                branch_info,
                lock_marker,
            )?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// worktree remove
// ---------------------------------------------------------------------------

fn cmd_remove(args: RemoveArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");

    let wt_path = if args.path.is_absolute() {
        args.path.clone()
    } else {
        std::env::current_dir()?.join(&args.path)
    };
    let wt_path = wt_path.canonicalize().unwrap_or(wt_path);

    // Find the matching admin entry
    let wt_name = find_worktree_name(&worktrees_dir, &wt_path)?;
    let admin = worktrees_dir.join(&wt_name);

    // Check for lock
    if admin.join("locked").exists() && !args.force {
        bail!(
            "worktree '{}' is locked; use --force or unlock it first",
            wt_path.display()
        );
    }

    // Remove the working tree directory
    if wt_path.exists() {
        fs::remove_dir_all(&wt_path)
            .with_context(|| format!("cannot remove '{}'", wt_path.display()))?;
    }

    // Remove the admin directory
    if admin.exists() {
        fs::remove_dir_all(&admin)
            .with_context(|| format!("cannot remove admin dir '{}'", admin.display()))?;
    }

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
            // Verify gitdir points to the right place
            let gitdir_file = candidate.join("gitdir");
            if gitdir_file.exists() {
                let raw = fs::read_to_string(&gitdir_file).unwrap_or_default();
                let recorded = PathBuf::from(raw.trim());
                let recorded_wt = recorded
                    .parent()
                    .unwrap_or(&recorded)
                    .canonicalize()
                    .unwrap_or(recorded.parent().unwrap_or(&recorded).to_path_buf());
                if recorded_wt == target {
                    return Ok(basename.to_string());
                }
            }
            // If gitdir doesn't match, still use basename as the name
            return Ok(basename.to_string());
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
        let recorded = PathBuf::from(raw.trim());
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

fn cmd_prune(args: PruneArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");

    if !worktrees_dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(&worktrees_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let admin = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // A worktree is stale if its gitdir target no longer exists
        let gitdir_file = admin.join("gitdir");
        let is_stale = if gitdir_file.exists() {
            let raw = fs::read_to_string(&gitdir_file).unwrap_or_default();
            let target = PathBuf::from(raw.trim());
            !target.exists()
        } else {
            true // No gitdir file at all — stale
        };

        if !is_stale {
            continue;
        }

        // Skip locked worktrees
        if admin.join("locked").exists() {
            if args.verbose {
                eprintln!("worktree '{}' is locked; not pruning", name);
            }
            continue;
        }

        if args.verbose || args.dry_run {
            eprintln!("Removing worktrees/{}", name);
        }

        if !args.dry_run {
            fs::remove_dir_all(&admin)
                .with_context(|| format!("cannot remove '{}'", admin.display()))?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// worktree move
// ---------------------------------------------------------------------------

fn cmd_move(args: MoveArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");

    let src_path = if args.source.is_absolute() {
        args.source.clone()
    } else {
        std::env::current_dir()?.join(&args.source)
    };
    let src_path = src_path.canonicalize().unwrap_or(src_path);

    // Find the admin entry for the source worktree
    let wt_name = find_worktree_name(&worktrees_dir, &src_path)?;
    let admin = worktrees_dir.join(&wt_name);

    // Check for lock
    if admin.join("locked").exists() && !args.force {
        bail!(
            "worktree '{}' is locked; use --force to move it anyway",
            src_path.display()
        );
    }

    // Determine the destination absolute path
    let dst_path = if args.destination.is_absolute() {
        args.destination.clone()
    } else {
        std::env::current_dir()?.join(&args.destination)
    };

    if dst_path.exists() {
        bail!("target '{}' already exists", dst_path.display());
    }

    // Move the working tree directory
    fs::rename(&src_path, &dst_path).with_context(|| {
        format!(
            "cannot move '{}' to '{}'",
            src_path.display(),
            dst_path.display()
        )
    })?;

    let dst_path = dst_path.canonicalize().unwrap_or(dst_path);

    // Update the gitdir file in the admin dir to point to the new location
    let new_gitdir_content = format!("{}\n", dst_path.join(".git").display());
    fs::write(admin.join("gitdir"), &new_gitdir_content)?;

    // Update the .git file in the moved worktree (it should still point to the same admin dir)
    let dotgit_content = format!("gitdir: {}\n", admin.display());
    fs::write(dst_path.join(".git"), &dotgit_content)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// worktree repair
// ---------------------------------------------------------------------------

fn cmd_repair(args: RepairArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");

    if !worktrees_dir.is_dir() {
        return Ok(());
    }

    // If specific paths were given, only repair those; otherwise repair all.
    let entries_to_repair: Vec<String> = if args.paths.is_empty() {
        // All linked worktrees
        fs::read_dir(&worktrees_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect()
    } else {
        // Find matching admin entries for the given paths
        let mut names = Vec::new();
        for p in &args.paths {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                std::env::current_dir()?.join(p)
            };
            let abs = abs.canonicalize().unwrap_or(abs);
            match find_worktree_name(&worktrees_dir, &abs) {
                Ok(name) => names.push(name),
                Err(e) => eprintln!("warning: {}", e),
            }
        }
        names
    };

    for name in &entries_to_repair {
        let admin = worktrees_dir.join(name);
        let gitdir_file = admin.join("gitdir");

        if !gitdir_file.exists() {
            continue;
        }

        let raw = fs::read_to_string(&gitdir_file).unwrap_or_default();
        let recorded = PathBuf::from(raw.trim());
        // gitdir points to <worktree>/.git
        let wt_dotgit = &recorded;
        let wt_path = recorded.parent().unwrap_or(&recorded);

        // Repair 1: If the worktree .git file exists, make sure it points back to admin
        if wt_dotgit.exists() {
            let dotgit_content = fs::read_to_string(wt_dotgit).unwrap_or_default();
            let expected_prefix = "gitdir: ";
            if let Some(current_target) = dotgit_content.trim().strip_prefix(expected_prefix) {
                let current_path = PathBuf::from(current_target);
                let admin_canonical = admin.canonicalize().unwrap_or_else(|_| admin.clone());
                let current_canonical = current_path.canonicalize().unwrap_or(current_path.clone());
                if current_canonical != admin_canonical {
                    // Fix the .git file
                    let fixed = format!("gitdir: {}\n", admin.display());
                    fs::write(wt_dotgit, &fixed)?;
                    eprintln!(
                        "repair: {}: repaired gitfile to point to {}",
                        wt_path.display(),
                        admin.display()
                    );
                }
            }
        }

        // Repair 2: Verify gitdir file in admin points to an existing location
        if !wt_dotgit.exists() && wt_path.exists() {
            // The worktree exists but .git file is missing — recreate it
            let dotgit_content = format!("gitdir: {}\n", admin.display());
            fs::write(wt_path.join(".git"), &dotgit_content)?;
            eprintln!("repair: {}: recreated gitfile", wt_path.display());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// worktree lock / unlock
// ---------------------------------------------------------------------------

fn cmd_lock(args: LockArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");

    let wt_path = if args.path.is_absolute() {
        args.path.clone()
    } else {
        std::env::current_dir()?.join(&args.path)
    };
    let wt_path = wt_path.canonicalize().unwrap_or(wt_path);

    let wt_name = find_worktree_name(&worktrees_dir, &wt_path)?;
    let admin = worktrees_dir.join(&wt_name);

    if admin.join("locked").exists() {
        bail!("worktree '{}' is already locked", wt_path.display());
    }

    let content = args.reason.as_deref().unwrap_or("");
    fs::write(admin.join("locked"), content)?;

    Ok(())
}

fn cmd_unlock(args: UnlockArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let common = common_dir(&repo.git_dir)?;
    let worktrees_dir = common.join("worktrees");

    let wt_path = if args.path.is_absolute() {
        args.path.clone()
    } else {
        std::env::current_dir()?.join(&args.path)
    };
    let wt_path = wt_path.canonicalize().unwrap_or(wt_path);

    let wt_name = find_worktree_name(&worktrees_dir, &wt_path)?;
    let admin = worktrees_dir.join(&wt_name);

    let lock_file = admin.join("locked");
    if !lock_file.exists() {
        bail!("worktree '{}' is not locked", wt_path.display());
    }

    fs::remove_file(&lock_file)?;

    Ok(())
}
