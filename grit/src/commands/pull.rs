//! `grit pull` — fetch from a remote and integrate changes.
//!
//! Equivalent to running `grit fetch` followed by `grit merge` (or
//! `grit rebase` with `--rebase`).  Only local transports are supported.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;

/// Arguments for `grit pull`.
#[derive(Debug, ClapArgs)]
#[command(about = "Fetch from and integrate with another repository")]
pub struct Args {
    /// Remote name (defaults to "origin").
    #[arg(value_name = "REMOTE")]
    pub remote: Option<String>,

    /// Remote branch to pull (defaults to tracking branch or current branch name).
    #[arg(value_name = "BRANCH")]
    pub branch: Option<String>,

    /// Rebase instead of merge.
    #[arg(long = "rebase", short = 'r')]
    pub rebase: bool,

    /// Only allow fast-forward merges.
    #[arg(long = "ff-only")]
    pub ff_only: bool,

    /// Do not allow fast-forward (always create merge commit).
    #[arg(long = "no-ff")]
    pub no_ff: bool,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    let head = resolve_head(&repo.git_dir)?;
    let current_branch = head.branch_name().map(|s| s.to_owned());

    // Determine remote name: explicit arg > branch.<name>.remote > "origin"
    let remote_name_owned: String = if let Some(ref r) = args.remote {
        r.clone()
    } else if let Some(ref branch) = current_branch {
        config
            .get(&format!("branch.{branch}.remote"))
            .unwrap_or_else(|| "origin".to_owned())
    } else {
        "origin".to_owned()
    };
    let remote_name = remote_name_owned.as_str();

    // Determine merge branch
    let merge_branch = if let Some(ref b) = args.branch {
        b.clone()
    } else if let Some(ref branch) = current_branch {
        // Check branch.<name>.merge (e.g. "refs/heads/main")
        if let Some(merge_ref) = config.get(&format!("branch.{branch}.merge")) {
            merge_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(&merge_ref)
                .to_owned()
        } else {
            branch.clone()
        }
    } else {
        bail!("no tracking branch configured and no branch specified");
    };

    // Step 1: Fetch
    let fetch_args = super::fetch::Args {
        remote: Some(remote_name.to_owned()),
        all: false,
        tags: false,
        no_tags: false,
        prune: false,
        quiet: args.quiet,
    };
    super::fetch::run(fetch_args)?;

    // Step 2: Determine the remote-tracking ref to merge/rebase from
    let tracking_ref = format!("refs/remotes/{remote_name}/{merge_branch}");
    let remote_oid = refs::resolve_ref(&repo.git_dir, &tracking_ref)
        .with_context(|| format!("no tracking ref '{tracking_ref}' after fetch"))?;

    // Step 3: Merge or rebase
    if args.rebase {
        let rebase_args = super::rebase::Args {
            upstream: Some(tracking_ref),
            onto: None,
            interactive: false,
            r#continue: false,
            abort: false,
            skip: false,
        };
        super::rebase::run(rebase_args)
    } else {
        let merge_args = super::merge::Args {
            commits: vec![remote_oid.to_hex()],
            message: None,
            ff_only: args.ff_only,
            no_ff: args.no_ff,
            squash: false,
            abort: false,
            continue_merge: false,
            quiet: args.quiet,
        };
        super::merge::run(merge_args)
    }
}
