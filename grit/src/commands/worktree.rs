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

    /// Do not set up tracking.
    #[arg(long)]
    pub no_track: bool,

    /// Store paths as relative to the main worktree.
    #[arg(long)]
    pub relative_paths: bool,

    /// Store paths as absolute.
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

    /// NUL-terminated output (for --porcelain).
    #[arg(short = 'z')]
    pub nul: bool,

    /// Show verbose output including lock/prune info.
    #[arg(short, long, conflicts_with = "porcelain")]
    pub verbose: bool,
}

#[derive(Debug, ClapArgs)]
pub struct RemoveArgs {
    /// Path of the worktree to remove.
    pub path: PathBuf,

    /// Force removal. Once: allow removing with dirty files. Twice: also allow removing locked worktrees.
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

    /// Prune entries older than a specific time.
    #[arg(long)]
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

    /// Force move. Once: allow moving to existing path. Twice: also allow moving locked worktree.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub force: u8,

    /// Store paths as relative.
    #[arg(long)]
    pub relative_paths: bool,

    /// Store paths as absolute.
    #[arg(long = "no-relative-paths")]
    pub no_relative_paths: bool,
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
    // Try as a revision with navigation (e.g., HEAD~1, main^2)
    if let Ok(oid) = grit_lib::rev_parse::resolve_revision(repo, spec) {
        // Ensure it's a commit or can be resolved to one
        if let Ok(obj) = repo.odb.read(&oid) {
            if obj.kind == grit_lib::objects::ObjectKind::Commit {
                return Ok(oid);
            }
        }
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
        // Note: --orphan is compatible with -b (provides branch name) but not -B or --detach
        if args.orphan && (args.force_new_branch.is_some() || args.detach) {
            exclusive.push("--orphan");
        }
        if exclusive.len() > 1 {
            eprintln!(
                "fatal: options '{}' and '{}' cannot be used together",
                exclusive[0], exclusive[1]
            );
            std::process::exit(1);
        }
        if args.orphan && args.no_checkout {
            eprintln!("fatal: options '--orphan' and '--no-checkout' cannot be used together");
            std::process::exit(1);
        }
        if args.orphan && args.branch.is_some() {
            eprintln!("fatal: options '--orphan' and '<branch>' cannot be used together");
            std::process::exit(1);
        }
        // Additional mutual exclusions not caught above
        if args.detach && args.orphan {
            eprintln!("fatal: options '--detach' and '--orphan' cannot be used together");
            std::process::exit(1);
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

    // Canonicalize the path (don't create it yet — that happens later after validation)
    let wt_path = if wt_path.exists() {
        wt_path.canonicalize().unwrap_or(wt_path.clone())
    } else {
        // Not created yet — use absolute path from cwd
        if wt_path.is_absolute() {
            wt_path.clone()
        } else {
            std::env::current_dir().unwrap_or_default().join(&wt_path)
        }
    };

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
        // Use -b branch name if provided, otherwise use path basename
        let orphan_branch = args.new_branch.as_deref().unwrap_or(&wt_name);
        setup_unborn_worktree(
            &common,
            &wt_admin,
            &wt_path,
            orphan_branch,
            args.lock,
            args.reason.as_deref(),
        )?;
        return Ok(());
    }

    // Determine branch target and commit.
    let head_state = resolve_head(&common)?;
    let head_oid = head_state.oid().copied();

    // Determine branch mode and starting commit.
    // `worktree add <path> <branch>` — if <branch> exists as a ref, check it out;
    //   otherwise create a new branch from HEAD.
    // `worktree add <path> <commit-ish>` — check out detached HEAD at that commit.
    // `worktree add -b <new> <path>` — always create a new branch from HEAD.
    let mut inferred_orphan = false;
    let (branch_name, commit_oid, implicit_detach) = if let Some(ref new_b) = args.force_new_branch
    {
        // -B: create or reset branch (args.branch is the start point)
        let oid = if let Some(ref start_spec) = args.branch {
            resolve_commitish(&repo, start_spec)?
        } else {
            head_oid.ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?
        };
        (Some(new_b.clone()), Some(oid), false)
    } else if let Some(ref new_b) = args.new_branch {
        // -b: create branch (args.branch is the start point if given)
        let oid = if let Some(ref start_spec) = args.branch {
            resolve_commitish(&repo, start_spec)?
        } else {
            head_oid.ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?
        };
        (Some(new_b.clone()), Some(oid), false)
    } else if let Some(ref spec) = args.branch {
        // Handle "-" shorthand (previous branch)
        let spec = if spec == "-" {
            // Resolve @{-1} to get the previous branch
            refs::resolve_at_n_branch(&common, "@{-1}")
                .ok()
                .ok_or_else(|| anyhow::anyhow!("-: no previous branch"))?
        } else {
            spec.clone()
        };
        let spec = &spec;
        // Existing local branch: check out attached.
        if let Ok(oid) = refs::resolve_ref(&common, &format!("refs/heads/{spec}")) {
            (Some(spec.clone()), Some(oid), false)
        } else {
            // Existing non-branch commit-ish (e.g. tag): check out detached.
            match resolve_commitish(&repo, spec) {
                Ok(oid) => (None, Some(oid), true),
                Err(_) => {
                    // Unknown name: fail unless DWIM via remote is available
                    // Try DWIM from remote tracking refs
                    let remote_refs =
                        grit_lib::refs::list_refs(&common, "refs/remotes/").unwrap_or_default();
                    let matching: Vec<_> = remote_refs
                        .iter()
                        .filter(|(r, _)| {
                            let parts: Vec<&str> = r
                                .trim_start_matches("refs/remotes/")
                                .splitn(2, '/')
                                .collect();
                            parts.len() == 2 && parts[1] == spec
                        })
                        .collect();
                    if matching.len() == 1 {
                        // DWIM: create tracking branch from remote
                        let oid = matching[0].1;
                        // Get remote name for tracking setup
                        let remote_name = matching[0]
                            .0
                            .trim_start_matches("refs/remotes/")
                            .splitn(2, '/')
                            .next()
                            .unwrap_or("origin")
                            .to_owned();
                        // Write tracking config
                        let cfg_path = common.join("config");
                        if let Ok(mut cfg_content) = std::fs::read_to_string(&cfg_path) {
                            let section = format!(
                                "\n[branch \"{}\"]\
\n\tremote = {}\
\n\tmerge = refs/heads/{}\n",
                                spec, remote_name, spec
                            );
                            cfg_content.push_str(&section);
                            let _ = std::fs::write(&cfg_path, cfg_content);
                        }
                        (Some(spec.clone()), Some(oid), false)
                    } else {
                        bail!("fatal: invalid reference: '{}'", spec);
                    }
                }
            }
        }
    } else {
        match head_oid {
            Some(oid) => (Some(wt_name.clone()), Some(oid), false),
            None => {
                // Check if there are remote branches we can DWIM from
                let has_remotes = !grit_lib::refs::list_refs(&common, "refs/remotes/")
                    .unwrap_or_default()
                    .is_empty();
                if has_remotes && !args.no_guess_remote {
                    // DWIM: infer --orphan when remotes exist
                    inferred_orphan = true;
                    (Some(wt_name.clone()), None, false)
                } else {
                    // No remotes, no way to branch — fail with hint
                    let branch_n = &wt_name;
                    eprintln!(
                        "hint: If you meant to create a worktree containing a new unborn branch"
                    );
                    eprintln!(
                        "hint: named '{}', use the option '--orphan' as follows:",
                        branch_n
                    );
                    eprintln!("hint:");
                    if args.new_branch.is_some() {
                        let hint_branch = args.new_branch.as_deref().unwrap_or(branch_n);
                        eprintln!(
                            "hint:     git worktree add --orphan -b {} {}",
                            hint_branch,
                            args.path.display()
                        );
                    } else {
                        eprintln!(
                            "hint:     git worktree add --orphan {}",
                            args.path.display()
                        );
                    }
                    bail!("invalid reference: HEAD");
                }
            }
        }
    };

    if inferred_orphan {
        if !args.quiet {
            eprintln!("No possible source branch, inferring '--orphan'");
        }
        let branch = branch_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal error: missing orphan branch name"))?;
        setup_unborn_worktree(
            &common,
            &wt_admin,
            &wt_path,
            branch,
            args.lock,
            args.reason.as_deref(),
        )?;
        return Ok(());
    };

    // Check if the branch is already checked out in another worktree
    // Only applies when NOT in detach mode
    let detach_head_mode = args.detach || implicit_detach;
    if !detach_head_mode {
        if let Some(ref name) = branch_name {
            if !args.force {
                let branch_ref = format!("refs/heads/{name}");
                // Check all worktrees (main + linked)
                let main_head = resolve_head(&common).unwrap_or(HeadState::Invalid);
                if let HeadState::Branch { ref refname, .. } = main_head {
                    if *refname == branch_ref {
                        bail!(
                            "fatal: '{}' is already checked out at '{}'",
                            name,
                            common.parent().unwrap_or(&common).display()
                        );
                    }
                }
                // Check linked worktrees
                let wt_dir = common.join("worktrees");
                if wt_dir.is_dir() {
                    for entry in std::fs::read_dir(&wt_dir).into_iter().flatten().flatten() {
                        let head_file = entry.path().join("HEAD");
                        if let Ok(content) = std::fs::read_to_string(&head_file) {
                            if let Some(refname) = content.trim().strip_prefix("ref: ") {
                                if refname == branch_ref {
                                    let gitdir_file = entry.path().join("gitdir");
                                    let wt_path_str =
                                        if let Ok(raw) = std::fs::read_to_string(&gitdir_file) {
                                            let p = std::path::Path::new(raw.trim());
                                            p.parent().unwrap_or(p).display().to_string()
                                        } else {
                                            entry.file_name().to_string_lossy().to_string()
                                        };
                                    bail!(
                                        "fatal: '{}' is already checked out at '{}'",
                                        name,
                                        wt_path_str
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    } // end detach_head_mode check

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
    let detach_head = args.detach || implicit_detach;
    if detach_head {
        let commit_oid = commit_oid.ok_or_else(|| {
            anyhow::anyhow!("HEAD does not point to a valid commit; specify a branch")
        })?;
        fs::write(wt_admin.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
    } else {
        let branch_name = branch_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal error: missing branch name"))?;
        let commit_oid = commit_oid.ok_or_else(|| {
            anyhow::anyhow!("HEAD does not point to a valid commit; specify a branch")
        })?;
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

    if detach_head {
        let commit_oid = commit_oid.ok_or_else(|| {
            anyhow::anyhow!("HEAD does not point to a valid commit; specify a branch")
        })?;
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
    if !args.no_checkout {
        let commit_oid = commit_oid.ok_or_else(|| {
            anyhow::anyhow!("HEAD does not point to a valid commit; specify a branch")
        })?;
        populate_worktree(&repo.odb, &common, &commit_oid, &wt_path, &wt_admin)?;
    }

    Ok(())
}

fn setup_unborn_worktree(
    common: &Path,
    wt_admin: &Path,
    wt_path: &Path,
    branch_name: &str,
    lock: bool,
    reason: Option<&str>,
) -> Result<()> {
    // Check if the branch already exists
    let branch_ref = format!("refs/heads/{branch_name}");
    if refs::resolve_ref(common, &branch_ref).is_ok() {
        bail!("fatal: a branch named '{}' already exists", branch_name);
    }

    fs::create_dir_all(wt_path)
        .with_context(|| format!("cannot create directory '{}'", wt_path.display()))?;
    fs::create_dir_all(wt_admin)
        .with_context(|| format!("cannot create '{}'", wt_admin.display()))?;

    let gitdir_content = format!("{}\n", wt_path.join(".git").display());
    fs::write(wt_admin.join("gitdir"), &gitdir_content)?;
    fs::write(
        wt_admin.join("commondir"),
        format!("{}\n", common.display()),
    )?;
    fs::write(
        wt_admin.join("HEAD"),
        format!("ref: refs/heads/{}\n", branch_name),
    )?;
    fs::write(
        wt_path.join(".git"),
        format!("gitdir: {}\n", wt_admin.display()),
    )?;

    if lock {
        fs::write(
            wt_admin.join("locked"),
            format!("{}\n", reason.unwrap_or("")),
        )?;
    }

    println!(
        "Preparing worktree (new branch '{}') at '{}'",
        branch_name,
        wt_path.display()
    );
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
    // Determine if the repo is bare based on the common dir
    let common_is_bare = !common.ends_with(".git") && common.join("config").exists() && {
        // Check core.bare in config
        if let Ok(content) = std::fs::read_to_string(common.join("config")) {
            content.contains("bare = true")
        } else {
            false
        }
    };
    // The main worktree path: for non-bare repos, it's common.parent() (i.e. /repo for /repo/.git)
    // For bare repos, it's the common dir itself
    let main_path = if common_is_bare {
        common.clone()
    } else {
        common.parent().unwrap_or(&common).to_path_buf()
    };
    let is_bare = common_is_bare;
    entries.push(WorktreeInfo {
        path: main_path,
        head: main_head,
        is_bare,
        is_locked: false,
        lock_reason: None,
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
                let parent = p.parent().unwrap_or(&p).to_path_buf();
                // Canonicalize to resolve relative paths like '../there'
                parent.canonicalize().unwrap_or(parent)
            } else {
                admin.clone()
            };

            let locked_file = admin.join("locked");
            let is_locked = locked_file.exists();
            let lock_reason = if is_locked {
                let content = fs::read_to_string(&locked_file).unwrap_or_default();
                let reason = content.trim().to_string();
                if reason.is_empty() {
                    None
                } else {
                    Some(reason)
                }
            } else {
                None
            };

            entries.push(WorktreeInfo {
                path: wt_path,
                head: wt_head,
                is_bare: false,
                is_locked,
                lock_reason,
            });
        }
    }

    Ok(entries)
}

fn cmd_list(args: ListArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    // -z requires --porcelain
    if args.nul && !args.porcelain {
        bail!("--null requires --porcelain");
    }
    let entries = collect_worktrees(&repo)?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if args.porcelain {
        // With -z, use NUL between fields and between entries; without -z use newlines.
        let sep: u8 = if args.nul { 0 } else { b'\n' };
        let entry_sep: &[u8] = if args.nul { b"\0" } else { b"\n" };
        for entry in &entries {
            out.write_all(format!("worktree {}", entry.path.display()).as_bytes())?;
            out.write_all(&[sep])?;
            if !entry.is_bare {
                let head_oid = match &entry.head {
                    HeadState::Branch { oid: Some(oid), .. } => oid.to_hex(),
                    HeadState::Detached { oid } => oid.to_hex(),
                    _ => "0".repeat(40),
                };
                out.write_all(format!("HEAD {head_oid}").as_bytes())?;
                out.write_all(&[sep])?;
                match &entry.head {
                    HeadState::Branch { refname, .. } => {
                        out.write_all(format!("branch {refname}").as_bytes())?;
                        out.write_all(&[sep])?;
                    }
                    HeadState::Detached { .. } => {
                        out.write_all(b"detached")?;
                        out.write_all(&[sep])?;
                    }
                    _ => {}
                }
            }
            if entry.is_bare {
                out.write_all(b"bare")?;
                out.write_all(&[sep])?;
            }
            if entry.is_locked {
                if let Some(ref reason) = entry.lock_reason {
                    // Quote and escape the reason if it contains newlines
                    if reason.contains('\n') || reason.contains('\r') {
                        let escaped = reason.replace('\r', "\\r").replace('\n', "\\n");
                        out.write_all(format!("locked \"{escaped}\"").as_bytes())?;
                    } else {
                        out.write_all(format!("locked {reason}").as_bytes())?;
                    }
                } else {
                    out.write_all(b"locked")?;
                }
                out.write_all(&[sep])?;
            }
            // prunable: worktree path no longer exists on disk
            if !entry.is_bare && !entry.path.exists() {
                out.write_all(b"prunable gitdir file points to non-existent location")?;
                out.write_all(&[sep])?;
            }
            out.write_all(entry_sep)?;
        }
    } else {
        // Compute max path display width for column alignment (min 40, use char count not bytes)
        let max_path_len = entries
            .iter()
            .map(|e| e.path.display().to_string().chars().count())
            .max()
            .unwrap_or(0)
            .max(40);
        for entry in &entries {
            // Bare repos don't show a SHA in the non-porcelain output
            let sha = if entry.is_bare {
                String::new()
            } else {
                match &entry.head {
                    HeadState::Branch { oid: Some(oid), .. } => oid.to_hex()[..7].to_string(),
                    HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
                    _ => "0000000".to_string(),
                }
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

            // In verbose mode, locks with reasons are shown on separate lines (not as suffix)
            let lock_marker = if entry.is_locked {
                if args.verbose && entry.lock_reason.is_some() {
                    ""
                } else {
                    " locked"
                }
            } else {
                ""
            };
            // "prunable" annotation for worktrees whose path no longer exists
            // In verbose mode, prunable info goes on a separate line
            let is_prunable = !entry.is_bare && !entry.path.exists();
            let prunable_marker = if is_prunable && !args.verbose {
                " prunable"
            } else {
                ""
            };
            let path_str = entry.path.display().to_string();
            if entry.is_bare {
                writeln!(
                    out,
                    "{:<width$} {}{}{}",
                    path_str,
                    branch_info,
                    lock_marker,
                    prunable_marker,
                    width = max_path_len,
                )?;
            } else {
                writeln!(
                    out,
                    "{:<width$} {} {}{}{}",
                    path_str,
                    sha,
                    branch_info,
                    lock_marker,
                    prunable_marker,
                    width = max_path_len,
                )?;
            }
            // In verbose mode, show lock reason and prunable details
            if args.verbose {
                if entry.is_locked {
                    if let Some(ref reason) = entry.lock_reason {
                        writeln!(out, "\tlocked: {reason}")?;
                    }
                }
                if is_prunable {
                    writeln!(
                        out,
                        "\tprunable: gitdir file points to non-existent location"
                    )?;
                }
            }
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
    // Locked: needs --force --force (force >= 2) to remove
    if admin.join("locked").exists() && args.force < 2 {
        if args.force >= 1 {
            bail!(
                "worktree '{}' is locked; use 'git worktree remove --force --force'",
                wt_path.display()
            );
        }
        bail!(
            "worktree '{}' is locked; use --force or unlock it first",
            wt_path.display()
        );
    }

    // Check for dirty/untracked files unless --force >= 1
    if args.force < 1 && wt_path.exists() {
        // Load the linked worktree's index (stored in the admin directory)
        let index_path = admin.join("index");
        if index_path.exists() {
            if let Ok(index) = grit_lib::index::Index::load(&index_path) {
                // Check for untracked files
                if has_untracked_files(&wt_path, &index) {
                    bail!("worktree '{}' contains modified or untracked files; use --force to delete it", wt_path.display());
                }
                // Check for dirty tracked files
                if has_dirty_files(&wt_path, &index, &repo) {
                    bail!("worktree '{}' contains modified or untracked files; use --force to delete it", wt_path.display());
                }
            }
        }
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

    // If .git/worktrees is now empty, remove it too
    if worktrees_dir.exists() {
        if let Ok(mut entries) = fs::read_dir(&worktrees_dir) {
            if entries.next().is_none() {
                let _ = fs::remove_dir(&worktrees_dir);
            }
        }
    }

    Ok(())
}

/// Find a worktree admin directory name by matching the path recorded in its
/// `gitdir` file.
/// Check if a worktree was checked out recently (relative to an expire time like "2.days.ago").
/// Returns true if it was checked out AFTER the expire threshold (i.e., not expired).
fn is_recently_checked_out(admin: &Path, expire: &str) -> bool {
    // Parse expire string like "2.days.ago", "1.hour.ago", "now"
    let threshold_secs = parse_expire_to_secs(expire);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let threshold = now - threshold_secs;

    // Check the gitdir file's mtime
    let gitdir_file = admin.join("gitdir");
    if let Ok(meta) = std::fs::metadata(&gitdir_file) {
        if let Ok(mtime) = meta.modified() {
            if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                return (d.as_secs() as i64) > threshold;
            }
        }
    }
    false
}

/// Parse git's expire format like "2.days.ago", "1.hour.ago", "2.weeks.ago", "now".
/// Returns number of seconds to subtract from now to get the threshold.
fn parse_expire_to_secs(expire: &str) -> i64 {
    if expire == "now" {
        return 0;
    }
    // Format: N.unit.ago
    let parts: Vec<&str> = expire.split('.').collect();
    if parts.len() >= 3 && parts[2] == "ago" {
        if let Ok(n) = parts[0].parse::<i64>() {
            let secs = match parts[1] {
                "seconds" | "second" => n,
                "minutes" | "minute" => n * 60,
                "hours" | "hour" => n * 3600,
                "days" | "day" => n * 86400,
                "weeks" | "week" => n * 604800,
                _ => n * 86400,
            };
            return secs;
        }
    }
    // Default: treat as days if just a number
    if let Ok(n) = expire.parse::<i64>() {
        return n * 86400;
    }
    0
}

/// Check if a directory contains an initialized submodule (has .git directory inside).
fn has_initialized_submodule(wt_path: &Path) -> bool {
    walk_for_submodule(wt_path, wt_path)
}

fn walk_for_submodule(base: &Path, dir: &Path) -> bool {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.file_name().map(|n| n == ".git").unwrap_or(false) {
            if path != base.join(".git") {
                // Found a .git directory that's NOT the worktree's own .git
                return true;
            }
        } else if path.is_dir() && path.file_name().map(|n| n != ".git").unwrap_or(true) {
            if walk_for_submodule(base, &path) {
                return true;
            }
        }
    }
    false
}

/// Check if a worktree has untracked files.
fn has_untracked_files(work_tree: &Path, index: &grit_lib::index::Index) -> bool {
    let staged: std::collections::HashSet<Vec<u8>> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    walk_for_untracked(work_tree, work_tree, &staged)
}

fn walk_for_untracked(
    base: &Path,
    dir: &Path,
    staged: &std::collections::HashSet<Vec<u8>>,
) -> bool {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        if path.is_dir() {
            if walk_for_untracked(base, &path, staged) {
                return true;
            }
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                let rel_bytes = rel.to_string_lossy().as_bytes().to_vec();
                if !staged.contains(&rel_bytes) {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if a worktree has dirty tracked files.
fn has_dirty_files(
    work_tree: &Path,
    index: &grit_lib::index::Index,
    repo: &grit_lib::repo::Repository,
) -> bool {
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let rel = String::from_utf8_lossy(&entry.path);
        let abs = work_tree.join(rel.as_ref());
        match std::fs::read(&abs) {
            Ok(data) => {
                let oid = grit_lib::odb::Odb::hash_object_data(
                    grit_lib::objects::ObjectKind::Blob,
                    &data,
                );
                if oid != entry.oid {
                    return true;
                }
            }
            Err(_) => return true, // missing file = dirty
        }
    }
    false
}

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

    bail!("'{}' is not a working tree", target.display());
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
        let file_type = entry.file_type()?;
        let admin = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Files inside worktrees/ are invalid
        if !file_type.is_dir() {
            if args.verbose || args.dry_run {
                eprintln!("Removing worktrees/{name}: not a valid directory");
            }
            if !args.dry_run {
                let _ = fs::remove_file(&admin);
            }
            continue;
        }

        // A worktree is stale if its gitdir target no longer exists
        let gitdir_file = admin.join("gitdir");
        let (is_stale, stale_reason) = if !gitdir_file.exists() {
            (true, "gitdir file does not exist")
        } else {
            match fs::read_to_string(&gitdir_file) {
                Err(_) => (true, "unable to read gitdir file"),
                Ok(raw) => {
                    let target_str = raw.trim();
                    if target_str.is_empty() {
                        (true, "invalid gitdir file")
                    } else {
                        let target = PathBuf::from(target_str);
                        if !target.exists() {
                            (true, "gitdir file points to non-existent location")
                        } else {
                            (false, "")
                        }
                    }
                }
            }
        };

        if !is_stale {
            continue; // Not stale, keep it
        }

        // Stale: if --expire is set, only prune if older than expire time
        if let Some(ref expire_str) = args.expire {
            if is_recently_checked_out(&admin, expire_str) {
                continue; // Recent enough, don't prune
            }
        }

        // Skip locked worktrees
        if admin.join("locked").exists() {
            if args.verbose {
                eprintln!("worktree '{name}' is locked; not pruning");
            }
            continue;
        }

        if args.verbose || args.dry_run {
            eprintln!("Removing worktrees/{name}: {stale_reason}");
        }

        if !args.dry_run {
            fs::remove_dir_all(&admin)
                .with_context(|| format!("cannot remove '{}'", admin.display()))?;
        }
    }

    // Check for duplicate entries (multiple admins pointing to same gitdir)
    if worktrees_dir.is_dir() {
        let mut gitdir_targets: std::collections::HashMap<PathBuf, String> =
            std::collections::HashMap::new();
        let mut all_entries: Vec<String> = fs::read_dir(&worktrees_dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        all_entries.sort();
        for name in &all_entries {
            let admin = worktrees_dir.join(name);
            let gitdir_file = admin.join("gitdir");
            if let Ok(raw) = fs::read_to_string(&gitdir_file) {
                let target = PathBuf::from(raw.trim());
                let target_canonical = target.canonicalize().unwrap_or(target);
                if let Some(first_name) = gitdir_targets.get(&target_canonical) {
                    if first_name != name {
                        // Duplicate! Remove this one.
                        if args.verbose || args.dry_run {
                            eprintln!("Removing worktrees/{name}: duplicate entry");
                        }
                        if !args.dry_run {
                            let _ = fs::remove_dir_all(&admin);
                        }
                    }
                } else {
                    gitdir_targets.insert(target_canonical, name.clone());
                }
            }
        }
    }

    // If .git/worktrees is now empty, remove it too
    if !args.dry_run && worktrees_dir.exists() {
        if let Ok(mut entries) = fs::read_dir(&worktrees_dir) {
            if entries.next().is_none() {
                let _ = fs::remove_dir(&worktrees_dir);
            }
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
    if admin.join("locked").exists() && args.force < 2 {
        if args.force >= 1 {
            bail!(
                "worktree '{}' is locked; use 'git worktree move --force --force' to force",
                src_path.display()
            );
        }
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

    // If destination is an existing directory, append the source basename
    let dst_path = if dst_path.exists() && dst_path.is_dir() {
        let src_name = src_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("cannot determine source name"))?;
        dst_path.join(src_name)
    } else {
        dst_path
    };

    // If destination is registered as a worktree (but possibly missing from disk), require --force
    let dst_canonical = dst_path.canonicalize().unwrap_or(dst_path.clone());
    let is_registered_wt = worktrees_dir.is_dir() && {
        std::fs::read_dir(&worktrees_dir)
            .ok()
            .map(|entries| {
                entries.flatten().any(|e| {
                    let gitdir_file = e.path().join("gitdir");
                    if let Ok(raw) = std::fs::read_to_string(&gitdir_file) {
                        let p = std::path::Path::new(raw.trim());
                        let wt = p.parent().unwrap_or(p);
                        wt.canonicalize().unwrap_or(wt.to_path_buf()) == dst_canonical
                    } else {
                        false
                    }
                })
            })
            .unwrap_or(false)
    };
    if !dst_path.exists() && is_registered_wt && args.force < 1 {
        bail!(
            "'{}' is a missing but registered worktree; use --force to overwrite",
            dst_path.display()
        );
    }

    if dst_path.exists() {
        bail!("target '{}' already exists", dst_path.display());
    }

    // Check for initialized submodules (cannot move a worktree with active submodules)
    if args.force < 1 {
        if has_initialized_submodule(&src_path) {
            bail!("cannot move a working tree containing an initialized submodule");
        }
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

    // Pre-validate specific paths before checking worktrees_dir
    if !args.paths.is_empty() {
        for p in &args.paths {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                std::env::current_dir()?.join(p)
            };
            let abs = abs.canonicalize().unwrap_or_else(|_| abs.clone());
            // Real git repos (with .git directory) are not worktrees
            if abs.join(".git").is_dir() {
                eprintln!("error: '{}': .git is not a file", abs.display());
                std::process::exit(1);
            }
            // .git file pointing to non-git location
            let git_file = abs.join(".git");
            if git_file.is_file() {
                let content = fs::read_to_string(&git_file).unwrap_or_default();
                let target_str = content.trim().strip_prefix("gitdir: ").unwrap_or("");
                let target = std::path::Path::new(target_str);
                if target.exists() && target.is_dir() && !target_str.contains("worktrees") {
                    // Target is a directory but not a git admin dir
                    eprintln!(
                        "error: '{}': .git file does not reference a repository",
                        abs.display()
                    );
                    std::process::exit(1);
                } else if !target.exists() && !target_str.is_empty() {
                    eprintln!("error: '{}': .git file broken", abs.display());
                    std::process::exit(1);
                }
            }
        }
    }

    if !worktrees_dir.is_dir() {
        // If paths given but no worktrees dir, they're invalid
        if !args.paths.is_empty() {
            for p in &args.paths {
                eprintln!("error: '{}': not a valid path", p.display());
            }
            std::process::exit(1);
        }
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
                Err(_) => {
                    // Maybe the worktree was moved — check if it has a .git file pointing to our admin
                    let dotgit = abs.join(".git");
                    if let Ok(content) = fs::read_to_string(&dotgit) {
                        if let Some(admin_path) = content.trim().strip_prefix("gitdir: ") {
                            let admin = PathBuf::from(admin_path);
                            let admin_dir = if admin.is_absolute() {
                                admin.clone()
                            } else {
                                abs.join(&admin)
                            };
                            let admin_dir = admin_dir.canonicalize().unwrap_or(admin_dir);
                            // Check that this admin dir is under our worktrees_dir
                            if admin_dir.starts_with(&worktrees_dir) {
                                // The worktree was moved — update admin's gitdir
                                let new_gitdir_path = abs.join(".git");
                                let old_gitdir_file = admin_dir.join("gitdir");
                                let reason = if !old_gitdir_file.exists() {
                                    "gitdir unreadable"
                                } else {
                                    "gitdir incorrect"
                                };
                                let new_content = format!("{}\n", new_gitdir_path.display());
                                fs::write(&old_gitdir_file, &new_content)?;
                                eprintln!(
                                    "repair: {}: {reason}: {}",
                                    abs.display(),
                                    old_gitdir_file.display()
                                );
                                continue;
                            }
                        }
                    }
                    eprintln!("error: '{}': not a valid path", p.display());
                    std::process::exit(1);
                }
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

        // Repair 1: If the worktree .git file exists and points to the correct admin dir, it's fine.
        // If it exists but points to an EXISTING but different admin dir, repair the pointer.
        // If it exists but points to a NON-EXISTENT location, fall through to Repair 2.
        if wt_dotgit.exists() {
            let dotgit_content = fs::read_to_string(wt_dotgit).unwrap_or_default();
            let expected_prefix = "gitdir: ";
            if let Some(current_target) = dotgit_content.trim().strip_prefix(expected_prefix) {
                let current_path = PathBuf::from(current_target);
                // Only repair if the target exists but is wrong
                if current_path.exists() {
                    let admin_canonical = admin.canonicalize().unwrap_or_else(|_| admin.clone());
                    let current_canonical =
                        current_path.canonicalize().unwrap_or(current_path.clone());
                    if current_canonical != admin_canonical {
                        // Check if the current target is our own admin dir or a different one
                        // If it points to a different valid git admin, report as "incorrect"
                        let is_our_admin = current_target.contains("worktrees");
                        if !is_our_admin {
                            eprintln!(
                                "repair: {}: .git file incorrect; repaired",
                                wt_path.display()
                            );
                        } else {
                            eprintln!(
                                "repair: {}: repaired gitfile to point to {}",
                                wt_path.display(),
                                admin.display()
                            );
                        }
                        // Fix the .git file (it points to different valid location)
                        let fixed = format!("gitdir: {}\n", admin.display());
                        fs::write(wt_dotgit, &fixed)?;
                    }
                    // If already correct, nothing to do
                    continue;
                }
                // current_path doesn't exist → fall through to Repair 2
            }
        }

        // Repair 2: Verify gitdir file in admin points to an existing location
        let need_repair_reason = if !wt_dotgit.exists() {
            Some(".git file broken")
        } else {
            let content = fs::read_to_string(wt_dotgit).unwrap_or_default();
            let target = content.trim().strip_prefix("gitdir: ").unwrap_or("");
            if target.is_empty() {
                Some(".git file broken")
            } else {
                let target_path = PathBuf::from(target);
                if !target_path.exists() {
                    Some(".git file broken")
                } else {
                    None
                }
            }
        };
        if let Some(reason) = need_repair_reason {
            if wt_path.exists() {
                if !wt_path.is_dir() {
                    eprintln!("error: {}: not a directory", wt_path.display());
                    std::process::exit(1);
                }
                // Don't clobber an existing .git directory (real repo)
                let dotgit_path = wt_path.join(".git");
                if dotgit_path.is_dir() {
                    eprintln!("error: {}: .git is not a file", wt_path.display());
                    std::process::exit(1);
                }
                let dotgit_content = format!("gitdir: {}\n", admin.display());
                fs::write(&dotgit_path, &dotgit_content)?;
                eprintln!(
                    "repair: {wt_path}: {reason}; recreated gitfile",
                    wt_path = wt_path.display()
                );
            }
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

    let reason = args.reason.as_deref().unwrap_or("");
    // Write reason with trailing newline (to match `echo reason > locked`)
    let content = if reason.is_empty() {
        String::new()
    } else {
        format!("{reason}\n")
    };
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
