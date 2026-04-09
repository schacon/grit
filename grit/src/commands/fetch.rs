//! `grit fetch` — download objects and refs from a local repository.
//!
//! Only the **local (file://)** transport is supported.  Reads the remote
//! URL from `remote.<name>.url` in the local config, opens the remote
//! repository, copies missing objects (loose + packs), and updates
//! remote-tracking refs under `refs/remotes/<remote>/`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::merge_base;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit fetch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download objects and refs from another repository")]
pub struct Args {
    /// Remote name or path to fetch from (defaults to "origin").
    #[arg(value_name = "REMOTE")]
    pub remote: Option<String>,

    /// Refspec(s) to fetch (e.g. "main", "main:refs/heads/from-one").
    ///
    /// Negative refspecs start with `^` and must not be parsed as flags.
    #[arg(
        value_name = "REFSPEC",
        allow_hyphen_values = true,
        allow_negative_numbers = true
    )]
    pub refspecs: Vec<String>,

    /// Fetch all configured remotes.
    #[arg(long)]
    pub all: bool,

    /// Fetch tags from the remote.
    #[arg(long)]
    pub tags: bool,

    /// Do not fetch tags.
    #[arg(long = "no-tags")]
    pub no_tags: bool,

    /// Remove remote-tracking refs that no longer exist on the remote.
    #[arg(long)]
    pub prune: bool,

    /// Remove local tags that no longer exist on the remote (implies --prune).
    #[arg(long)]
    pub prune_tags: bool,

    /// Deepen a shallow clone by N commits.
    #[arg(long, value_name = "N")]
    pub deepen: Option<usize>,

    /// Limit fetching to the specified number of commits from the tip.
    #[arg(long, value_name = "N")]
    pub depth: Option<usize>,

    /// Partial clone filter spec (accepted for compatibility).
    #[arg(long = "filter", value_name = "FILTER-SPEC")]
    pub filter: Option<String>,

    /// Deepen history of a shallow clone back to a date.
    #[arg(long, value_name = "DATE")]
    pub shallow_since: Option<String>,

    /// Deepen history of a shallow clone excluding a revision.
    #[arg(long, value_name = "REV")]
    pub shallow_exclude: Option<String>,

    /// Re-fetch all objects even if they already exist locally.
    #[arg(long)]
    pub refetch: bool,

    /// Write machine-readable fetch output to the given file.
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Be quiet — suppress informational output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Show detailed progress (Git: same as non-quiet for local transport).
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Number of parallel children for fetching (accepted but ignored).
    #[arg(short = 'j', long = "jobs", value_name = "N")]
    pub jobs: Option<usize>,

    /// Machine-readable porcelain output.
    #[arg(long)]
    pub porcelain: bool,

    /// Do not show forced updates.
    #[arg(long = "no-show-forced-updates")]
    pub no_show_forced_updates: bool,

    /// Show forced updates (default, overrides --no-show-forced-updates).
    #[arg(long = "show-forced-updates")]
    pub show_forced_updates: bool,

    /// Only negotiate, do not fetch objects.
    #[arg(long)]
    pub negotiate_only: bool,

    /// Allow updating the current branch head (normally refused).
    #[arg(long)]
    pub update_head_ok: bool,
    /// Rewrite positive refspec destinations under `refs/prefetch/` (Git maintenance prefetch).
    #[arg(long)]
    pub prefetch: bool,
    /// Update remote-tracking refs after fetch.
    #[arg(long = "update-refs")]
    pub update_refs: bool,

    /// Command to run on the remote side for pack transfer (protocol v0).
    #[arg(long = "upload-pack", value_name = "PATH")]
    pub upload_pack: Option<String>,

    /// Recurse into submodules and fetch each default remote.
    #[arg(long = "recurse-submodules", num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub recurse_submodules: Option<String>,

    /// Disable submodule recursion (overrides config).
    #[arg(long = "no-recurse-submodules")]
    pub no_recurse_submodules: bool,
}

pub fn run(args: Args) -> Result<()> {
    if args.negotiate_only {
        // Negotiate-only mode: just exit successfully without fetching.
        return Ok(());
    }

    let git_dir = resolve_git_dir()?;
    let config = ConfigSet::load(Some(&git_dir), true)?;

    // Validate fetch.output config if set
    if let Some(val) = config.get("fetch.output") {
        match val.as_str() {
            "full" | "compact" => {}
            _ => bail!("invalid value for 'fetch.output': '{}'", val),
        }
    }

    let result = if args.all {
        let remotes = collect_remote_names(&config);
        if remotes.is_empty() {
            bail!("no remotes configured");
        }
        for name in &remotes {
            fetch_remote(&git_dir, &config, name, None, &args)?;
        }
        Ok(())
    } else {
        let remote_name = args.remote.as_deref().unwrap_or("origin");
        // Remote config takes precedence over path-like names, even if the
        // remote name contains '/' or matches an existing directory.
        let url_key = format!("remote.{remote_name}.url");
        if config.get(&url_key).is_some() {
            fetch_remote(&git_dir, &config, remote_name, None, &args)
        } else {
            let group_key = format!("remotes.{remote_name}");
            let group_lines = config.get_all(&group_key);
            if !group_lines.is_empty() {
                let mut seen = HashSet::<String>::new();
                let mut members = Vec::new();
                for line in &group_lines {
                    for m in line.split_whitespace() {
                        if seen.insert(m.to_string()) {
                            members.push(m.to_string());
                        }
                    }
                }
                for m in members {
                    fetch_remote(&git_dir, &config, &m, None, &args)?;
                }
                Ok(())
            } else if remote_name.starts_with('.')
                || remote_name.contains('/')
                || std::path::Path::new(remote_name).is_dir()
            {
                // Treat as a local directory path.
                fetch_remote(&git_dir, &config, remote_name, Some(remote_name), &args)
            } else {
                fetch_remote(&git_dir, &config, remote_name, None, &args)
            }
        }
    };

    if result.is_ok() && should_recurse_fetch_submodules(&config, &args) {
        super::submodule::recursive_fetch_submodules(true)?;
    }
    result
}

fn should_recurse_fetch_submodules(config: &ConfigSet, args: &Args) -> bool {
    if args.no_recurse_submodules {
        return false;
    }
    if args.recurse_submodules.as_deref() == Some("no")
        || args.recurse_submodules.as_deref() == Some("false")
    {
        return false;
    }
    if args.recurse_submodules.is_some() {
        return true;
    }
    config
        .get("fetch.recursesubmodules")
        .or_else(|| config.get("fetch.recurseSubmodules"))
        .map(|v| {
            let l = v.to_ascii_lowercase();
            l == "true" || l == "yes" || l == "on" || l == "1"
        })
        .unwrap_or(false)
}

