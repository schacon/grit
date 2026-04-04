//! `grit push` — update remote refs and associated objects.
//!
//! Only **local (file://)** transports are supported.  Copies objects from
//! the local repository to the remote and updates remote refs.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::hooks::{run_hook, run_hook_capture, run_hook_with_env_cwd, HookResult};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit push`.
#[derive(Debug, ClapArgs)]
#[command(about = "Update remote refs along with associated objects")]
pub struct Args {
    /// Remote name or URL (defaults to "origin").
    #[arg(value_name = "REMOTE")]
    pub remote: Option<String>,

    /// Refspec(s) to push (e.g. "main", "main:main", "refs/heads/main:refs/heads/main").
    #[arg(value_name = "REFSPEC")]
    pub refspecs: Vec<String>,

    /// Allow non-fast-forward updates.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Push all tags.
    #[arg(long = "tags")]
    pub tags: bool,

    /// Show what would be done, without making changes.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Delete remote refs.
    #[arg(long = "delete", short = 'd')]
    pub delete: bool,

    /// Set upstream tracking reference.
    #[arg(short = 'u', long = "set-upstream")]
    pub set_upstream: bool,

    /// Force push only if the remote ref matches the expected old value.
    /// Accepts optional `=<refname>` or `=<refname>:<expect>` syntax.
    #[arg(long = "force-with-lease", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    pub force_with_lease: Option<String>,

    /// Disable force-with-lease.
    #[arg(long = "no-force-with-lease", hide = true)]
    pub no_force_with_lease: bool,

    /// Request an atomic push: either all refs update or none do.
    #[arg(long)]
    pub atomic: bool,

    /// Send a push option string to the server.
    #[arg(long = "push-option", short = 'o', value_name = "OPTION")]
    pub push_option: Vec<String>,

    /// Machine-readable output (one line per ref update).
    #[arg(long)]
    pub porcelain: bool,

    /// Push all branches (refs/heads/*).
    #[arg(long)]
    pub all: bool,

    /// Push all branches (alias for --all).
    #[arg(long)]
    pub branches: bool,

    /// Mirror all refs to the remote.
    #[arg(long)]
    pub mirror: bool,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Skip the pre-push hook.
    #[arg(long = "no-verify")]
    pub no_verify: bool,

    /// Check, on-demand, or no recursion into submodules.
    #[arg(long = "recurse-submodules")]
    pub recurse_submodules: Option<String>,

    /// GPG sign the push (accepted but not yet implemented).
    #[arg(long = "signed", num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub signed: Option<String>,

    /// Disable GPG signing.
    #[arg(long = "no-signed", hide = true)]
    pub no_signed: bool,
}

/// Parse `--force-with-lease` value to determine expected OID for a given remote ref.
///
/// Formats:
/// - `""` (bare flag) → use tracking ref
/// - `"<refname>"` → use tracking ref, but only for matching ref
/// - `"<refname>:<expect>"` → resolve `<expect>` as a ref or OID
fn force_with_lease_expected(
    lease_val: &Option<String>,
    remote_ref: &str,
    remote_name: &str,
    git_dir: &std::path::Path,
) -> Option<ObjectId> {
    let val = match lease_val {
        Some(v) => v,
        None => return None,
    };

    if val.is_empty() {
        // Bare --force-with-lease: use tracking ref for this remote ref
        let branch = remote_ref.strip_prefix("refs/heads/").unwrap_or(remote_ref);
        let tracking_ref = format!("refs/remotes/{remote_name}/{branch}");
        return refs::resolve_ref(git_dir, &tracking_ref).ok();
    }

    if let Some((lease_ref, expect_str)) = val.split_once(':') {
        // --force-with-lease=<refname>:<expect>
        let lease_full = if lease_ref.starts_with("refs/") {
            lease_ref.to_string()
        } else {
            format!("refs/heads/{lease_ref}")
        };
        let remote_full = if remote_ref.starts_with("refs/") {
            remote_ref.to_string()
        } else {
            format!("refs/heads/{remote_ref}")
        };
        if lease_full != remote_full {
            return None; // lease doesn't apply to this ref
        }
        // Try to resolve expect_str as a ref, then as a hex OID
        if let Ok(oid) = refs::resolve_ref(git_dir, expect_str) {
            return Some(oid);
        }
        if let Ok(oid) = refs::resolve_ref(git_dir, &format!("refs/heads/{expect_str}")) {
            return Some(oid);
        }
        if let Ok(oid) = expect_str.parse::<ObjectId>() {
            return Some(oid);
        }
        return None;
    }

    // --force-with-lease=<refname>: use tracking ref, but only for this ref
    let lease_full = if val.starts_with("refs/") {
        val.to_string()
    } else {
        format!("refs/heads/{val}")
    };
    let remote_full = if remote_ref.starts_with("refs/") {
        remote_ref.to_string()
    } else {
        format!("refs/heads/{remote_ref}")
    };
    if lease_full != remote_full {
        return None;
    }
    let branch = remote_ref.strip_prefix("refs/heads/").unwrap_or(remote_ref);
    let tracking_ref = format!("refs/remotes/{remote_name}/{branch}");
    refs::resolve_ref(git_dir, &tracking_ref).ok()
}

