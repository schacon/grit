//! `grit cherry` — find commits not yet applied upstream.
//!
//! Compares the patch content (not the OID) of commits on `<head>` against
//! commits reachable from `<upstream>`.  For each commit in `<head>` that is
//! not reachable from `<upstream>`, outputs:
//!
//! - `+ <oid>` — the commit's patch is *not* present in upstream.
//! - `- <oid>` — the commit's patch *is* already present in upstream.
//!
//! With `-v` the commit subject is appended after the OID.
//!
//! Commits are listed in chronological order (oldest first), matching
//! `git cherry` output.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::patch_ids::compute_patch_id;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, OrderingMode, RevListOptions};
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;
use std::collections::HashSet;
use std::io::{self, Write};

/// Arguments for `grit cherry`.
#[derive(Debug, ClapArgs)]
#[command(about = "Find commits not yet applied upstream")]
pub struct Args {
    /// Show the commit subject alongside each OID.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Upstream branch to compare against.
    #[arg(value_name = "UPSTREAM")]
    pub upstream: Option<String>,

    /// Branch to check (defaults to HEAD).
    #[arg(value_name = "HEAD")]
    pub head: Option<String>,

    /// Exclude commits reachable from this commit from the output.
    #[arg(value_name = "LIMIT")]
    pub limit: Option<String>,
}

/// Run the `cherry` command.
///
/// # Errors
///
/// Returns an error when the repository cannot be found, revision specs cannot
/// be resolved, or object reads fail.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let head_spec = args.head.as_deref().unwrap_or("HEAD").to_owned();
    let upstream_spec = match args.upstream {
        Some(ref up) => up.clone(),
        None => find_tracking_upstream(&repo).context(
            "Could not find a tracked remote branch, please specify <upstream> manually",
        )?,
    };

    let head_oid = resolve_revision(&repo, &head_spec)
        .with_context(|| format!("unknown commit '{head_spec}'"))?;
    let upstream_oid = resolve_revision(&repo, &upstream_spec)
        .with_context(|| format!("unknown commit '{upstream_spec}'"))?;

    // If head and upstream are the same commit, output nothing.
    if head_oid == upstream_oid {
        return Ok(());
    }

    let walk_opts = RevListOptions {
        ordering: OrderingMode::Default,
        ..Default::default()
    };

    // Collect patch-IDs from upstream-unique commits (reachable from upstream
    // but not from head).
    let upstream_unique = rev_list(
        &repo,
        std::slice::from_ref(&upstream_spec),
        std::slice::from_ref(&head_spec),
        &walk_opts,
    )?;

    let mut upstream_patch_ids: HashSet<ObjectId> = HashSet::new();
    for commit_oid in &upstream_unique.commits {
        if let Some(pid) = compute_patch_id(&repo.odb, commit_oid)
            .with_context(|| format!("computing patch-id for {commit_oid}"))?
        {
            upstream_patch_ids.insert(pid);
        }
    }

    // Collect head-unique commits (reachable from head but not upstream, and
    // not from limit if given).
    let mut negative_specs = vec![upstream_spec];
    if let Some(ref limit) = args.limit {
        negative_specs.push(limit.clone());
    }
    let head_unique = rev_list(&repo, &[head_spec], &negative_specs, &walk_opts)?;

    // rev_list returns newest-first; cherry outputs oldest-first.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for commit_oid in head_unique.commits.iter().rev() {
        let patch_id = compute_patch_id(&repo.odb, commit_oid)
            .with_context(|| format!("computing patch-id for {commit_oid}"))?;

        let sign = match patch_id {
            Some(ref pid) if upstream_patch_ids.contains(pid) => '-',
            _ => '+',
        };

        let oid_hex = commit_oid.to_hex();

        if args.verbose {
            let obj = repo
                .odb
                .read(commit_oid)
                .with_context(|| format!("reading commit {commit_oid}"))?;
            let commit_data =
                parse_commit(&obj.data).with_context(|| format!("parsing commit {commit_oid}"))?;
            let subject = commit_data.message.lines().next().unwrap_or("").trim_end();
            writeln!(out, "{sign} {oid_hex} {subject}")?;
        } else {
            writeln!(out, "{sign} {oid_hex}")?;
        }
    }

    Ok(())
}

/// Attempt to find the configured upstream tracking branch for the current
/// branch.
///
/// Reads `branch.<name>.remote` and `branch.<name>.merge` from the local
/// config and derives the remote-tracking ref (e.g. `origin/main`).
///
/// # Errors
///
/// Returns an error when HEAD is detached or no upstream is configured.
fn find_tracking_upstream(repo: &Repository) -> Result<String> {
    let head = resolve_head(&repo.git_dir).context("resolving HEAD")?;
    let branch_name = head
        .branch_name()
        .context("HEAD is detached; please specify <upstream> explicitly")?
        .to_owned();

    let config_path = repo.git_dir.join("config");
    let config_file = ConfigFile::from_path(&config_path, ConfigScope::Local)
        .context("reading repository config")?;
    let mut config_set = ConfigSet::new();
    if let Some(cf) = config_file {
        config_set.merge(&cf);
    }

    let remote_key = format!("branch.{branch_name}.remote");
    let merge_key = format!("branch.{branch_name}.merge");

    let remote = config_set
        .get(&remote_key)
        .with_context(|| format!("no upstream remote configured for branch '{branch_name}'"))?;

    let merge_ref = config_set
        .get(&merge_key)
        .with_context(|| format!("no upstream merge ref configured for branch '{branch_name}'"))?;

    // Derive the short branch name from the merge refspec
    // (e.g. "refs/heads/main" -> "main").
    let remote_branch = merge_ref.strip_prefix("refs/heads/").unwrap_or(&merge_ref);

    Ok(format!("{remote}/{remote_branch}"))
}