/// Fetch from a single remote.
///
/// If `url_override` is Some, use it directly as the remote URL instead of
/// looking it up in config.  This supports path-based remotes like `../one`.
fn fetch_remote(
    git_dir: &Path,
    config: &ConfigSet,
    remote_name: &str,
    url_override: Option<&str>,
    args: &Args,
) -> Result<()> {
    // Determine remote URL: use override (path-based) or config lookup
    let url = if let Some(u) = url_override {
        u.to_owned()
    } else {
        let url_key = format!("remote.{remote_name}.url");
        config
            .get(&url_key)
            .with_context(|| format!("remote '{remote_name}' not found; no such remote"))?
    };

    let mut remote_path = if crate::ssh_transport::is_configured_ssh_url(&url) {
        crate::protocol::check_protocol_allowed("ssh", Some(git_dir))?;
        let spec = crate::ssh_transport::parse_ssh_url(&url)?;
        let Some(gd) = crate::ssh_transport::try_local_git_dir(&spec) else {
            bail!(
                "ssh: could not resolve remote URL '{}' to a local repository",
                url
            );
        };
        gd
    } else {
        crate::protocol::check_protocol_allowed("file", Some(git_dir))?;
        // Strip file:// prefix if present.
        // For configured remotes, resolve relative paths from the repository root
        // (not the process CWD), matching Git's behavior for remote.<name>.url.
        if let Some(stripped) = url.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(&url)
        }
    };
    // Resolve relative paths from the repository root (not process CWD), for both
    // configured `remote.<name>.url` and path-based remotes (`git fetch ./server`).
    if remote_path.is_relative() {
        let base = configured_remote_base(git_dir);
        remote_path = base.join(&remote_path);
        if url_override.is_none() && !remote_path.exists() {
            let mut trimmed = url.as_str();
            let mut stripped_any_parent = false;
            while let Some(rest) = trimmed.strip_prefix("../") {
                stripped_any_parent = true;
                trimmed = rest;
            }
            if stripped_any_parent {
                let fallback = base.join(trimmed);
                if fallback.exists() {
                    remote_path = fallback;
                }
            }
        }
    }

    // Open the remote repository
    let remote_repo = open_repo(&remote_path).with_context(|| {
        format!(
            "could not open remote repository at '{}'",
            remote_path.display()
        )
    })?;

    let fetch_key = format!("remote.{remote_name}.fetch");
    let mut configured_refspecs = collect_refspecs(config, &fetch_key);
    let had_configured_fetch = !configured_refspecs.is_empty();
    let user_passed_cli_refspecs = !args.refspecs.is_empty();

    let mut effective_cli_refspecs = if user_passed_cli_refspecs {
        args.refspecs.clone()
    } else {
        Vec::new()
    };

    if args.prefetch {
        if user_passed_cli_refspecs {
            let mut specs = parse_cli_fetch_refspecs(&effective_cli_refspecs);
            apply_prefetch_to_refspecs(&mut specs);
            effective_cli_refspecs = specs.iter().map(fetch_refspec_to_cli_string).collect();
        } else {
            apply_prefetch_to_refspecs(&mut configured_refspecs);
        }
    }

    let cli_refspecs: &[String] = if user_passed_cli_refspecs {
        &effective_cli_refspecs
    } else {
        &args.refspecs
    };

    let refspecs = if user_passed_cli_refspecs {
        Vec::new()
    } else {
        configured_refspecs.clone()
    };

    let prefetch_left_no_positive = args.prefetch
        && effective_cli_refspecs.is_empty()
        && (user_passed_cli_refspecs || had_configured_fetch);
    let use_default_remote_tracking = refspecs.is_empty() && !prefetch_left_no_positive;

    // Enumerate remote refs (or receive them from upload-pack transport)
    let use_skipping = config
        .get("fetch.negotiationalgorithm")
        .map(|v| v.eq_ignore_ascii_case("skipping"))
        .unwrap_or(false);

    let upload_pack_cmd = args.upload_pack.clone().or_else(|| {
        let key = format!("remote.{remote_name}.uploadpack");
        config.get(&key)
    });

    // Local fetch with skipping negotiator uses upload-pack for configured/default fetches.
    // Explicit command-line refspecs (including negative `^` patterns) are handled only on the
    // direct local path — upload-pack negotiation does not build FETCH_HEAD for those (t5582).
    let use_upload_pack_negotiation = use_skipping
        && !crate::ssh_transport::is_configured_ssh_url(&url)
        && !user_passed_cli_refspecs;

    let upload_pack_refspecs: &[String] = if prefetch_left_no_positive {
        &[]
    } else {
        cli_refspecs
    };

    let (remote_heads, remote_tags) = if use_upload_pack_negotiation {
        crate::protocol::check_protocol_allowed("file", Some(git_dir))?;
        crate::fetch_transport::fetch_via_upload_pack_skipping(
            git_dir,
            &remote_path,
            upload_pack_cmd.as_deref(),
            upload_pack_refspecs,
        )?
    } else {
        let heads = refs::list_refs(&remote_repo.git_dir, "refs/heads/")?;
        let tags = refs::list_refs(&remote_repo.git_dir, "refs/tags/")?;
        // Copy objects from remote → local. Match Git: only copy the closure of refs this
        // fetch would update (CLI refspecs or configured/default refspecs), not every ref
        // on the remote (which would mirror unrelated branches into the destination ODB).
        let object_copy_roots = if prefetch_left_no_positive {
            fetch_object_copy_roots(&remote_repo.git_dir, &[], &[], &heads, &tags)?
        } else {
            fetch_object_copy_roots(&remote_repo.git_dir, cli_refspecs, &refspecs, &heads, &tags)?
        };
        if args.refetch {
            copy_objects(&remote_repo.git_dir, git_dir, true)
                .context("copying objects from remote")?;
        } else {
            copy_reachable_objects(&remote_repo.git_dir, git_dir, &object_copy_roots)
                .context("copying reachable objects from remote")?;
        }
        check_connectivity(git_dir, &object_copy_roots)?;
        (heads, tags)
    };

    let tip_oids: Vec<ObjectId> = remote_heads
        .iter()
        .chain(remote_tags.iter())
        .map(|(_, oid)| *oid)
        .collect();
    crate::trace_packet::trace_fetch_tip_availability(&git_dir.join("objects"), &tip_oids);

    // Handle --depth / --deepen: write shallow graft info
    let effective_depth = args.depth.or(args.deepen);
    if let Some(depth) = effective_depth {
        write_shallow_info(git_dir, &remote_heads, &remote_repo, depth)?;
    }

    // Prune namespace: URL/path remotes with explicit refspecs update refs outside
    // refs/remotes/<name>/; prune must cover those destinations (Git behavior).
    let is_url_remote = url_override.is_some();
    let prune_namespace =
        (args.prune || args.prune_tags) && is_url_remote && user_passed_cli_refspecs;
    let dst_prefix = if prune_namespace {
        longest_common_ref_prefix_from_cli_positive(cli_refspecs)
            .unwrap_or_else(|| "refs/".to_string())
    } else {
        format!("refs/remotes/{remote_name}/")
    };

    // Track which remote-tracking refs we updated (for prune)
    let mut updated_refs: Vec<String> = Vec::new();
    let mut has_updates = false;

    // Determine the remote's HEAD branch for FETCH_HEAD
    let remote_head_branch = determine_remote_head(&remote_repo.git_dir);

    // Collect FETCH_HEAD entries
    let mut fetch_head_entries: Vec<String> = Vec::new();

    // Upload-pack negotiation already honored CLI refspecs via `collect_wants`; the object-less
    // `refs::resolve_ref` path below would fail for tag-only names like `to_fetch`.
    if user_passed_cli_refspecs && !prefetch_left_no_positive && !use_upload_pack_negotiation {
        // Collect negative refspecs first (^pattern)
        let negative_patterns: Vec<&str> = cli_refspecs
            .iter()
            .filter_map(|s| s.strip_prefix('^'))
            .collect();

        // Validate negative refspecs: they must be ref patterns, not OIDs
        for pat in &negative_patterns {
            let clean = pat.strip_prefix("refs/").unwrap_or(pat);
            if clean.chars().all(|c| c.is_ascii_hexdigit()) && clean.len() >= 7 {
                bail!("negative refspecs do not support object ids: ^{pat}");
            }
        }

        let is_excluded = |refname: &str| -> bool {
            negative_patterns
                .iter()
                .any(|pat| ref_excluded_by_negative_pattern(pat, refname))
        };

        // Pre-check: detect conflicting CLI refspec mappings
        {
            let mut dst_to_src: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let remote_all_refs = refs::list_refs(&remote_repo.git_dir, "refs/")?;
            for spec in cli_refspecs {
                if spec.starts_with('^') {
                    continue;
                }
                let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
                let (src, dst) = if let Some(idx) = spec_clean.find(':') {
                    (
                        spec_clean[..idx].to_owned(),
                        spec_clean[idx + 1..].to_owned(),
                    )
                } else {
                    continue;
                };
                if dst.is_empty() {
                    continue;
                }
                if src.contains('*') {
                    for (refname, _) in &remote_all_refs {
                        if is_excluded(refname) {
                            continue;
                        }
                        if let Some(matched) = match_glob_pattern(&src, refname) {
                            let local_ref = dst.replacen('*', matched, 1);
                            if let Some(prev_src) = dst_to_src.get(&local_ref) {
                                if prev_src != refname {
                                    {
                                        eprintln!(
                                            "fatal: Cannot fetch both {} and {} to {}",
                                            prev_src, refname, local_ref
                                        );
                                        std::process::exit(128);
                                    }
                                }
                            } else {
                                dst_to_src.insert(local_ref, refname.clone());
                            }
                        }
                    }
                } else {
                    let remote_ref = if src.starts_with("refs/") {
                        src.clone()
                    } else {
                        format!("refs/heads/{src}")
                    };
                    let local_ref = if dst.starts_with("refs/") {
                        dst.clone()
                    } else {
                        format!("refs/heads/{dst}")
                    };
                    if let Some(prev_src) = dst_to_src.get(&local_ref) {
                        if prev_src != &remote_ref {
                            {
                                eprintln!(
                                    "fatal: Cannot fetch both {} and {} to {}",
                                    prev_src, remote_ref, local_ref
                                );
                                std::process::exit(128);
                            }
                        }
                    } else {
                        dst_to_src.insert(local_ref, remote_ref);
                    }
                }
            }
        }

        // Process command-line refspecs directly.
        for spec in cli_refspecs {
            // Skip negative refspecs (already collected above)
            if spec.starts_with('^') {
                continue;
            }
            // Check for force prefix '+'
            let (force, spec_clean) = if spec.starts_with('+') {
                (true, &spec[1..])
            } else {
                (false, spec.as_str())
            };
            let (src, dst) = if let Some(idx) = spec_clean.find(':') {
                (
                    spec_clean[..idx].to_owned(),
                    spec_clean[idx + 1..].to_owned(),
                )
            } else {
                (spec_clean.to_owned(), String::new())
            };

            // Handle glob refspecs (e.g. refs/remotes/*:refs/remotes/*)
            if src.contains('*') {
                let remote_all_refs = refs::list_refs(&remote_repo.git_dir, "refs/")?;
                for (refname, remote_oid) in &remote_all_refs {
                    if let Some(matched) = match_glob_pattern(&src, refname) {
                        let local_ref = dst.replacen('*', matched, 1);
                        if is_excluded(refname) {
                            // Negative refspec: do not update, but remote ref still exists — keep
                            // local destination out of prune (matches Git's fetch --prune).
                            if !updated_refs.contains(&local_ref) {
                                updated_refs.push(local_ref.clone());
                            }
                            continue;
                        }
                        if !updated_refs.contains(&local_ref) {
                            updated_refs.push(local_ref.clone());
                        }
                        let old_oid = read_ref_oid(git_dir, &local_ref);
                        let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
                        fetch_head_entries.push(format!(
                            "{}\tnot-for-merge\tbranch '{branch}' of {url}",
                            remote_oid,
                        ));
                        if old_oid.as_ref() == Some(remote_oid) {
                            continue;
                        }

                        if !has_updates && !args.quiet {
                            eprintln!("From {url}");
                            has_updates = true;
                        }

                        // Refuse to update a ref that's checked out in a worktree (unless
                        // `--update-head-ok`, matching Git's fetch safety valve).
                        if local_ref.starts_with("refs/heads/") && !args.update_head_ok {
                            if let Some(wt_path) = is_branch_in_worktree(git_dir, &local_ref) {
                                bail!(
                                    "refusing to fetch into branch '{}' checked out at '{}'",
                                    local_ref,
                                    wt_path
                                );
                            }
                        }
                        refs::write_ref(git_dir, &local_ref, remote_oid)
                            .with_context(|| format!("updating ref {local_ref}"))?;

                        if !args.quiet {
                            let short = local_ref
                                .strip_prefix("refs/heads/")
                                .or_else(|| local_ref.strip_prefix("refs/tags/"))
                                .unwrap_or(&local_ref);
                            match old_oid {
                                None => eprintln!(" * [new branch]      {branch:<17} -> {short}"),
                                Some(old) => eprintln!(
                                    "   {}..{}  {branch:<17} -> {short}",
                                    &old.to_string()[..7],
                                    &remote_oid.to_string()[..7],
                                ),
                            }
                        }
                    }
                }
                // Negative refspec + --prune: remote refs excluded by `^` are not updated, but if
                // the remote deletes such a ref we must not prune the stale local copy (Git).
                if args.prune && !negative_patterns.is_empty() && dst.contains('*') {
                    if let Some((dst_pre, _)) = dst.split_once('*') {
                        if dst_pre.ends_with('/') {
                            if let Ok(locals) = refs::list_refs(git_dir, dst_pre) {
                                for (local_ref, _) in locals {
                                    let Some(matched) = match_glob_pattern(&dst, &local_ref) else {
                                        continue;
                                    };
                                    let remote_src = src.replacen('*', matched, 1);
                                    if is_excluded(&remote_src)
                                        && !updated_refs.contains(&local_ref)
                                    {
                                        updated_refs.push(local_ref);
                                    }
                                }
                            }
                        }
                    }
                }
                // Also copy symbolic refs for the matched pattern
                copy_symrefs(&remote_repo.git_dir, git_dir, &src, &dst)?;
                continue;
            }

            // Normalize source: if it doesn't start with refs/, assume refs/heads/
            let remote_ref = if src.starts_with("refs/") {
                src.clone()
            } else {
                format!("refs/heads/{src}")
            };

            // Resolve the remote ref
            let remote_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref)
                .with_context(|| format!("couldn't find remote ref '{}'", src))?;

            // Build FETCH_HEAD entry
            let branch = remote_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(&remote_ref);
            fetch_head_entries.push(format!("{}\tbranch '{}' of {url}", remote_oid, branch,));

            // If a destination is specified, write the ref there
            if !dst.is_empty() {
                let local_ref = if dst.starts_with("refs/") {
                    dst.clone()
                } else {
                    format!("refs/heads/{dst}")
                };
                if !updated_refs.contains(&local_ref) {
                    updated_refs.push(local_ref.clone());
                }

                let old_oid = read_ref_oid(git_dir, &local_ref);

                // Check fast-forward: reject non-ff updates unless forced
                if let Some(ref old) = old_oid {
                    if old != &remote_oid && !force {
                        let is_ff = merge_base::is_ancestor(&remote_repo, *old, remote_oid)
                            .unwrap_or(false);
                        if !is_ff {
                            eprintln!(" ! [rejected]        {src} -> {dst} (non-fast-forward)");
                            bail!("cannot fast-forward ref '{local_ref}'");
                        }
                    }
                }

                if old_oid.as_ref() != Some(&remote_oid) {
                    if !has_updates && !args.quiet {
                        eprintln!("From {url}");
                        has_updates = true;
                    }

                    // Check if branch is checked out in a worktree before updating.
                    if local_ref.starts_with("refs/heads/") && !args.update_head_ok {
                        if let Some(wt_path) = is_branch_in_worktree(git_dir, &local_ref) {
                            bail!(
                                "refusing to fetch into branch '{}' checked out at '{}'",
                                local_ref,
                                wt_path
                            );
                        }
                    }
                    refs::write_ref(git_dir, &local_ref, &remote_oid)
                        .with_context(|| format!("updating ref {local_ref}"))?;

                    if !args.quiet {
                        let short = local_ref
                            .strip_prefix("refs/heads/")
                            .or_else(|| local_ref.strip_prefix("refs/tags/"))
                            .unwrap_or(&local_ref);
                        match old_oid {
                            None => {
                                eprintln!(" * [new branch]      {branch:<17} -> {short}");
                            }
                            Some(old) => {
                                eprintln!(
                                    "   {}..{}  {branch:<17} -> {short}",
                                    &old.to_string()[..7],
                                    &remote_oid.to_string()[..7],
                                );
                            }
                        }
                    }
                }
            }
        }

        // Emit warnings when CLI refspec destinations conflict with configured tracking
        let configured_refspecs = collect_refspecs(config, &fetch_key);
        if !configured_refspecs.is_empty() {
            for spec in cli_refspecs {
                if spec.starts_with('^') {
                    continue;
                }
                let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
                let (src, dst) = if let Some(idx) = spec_clean.find(':') {
                    (
                        spec_clean[..idx].to_owned(),
                        spec_clean[idx + 1..].to_owned(),
                    )
                } else {
                    continue;
                };
                if dst.is_empty() || src.contains('*') {
                    continue;
                }
                let remote_ref = if src.starts_with("refs/") {
                    src.clone()
                } else {
                    format!("refs/heads/{src}")
                };
                let local_ref = if dst.starts_with("refs/") {
                    dst.clone()
                } else {
                    format!("refs/heads/{dst}")
                };
                // Check what the configured refspec would map this destination to
                if let Some(usual_src) = reverse_map_refspec(&local_ref, &configured_refspecs) {
                    if usual_src != remote_ref {
                        eprintln!(
                            "warning: {} usually tracks {}, not {}",
                            local_ref, usual_src, remote_ref
                        );
                    }
                }
            }
        }
    } else {
        // Pre-check: detect conflicting refspec mappings (multiple src → same dst)
        if !refspecs.is_empty() {
            let mut dst_to_src: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (refname, _) in &remote_heads {
                if let Some(local_ref) = map_ref_through_refspecs(refname, &refspecs) {
                    if let Some(prev_src) = dst_to_src.get(&local_ref) {
                        if prev_src != refname {
                            {
                                eprintln!(
                                    "fatal: Cannot fetch both {} and {} to {}",
                                    prev_src, refname, local_ref
                                );
                                std::process::exit(128);
                            }
                        }
                    } else {
                        dst_to_src.insert(local_ref, refname.clone());
                    }
                }
            }
        }

        // Standard path: update remote-tracking refs from remote heads
        for (refname, remote_oid) in &remote_heads {
            // refname is like "refs/heads/main"
            let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);

            // Map through refspecs if configured, otherwise use default mapping
            let local_ref = if use_default_remote_tracking {
                format!("{dst_prefix}{branch}")
            } else {
                match map_ref_through_refspecs(refname, &refspecs) {
                    Some(mapped) => mapped,
                    None => continue, // ref not matched by any refspec, skip
                }
            };
            updated_refs.push(local_ref.clone());

            // Build FETCH_HEAD entry
            let is_default = remote_head_branch.as_deref() == Some(branch);
            let not_for_merge = if is_default { "" } else { "\tnot-for-merge" };
            fetch_head_entries.push(format!(
                "{}{not_for_merge}\tbranch '{branch}' of {url}",
                remote_oid,
            ));

            let old_oid = read_ref_oid(git_dir, &local_ref);

            if old_oid.as_ref() == Some(remote_oid) {
                // Already up to date
                continue;
            }

            if !has_updates && !args.quiet {
                eprintln!("From {url}");
                has_updates = true;
            }

            refs::write_ref(git_dir, &local_ref, remote_oid)
                .with_context(|| format!("updating ref {local_ref}"))?;
            let _ = append_fetch_reflog(
                git_dir,
                &local_ref,
                old_oid.as_ref(),
                remote_oid,
                &url,
                branch,
            );

            if args.porcelain {
                let zero = "0".repeat(40);
                let old_hex = old_oid
                    .as_ref()
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| zero.clone());
                let flag = if old_oid.is_none() { "*" } else { " " };
                println!("{flag} {old_hex} {remote_oid} {local_ref}");
            } else if !args.quiet {
                print_update(&old_oid, remote_oid, branch, remote_name);
            }
        }
    }

    // Determine whether to fetch tags:
    // CLI --tags/--no-tags override, then remote.<name>.tagopt, then default (fetch tags)
    let should_fetch_tags = if args.tags {
        true
    } else if args.no_tags {
        false
    } else {
        // Check remote.<name>.tagopt config
        let tagopt_key = format!("remote.{remote_name}.tagopt");
        match config.get(&tagopt_key).as_deref() {
            Some("--no-tags") => false,
            Some("--tags") => true,
            _ => true, // default: fetch tags
        }
    };
    if should_fetch_tags {
        for (refname, remote_oid) in &remote_tags {
            let old_oid = read_ref_oid(git_dir, refname);
            if old_oid.as_ref() == Some(remote_oid) {
                continue;
            }

            if !has_updates && !args.quiet {
                eprintln!("From {url}");
                has_updates = true;
            }

            refs::write_ref(git_dir, refname, remote_oid)
                .with_context(|| format!("updating tag {refname}"))?;
            let _ = append_fetch_reflog(git_dir, refname, old_oid.as_ref(), remote_oid, &url, "");

            if !args.quiet {
                let tag_name = refname.strip_prefix("refs/tags/").unwrap_or(refname);
                if let Some(old) = old_oid {
                    eprintln!(
                        "   {}..{}  {tag_name:<17} -> {tag_name}",
                        &old.to_string()[..7],
                        &remote_oid.to_string()[..7],
                    );
                } else {
                    eprintln!(" * [new tag]         {tag_name:<17} -> {tag_name}");
                }
            }
        }
    }

    // Prune tags that no longer exist on the remote
    if args.prune_tags {
        let local_tags = refs::list_refs(git_dir, "refs/tags/")?;
        for (local_tag_ref, _oid) in &local_tags {
            let exists_on_remote = remote_tags.iter().any(|(r, _)| r == local_tag_ref);
            if !exists_on_remote {
                if !has_updates && !args.quiet {
                    eprintln!("From {url}");
                    has_updates = true;
                }
                refs::delete_ref(git_dir, local_tag_ref)
                    .with_context(|| format!("pruning tag {local_tag_ref}"))?;
                if !args.quiet {
                    let tag_name = local_tag_ref
                        .strip_prefix("refs/tags/")
                        .unwrap_or(local_tag_ref);
                    eprintln!(" - [deleted]         (none)     -> {tag_name}");
                }
            }
        }
    }

    // Prune stale remote-tracking refs
    if args.prune || args.prune_tags {
        if !has_updates && !args.quiet {
            // Check if prune will actually delete anything
            let existing = refs::list_refs(git_dir, &dst_prefix)?;
            let will_prune = existing.iter().any(|(r, _)| !updated_refs.contains(r));
            if will_prune {
                eprintln!("From {url}");
            }
        }
        prune_stale_refs(git_dir, &dst_prefix, &updated_refs, remote_name, args.quiet)?;
    }

    // Update refs/remotes/<remote>/HEAD to mirror the remote's default branch.
    // We store it as a direct ref to keep completion and ref lookups aligned.
    if let Some(default_branch) = remote_head_branch.as_deref() {
        let head_source = format!("refs/heads/{default_branch}");
        let mapped_default = if refspecs.is_empty() {
            Some(format!("{dst_prefix}{default_branch}"))
        } else {
            map_ref_through_refspecs(&head_source, &refspecs)
        };
        if let Some(mapped_default_ref) = mapped_default {
            if let Ok(default_oid) = refs::resolve_ref(git_dir, &mapped_default_ref) {
                let remote_head_ref = format!("refs/remotes/{remote_name}/HEAD");
                updated_refs.push(remote_head_ref.clone());
                if read_ref_oid(git_dir, &remote_head_ref).as_ref() != Some(&default_oid) {
                    refs::write_ref(git_dir, &remote_head_ref, &default_oid)
                        .with_context(|| format!("updating ref {remote_head_ref}"))?;
                }
            }
        }
    }

    // Write FETCH_HEAD (default branch first, then not-for-merge entries)
    if !fetch_head_entries.is_empty() {
        fn fetch_head_branch_name(line: &str) -> &str {
            if let Some(i) = line.find("branch '") {
                let rest = &line[i + "branch '".len()..];
                if let Some(end) = rest.find('\'') {
                    return &rest[..end];
                }
            }
            line
        }
        // Sort: merge candidates first, then by ref name (Git orders FETCH_HEAD stably).
        fetch_head_entries.sort_by(|a, b| {
            let a_nfm = a.contains("not-for-merge");
            let b_nfm = b.contains("not-for-merge");
            a_nfm
                .cmp(&b_nfm)
                .then_with(|| fetch_head_branch_name(a).cmp(fetch_head_branch_name(b)))
        });
        let fetch_head_path = git_dir.join("FETCH_HEAD");
        let content = fetch_head_entries.join("\n") + "\n";
        fs::write(&fetch_head_path, content).context("writing FETCH_HEAD")?;
    }

    if args.filter.as_deref() == Some("blob:none") {
        apply_blob_none_filter(git_dir, &remote_heads).context("applying blob:none filter")?;
    }

    // Write machine-readable output if --output is given
    if let Some(ref output_path) = args.output {
        let mut lines = Vec::new();
        for (refname, remote_oid) in &remote_heads {
            let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            let local_ref = format!("{dst_prefix}{branch}");
            let old_oid = read_ref_oid(git_dir, &local_ref);
            let old_hex = old_oid
                .map(|o| o.to_string())
                .unwrap_or_else(|| "0".repeat(40));
            let flag = if old_oid.is_none() {
                "*"
            } else if old_oid.as_ref() == Some(remote_oid) {
                "="
            } else {
                " "
            };
            lines.push(format!("{flag} {} {} {local_ref}", old_hex, remote_oid,));
        }
        let content = lines.join("\n") + "\n";
        fs::write(output_path, content).context("writing --output file")?;
    }

    Ok(())
}