/// A single ref update to perform on the remote.
struct RefUpdate {
    /// Local ref (None for delete).
    local_ref: Option<String>,
    /// Remote ref.
    remote_ref: String,
    /// Old OID on remote (None if new).
    old_oid: Option<ObjectId>,
    /// New OID (None for delete).
    new_oid: Option<ObjectId>,
    /// Expected old OID for force-with-lease (None = use actual old).
    expected_oid: Option<ObjectId>,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    let push_all = args.all || args.branches;

    // Validate flag combinations
    if push_all && !args.refspecs.is_empty() {
        bail!("--all/--branches can not be combined with refspecs");
    }
    if push_all && args.mirror {
        bail!("--all and --mirror cannot be used together");
    }
    // Handle --signed: reject unless if-asked (no GPG implementation)
    if let Some(ref signed_val) = args.signed {
        if signed_val != "if-asked" {
            bail!("push certificate signing is not supported");
        }
    }

    if push_all && args.tags {
        bail!("--all and --tags cannot be used together");
    }
    if push_all && args.delete {
        bail!("--all and --delete cannot be used together");
    }

    let head = resolve_head(&repo.git_dir)?;
    let current_branch = head.branch_name().map(|s| s.to_owned());

    // Determine remote name and URL.
    // If the remote argument looks like a path (contains '/' or starts with '.'),
    // use it directly as the URL instead of looking it up in config.
    let remote_name_owned: String;
    let url: String;
    let _is_path_remote: bool;

    if let Some(ref r) = args.remote {
        if r.contains('/') || r.starts_with('.') {
            // Path-based remote: use directly as URL
            _is_path_remote = true;
            remote_name_owned = r.clone();
            url = r.clone();
        } else {
            _is_path_remote = false;
            remote_name_owned = r.clone();
            let url_key = format!("remote.{}.url", remote_name_owned);
            url = config
                .get(&url_key)
                .with_context(|| format!("remote '{}' not found", remote_name_owned))?;
        }
    } else {
        _is_path_remote = false;
        remote_name_owned = if let Some(ref branch) = current_branch {
            config
                .get(&format!("branch.{branch}.remote"))
                .unwrap_or_else(|| "origin".to_owned())
        } else {
            "origin".to_owned()
        };
        let url_key = format!("remote.{}.url", remote_name_owned);
        url = config
            .get(&url_key)
            .with_context(|| format!("remote '{}' not found", remote_name_owned))?;
    };
    let remote_name = remote_name_owned.as_str();

    // Check protocol.file.allow before local push
    crate::protocol::check_protocol_allowed("file", Some(&repo.git_dir))?;

