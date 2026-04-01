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
/// resolved branch name.  We implement the simpler subset: validate that the
/// argument could be a valid branch name and print it as-is.  `@{-N}` syntax
/// (which requires a live repo) prints an error and exits 1.
fn run_branch_mode(arg: &str) -> Result<()> {
    // Reject branch names starting with '-' (git does the same).
    if arg.starts_with('-') {
        std::process::exit(1);
    }

    // @{-N} syntax requires reflog lookup — we can't resolve it without a
    // live repo.  Reject it for now.
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
            println!("{arg}");
            Ok(())
        }
        Err(_) => {
            std::process::exit(1);
        }
    }
}