fn apply_blob_none_filter(git_dir: &Path, remote_heads: &[(String, ObjectId)]) -> Result<()> {
    let patterns = load_sparse_patterns(git_dir)?;
    let odb = grit_lib::odb::Odb::new(&git_dir.join("objects"));
    let mut seen_trees = HashSet::new();
    let mut all_blobs = HashSet::new();
    let mut keep_blobs = HashSet::new();

    for (_, commit_oid) in remote_heads {
        let commit_obj = match odb.read(commit_oid) {
            Ok(obj) => obj,
            Err(_) => continue,
        };
        if commit_obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = match parse_commit(&commit_obj.data) {
            Ok(c) => c,
            Err(_) => continue,
        };
        collect_blob_sets_for_tree(
            &odb,
            commit.tree,
            "",
            &patterns,
            &mut seen_trees,
            &mut all_blobs,
            &mut keep_blobs,
        )?;
    }

    for oid in all_blobs.drain() {
        if keep_blobs.contains(&oid) {
            continue;
        }
        let hex = oid.to_hex();
        if hex.len() < 3 {
            continue;
        }
        let loose_path = git_dir
            .join("objects")
            .join(&hex[..2])
            .join(&hex[2..hex.len()]);
        if loose_path.exists() {
            let _ = fs::remove_file(loose_path);
        }
    }

    Ok(())
}