    let remote_path = if let Some(stripped) = url.strip_prefix("file://") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(&url)
    };

    // Open remote repo
    let remote_repo = open_repo(&remote_path)
        .with_context(|| format!("could not open remote repository at '{}'", remote_path.display()))?;

    // Build list of ref updates
    let mut updates = Vec::new();

    if args.mirror {
        // Mirror: push all local refs to remote, and delete remote refs
        // that don't exist locally.
        let local_all = refs::list_refs(&repo.git_dir, "refs/")?;
        for (refname, local_oid) in &local_all {
            // Skip special refs like HEAD, FETCH_HEAD, etc.
            if !refname.starts_with("refs/") {
                continue;
            }
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, refname).ok();
            if old_oid.as_ref() == Some(local_oid) {
                continue;
            }
            updates.push(RefUpdate {
                local_ref: Some(refname.clone()),
                remote_ref: refname.clone(),
                old_oid,
                new_oid: Some(*local_oid),
                expected_oid: None,
            });
        }
        // Delete remote refs that don't exist locally
        let remote_all = refs::list_refs(&remote_repo.git_dir, "refs/")?;
        for (refname, _remote_oid) in &remote_all {
            if !refname.starts_with("refs/") {
                continue;
            }
            if !local_all.iter().any(|(r, _)| r == refname) {
                let old_oid = refs::resolve_ref(&remote_repo.git_dir, refname).ok();
                updates.push(RefUpdate {
                    local_ref: None,
                    remote_ref: refname.clone(),
                    old_oid,
                    new_oid: None,
                    expected_oid: None,
                });
            }
        }
    } else if push_all {
        // Push all branches (refs/heads/*)
        let local_branches = refs::list_refs(&repo.git_dir, "refs/heads/")?;
        for (refname, local_oid) in &local_branches {
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, refname).ok();
            if old_oid.as_ref() == Some(local_oid) {
                continue;
            }
            updates.push(RefUpdate {
                local_ref: Some(refname.clone()),
                remote_ref: refname.clone(),
                old_oid,
                new_oid: Some(*local_oid),
                expected_oid: None,
            });
        }
    } else if args.delete {
        // Delete mode: refspecs are remote ref names to delete
        if args.refspecs.is_empty() {
            bail!("--delete requires at least one refspec");
        }
        for spec in &args.refspecs {
            let remote_ref = normalize_ref(spec);
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();
            if old_oid.is_none() {
                bail!("remote ref '{}' not found", spec);
            }
            let expected_oid = force_with_lease_expected(
                &args.force_with_lease,
                &remote_ref,
                remote_name,
                &repo.git_dir,
            );
            updates.push(RefUpdate {
                local_ref: None,
                remote_ref,
                old_oid,
                new_oid: None,
                expected_oid,
            });
        }
    } else if !args.refspecs.is_empty() {
        // Explicit refspecs
        for spec in &args.refspecs {
            let (src, dst) = parse_refspec(spec);

            // Empty src (e.g. ":branch") means delete
            if src.is_empty() {
                let remote_ref = normalize_ref(&dst);
                let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();
                if old_oid.is_none() {
                    bail!("remote ref '{}' not found", dst);
                }
                updates.push(RefUpdate {
                    local_ref: None,
                    remote_ref,
                    old_oid,
                    new_oid: None,
                    expected_oid: None,
                });
                continue;
            }

            // Resolve HEAD to its target ref
            let resolved_src = if src == "HEAD" {
                match grit_lib::state::resolve_head(&repo.git_dir) {
                    Ok(head) => match head {
                        grit_lib::state::HeadState::Branch { refname, .. } => refname,
                        grit_lib::state::HeadState::Detached { oid, .. } => oid.to_hex(),
                        grit_lib::state::HeadState::Invalid => src.clone(),
                    },
                    Err(_) => src.clone(),
                }
            } else {
                src.clone()
            };
            let local_ref = normalize_ref(&resolved_src);
            let remote_ref = normalize_ref(&dst);

            let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
                .with_context(|| format!("src refspec '{}' does not match any", src))?;
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

            // For --force-with-lease, compute expected OID from the lease value.
            let expected_oid = force_with_lease_expected(
                &args.force_with_lease,
                &remote_ref,
                remote_name,
                &repo.git_dir,
            );

            updates.push(RefUpdate {
                local_ref: Some(local_ref),
                remote_ref,
                old_oid,
                new_oid: Some(local_oid),
                expected_oid,
            });
        }
    } else {
        // Default: push current branch
        let branch = current_branch
            .as_deref()
            .context("not on a branch; specify a refspec to push")?;

        let local_ref = format!("refs/heads/{branch}");
        let remote_ref = local_ref.clone();

        let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
            .with_context(|| format!("branch '{}' has no commits", branch))?;
        let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

        // For --force-with-lease, compute expected OID from the lease value.
        let expected_oid = force_with_lease_expected(
            &args.force_with_lease,
            &remote_ref,
            remote_name,
            &repo.git_dir,
        );

        updates.push(RefUpdate {
            local_ref: Some(local_ref),
            remote_ref,
            old_oid,
            new_oid: Some(local_oid),
            expected_oid,
        });
    }

    // Push tags if requested
    if args.tags {
        let local_tags = refs::list_refs(&repo.git_dir, "refs/tags/")?;
        for (refname, local_oid) in &local_tags {
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, refname).ok();
            if old_oid.as_ref() == Some(local_oid) {
                continue; // already up to date
            }
            updates.push(RefUpdate {
                local_ref: Some(refname.clone()),
                remote_ref: refname.clone(),
                old_oid,
                new_oid: Some(*local_oid),
                expected_oid: None,
            });
        }
    }

    if updates.is_empty() {
        if !args.quiet {
            println!("Everything up-to-date");
        }
        return Ok(());
    }

    // Validate updates (fast-forward check unless --force or --force-with-lease)
    for update in &updates {
        // force-with-lease: verify remote ref matches what we think it is
        // (i.e., what our remote-tracking ref says)
        if let Some(expected) = &update.expected_oid {
            let actual_remote = refs::resolve_ref(&remote_repo.git_dir, &update.remote_ref).ok();
            if actual_remote.as_ref() != Some(expected) {
                bail!(
                    "failed to push some refs: stale info for '{}' \
                     (force-with-lease check failed)",
                    update.remote_ref
                );
            }
        }

        if let (Some(old), Some(new)) = (&update.old_oid, &update.new_oid) {
            if old == new {
                continue;
            }
            if !args.force && args.force_with_lease.is_none() && !is_ancestor(&repo, *old, *new)? {
                bail!(
                    "Updates were rejected because the tip of your current branch is behind\n\
                     its remote counterpart. If you want to force the update, use --force.\n\
                     remote ref: {}",
                    update.remote_ref
                );
            }
        }
    }

    // Run pre-push hook (unless --no-verify)
    if !args.no_verify {
        let zero_oid = "0".repeat(40);
        let mut hook_lines = Vec::new();
        for update in &updates {
            let local_ref = update.local_ref.as_deref().unwrap_or("(delete)");
            let local_oid = update
                .new_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid.clone());
            let remote_ref = &update.remote_ref;
            let remote_oid = update
                .old_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid.clone());
            hook_lines.push(format!(
                "{local_ref} {local_oid} {remote_ref} {remote_oid}\n"
            ));
        }
        let stdin_data = hook_lines.join("");
        let result = run_hook(
            &repo,
            "pre-push",
            &[remote_name, &url],
            Some(stdin_data.as_bytes()),
        );
        match result {
            HookResult::Failed(code) => {
                bail!("pre-push hook declined the push (exit code {code})");
            }
            _ => {}
        }
    }

    // Check remote's receive.advertisePushOptions config
    if !args.push_option.is_empty() {
        let remote_config = ConfigSet::load(Some(&remote_repo.git_dir), true)?;
        let advertise = remote_config
            .get("receive.advertisePushOptions")
            .map(|v| v != "false")
            .unwrap_or(true);
        if !advertise {
            bail!("the receiving end does not support push options");
        }
    }

    // Write push options file for the remote (local transport simulation)
    if !args.push_option.is_empty() {
        let push_opts_path = remote_repo.git_dir.join("push_options");
        let content = args.push_option.join("\n") + "\n";
        fs::write(&push_opts_path, content)
            .context("writing push options")?;
    }

    if !args.dry_run {
        // Copy objects from local → remote
        copy_objects(&repo.git_dir, &remote_repo.git_dir)
            .context("copying objects to remote")?;
    }

    // For --atomic, verify all refs can be updated before writing any.
    // In local transport we do this by checking that nothing changed between
    // our initial read and now.
    if args.atomic {
        for update in &updates {
            let current = refs::resolve_ref(&remote_repo.git_dir, &update.remote_ref).ok();
            if current != update.old_oid {
                bail!(
                    "atomic push failed: remote ref '{}' changed during push",
                    update.remote_ref
                );
            }
        }
    }

    // Apply ref updates, running remote-side hooks first
    if !args.quiet && !args.porcelain {
        println!("To {url}");
    }

    // Build push-option env vars for hooks
    let push_option_env: Vec<(String, String)> = if !args.push_option.is_empty() {
        let mut env = vec![
            ("GIT_PUSH_OPTION_COUNT".to_string(), args.push_option.len().to_string()),
        ];
        for (i, opt) in args.push_option.iter().enumerate() {
            env.push((format!("GIT_PUSH_OPTION_{i}"), opt.clone()));
        }
        env
    } else {
        Vec::new()
    };
    let push_option_env_refs: Vec<(&str, &str)> = push_option_env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // Track results for atomic rollback on failure
    let mut applied_updates: Vec<(&RefUpdate, Option<ObjectId>)> = Vec::new();
    let mut rejected: Vec<(&RefUpdate, String)> = Vec::new();
    let zero_oid_str = "0".repeat(40);

    // Run remote-side pre-receive hook
    if !args.dry_run {
        let mut pre_receive_lines = String::new();
        for update in &updates {
            let old_hex = update
                .old_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            let new_hex = update
                .new_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            pre_receive_lines.push_str(&format!("{old_hex} {new_hex} {}\n", update.remote_ref));
        }
        let result = run_hook_with_env_cwd(
            &remote_repo,
            "pre-receive",
            &[],
            Some(pre_receive_lines.as_bytes()),
            &push_option_env_refs,
            Some(&remote_repo.git_dir),
        );
        if let HookResult::Failed(code) = result {
            bail!("pre-receive hook declined the push (exit code {code})");
        }
    }

    for update in &updates {
        // Run the remote's `update` hook: update <refname> <old-oid> <new-oid>
        if !args.dry_run {
            let old_hex = update
                .old_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            let new_hex = update
                .new_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            let (hook_result, hook_output) = run_hook_capture(
                &remote_repo,
                "update",
                &[&update.remote_ref, &old_hex, &new_hex],
                None,
            );
            // Forward hook output to stderr, optionally colorized
            if !hook_output.is_empty() {
                let output_str = String::from_utf8_lossy(&hook_output);
                let color_remote = resolve_color_remote(&repo, &args);
                colorize_remote_output(&output_str, color_remote);
            }
            if let HookResult::Failed(_code) = hook_result {
                if args.atomic {
                    // Rollback all applied updates
                    for (prev_update, prev_old) in &applied_updates {
                        if let Some(old_oid) = prev_old {
                            let _ = refs::write_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                                old_oid,
                            );
                        } else {
                            let _ = refs::delete_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                            );
                        }
                    }
                    // Report all updates as rejected for atomic
                    rejected.push((update, "hook declined".to_string()));
                    for remaining in updates.iter().skip(updates.iter().position(|u| std::ptr::eq(u, update)).unwrap_or(0) + 1) {
                        rejected.push((remaining, "atomic push failed".to_string()));
                    }
                    break;
                }
                rejected.push((update, "hook declined".to_string()));
                continue;
            }
        }

        let result = apply_ref_update(
            &repo,
            &remote_repo,
            update,
            &args,
            &url,
        );

        match result {
            Ok(()) => {
                applied_updates.push((update, update.old_oid));
            }
            Err(e) => {
                if args.atomic {
                    // Rollback all applied updates
                    for (prev_update, prev_old) in &applied_updates {
                        if let Some(old_oid) = prev_old {
                            let _ = refs::write_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                                old_oid,
                            );
                        } else {
                            let _ = refs::delete_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                            );
                        }
                    }
                    bail!("atomic push failed: {}", e);
                }
                return Err(e);
            }
        }
    }

    // Run remote-side post-receive hook for successfully applied updates
    if !args.dry_run && !applied_updates.is_empty() {
        let mut post_receive_lines = String::new();
        for (update, old_oid) in &applied_updates {
            let old_hex = old_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            let new_hex = update
                .new_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            post_receive_lines.push_str(&format!("{old_hex} {new_hex} {}\n", update.remote_ref));
        }
        let _ = run_hook_with_env_cwd(
            &remote_repo,
            "post-receive",
            &[],
            Some(post_receive_lines.as_bytes()),
            &push_option_env_refs,
            Some(&remote_repo.git_dir),
        );
    }

    // Report rejected refs to stderr
    if !rejected.is_empty() {
        for (update, reason) in &rejected {
            let src_short = update
                .local_ref
                .as_deref()
                .and_then(|r| r.strip_prefix("refs/heads/"))
                .or_else(|| {
                    update
                        .local_ref
                        .as_deref()
                        .and_then(|r| r.strip_prefix("refs/tags/"))
                })
                .unwrap_or(
                    update.local_ref.as_deref().unwrap_or("(delete)"),
                );
            let dst_short = update
                .remote_ref
                .strip_prefix("refs/heads/")
                .or_else(|| update.remote_ref.strip_prefix("refs/tags/"))
                .unwrap_or(&update.remote_ref);
            eprintln!(
                " ! [remote rejected] {} -> {} ({})",
                src_short, dst_short, reason
            );
        }
        bail!("failed to push some refs to '{url}'");
    }

    // Set upstream tracking if requested
    if args.set_upstream {
        if let Some(ref branch) = current_branch {
            let local_ref = format!("refs/heads/{branch}");
            if updates.iter().any(|u| u.local_ref.as_deref() == Some(local_ref.as_str())) {
                set_upstream_config(&repo.git_dir, branch, remote_name)?;
                if !args.quiet {
                    eprintln!(
                        "branch '{branch}' set up to track '{remote_name}/{branch}'."
                    );
                }
            }
        }
    }

    Ok(())
}

