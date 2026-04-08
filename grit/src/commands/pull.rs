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

    /// Rebase instead of merge. Optionally accepts a strategy like "merges".
    #[arg(long = "rebase", short = 'r', num_args = 0..=1, default_missing_value = "true")]
    pub rebase: Option<String>,

    /// Only allow fast-forward merges.
    #[arg(long = "ff-only")]
    pub ff_only: bool,

    /// Do not allow fast-forward (always create merge commit).
    #[arg(long = "no-ff")]
    pub no_ff: bool,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Merge strategy to use.
    #[arg(short = 's', long = "strategy")]
    pub strategy: Option<String>,

    /// Strategy option.
    #[arg(short = 'X', long = "strategy-option")]
    pub strategy_option: Vec<String>,

    /// Disable rebase (use merge, the default).
    #[arg(long = "no-rebase")]
    pub no_rebase: bool,

    /// Allow fast-forward (default).
    #[arg(long = "ff")]
    pub ff: bool,

    /// Include one-line descriptions from commit messages in the merge commit.
    /// Optionally limit the number of entries.
    #[arg(long = "log", num_args = 0..=1, default_missing_value = "0", require_equals = true)]
    pub log: Option<String>,

    /// Do not include one-line descriptions.
    #[arg(long = "no-log")]
    pub no_log: bool,
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

    // Check if remote is local (path-based: "." or contains "/")
    let is_local = remote_name == "." || remote_name.contains('/');

    if is_local && remote_name != "." {
        // For local path remotes, open the remote repo, copy objects, and resolve the branch there.
        let remote_path = if let Some(stripped) = remote_name.strip_prefix("file://") {
            std::path::PathBuf::from(stripped)
        } else {
            std::path::PathBuf::from(remote_name)
        };

        // Open remote repo (bare or non-bare)
        let remote_repo = if let Ok(r) = Repository::open(&remote_path, None) {
            r
        } else {
            let git_dir = remote_path.join(".git");
            Repository::open(&git_dir, Some(&remote_path)).with_context(|| {
                format!(
                    "could not open remote repository at '{}'",
                    remote_path.display()
                )
            })?
        };

        // Copy objects from remote to local
        super::fetch::copy_objects_for_pull(&remote_repo.git_dir, &repo.git_dir)?;

        // Determine which branch to merge: try the specified merge_branch on the remote,
        // falling back to the remote's HEAD
        let remote_oid = if let Ok(oid) =
            refs::resolve_ref(&remote_repo.git_dir, &format!("refs/heads/{merge_branch}"))
        {
            oid
        } else if let Ok(oid) = refs::resolve_ref(&remote_repo.git_dir, "HEAD") {
            oid
        } else {
            bail!("bad revision '{merge_branch}': could not resolve in remote");
        };

        // Write FETCH_HEAD for compatibility
        let fetch_head = format!("{}\t\t{}\n", remote_oid.to_hex(), merge_branch);
        std::fs::write(repo.git_dir.join("FETCH_HEAD"), &fetch_head)?;

        return do_merge_or_rebase(&args, &config, remote_oid.to_hex(), &merge_branch);
    } else if is_local {
        // "." remote — merge the configured upstream ref (e.g. `refs/heads/main`), not a
        // synthetic `refs/remotes/./main` remote-tracking ref (matches git pull).
        let remote_oid = grit_lib::rev_parse::resolve_revision(&repo, &merge_branch)
            .with_context(|| format!("bad revision '{merge_branch}'"))?;

        let merge_ref = current_branch
            .as_ref()
            .and_then(|b| config.get(&format!("branch.{b}.merge")))
            .unwrap_or_else(|| format!("refs/heads/{merge_branch}"));

        let fetch_head = format!("{}\t\t{}\n", remote_oid.to_hex(), merge_branch);
        std::fs::write(repo.git_dir.join("FETCH_HEAD"), &fetch_head)?;

        return do_merge_or_rebase(&args, &config, remote_oid.to_hex(), &merge_ref);
    }

    // Step 1: Fetch
    let fetch_args = super::fetch::Args {
        remote: Some(remote_name.to_owned()),
        refspecs: Vec::new(),
        filter: None,
        all: false,
        tags: false,
        no_tags: false,
        prune: false,
        prune_tags: false,
        deepen: None,
        depth: None,
        shallow_since: None,
        shallow_exclude: None,
        refetch: false,
        output: None,
        quiet: args.quiet,
        jobs: None,
        porcelain: false,
        no_show_forced_updates: false,
        show_forced_updates: false,
        negotiate_only: false,
        update_head_ok: false,
        update_refs: false,
        upload_pack: None,
    };
    super::fetch::run(fetch_args)?;

    // Step 2: Determine the remote-tracking ref to merge/rebase from
    let tracking_ref = format!("refs/remotes/{remote_name}/{merge_branch}");
    let remote_oid = refs::resolve_ref(&repo.git_dir, &tracking_ref)
        .with_context(|| format!("no tracking ref '{tracking_ref}' after fetch"))?;

    do_merge_or_rebase(&args, &config, remote_oid.to_hex(), &tracking_ref)
}

fn do_merge_or_rebase(
    args: &Args,
    config: &ConfigSet,
    oid_hex: String,
    ref_name: &str,
) -> Result<()> {
    if args.rebase.is_some() {
        let rebase_args = super::rebase::Args {
            upstream: Some(ref_name.to_owned()),
            branch: None,
            onto: None,
            interactive: false,
            r#continue: false,
            abort: false,
            skip: false,
            exec: None,
            merge: false,
            apply: false,
            rebase_merges: false,
            no_ff: false,
            keep_base: false,
            fork_point: false,
            no_fork_point: false,
            reapply_cherry_picks: false,
            no_reapply_cherry_picks: false,
            verbose: false,
            update_refs: false,
            stat: false,
            no_stat: false,
            context_lines: None,
            whitespace: None,
        };
        super::rebase::run(rebase_args)
    } else {
        // Determine ff flags from pull.ff config, CLI overrides
        let (mut ff, mut no_ff, mut ff_only) = (args.ff, args.no_ff, args.ff_only);
        if !ff && !no_ff && !ff_only {
            if let Some(val) = config.get("pull.ff") {
                match val.to_lowercase().as_str() {
                    "true" | "yes" => ff = true,
                    "false" | "no" => no_ff = true,
                    "only" => ff_only = true,
                    _ => {}
                }
            }
        }

        let merge_args = super::merge::Args {
            commits: vec![oid_hex],
            message: None,
            ff_only,
            no_ff,
            no_commit: false,
            squash: false,
            abort: false,
            continue_merge: false,
            strategy: args.strategy.clone().into_iter().collect::<Vec<_>>(),
            strategy_option: args.strategy_option.clone(),
            quiet: args.quiet,
            progress: false,
            no_progress: false,
            no_edit: true,
            edit: false,
            signoff: false,
            no_signoff: false,
            stat: false,
            no_stat: false,
            log: args.log.as_ref().map(|v| {
                let n = v.parse::<usize>().unwrap_or(0);
                if n == 0 {
                    20
                } else {
                    n
                }
            }),
            no_log: args.no_log,
            compact_summary: false,
            summary: false,
            ff,
            commit: false,
            no_squash: false,
            quit: false,
            autostash: false,
            allow_unrelated_histories: false,
            cleanup: None,
            file: None,
        };
        super::merge::run(merge_args)
    }
}