fn collect_blob_sets_for_tree(
    odb: &grit_lib::odb::Odb,
    tree_oid: ObjectId,
    prefix: &str,
    patterns: &[String],
    seen_trees: &mut HashSet<ObjectId>,
    all_blobs: &mut HashSet<ObjectId>,
    keep_blobs: &mut HashSet<ObjectId>,
) -> Result<()> {
    if !seen_trees.insert(tree_oid) {
        return Ok(());
    }

    let tree_obj = match odb.read(&tree_oid) {
        Ok(obj) => obj,
        Err(_) => return Ok(()),
    };
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(());
    }

    let entries = parse_tree(&tree_obj.data)?;
    for entry in entries {
        if entry.mode == 0o160000 {
            continue;
        }
        let name = String::from_utf8_lossy(&entry.name);
        let rel_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if (entry.mode & 0o170000) == 0o040000 {
            collect_blob_sets_for_tree(
                odb, entry.oid, &rel_path, patterns, seen_trees, all_blobs, keep_blobs,
            )?;
            continue;
        }
        let blob_obj = match odb.read(&entry.oid) {
            Ok(obj) => obj,
            Err(_) => continue,
        };
        if blob_obj.kind != ObjectKind::Blob {
            continue;
        }
        all_blobs.insert(entry.oid);
        if sparse_path_is_included(patterns, &rel_path) {
            keep_blobs.insert(entry.oid);
        }
    }
    Ok(())
}