/// Apply a single ref update on the remote, printing output as appropriate.
fn apply_ref_update(
    _repo: &Repository,
    remote_repo: &Repository,
    update: &RefUpdate,
    args: &Args,
    _url: &str,
) -> Result<()> {
    let zero_oid = "0".repeat(40);

    match (&update.new_oid, &update.old_oid) {
        (Some(new_oid), old_oid_opt) => {
            if !args.dry_run {
                refs::write_ref(&remote_repo.git_dir, &update.remote_ref, new_oid)
                    .with_context(|| {
                        format!("updating remote ref {}", update.remote_ref)
                    })?;
            }

            let branch_short = update
                .remote_ref
                .strip_prefix("refs/heads/")
                .or_else(|| update.remote_ref.strip_prefix("refs/tags/"))
                .unwrap_or(&update.remote_ref);
            let src_short = update
                .local_ref
                .as_deref()
                .and_then(|r| r.strip_prefix("refs/heads/"))
                .or_else(|| {
                    update
                        .local_ref
                        .as_deref()
                        .and_then(|r| r.strip_prefix("refs/tags/"))
                })
                .unwrap_or(
                    update.local_ref.as_deref().unwrap_or("(unknown)"),
                );

            if args.porcelain {
                let old_hex = old_oid_opt
                    .map(|o| o.to_hex())
                    .unwrap_or_else(|| zero_oid.clone());
                let flag = match old_oid_opt {
                    Some(old) if old != new_oid => " ",
                    None => "*",
                    _ => "=",
                };
                let local_ref_str = update.local_ref.as_deref().unwrap_or("(delete)");
                println!(
                    "{flag}\t{local_ref_str}:{remote_ref}\t{old_hex}..{new_hex}\t{src_short} -> {branch_short}",
                    remote_ref = update.remote_ref,
                    old_hex = &old_hex[..7],
                    new_hex = &new_oid.to_hex()[..7],
                );
            } else if !args.quiet {
                match old_oid_opt {
                    Some(old) if old != new_oid => {
                        println!(
                            "   {}..{}  {} -> {}",
                            &old.to_hex()[..7],
                            &new_oid.to_hex()[..7],
                            src_short,
                            branch_short,
                        );
                    }
                    None => {
                        let kind = if update.remote_ref.starts_with("refs/tags/") {
                            "tag"
                        } else {
                            "branch"
                        };
                        println!(" * [new {kind}]      {src_short} -> {branch_short}");
                    }
                    _ => {
                        println!(" = [up to date]      {} -> {}", src_short, branch_short);
                    }
                }
            }
        }
        (None, Some(old_oid)) => {
            // Delete
            if !args.dry_run {
                refs::delete_ref(&remote_repo.git_dir, &update.remote_ref)
                    .with_context(|| {
                        format!("deleting remote ref {}", update.remote_ref)
                    })?;
            }

            let branch_short = update
                .remote_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(&update.remote_ref);

            if args.porcelain {
                println!(
                    "-\t:{remote_ref}\t{old_hex}..{zero}\t(delete) -> {branch_short}",
                    remote_ref = update.remote_ref,
                    old_hex = &old_oid.to_hex()[..7],
                    zero = &zero_oid[..7],
                );
            } else if !args.quiet {
                println!(
                    " - [deleted]         {} -> {}",
                    &old_oid.to_hex()[..7],
                    branch_short,
                );
            }
        }
        _ => {}
    }

    Ok(())
}

