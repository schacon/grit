//! `grit check-ref-format` — validate a ref name.
//!
//! Checks whether a given ref name is acceptable and optionally normalises it.
//!
//! Exit codes: 0 = valid, 1 = invalid (same as git).

use anyhow::Result;
use clap::Args as ClapArgs;
use grit_lib::check_ref_format::{check_refname_format, RefNameOptions};

/// Arguments for `grit check-ref-format`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Allow a single-level refname with no `/` separator.
    #[arg(long = "allow-onelevel")]
    pub allow_onelevel: bool,

    /// Allow a single `*` wildcard anywhere in the refname (refspec pattern).
    #[arg(long = "refspec-pattern")]
    pub refspec_pattern: bool,

    /// Normalise the refname by stripping a leading `/` and collapsing
    /// consecutive slashes, then print it if valid.
    #[arg(long, alias = "print")]
    pub normalize: bool,

    /// Treat the argument as a branch shorthand (expand `@{-N}` syntax,
    /// validate against branch rules).  Mutually exclusive with other flags.
    #[arg(long)]
    pub branch: bool,

    /// The refname (or branch shorthand) to check.
    #[arg(value_name = "REFNAME", allow_hyphen_values = true)]
    pub refname: String,
}

/// Run `grit check-ref-format`.
///
/// Exits with code 0 when the ref name is valid, 1 when it is invalid.
/// Error details are **not** printed — git is also silent on invalid names.
pub fn run(args: Args) -> Result<()> {
    if args.branch {
        return run_branch_mode(&args.refname);
    }

    let opts = RefNameOptions {
        allow_onelevel: args.allow_onelevel,
        refspec_pattern: args.refspec_pattern,
        normalize: args.normalize,
    };

    match check_refname_format(&args.refname, &opts) {
        Ok(normalized) => {
            if args.normalize {
                println!("{normalized}");
            }
            Ok(())
        }
        Err(_) => {
            // Exit 1 without printing anything, matching git behaviour.
            std::process::exit(1);
        }
    }
}

/// Handle `--branch`: validate that the argument is a valid branch shorthand.
///
/// Git's `--branch` mode resolves `@{-N}` against the reflog and prints the
/// resolved branch name (or a full SHA if the previous checkout was detached).
/// For non-`@{` arguments, it validates as a branch name and prints it.
fn run_branch_mode(arg: &str) -> Result<()> {
    // Reject branch names starting with '-' (git does the same).
    if arg.starts_with('-') {
        std::process::exit(1);
    }

    // @{-N} syntax requires reflog lookup
    if arg.starts_with("@{-") && arg.ends_with('}') {
        let inner = &arg[3..arg.len() - 1];
        if let Ok(n) = inner.parse::<usize>() {
            if n >= 1 {
                match resolve_at_minus_for_branch(n) {
                    Some(name) => {
                        if name == "HEAD" {
                            std::process::exit(1);
                        }
                        println!("{name}");
                        return Ok(());
                    }
                    None => std::process::exit(1),
                }
            }
        }
        std::process::exit(1);
    }

    // Reject other @{ forms without a repo
    if arg.contains("@{") {
        std::process::exit(1);
    }

    // Validate as a single-level or multi-level ref (branch names may or may
    // not contain slashes).
    let opts = RefNameOptions {
        allow_onelevel: true,
        refspec_pattern: false,
        normalize: false,
    };

    match check_refname_format(arg, &opts) {
        Ok(_) => {
            // Match git's `check_branch_ref`: `refs/heads/HEAD` is never a valid branch ref.
            if arg == "HEAD" {
                std::process::exit(1);
            }
            println!("{arg}");
            Ok(())
        }
        Err(_) => {
            std::process::exit(1);
        }
    }
}

/// Resolve `@{-N}` for `--branch` mode: return the branch name or detached SHA.
fn resolve_at_minus_for_branch(n: usize) -> Option<String> {
    use grit_lib::reflog::read_reflog;
    use grit_lib::repo::Repository;

    let repo = Repository::discover(None).ok()?;
    let entries = read_reflog(&repo.git_dir, "HEAD").ok()?;
    let mut count = 0usize;
    for entry in entries.iter().rev() {
        let msg = &entry.message;
        if let Some(rest) = msg.strip_prefix("checkout: moving from ") {
            count += 1;
            if count == n {
                if let Some(to_pos) = rest.find(" to ") {
                    let from_branch = &rest[..to_pos];
                    // If the "from" branch is a valid ref, return just the short name
                    let ref_name = format!("refs/heads/{from_branch}");
                    if grit_lib::refs::resolve_ref(&repo.git_dir, &ref_name).is_ok() {
                        return Some(from_branch.to_string());
                    }
                    // Try to expand abbreviated SHA to full SHA
                    if from_branch.len() >= 4 && from_branch.chars().all(|c| c.is_ascii_hexdigit())
                    {
                        if let Ok(oid) = grit_lib::rev_parse::resolve_revision(&repo, from_branch) {
                            return Some(oid.to_hex());
                        }
                    }
                    // Otherwise return as-is
                    return Some(from_branch.to_string());
                }
            }
        }
    }
    None
}