fn load_sparse_patterns(git_dir: &Path) -> Result<Vec<String>> {
    let sparse_path = git_dir.join("info").join("sparse-checkout");
    let content = match fs::read_to_string(&sparse_path) {
        Ok(content) => content,
        Err(_) => return Ok(Vec::new()),
    };
    let patterns = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_owned())
        .collect::<Vec<_>>();
    Ok(patterns)
}

fn sparse_path_is_included(patterns: &[String], path: &str) -> bool {
    if patterns.is_empty() {
        return false;
    }

    let mut include = false;
    for raw in patterns {
        let pattern = raw.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }
        let (exclude, pat) = if let Some(rest) = pattern.strip_prefix('!') {
            (true, rest)
        } else {
            (false, pattern)
        };
        if sparse_pattern_matches(pat, path) {
            include = !exclude;
        }
    }
    include
}

fn sparse_pattern_matches(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim();
    if pat.is_empty() {
        return false;
    }

    let anchored = pat.starts_with('/');
    let pat = pat.trim_start_matches('/');

    if let Some(dir) = pat.strip_suffix('/') {
        if anchored {
            return path == dir || path.starts_with(&format!("{dir}/"));
        }
        return path == dir
            || path.starts_with(&format!("{dir}/"))
            || path.split('/').any(|component| component == dir);
    }

    if anchored {
        return sparse_glob_match(pat.as_bytes(), path.as_bytes());
    }
    sparse_glob_match(pat.as_bytes(), path.as_bytes())
        || path
            .rsplit('/')
            .next()
            .is_some_and(|base| sparse_glob_match(pat.as_bytes(), base.as_bytes()))
}

fn sparse_glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut pi, mut ti) = (0, 0);
    let (mut star_p, mut star_t) = (usize::MAX, 0);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Print a ref update line (to stderr, matching git).
fn print_update(old_oid: &Option<ObjectId>, new_oid: &ObjectId, branch: &str, remote_name: &str) {
    let tracking = format!("{remote_name}/{branch}");
    match old_oid {
        None => {
            eprintln!(" * [new branch]      {branch:<17} -> {tracking}");
        }
        Some(old) => {
            eprintln!(
                "   {}..{}  {branch:<17} -> {tracking}",
                &old.to_string()[..7],
                &new_oid.to_string()[..7],
            );
        }
    }
}

/// Determine the remote's HEAD branch name.
fn determine_remote_head(remote_git_dir: &Path) -> Option<String> {
    let head_path = remote_git_dir.join("HEAD");
    if let Ok(content) = fs::read_to_string(&head_path) {
        let content = content.trim();
        if let Some(refname) = content.strip_prefix("ref: refs/heads/") {
            return Some(refname.to_string());
        }
    }
    None
}

/// Read a ref to get its OID, returning None if it doesn't exist.
fn read_ref_oid(git_dir: &Path, refname: &str) -> Option<ObjectId> {
    refs::resolve_ref(git_dir, refname).ok()
}

fn zero_oid() -> ObjectId {
    ObjectId::from_hex("0000000000000000000000000000000000000000").expect("null oid")
}

fn fetch_reflog_identity(git_dir: &Path) -> String {
    let config = ConfigSet::load(Some(git_dir), true).ok();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_NAME").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_EMAIL").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}

