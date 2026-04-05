//! `grit switch` — passthrough to the system Git binary.
//!
//! Branch creation, switching, and orphan-branch operations are forwarded to
//! the real `git switch` so that the complete set of semantics is available.
//! Before delegating, we check whether the target branch is already checked
//! out in another worktree (a check that older system `git` versions omit
//! for `-C`/`-c`).

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit switch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Switch branches")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit switch` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    // Pre-check: refuse to switch to a branch already checked out in another
    // worktree unless --ignore-other-worktrees is given.
    if let Err(msg) = check_worktree_conflict(&args.args) {
        eprintln!("fatal: {msg}");
        std::process::exit(128);
    }
    git_passthrough::run("switch", &args.args)
}

/// Parse the raw switch arguments to extract the target branch name and check
/// whether it is already checked out in another worktree.
fn check_worktree_conflict(args: &[String]) -> std::result::Result<(), String> {
    // Quick scan for --ignore-other-worktrees
    if args.iter().any(|a| a == "--ignore-other-worktrees") {
        return Ok(());
    }

    // Quick scan for --orphan (no conflict possible)
    if args.iter().any(|a| a == "--orphan") {
        return Ok(());
    }

    // Extract the target branch name.  Possibilities:
    //   git switch <branch>
    //   git switch -c <branch> [<start>]
    //   git switch -C <branch> [<start>]
    //   git switch --create <branch> [<start>]
    //   git switch --force-create <branch> [<start>]
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
        // Flags that consume the next argument (skip it)
        if (a == "-c" || a == "-C" || a == "--create" || a == "--force-create")
            && i + 1 < args.len() {
                branch = Some(args[i + 1].clone());
                i += 2;
                continue;
            }
        // Combined form: -c<branch>, -C<branch>
        if let Some(rest) = a.strip_prefix("-c").or_else(|| a.strip_prefix("-C")) {
            if !rest.is_empty() && !rest.starts_with('-') {
                branch = Some(rest.to_string());
                i += 1;
                continue;
            }
        }
        // Skip known flags
        if a.starts_with('-') {
            // Some flags take a value
            if a == "-d"
                || a == "--detach"
                || a == "-f"
                || a == "--force"
                || a == "--no-guess"
                || a == "--guess"
                || a == "-q"
                || a == "--quiet"
                || a == "--progress"
                || a == "--no-progress"
                || a == "--no-track"
                || a == "-t"
                || a == "--track"
                || a == "--recurse-submodules"
                || a == "--no-recurse-submodules"
                || a == "--ignore-other-worktrees"
                || a == "--discard-changes"
                || a == "-m"
                || a == "--merge"
                || a == "--conflict"
            {
                i += 1;
                continue;
            }
            // Unknown flag, skip
            i += 1;
            continue;
        }
        // Positional argument: branch name
        if branch.is_none() {
            branch = Some(a.clone());
        }
        i += 1;
    }

    let branch = match branch {
        Some(b) => b,
        None => return Ok(()), // no branch to check
    };

    // Now check if this branch is checked out in another worktree.
    check_branch_in_worktrees(&branch)
}

fn check_branch_in_worktrees(branch: &str) -> std::result::Result<(), String> {
    use grit_lib::repo::Repository;

    let repo = match Repository::discover(None) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let git_dir = &repo.git_dir;

    // Check if this is a worktree itself
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

    // Check main worktree HEAD
    let main_head_path = main_git_dir.join("HEAD");
    if main_head_path != git_dir.join("HEAD") {
        if let Ok(head_content) = std::fs::read_to_string(&main_head_path) {
            let head_trimmed = head_content.trim();
            if head_trimmed == branch_ref_no_nl
                || head_trimmed == format!("ref: refs/heads/{branch}")
            {
                // Find the main worktree path
                let wt_path = main_git_dir.parent().unwrap_or(&main_git_dir);
                return Err(format!(
                    "'{}' is already used by worktree at '{}'",
                    branch,
                    wt_path.display()
                ));
            }
        }
    }

    // Check linked worktrees
    let worktrees_dir = main_git_dir.join("worktrees");
    if worktrees_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&worktrees_dir) {
            for entry in entries.flatten() {
                let wt_git_dir = entry.path();
                // Skip our own worktree
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
                        // Read gitdir to find the worktree path
                        let wt_path = if let Ok(gitdir_content) =
                            std::fs::read_to_string(wt_git_dir.join("gitdir"))
                        {
                            let p = gitdir_content.trim().to_string();
                            // gitdir points to <worktree>/.git, get the parent
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
