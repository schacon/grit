//! `grit switch` — switch branches.
//!
//! Pre-checks: ambiguous remote-tracking branches and worktree conflicts.
//! Builds a [`checkout`](crate::commands::checkout) argument list from explicit
//! `git switch` flags, then delegates with the remaining positional arguments.

use crate::commands::checkout;
use anyhow::Result;
use clap::Args as ClapArgs;
use std::collections::BTreeSet;

/// Arguments for `grit switch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Switch branches")]
pub struct Args {
    /// Create a new branch and switch to it (`-c`).
    #[arg(short = 'c', long = "create", value_name = "BRANCH")]
    pub create: Option<String>,

    /// Create or reset a branch and switch (`-C`).
    #[arg(short = 'C', long = "force-create", value_name = "BRANCH")]
    pub force_create: Option<String>,

    /// Detach HEAD at the resolved commit.
    #[arg(long = "detach", short = 'd')]
    pub detach: bool,

    /// Quiet checkout output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Set up tracking (optional mode: `direct` / `inherit`).
    #[arg(long = "track", short = 't', value_name = "MODE", num_args = 0..=1)]
    pub track: Option<String>,

    #[arg(long = "no-track")]
    pub no_track: bool,

    /// Merge local modifications when switching.
    #[arg(long = "merge", short = 'm')]
    pub merge: bool,

    #[arg(long = "guess")]
    pub guess: bool,

    #[arg(long = "no-guess")]
    pub no_guess: bool,

    #[arg(long = "ignore-other-worktrees")]
    pub ignore_other_worktrees: bool,

    #[arg(long = "recurse-submodules")]
    pub recurse_submodules: bool,

    /// Create a new orphan branch.
    #[arg(long = "orphan", value_name = "BRANCH")]
    pub orphan: Option<String>,

    /// Remaining arguments (branch name, paths, `--`, etc.).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub rest: Vec<String>,
}

/// Run `grit switch`.
pub fn run(args: Args) -> Result<()> {
    let mut checkout_tail: Vec<String> = Vec::new();

    if let Some(b) = &args.create {
        checkout_tail.push("-c".to_string());
        checkout_tail.push(b.clone());
    }
    if let Some(b) = &args.force_create {
        checkout_tail.push("-C".to_string());
        checkout_tail.push(b.clone());
    }
    if args.detach {
        checkout_tail.push("--detach".to_string());
    }
    if args.quiet {
        checkout_tail.push("-q".to_string());
    }
    if let Some(mode) = &args.track {
        checkout_tail.push("--track".to_string());
        checkout_tail.push(mode.clone());
    }
    if args.no_track {
        checkout_tail.push("--no-track".to_string());
    }
    if args.merge {
        checkout_tail.push("-m".to_string());
    }
    if args.guess {
        checkout_tail.push("--guess".to_string());
    }
    if args.no_guess {
        checkout_tail.push("--no-guess".to_string());
    }
    if args.ignore_other_worktrees {
        checkout_tail.push("--ignore-other-worktrees".to_string());
    }
    if args.recurse_submodules {
        checkout_tail.push("--recurse-submodules".to_string());
    }
    if let Some(b) = &args.orphan {
        checkout_tail.push("--orphan".to_string());
        checkout_tail.push(b.clone());
    }

    checkout_tail.extend(args.rest);

    if let Some(ambiguous) =
        detect_ambiguous_remote_tracking(&checkout_tail).map_err(|e| anyhow::anyhow!(e))?
    {
        eprintln!(
            "hint: '{}' could refer to more than one remote-tracking branch:",
            ambiguous.branch
        );
        for candidate in &ambiguous.candidates {
            eprintln!("hint:   {candidate}");
        }
        eprintln!("hint: If you meant to check out one of these branches, use:");
        eprintln!("hint:   git switch --track <remote>/{}", ambiguous.branch);
        eprintln!(
            "fatal: '{}' matched multiple ({}) remote tracking branches",
            ambiguous.branch,
            ambiguous.candidates.len()
        );
        std::process::exit(128);
    }

    if let Err(msg) = check_worktree_conflict(&checkout_tail) {
        eprintln!("fatal: {msg}");
        std::process::exit(128);
    }
    checkout::run(checkout::Args {
        rest: checkout_tail,
        ..Default::default()
    })
}

/// Parse the raw switch arguments to extract the target branch name and check
/// whether it is already checked out in another worktree.
fn check_worktree_conflict(args: &[String]) -> std::result::Result<(), String> {
    if args.iter().any(|a| a == "--ignore-other-worktrees") {
        return Ok(());
    }

    if args.iter().any(|a| a == "--orphan") {
        return Ok(());
    }

    let branch = match extract_switch_target(args) {
        Some(b) => b,
        None => return Ok(()),
    };

    check_branch_in_worktrees(&branch)
}