/// Append a reflog line for a ref updated by fetch (remote-tracking branches and tags).
fn append_fetch_reflog(
    git_dir: &Path,
    refname: &str,
    old_oid: Option<&ObjectId>,
    new_oid: &ObjectId,
    remote_url: &str,
    branch: &str,
) -> anyhow::Result<()> {
    let old = old_oid.cloned().unwrap_or_else(zero_oid);
    let message = if branch.is_empty() {
        format!("fetch --append --prune {remote_url}")
    } else {
        format!("fetch --append --prune {remote_url} branch '{branch}' of {remote_url}")
    };
    let ident = fetch_reflog_identity(git_dir);
    refs::append_reflog(git_dir, refname, &old, new_oid, &ident, &message, true)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// OIDs whose object closure should be copied for this fetch (non-refetch local transport).
///
/// When the user passes explicit refspecs, only those sources are roots; otherwise configured
/// refspecs filter which remote refs participate; if there are no configured refspecs, all
/// remote heads and tags are roots (Git's default refspec set).
fn fetch_object_copy_roots(
    remote_git_dir: &Path,
    cli_refspecs: &[String],
    refspecs: &[FetchRefspec],
    heads: &[(String, ObjectId)],
    tags: &[(String, ObjectId)],
) -> Result<Vec<ObjectId>> {
    let mut roots = Vec::new();

    if !cli_refspecs.is_empty() {
        let negative_patterns: Vec<&str> = cli_refspecs
            .iter()
            .filter_map(|s| s.strip_prefix('^'))
            .collect();
        let is_excluded = |refname: &str| -> bool {
            negative_patterns
                .iter()
                .any(|pat| ref_excluded_by_negative_pattern(pat, refname))
        };

        for spec in cli_refspecs {
            if spec.starts_with('^') {
                continue;
            }
            let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
            let src = spec_clean
                .split_once(':')
                .map(|(a, _)| a)
                .unwrap_or(spec_clean);
            if src.contains('*') {
                let remote_all_refs = refs::list_refs(remote_git_dir, "refs/")?;
                for (refname, oid) in &remote_all_refs {
                    if is_excluded(refname) {
                        continue;
                    }
                    if match_glob_pattern(src, refname).is_some() {
                        roots.push(*oid);
                    }
                }
                continue;
            }
            let remote_ref = resolve_remote_ref_for_fetch_src(remote_git_dir, src)?;
            let oid = refs::resolve_ref(remote_git_dir, &remote_ref)
                .with_context(|| format!("couldn't find remote ref '{src}'"))?;
            roots.push(oid);
        }
    } else if refspecs.is_empty() {
        roots.extend(heads.iter().map(|(_, o)| *o));
        roots.extend(tags.iter().map(|(_, o)| *o));
    } else {
        for (refname, oid) in heads.iter().chain(tags.iter()) {
            let excluded = refspecs
                .iter()
                .any(|rs| rs.negative && ref_excluded_by_negative_pattern(&rs.src, refname));
            if excluded {
                continue;
            }
            if map_ref_through_refspecs(refname, refspecs).is_some() {
                roots.push(*oid);
            }
        }
    }

    roots.sort_by_key(|o| o.to_hex());
    roots.dedup();
    Ok(roots)
}

/// Resolve a short or full remote ref name for fetch (CLI refspec source side).
fn resolve_remote_ref_for_fetch_src(remote_git_dir: &Path, src: &str) -> Result<String> {
    if src.starts_with("refs/") {
        return Ok(src.to_owned());
    }
    let heads_ref = format!("refs/heads/{src}");
    if refs::resolve_ref(remote_git_dir, &heads_ref).is_ok() {
        return Ok(heads_ref);
    }
    let tags_ref = format!("refs/tags/{src}");
    if refs::resolve_ref(remote_git_dir, &tags_ref).is_ok() {
        return Ok(tags_ref);
    }
    Ok(heads_ref)
}

/// Copy all objects (loose + packs) from remote to local.
/// If `refetch` is true, re-copy objects even if they already exist locally.
/// Copy objects from a remote git dir to local git dir (public for pull).
pub fn copy_objects_for_pull(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    copy_objects(src_git_dir, dst_git_dir, false)
}

/// Copy objects reachable from `roots` from `src_git_dir` into `dst_git_dir` (loose only).
///
/// Used for local `fetch` so the destination does not receive unrelated objects from the
/// source object database (matching Git's behavior and keeping negotiation tests faithful).
fn copy_reachable_objects(
    src_git_dir: &Path,
    dst_git_dir: &Path,
    roots: &[ObjectId],
) -> Result<()> {
    let src_odb = Odb::new(&src_git_dir.join("objects"));
    let dst_odb = Odb::new(&dst_git_dir.join("objects"));
    let mut stack: Vec<ObjectId> = roots.to_vec();
    let mut seen = HashSet::new();

    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = src_odb.read(&oid).with_context(|| {
            format!("missing object {} while copying from remote", oid.to_hex())
        })?;
        if !dst_odb.exists(&oid) {
            dst_odb
                .write(obj.kind, &obj.data)
                .with_context(|| format!("write object {}", oid.to_hex()))?;
        }
        match obj.kind {
            ObjectKind::Commit => {
                let c = parse_commit(&obj.data)?;
                stack.push(c.tree);
                stack.extend_from_slice(&c.parents);
            }
            ObjectKind::Tree => {
                for e in parse_tree(&obj.data)? {
                    stack.push(e.oid);
                }
            }
            ObjectKind::Tag => {
                stack.push(parse_tag(&obj.data)?.object);
            }
            ObjectKind::Blob => {}
        }
    }
    Ok(())
}