/// Parse a refspec like "src:dst" or just "name" (meaning "name:name").
fn parse_refspec(spec: &str) -> (String, String) {
    if let Some(idx) = spec.find(':') {
        let src = spec[..idx].to_owned();
        let dst = spec[idx + 1..].to_owned();
        (src, dst)
    } else {
        (spec.to_owned(), spec.to_owned())
    }
}

/// Normalize a ref name: if it doesn't start with "refs/", assume "refs/heads/".
fn normalize_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

/// Write branch tracking config.
fn set_upstream_config(git_dir: &Path, branch: &str, remote: &str) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };
    config.set(
        &format!("branch.{branch}.remote"),
        remote,
    )?;
    config.set(
        &format!("branch.{branch}.merge"),
        &format!("refs/heads/{branch}"),
    )?;
    config.write()?;
    Ok(())
}

/// Copy all objects (loose + packs) from src to dst, skipping existing.
fn copy_objects(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    let src_objects = src_git_dir.join("objects");
    let dst_objects = dst_git_dir.join("objects");

    // Copy loose objects
    if src_objects.is_dir() {
        for entry in fs::read_dir(&src_objects)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str == "info" || name_str == "pack" {
                continue;
            }
            if !entry.file_type()?.is_dir() || name_str.len() != 2 {
                continue;
            }

            let dst_dir = dst_objects.join(&*name);
            for inner in fs::read_dir(entry.path())? {
                let inner = inner?;
                if inner.file_type()?.is_file() {
                    let dst_file = dst_dir.join(inner.file_name());
                    if !dst_file.exists() {
                        fs::create_dir_all(&dst_dir)?;
                        if fs::hard_link(inner.path(), &dst_file).is_err() {
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
                if !dst_file.exists() {
                    if fs::hard_link(entry.path(), &dst_file).is_err() {
                        fs::copy(entry.path(), &dst_file)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

/// Determine if remote messages should be colorized.
fn resolve_color_remote(repo: &Repository, _args: &Args) -> bool {
    // Check -c color.remote=always from environment
    // GIT_CONFIG_COUNT / GIT_CONFIG_KEY / GIT_CONFIG_VALUE override
    for (key, val) in std::env::vars() {
        if key.starts_with("GIT_CONFIG_KEY_") {
            if val == "color.remote" {
                let idx = key.strip_prefix("GIT_CONFIG_KEY_").unwrap_or("");
                let val_key = format!("GIT_CONFIG_VALUE_{idx}");
                if let Ok(v) = std::env::var(&val_key) {
                    return v == "always" || v == "true";
                }
            }
        }
    }
    // Check repo config
    if let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) {
        if let Some(val) = config.get("color.remote") {
            return val == "always" || val == "true";
        }
    }
    false
}

/// Write remote messages to stderr, colorizing keywords if enabled.
fn colorize_remote_output(output: &str, colorize: bool) {
    use std::io::Write;
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    for line in output.lines() {
        if colorize {
            let colored = colorize_remote_line(line);
            let _ = writeln!(err, "remote: {colored}");
        } else {
            let _ = writeln!(err, "remote: {line}");
        }
    }
}

/// Colorize a single remote message line based on keyword prefix.
fn colorize_remote_line(line: &str) -> String {
    // ANSI escape codes (separate bold + color for compatibility)
    let bold_red = "\x1b[1m\x1b[31m";
    let bold_yellow = "\x1b[1m\x1b[33m";
    let bold_green = "\x1b[1m\x1b[32m";
    let bold_cyan = "\x1b[1m\x1b[36m";
    let reset = "\x1b[m";

    // Check for keyword prefixes
    let keywords: &[(&str, &str)] = &[
        ("error", bold_red),
        ("warning", bold_yellow),
        ("hint", bold_cyan),
        ("success", bold_green),
    ];

    for (keyword, color) in keywords {
        if let Some(rest) = line.strip_prefix(keyword) {
            if rest.starts_with(':') || rest.starts_with(' ') {
                return format!("{color}{keyword}{reset}{rest}");
            }
        }
    }
    line.to_string()
}