fn check_branch_in_worktrees(branch: &str) -> std::result::Result<(), String> {
    use grit_lib::repo::Repository;

    let repo = match Repository::discover(None) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let git_dir = &repo.git_dir;

    let main_git_dir = if git_dir.join("commondir").exists() {
        let common = std::fs::read_to_string(git_dir.join("commondir")).unwrap_or_default();
        let common = common.trim();
        if std::path::Path::new(common).is_absolute() {
            std::path::PathBuf::from(common)
        } else {
            git_dir
                .join(common)
                .canonicalize()
                .unwrap_or_else(|_| git_dir.clone())
        }
    } else {
        git_dir.clone()
    };

    let branch_ref_no_nl = format!("ref: refs/heads/{branch}");

    let main_head_path = main_git_dir.join("HEAD");
    if main_head_path != git_dir.join("HEAD") {
        if let Ok(head_content) = std::fs::read_to_string(&main_head_path) {
            let head_trimmed = head_content.trim();
            if head_trimmed == branch_ref_no_nl
                || head_trimmed == format!("ref: refs/heads/{branch}")
            {
                let wt_path = main_git_dir.parent().unwrap_or(&main_git_dir);
                return Err(format!(
                    "'{}' is already used by worktree at '{}'",
                    branch,
                    wt_path.display()
                ));
            }
        }
    }

    let worktrees_dir = main_git_dir.join("worktrees");
    if worktrees_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&worktrees_dir) {
            for entry in entries.flatten() {
                let wt_git_dir = entry.path();
                if wt_git_dir
                    .canonicalize()
                    .unwrap_or_else(|_| wt_git_dir.clone())
                    == git_dir.canonicalize().unwrap_or_else(|_| git_dir.clone())
                {
                    continue;
                }
                let head_path = wt_git_dir.join("HEAD");
                if let Ok(head_content) = std::fs::read_to_string(&head_path) {
                    let head_trimmed = head_content.trim();
                    if head_trimmed == branch_ref_no_nl {
                        let wt_path = if let Ok(gitdir_content) =
                            std::fs::read_to_string(wt_git_dir.join("gitdir"))
                        {
                            let p = gitdir_content.trim().to_string();
                            std::path::Path::new(&p)
                                .parent()
                                .map(|p| p.display().to_string())
                                .unwrap_or(p)
                        } else {
                            wt_git_dir.display().to_string()
                        };
                        return Err(format!(
                            "'{}' is already used by worktree at '{}'",
                            branch, wt_path
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

struct AmbiguousRemoteBranch {
    branch: String,
    candidates: Vec<String>,
}

fn detect_ambiguous_remote_tracking(
    args: &[String],
) -> std::result::Result<Option<AmbiguousRemoteBranch>, String> {
    let branch = match extract_switch_target(args) {
        Some(b) => b,
        None => return Ok(None),
    };

    if branch.contains('/') {
        return Ok(None);
    }

    if branch == "-" || branch == "HEAD" {
        return Ok(None);
    }

    let repo = grit_lib::repo::Repository::discover(None).map_err(|e| e.to_string())?;

    let local_ref = format!("refs/heads/{branch}");
    if grit_lib::refs::resolve_ref(&repo.git_dir, &local_ref).is_ok() {
        return Ok(None);
    }

    let refs =
        grit_lib::refs::list_refs(&repo.git_dir, "refs/remotes/").map_err(|e| e.to_string())?;
    let mut candidates = BTreeSet::new();
    for (refname, _oid) in refs {
        if let Some(rest) = refname.strip_prefix("refs/remotes/") {
            if let Some((remote, remote_branch)) = rest.split_once('/') {
                if remote_branch == branch {
                    candidates.insert(format!("{remote}/{remote_branch}"));
                }
            }
        }
    }

    if candidates.len() <= 1 {
        return Ok(None);
    }

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    if let Some(default_remote) = config.get("checkout.defaultRemote") {
        let preferred = format!("{default_remote}/{branch}");
        if candidates.contains(&preferred) {
            return Ok(None);
        }
    }

    Ok(Some(AmbiguousRemoteBranch {
        branch,
        candidates: candidates.into_iter().collect(),
    }))
}

/// Extract the target branch name from `git switch` raw args.
///
/// Handles:
/// - `switch <branch>`
/// - `switch -c <branch> [<start>]`
/// - `switch -C <branch> [<start>]`
/// - `switch --create <branch> [<start>]`
/// - `switch --force-create <branch> [<start>]`
fn extract_switch_target(args: &[String]) -> Option<String> {
    let mut branch: Option<String> = None;
    let mut i = 0;
    let mut past_double_dash = false;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            past_double_dash = true;
            i += 1;
            continue;
        }
        if past_double_dash {
            if branch.is_none() {
                branch = Some(a.clone());
            }
            i += 1;
            continue;
        }
        if (a == "-c" || a == "-C" || a == "--create" || a == "--force-create")
            && i + 1 < args.len()
        {
            branch = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        if let Some(rest) = a.strip_prefix("-c").or_else(|| a.strip_prefix("-C")) {
            if !rest.is_empty() && !rest.starts_with('-') {
                branch = Some(rest.to_string());
                i += 1;
                continue;
            }
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        if branch.is_none() {
            branch = Some(a.clone());
        }
        i += 1;
    }
    branch
}