fn copy_objects(src_git_dir: &Path, dst_git_dir: &Path, refetch: bool) -> Result<()> {
    let src_objects = src_git_dir.join("objects");
    let dst_objects = dst_git_dir.join("objects");

    // Copy loose objects (fan-out directories: 00..ff)
    if src_objects.is_dir() {
        for entry in fs::read_dir(&src_objects)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Skip info/ and pack/ — handled separately
            if name_str == "info" || name_str == "pack" {
                continue;
            }

            // Only process 2-character hex fan-out dirs
            if !entry.file_type()?.is_dir() || name_str.len() != 2 {
                continue;
            }

            let dst_dir = dst_objects.join(&*name);
            for inner in fs::read_dir(entry.path())? {
                let inner = inner?;
                if inner.file_type()?.is_file() {
                    let dst_file = dst_dir.join(inner.file_name());
                    if refetch || !dst_file.exists() {
                        fs::create_dir_all(&dst_dir)?;
                        if refetch {
                            // Force copy when refetching
                            fs::copy(inner.path(), &dst_file)?;
                        } else if fs::hard_link(inner.path(), &dst_file).is_err() {
                            fs::copy(inner.path(), &dst_file)?;
                        }
                    }
                }
            }
        }
    }

    // Copy pack files
    let src_pack = src_objects.join("pack");
    let dst_pack = dst_objects.join("pack");
    if src_pack.is_dir() {
        fs::create_dir_all(&dst_pack)?;
        for entry in fs::read_dir(&src_pack)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let dst_file = dst_pack.join(entry.file_name());
                if refetch || !dst_file.exists() {
                    if refetch {
                        fs::copy(entry.path(), &dst_file)?;
                    } else if fs::hard_link(entry.path(), &dst_file).is_err() {
                        fs::copy(entry.path(), &dst_file)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Verify that all objects reachable from the given OIDs exist in the local ODB.
/// This is used after copying objects from a remote to detect incomplete transfers.
fn check_connectivity(git_dir: &Path, tip_oids: &[ObjectId]) -> Result<()> {
    use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
    use grit_lib::odb::Odb;
    use std::collections::HashSet;

    let odb = Odb::new(&git_dir.join("objects"));
    let mut seen = HashSet::new();
    let mut stack: Vec<ObjectId> = tip_oids.to_vec();

    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = odb
            .read(&oid)
            .with_context(|| "remote did not send all necessary objects".to_string())?;
        match obj.kind {
            ObjectKind::Commit => {
                if let Ok(commit) = parse_commit(&obj.data) {
                    stack.push(commit.tree);
                    for parent in &commit.parents {
                        stack.push(*parent);
                    }
                }
            }
            ObjectKind::Tree => {
                if let Ok(entries) = parse_tree(&obj.data) {
                    for entry in entries {
                        // Skip gitlink (submodule) entries
                        if entry.mode == 0o160000 {
                            continue;
                        }
                        stack.push(entry.oid);
                    }
                }
            }
            ObjectKind::Blob | ObjectKind::Tag => {
                // Blobs and tags are leaf objects, no further traversal needed
            }
        }
    }
    Ok(())
}

/// Remove remote-tracking refs that no longer exist on the remote.
fn prune_stale_refs(
    git_dir: &Path,
    prefix: &str,
    current_refs: &[String],
    remote_name: &str,
    quiet: bool,
) -> Result<()> {
    let existing = refs::list_refs(git_dir, prefix)?;
    for (refname, _oid) in &existing {
        if !current_refs.contains(refname) {
            refs::delete_ref(git_dir, refname).with_context(|| format!("pruning {refname}"))?;
            if !quiet {
                // Show short name: "origin/branch" instead of "refs/remotes/origin/branch"
                let short = refname.strip_prefix("refs/remotes/").unwrap_or(refname);
                let branch = short
                    .strip_prefix(&format!("{remote_name}/"))
                    .unwrap_or(short);
                eprintln!(" - [deleted]         (none)     -> {remote_name}/{branch}");
            }
        }
    }
    Ok(())
}

/// Write shallow graft information when --depth / --deepen is used.
///
/// For local transport we approximate shallowness by listing the commit(s) at
/// the boundary depth and recording them in `$GIT_DIR/shallow`.
fn write_shallow_info(
    git_dir: &Path,
    remote_heads: &[(String, ObjectId)],
    remote_repo: &Repository,
    depth: usize,
) -> Result<()> {
    use grit_lib::objects::{parse_commit, ObjectKind};
    use grit_lib::odb::Odb;

    let shallow_path = git_dir.join("shallow");
    // Collect existing shallow commits
    let mut shallow_set: std::collections::HashSet<String> = if shallow_path.exists() {
        fs::read_to_string(&shallow_path)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let odb = Odb::new(&remote_repo.git_dir.join("objects"));

    // For each remote head, walk `depth` commits and mark the boundary
    for (_refname, tip_oid) in remote_heads {
        let mut oid = *tip_oid;
        for _ in 0..depth.saturating_sub(1) {
            match odb.read(&oid) {
                Ok(obj) if obj.kind == ObjectKind::Commit => match parse_commit(&obj.data) {
                    Ok(c) => {
                        if c.parents.is_empty() {
                            break;
                        }
                        oid = c.parents[0];
                    }
                    Err(_) => break,
                },
                _ => break,
            }
        }
        shallow_set.insert(oid.to_string());
    }

    let mut entries: Vec<&str> = shallow_set.iter().map(|s| s.as_str()).collect();
    entries.sort();
    let content = entries.join("\n") + "\n";
    fs::write(&shallow_path, content).context("writing shallow file")?;
    Ok(())
}

/// Returns true if `refname` is excluded by a negative refspec pattern (without the leading `^`).
///
/// Patterns that start with `refs/` are matched against `refname` as written (glob or exact).
/// Unqualified patterns are matched only against `refname` itself — Git does **not** prepend
/// `refs/heads/` to the pattern (see t5582 "does not expand prefix").
fn ref_excluded_by_negative_pattern(pattern: &str, refname: &str) -> bool {
    match_glob_pattern(pattern, refname).is_some() || pattern == refname
}

fn ref_excluded_by_fetch_refspecs(refname: &str, refspecs: &[FetchRefspec]) -> bool {
    refspecs
        .iter()
        .any(|rs| rs.negative && ref_excluded_by_negative_pattern(&rs.src, refname))
}

/// Rewrite positive fetch refspec destinations under `refs/prefetch/`, matching Git's
/// `fetch --prefetch` behavior.
fn apply_prefetch_to_refspecs(specs: &mut Vec<FetchRefspec>) {
    const PREFETCH_NS: &str = "refs/prefetch/";
    let mut i = 0usize;
    while i < specs.len() {
        if specs[i].negative {
            i += 1;
            continue;
        }
        let src = specs[i].src.as_str();
        let dst = specs[i].dst.as_str();
        let remove = dst.is_empty() || src.starts_with("refs/tags/");
        if remove {
            specs.remove(i);
            continue;
        }
        let mut new_dst = String::from(PREFETCH_NS);
        if let Some(rest) = dst.strip_prefix("refs/") {
            new_dst.push_str(rest);
        } else {
            new_dst.push_str(dst);
        }
        specs[i].dst = new_dst;
        specs[i].force = true;
        i += 1;
    }
}

/// Parse command-line fetch refspec strings into [`FetchRefspec`] entries (for `--prefetch`).
fn parse_cli_fetch_refspecs(cli: &[String]) -> Vec<FetchRefspec> {
    let mut out = Vec::new();
    for spec in cli {
        if let Some(pat) = spec.strip_prefix('^') {
            out.push(FetchRefspec {
                src: pat.to_owned(),
                dst: String::new(),
                force: false,
                negative: true,
            });
            continue;
        }
        let (force, rest) = if let Some(s) = spec.strip_prefix('+') {
            (true, s)
        } else {
            (false, spec.as_str())
        };
        if let Some(colon) = rest.find(':') {
            out.push(FetchRefspec {
                src: rest[..colon].to_owned(),
                dst: rest[colon + 1..].to_owned(),
                force,
                negative: false,
            });
        } else {
            out.push(FetchRefspec {
                src: rest.to_owned(),
                dst: rest.to_owned(),
                force,
                negative: false,
            });
        }
    }
    out
}

fn fetch_refspec_to_cli_string(rs: &FetchRefspec) -> String {
    if rs.negative {
        return format!("^{}", rs.src);
    }
    let mut s = String::new();
    if rs.force {
        s.push('+');
    }
    s.push_str(&rs.src);
    s.push(':');
    s.push_str(&rs.dst);
    s
}

/// Longest directory prefix shared by all ref names (must end on `/`), for pruning after a
/// URL-remote fetch with explicit refspecs.
fn longest_common_ref_prefix(refs: &[String]) -> Option<String> {
    if refs.is_empty() {
        return None;
    }
    let first = refs[0].as_bytes();
    let mut len = first.len();
    for r in refs.iter().skip(1) {
        let b = r.as_bytes();
        let max = len.min(b.len());
        let mut common = 0usize;
        while common < max && first[common] == b[common] {
            common += 1;
        }
        len = len.min(common);
    }
    let prefix = std::str::from_utf8(&first[..len]).ok()?;
    let cut = prefix.rfind('/')?;
    if cut == 0 {
        return None;
    }
    Some(prefix[..=cut].to_string())
}

fn normalize_fetch_dst(dst: &str) -> String {
    if dst.starts_with("refs/") {
        dst.to_owned()
    } else {
        format!("refs/heads/{dst}")
    }
}

/// Local ref destinations from positive CLI refspecs (used for `--prune` namespace).
fn longest_common_ref_prefix_from_cli_positive(cli: &[String]) -> Option<String> {
    let mut locals = Vec::new();
    for spec in cli {
        if spec.starts_with('^') {
            continue;
        }
        let clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
        let Some(colon) = clean.find(':') else {
            continue;
        };
        let (src, dst) = (&clean[..colon], &clean[colon + 1..]);
        if dst.is_empty() {
            continue;
        }
        if src.contains('*') {
            let base = dst.split_once('*').map(|(p, _)| p).unwrap_or(dst);
            locals.push(normalize_fetch_dst(base));
        } else {
            locals.push(normalize_fetch_dst(dst));
        }
    }
    longest_common_ref_prefix(&locals)
}

/// A parsed refspec (e.g. "+refs/heads/*:refs/remotes/origin/*").
#[derive(Clone)]
struct FetchRefspec {
    /// Source pattern (remote side), e.g. "refs/heads/*".
    src: String,
    /// Destination pattern (local side), e.g. "refs/remotes/origin/*".
    dst: String,
    /// Whether this is a force refspec (leading '+').
    #[allow(dead_code)]
    force: bool,
    /// Whether this is a negative (exclusion) refspec (leading '^').
    negative: bool,
}

/// Collect all fetch refspecs from a config key (may be multi-valued).
fn collect_refspecs(config: &ConfigSet, key: &str) -> Vec<FetchRefspec> {
    let mut result = Vec::new();
    for entry in config.entries() {
        if entry.key == key {
            if let Some(ref val) = entry.value {
                let val = val.trim();
                // Check for negative refspec (^pattern)
                if let Some(pattern) = val.strip_prefix('^') {
                    result.push(FetchRefspec {
                        src: pattern.to_owned(),
                        dst: String::new(),
                        force: false,
                        negative: true,
                    });
                    continue;
                }
                let (force, val) = if let Some(stripped) = val.strip_prefix('+') {
                    (true, stripped)
                } else {
                    (false, val)
                };
                if let Some(colon) = val.find(':') {
                    result.push(FetchRefspec {
                        src: val[..colon].to_owned(),
                        dst: val[colon + 1..].to_owned(),
                        force,
                        negative: false,
                    });
                } else {
                    result.push(FetchRefspec {
                        src: val.to_owned(),
                        dst: val.to_owned(),
                        force,
                        negative: false,
                    });
                }
            }
        }
    }
    result
}

/// Map a remote ref through fetch refspecs.
///
/// For a refspec like `refs/heads/*:refs/remotes/origin/*`, if the remote ref
/// is `refs/heads/main`, the result is `refs/remotes/origin/main`.
/// Returns None if no refspec matches.
fn map_ref_through_refspecs(remote_ref: &str, refspecs: &[FetchRefspec]) -> Option<String> {
    if ref_excluded_by_fetch_refspecs(remote_ref, refspecs) {
        return None;
    }
    for rs in refspecs {
        if rs.negative {
            continue;
        }
        if let Some(mapped) = match_refspec_pattern(&rs.src, &rs.dst, remote_ref) {
            return Some(mapped);
        }
    }
    None
}

/// Reverse-map a local ref through configured refspecs to find
/// the remote ref that would normally map to it.
fn reverse_map_refspec(local_ref: &str, refspecs: &[FetchRefspec]) -> Option<String> {
    for rs in refspecs {
        if rs.negative || rs.dst.is_empty() {
            continue;
        }
        // Try to reverse the dst pattern to find what src would produce local_ref
        if let Some(star_pos) = rs.dst.find('*') {
            let prefix = &rs.dst[..star_pos];
            let suffix = &rs.dst[star_pos + 1..];
            if local_ref.starts_with(prefix) && local_ref.ends_with(suffix) {
                let matched = &local_ref[prefix.len()..local_ref.len() - suffix.len()];
                let remote_ref = rs.src.replacen('*', matched, 1);
                return Some(remote_ref);
            }
        } else if rs.dst == local_ref {
            return Some(rs.src.clone());
        }
    }
    None
}

/// Match a single refspec pattern. Both src and dst may contain a single '*'.
fn match_refspec_pattern(src_pattern: &str, dst_pattern: &str, refname: &str) -> Option<String> {
    if let Some(star_pos) = src_pattern.find('*') {
        let prefix = &src_pattern[..star_pos];
        let suffix = &src_pattern[star_pos + 1..];
        if refname.starts_with(prefix) && refname.ends_with(suffix) {
            let matched = &refname[prefix.len()..refname.len() - suffix.len()];
            let result = dst_pattern.replacen('*', matched, 1);
            return Some(result);
        }
    } else if src_pattern == refname {
        // Exact match (no wildcard)
        return Some(dst_pattern.to_owned());
    }
    None
}

/// Match a glob pattern against a ref name, returning the matched wildcard portion.
fn match_glob_pattern<'a>(pattern: &str, refname: &'a str) -> Option<&'a str> {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        if refname.starts_with(prefix)
            && refname.ends_with(suffix)
            && refname.len() >= prefix.len() + suffix.len()
        {
            Some(&refname[prefix.len()..refname.len() - suffix.len()])
        } else {
            None
        }
    } else if pattern == refname {
        Some(refname)
    } else {
        None
    }
}

/// Copy symbolic refs that match a glob pattern from remote to local.
fn copy_symrefs(
    remote_git_dir: &Path,
    local_git_dir: &Path,
    src_pattern: &str,
    dst_pattern: &str,
) -> Result<()> {
    // Walk the remote refs directory for symbolic refs
    let refs_dir = remote_git_dir.join("refs");
    if !refs_dir.is_dir() {
        return Ok(());
    }
    for_each_ref_file(&refs_dir, "refs", &mut |refname, path| {
        if let Some(matched) = match_glob_pattern(src_pattern, &refname) {
            let content = fs::read_to_string(path)?;
            let content = content.trim();
            if let Some(target) = content.strip_prefix("ref: ") {
                // It's a symbolic ref — write it locally
                let local_ref = dst_pattern.replacen('*', matched, 1);
                let local_path =
                    local_git_dir.join(local_ref.replace('/', std::path::MAIN_SEPARATOR_STR));
                if let Some(parent) = local_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&local_path, format!("ref: {target}\n"))?;
            }
        }
        Ok(())
    })?;
    Ok(())
}

fn for_each_ref_file(
    dir: &Path,
    prefix: &str,
    cb: &mut dyn FnMut(String, &Path) -> Result<()>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let refname = format!("{prefix}/{name_str}");
        if entry.file_type()?.is_dir() {
            for_each_ref_file(&entry.path(), &refname, cb)?;
        } else {
            cb(refname, &entry.path())?;
        }
    }
    Ok(())
}

/// Collect all configured remote names.
fn collect_remote_names(config: &ConfigSet) -> Vec<String> {
    let mut names = Vec::new();
    for entry in config.entries() {
        let parts: Vec<&str> = entry.key.splitn(3, '.').collect();
        if parts.len() == 3 && parts[0] == "remote" && parts[2] == "url" {
            let name = parts[1].to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

fn configured_remote_base(git_dir: &Path) -> PathBuf {
    if git_dir.file_name().is_some_and(|name| name == ".git") {
        git_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| git_dir.to_path_buf())
    } else {
        git_dir.to_path_buf()
    }
}

/// Resolve the git directory from CWD.
fn resolve_git_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("GIT_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let mut cur = cwd.as_path();
    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() {
            return Ok(dot_git);
        }
        if dot_git.is_file() {
            if let Ok(content) = fs::read_to_string(&dot_git) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("gitdir:") {
                        let path = rest.trim();
                        let resolved = if Path::new(path).is_absolute() {
                            PathBuf::from(path)
                        } else {
                            cur.join(path)
                        };
                        return Ok(resolved);
                    }
                }
            }
        }
        // Check if this is a bare repo
        if cur.join("objects").is_dir() && cur.join("HEAD").is_file() {
            return Ok(cur.to_path_buf());
        }
        cur = match cur.parent() {
            Some(p) => p,
            None => bail!("not a git repository (or any of the parent directories): .git"),
        };
    }
}

/// Check if a branch ref is checked out in any worktree, return the worktree path.
fn is_branch_in_worktree(git_dir: &std::path::Path, branch_ref: &str) -> Option<String> {
    let common = grit_lib::refs::common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf());
    // Check main worktree
    if let Ok(head) = grit_lib::state::resolve_head(&common) {
        if let grit_lib::state::HeadState::Branch { ref refname, .. } = head {
            if refname == branch_ref {
                return Some(common.parent().unwrap_or(&common).display().to_string());
            }
        }
    }
    // Check linked worktrees
    let wt_dir = common.join("worktrees");
    if wt_dir.is_dir() {
        for entry in std::fs::read_dir(&wt_dir).into_iter().flatten().flatten() {
            let admin = entry.path();
            if !admin.is_dir() {
                continue;
            }
            let head_file = admin.join("HEAD");
            if let Ok(content) = std::fs::read_to_string(&head_file) {
                if let Some(refname) = content.trim().strip_prefix("ref: ") {
                    if refname.trim() == branch_ref {
                        let gitdir_file = admin.join("gitdir");
                        let path = if let Ok(raw) = std::fs::read_to_string(&gitdir_file) {
                            let p = std::path::Path::new(raw.trim());
                            p.parent().unwrap_or(p).display().to_string()
                        } else {
                            entry.file_name().to_string_lossy().to_string()
                        };
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}
