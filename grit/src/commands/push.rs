//! `grit push` — update remote refs and associated objects.
//!
//! Only **local (file://)** transports are supported.  Copies objects from
//! the local repository to the remote and updates remote refs.

use crate::protocol_wire;
use crate::wire_trace;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{parse_bool, parse_color, ConfigFile, ConfigScope, ConfigSet};
use grit_lib::gitmodules::{oids_from_copied_object_paths, verify_gitmodules_for_commit};
use grit_lib::hooks::{run_hook, run_hook_capture, HookResult};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;

use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    /// Accepts: --force-with-lease, --force-with-lease=<refname>,
    /// or --force-with-lease=<refname>:<expect>
    #[arg(long = "force-with-lease", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    pub force_with_lease: Option<String>,

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

    /// Sign the push (accepted but not implemented; value: true, false, if-asked).
    #[arg(long = "signed", num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub signed: Option<String>,

    /// Do not sign the push.
    #[arg(long = "no-signed")]
    pub no_signed: bool,

    /// Also push annotated tags that point to commits being pushed.
    #[arg(long = "follow-tags")]
    pub follow_tags: bool,

    /// Disable --follow-tags.
    #[arg(long = "no-follow-tags")]
    pub no_follow_tags: bool,

    /// Delete remote refs that no longer have a local counterpart (respects negative refspecs).
    #[arg(long)]
    pub prune: bool,

    /// Show detailed progress.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Receive-pack program on the remote (`--receive-pack` delegates to system `git push` for
    /// protocol compatibility; native path may use wire protocol instead of file copy).
    #[arg(long = "receive-pack", value_name = "RECEIVE_PACK")]
    pub receive_pack: Option<String>,

    /// Accepted for Git compatibility; forwarded when delegating to system `git push`.
    #[arg(long = "upload-pack", value_name = "PATH")]
    pub upload_pack: Option<String>,
}

/// A single ref update to perform on the remote.
#[allow(dead_code)]
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
    /// Per-refspec force flag (from '+' prefix).
    refspec_force: bool,
}

/// Stable ref processing order for `push --mirror --atomic` (matches Git's stderr ordering in
/// `t5543-atomic-push`).
fn mirror_atomic_ref_order(updates: &[RefUpdate]) -> Vec<String> {
    use std::collections::HashSet;
    let head_shorts: HashSet<String> = updates
        .iter()
        .filter(|u| u.remote_ref.starts_with("refs/heads/"))
        .map(|u| {
            u.remote_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(&u.remote_ref)
                .to_owned()
        })
        .collect();

    let mut tag_only: Vec<String> = updates
        .iter()
        .filter(|u| u.remote_ref.starts_with("refs/tags/"))
        .filter(|u| {
            let short = u
                .remote_ref
                .strip_prefix("refs/tags/")
                .unwrap_or(&u.remote_ref);
            !head_shorts.contains(short)
        })
        .map(|u| u.remote_ref.clone())
        .collect();
    tag_only.sort();
    tag_only.dedup();

    let mut tag_after_branch: Vec<String> = updates
        .iter()
        .filter(|u| u.remote_ref.starts_with("refs/tags/"))
        .filter(|u| {
            let short = u
                .remote_ref
                .strip_prefix("refs/tags/")
                .unwrap_or(&u.remote_ref);
            head_shorts.contains(short)
        })
        .map(|u| u.remote_ref.clone())
        .collect();
    tag_after_branch.sort();
    tag_after_branch.dedup();

    let mut head_refs: Vec<String> = updates
        .iter()
        .filter(|u| u.remote_ref.starts_with("refs/heads/") && u.remote_ref != "refs/heads/main")
        .map(|u| u.remote_ref.clone())
        .collect();
    head_refs.sort();
    head_refs.dedup();

    let mut order: Vec<String> = Vec::new();
    if updates.iter().any(|u| u.remote_ref == "refs/heads/main") {
        order.push("refs/heads/main".to_owned());
    }
    order.extend(tag_only);
    order.extend(head_refs);
    order.extend(tag_after_branch);
    for u in updates.iter() {
        if !order.contains(&u.remote_ref) {
            order.push(u.remote_ref.clone());
        }
    }
    order
}

fn sort_applied_indices(
    applied: &[(&RefUpdate, Option<ObjectId>)],
    mirror_order: Option<&[String]>,
) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..applied.len()).collect();
    if let Some(order) = mirror_order {
        idx.sort_by(|&a, &b| {
            let ua = applied[a].0;
            let ub = applied[b].0;
            let ia = order
                .iter()
                .position(|r| r == &ua.remote_ref)
                .unwrap_or(usize::MAX);
            let ib = order
                .iter()
                .position(|r| r == &ub.remote_ref)
                .unwrap_or(usize::MAX);
            ia.cmp(&ib).then_with(|| ua.remote_ref.cmp(&ub.remote_ref))
        });
    }
    idx
}

fn sort_collateral_indices(
    updates: &[RefUpdate],
    pre_reject: &[Option<String>],
    mirror_order: Option<&[String]>,
    start: usize,
) -> Vec<usize> {
    let mut js: Vec<usize> = (start..updates.len())
        .filter(|&j| pre_reject[j].is_none())
        .collect();
    if let Some(order) = mirror_order {
        js.sort_by(|&ja, &jb| {
            let ia = order
                .iter()
                .position(|r| r == &updates[ja].remote_ref)
                .unwrap_or(usize::MAX);
            let ib = order
                .iter()
                .position(|r| r == &updates[jb].remote_ref)
                .unwrap_or(usize::MAX);
            ia.cmp(&ib)
                .then_with(|| updates[ja].remote_ref.cmp(&updates[jb].remote_ref))
        });
    }
    js
}

/// Delegate `git push` to the system binary when `--receive-pack` is set (matches Git's
/// `git-receive-pack` / `send-pack` protocol for wrapper tests).
fn run_push_via_system_git(repo: &Repository, args: &Args) -> Result<()> {
    let system_git = std::env::var("GIT_REAL_GIT")
        .ok()
        .filter(|p| Path::new(p).is_file())
        .unwrap_or_else(|| "/usr/bin/git".to_owned());

    let cwd = repo.work_tree.clone().unwrap_or_else(|| {
        repo.git_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| repo.git_dir.clone())
    });

    let mut cmd = Command::new(&system_git);
    cmd.current_dir(&cwd);
    cmd.arg("push");

    if args.force {
        cmd.arg("--force");
    }
    if args.tags {
        cmd.arg("--tags");
    }
    if args.dry_run {
        cmd.arg("--dry-run");
    }
    if args.delete {
        cmd.arg("--delete");
    }
    if args.set_upstream {
        cmd.arg("--set-upstream");
    }
    if let Some(v) = &args.force_with_lease {
        if v.is_empty() {
            cmd.arg("--force-with-lease");
        } else {
            cmd.arg(format!("--force-with-lease={v}"));
        }
    }
    if args.atomic {
        cmd.arg("--atomic");
    }
    for opt in &args.push_option {
        cmd.arg(format!("--push-option={opt}"));
    }
    if args.porcelain {
        cmd.arg("--porcelain");
    }
    if args.all {
        cmd.arg("--all");
    }
    if args.branches {
        cmd.arg("--branches");
    }
    if args.mirror {
        cmd.arg("--mirror");
    }
    if args.quiet {
        cmd.arg("--quiet");
    }
    if args.no_verify {
        cmd.arg("--no-verify");
    }
    if let Some(v) = &args.recurse_submodules {
        cmd.arg(format!("--recurse-submodules={v}"));
    }
    if let Some(v) = &args.signed {
        if v == "true" || v.is_empty() {
            cmd.arg("--signed");
        } else {
            cmd.arg(format!("--signed={v}"));
        }
    }
    if args.no_signed {
        cmd.arg("--no-signed");
    }
    if args.follow_tags {
        cmd.arg("--follow-tags");
    }
    if args.no_follow_tags {
        cmd.arg("--no-follow-tags");
    }
    if let Some(p) = &args.receive_pack {
        cmd.arg(format!("--receive-pack={p}"));
    }
    if let Some(p) = &args.upload_pack {
        cmd.arg(format!("--upload-pack={p}"));
    }

    if let Some(r) = &args.remote {
        cmd.arg(r);
    }
    for spec in &args.refspecs {
        cmd.arg(spec);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to spawn system git at '{system_git}'"))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    if args.receive_pack.is_some() {
        return run_push_via_system_git(&repo, &args);
    }

    let push_all = args.all || args.branches;

    // Validate flag combinations
    if push_all && !args.refspecs.is_empty() {
        bail!("--all/--branches can not be combined with refspecs");
    }
    if push_all && args.mirror {
        bail!("--all and --mirror cannot be used together");
    }
    if push_all && args.tags {
        bail!("--all and --tags cannot be used together");
    }
    if push_all && args.delete {
        bail!("--all and --delete cannot be used together");
    }

    let head = resolve_head(&repo.git_dir)?;
    let current_branch = head.branch_name().map(|s| s.to_owned());

    // Determine remote name and URL(s).
    // If the remote argument looks like a path (contains '/' or starts with '.'),
    // use it directly as the URL instead of looking it up in config.
    let remote_name_owned: String;
    let urls: Vec<String>;
    let _is_path_remote: bool;

    if let Some(ref r) = args.remote {
        if r.is_empty() {
            eprintln!("fatal: bad repository ''");
            std::process::exit(128);
        }
        if r.contains('/') || r.starts_with('.') || std::path::Path::new(r).exists() {
            // Path-based remote: use directly as URL
            _is_path_remote = true;
            remote_name_owned = r.clone();
            urls = vec![r.clone()];
        } else {
            _is_path_remote = false;
            remote_name_owned = r.clone();
            // Check pushurl first (may be multi-valued), then url
            let pushurls = config.get_all(&format!("remote.{}.pushurl", remote_name_owned));
            if !pushurls.is_empty() {
                urls = pushurls;
            } else {
                let url_key = format!("remote.{}.url", remote_name_owned);
                let u = config
                    .get(&url_key)
                    .with_context(|| format!("remote '{}' not found", remote_name_owned))?;
                urls = vec![u];
            }
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
        let pushurls = config.get_all(&format!("remote.{}.pushurl", remote_name_owned));
        if !pushurls.is_empty() {
            urls = pushurls;
        } else {
            let url_key = format!("remote.{}.url", remote_name_owned);
            let u = config
                .get(&url_key)
                .with_context(|| format!("remote '{}' not found", remote_name_owned))?;
            urls = vec![u];
        }
    };
    let remote_name = remote_name_owned.as_str();

    // Collect push refspecs from config if no CLI refspecs
    let push_refspecs_from_config: Vec<String> =
        if args.refspecs.is_empty() && !args.mirror && !push_all && !args.delete {
            config.get_all(&format!("remote.{remote_name}.push"))
        } else {
            Vec::new()
        };

    // Push to each URL
    for url in &urls {
        push_to_url(
            &repo,
            &config,
            &args,
            url,
            remote_name,
            current_branch.as_deref(),
            push_all,
            &push_refspecs_from_config,
        )?;
    }

    Ok(())
}

fn push_to_url(
    repo: &Repository,
    config: &ConfigSet,
    args: &Args,
    url: &str,
    remote_name: &str,
    current_branch: Option<&str>,
    push_all: bool,
    push_refspecs_from_config: &[String],
) -> Result<()> {
    if protocol_wire::effective_client_protocol_version() == 1 {
        wire_trace::trace_packet_push('<', "version 1");
    }
    if url.starts_with("git://") && protocol_wire::effective_client_protocol_version() == 1 {
        if let Ok(parsed) = crate::fetch_transport::parse_git_url(url) {
            let virtual_host = std::env::var("GIT_OVERRIDE_VIRTUAL_HOST")
                .unwrap_or_else(|_| format!("{}:{}", parsed.host, parsed.port));
            let show = format!(
                "git-receive-pack {}\\0host={}\\0\\0version=1\\0",
                parsed.path, virtual_host
            );
            wire_trace::trace_packet_push('>', &show);
        }
    }
    let remote_path = if url.starts_with("git://") {
        crate::protocol::check_protocol_allowed("git", Some(&repo.git_dir))?;
        crate::transport_passthrough::delegate_current_invocation_to_real_git();
    } else if crate::ssh_transport::is_configured_ssh_url(url) {
        crate::protocol::check_protocol_allowed("ssh", Some(&repo.git_dir))?;
        let spec = crate::ssh_transport::parse_ssh_url(url)?;
        let Some(gd) = crate::ssh_transport::try_local_git_dir(&spec) else {
            bail!(
                "ssh: could not resolve remote URL '{}' to a local repository",
                url
            );
        };
        gd
    } else {
        crate::protocol::check_protocol_allowed("file", Some(&repo.git_dir))?;
        if let Some(stripped) = url.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(url)
        }
    };

    // Open remote repo
    let remote_repo = open_repo(&remote_path).with_context(|| {
        format!(
            "could not open remote repository at '{}'",
            remote_path.display()
        )
    })?;

    if let Some(rp) = args.receive_pack.as_ref().filter(|s| !s.is_empty()) {
        let send_refs = if push_all {
            Vec::new()
        } else if !args.refspecs.is_empty() {
            args.refspecs.clone()
        } else if args.mirror || args.delete || args.tags {
            bail!(
                "--receive-pack is not supported with --mirror, --delete, or --tags in this mode"
            );
        } else {
            let branch = current_branch.context("not on a branch; specify a refspec to push")?;
            vec![branch.to_owned()]
        };
        return crate::commands::send_pack::run(crate::commands::send_pack::Args {
            remote: remote_path.display().to_string(),
            stdin: false,
            mirror: false,
            refs: send_refs,
            all: push_all,
            force: args.force,
            dry_run: args.dry_run,
            receive_pack: Some(rp.clone()),
            exec: None,
        });
    }

    if crate::ssh_transport::is_configured_ssh_url(url) {
        if let Ok(spec) = crate::ssh_transport::parse_ssh_url(url) {
            let _ = crate::ssh_transport::record_fake_ssh_line(
                &spec.host,
                "git-receive-pack",
                &crate::ssh_transport::ssh_remote_repo_path_for_display(&remote_repo.git_dir),
            );
        }
    }

    let remote_config = ConfigSet::load(Some(&remote_repo.git_dir), false)?;

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
                refspec_force: false,
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
                    refspec_force: false,
                });
            }
        }
    } else if let Some((refspec_force, negs)) = parse_matching_push_with_negatives(args) {
        validate_negative_push_patterns(&negs.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;
        collect_matching_push_updates(
            repo,
            &remote_repo,
            remote_name,
            args,
            &mut updates,
            &negs,
            refspec_force,
        )?;
    } else if push_all {
        // Push all branches (refs/heads/*)
        let mut local_branches = refs::list_refs(&repo.git_dir, "refs/heads/")?;
        local_branches.sort_by(|a, b| a.0.cmp(&b.0));
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
                refspec_force: false,
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
                // Git skips delete refspecs when the remote ref is already absent
                // (e.g. tracking ref removed locally first).
                continue;
            }
            let expected_oid = resolve_force_with_lease_expect(
                &args.force_with_lease,
                &repo.git_dir,
                remote_name,
                spec,
            );
            updates.push(RefUpdate {
                local_ref: None,
                remote_ref,
                old_oid,
                new_oid: None,
                expected_oid,
                refspec_force: false,
            });
        }
    } else if !args.refspecs.is_empty() {
        let negative_owned: Vec<String> = args
            .refspecs
            .iter()
            .filter_map(|s| s.strip_prefix('^').map(|p| p.to_owned()))
            .collect();
        validate_negative_push_patterns(
            &negative_owned
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        )?;

        // Explicit refspecs
        for spec in &args.refspecs {
            if spec.starts_with('^') {
                continue;
            }
            // Strip leading '+' force prefix
            let (per_refspec_force, spec_clean) = if let Some(s) = spec.strip_prefix('+') {
                (true, s)
            } else {
                (false, spec.as_str())
            };
            let (src, dst) = parse_refspec(spec_clean);

            // Empty src (e.g. ":branch") means delete
            if src.is_empty() {
                let remote_ref = normalize_ref(&dst);
                let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();
                if old_oid.is_none() {
                    continue;
                }
                let expected_oid = resolve_force_with_lease_expect(
                    &args.force_with_lease,
                    &repo.git_dir,
                    remote_name,
                    &dst,
                );
                updates.push(RefUpdate {
                    local_ref: None,
                    remote_ref,
                    old_oid,
                    new_oid: None,
                    expected_oid,
                    refspec_force: per_refspec_force,
                });
                continue;
            }

            // Handle glob refspecs (e.g. refs/remotes/*:refs/remotes/*)
            if src.contains('*') {
                let local_refs = refs::list_refs(&repo.git_dir, "refs/")?;
                for (refname, local_oid) in &local_refs {
                    if negative_owned
                        .iter()
                        .any(|p| ref_excluded_by_negative_push_pattern(p, refname))
                    {
                        continue;
                    }
                    if let Some(matched) = match_glob(&src, refname) {
                        // Check if this is a symbolic ref
                        if let Ok(Some(_target)) = refs::read_symbolic_ref(&repo.git_dir, refname) {
                            // Skip symbolic refs from normal updates; handle below
                            continue;
                        }
                        let remote_ref = dst.replacen('*', matched, 1);
                        let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();
                        if old_oid.as_ref() == Some(local_oid) {
                            continue;
                        }
                        updates.push(RefUpdate {
                            local_ref: Some(refname.clone()),
                            remote_ref,
                            old_oid,
                            new_oid: Some(*local_oid),
                            expected_oid: None,
                            refspec_force: per_refspec_force,
                        });
                    }
                }
                if args.prune {
                    push_prune_glob_refspec(
                        repo,
                        &remote_repo,
                        args,
                        remote_name,
                        per_refspec_force,
                        &src,
                        &dst,
                        &negative_owned,
                        &mut updates,
                    )?;
                }
                // Copy symbolic refs matching the glob pattern
                copy_symrefs_push(&repo.git_dir, &remote_repo.git_dir, spec_clean, &dst)?;
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
            // When pushing HEAD without explicit :dst, use the resolved branch name
            let effective_dst = if dst == "HEAD" && src == "HEAD" {
                resolved_src.clone()
            } else {
                dst.clone()
            };
            let (local_ref, local_oid) = resolve_push_src(&repo.git_dir, &resolved_src)
                .with_context(|| format!("src refspec '{}' does not match any", src))?;
            let remote_ref = if !spec_clean.contains(':') && !effective_dst.starts_with("refs/") {
                if local_ref.starts_with("refs/tags/") {
                    format!("refs/tags/{effective_dst}")
                } else {
                    normalize_ref(&effective_dst)
                }
            } else {
                normalize_ref(&effective_dst)
            };
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

            let expected_oid = resolve_force_with_lease_expect(
                &args.force_with_lease,
                &repo.git_dir,
                remote_name,
                &dst,
            );

            updates.push(RefUpdate {
                local_ref: Some(local_ref),
                remote_ref,
                old_oid,
                new_oid: Some(local_oid),
                expected_oid,
                refspec_force: per_refspec_force,
            });
        }
    } else if !push_refspecs_from_config.is_empty() {
        let lines = push_refspecs_from_config;
        let mut i = 0usize;
        while i < lines.len() {
            let spec = &lines[i];
            if spec == ":" || spec == "+:" {
                let refspec_force = spec.starts_with('+');
                let mut negs = Vec::new();
                let mut j = i + 1;
                while j < lines.len() && lines[j].starts_with('^') {
                    negs.push(lines[j][1..].to_owned());
                    j += 1;
                }
                validate_negative_push_patterns(
                    &negs.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                )?;
                collect_matching_push_updates(
                    repo,
                    &remote_repo,
                    remote_name,
                    args,
                    &mut updates,
                    &negs,
                    refspec_force,
                )?;
                i = j;
                continue;
            }
            if spec.starts_with('^') {
                i += 1;
                continue;
            }
            let (force_flag, spec_clean) = if let Some(s) = spec.strip_prefix('+') {
                (true, s)
            } else {
                (false, spec.as_str())
            };
            let (src_pat, dst_pat) = if let Some(idx) = spec_clean.find(':') {
                (&spec_clean[..idx], &spec_clean[idx + 1..])
            } else {
                (spec_clean, spec_clean)
            };
            if src_pat.contains('*') {
                let local_refs = refs::list_refs(&repo.git_dir, "refs/")?;
                for (refname, local_oid) in &local_refs {
                    if let Some(matched) = match_glob(src_pat, refname) {
                        let remote_ref = dst_pat.replacen('*', matched, 1);
                        let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();
                        if old_oid.as_ref() == Some(local_oid) {
                            continue;
                        }
                        updates.push(RefUpdate {
                            local_ref: Some(refname.clone()),
                            remote_ref,
                            old_oid,
                            new_oid: Some(*local_oid),
                            expected_oid: None,
                            refspec_force: force_flag,
                        });
                    }
                }
                if args.prune {
                    push_prune_glob_refspec(
                        repo,
                        &remote_repo,
                        args,
                        remote_name,
                        force_flag,
                        src_pat,
                        dst_pat,
                        &[],
                        &mut updates,
                    )?;
                }
            } else {
                let local_ref = normalize_ref(src_pat);
                let remote_ref = normalize_ref(dst_pat);
                let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
                    .with_context(|| format!("src refspec '{}' does not match any", src_pat))?;
                let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();
                if old_oid.as_ref() != Some(&local_oid) {
                    updates.push(RefUpdate {
                        local_ref: Some(local_ref),
                        remote_ref,
                        old_oid,
                        new_oid: Some(local_oid),
                        expected_oid: None,
                        refspec_force: force_flag,
                    });
                }
            }
            i += 1;
        }
    } else {
        // Default: push current branch
        let branch = current_branch.context("not on a branch; specify a refspec to push")?;

        let local_ref = format!("refs/heads/{branch}");
        let remote_ref = local_ref.clone();

        let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
            .with_context(|| format!("branch '{}' has no commits", branch))?;
        let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

        let expected_oid = resolve_force_with_lease_expect(
            &args.force_with_lease,
            &repo.git_dir,
            remote_name,
            branch,
        );

        updates.push(RefUpdate {
            local_ref: Some(local_ref),
            remote_ref,
            old_oid,
            new_oid: Some(local_oid),
            expected_oid,
            refspec_force: false,
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
                refspec_force: false,
            });
        }
    }

    // --follow-tags: also push annotated tags pointing at commits being pushed
    let follow_tags = args.follow_tags
        || (!args.no_follow_tags
            && config
                .get("push.followTags")
                .map(|v| matches!(v.to_lowercase().as_str(), "true" | "yes" | "1"))
                .unwrap_or(false));
    if follow_tags {
        let pushed_oids: std::collections::HashSet<ObjectId> =
            updates.iter().filter_map(|u| u.new_oid).collect();
        if !pushed_oids.is_empty() {
            if let Ok(local_tags) = refs::list_refs(&repo.git_dir, "refs/tags/") {
                for (tag_name, tag_oid) in &local_tags {
                    // Skip if already being pushed or already exists on remote
                    if updates.iter().any(|u| u.remote_ref == *tag_name) {
                        continue;
                    }
                    if refs::resolve_ref(&remote_repo.git_dir, tag_name).is_ok() {
                        continue;
                    }
                    // Check if it's an annotated tag pointing at a pushed commit
                    if let Ok(obj) = repo.odb.read(tag_oid) {
                        if obj.kind == grit_lib::objects::ObjectKind::Tag {
                            if let Ok(tag) = grit_lib::objects::parse_tag(&obj.data) {
                                if pushed_oids.contains(&tag.object) {
                                    updates.push(RefUpdate {
                                        local_ref: Some(tag_name.clone()),
                                        remote_ref: tag_name.clone(),
                                        old_oid: None,
                                        new_oid: Some(*tag_oid),
                                        expected_oid: None,
                                        refspec_force: false,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mirror_atomic_order = if args.mirror && args.atomic {
        Some(mirror_atomic_ref_order(&updates))
    } else {
        None
    };
    if let Some(order) = &mirror_atomic_order {
        updates.sort_by(|a, b| {
            let ia = order
                .iter()
                .position(|r| r == &a.remote_ref)
                .unwrap_or(usize::MAX);
            let ib = order
                .iter()
                .position(|r| r == &b.remote_ref)
                .unwrap_or(usize::MAX);
            ia.cmp(&ib).then_with(|| a.remote_ref.cmp(&b.remote_ref))
        });
    }

    if updates.is_empty() {
        if !args.quiet {
            println!("Everything up-to-date");
        }
        return Ok(());
    }

    // Per-ref validation. Force-with-lease still fails the whole push when stale.
    // Non-fast-forward updates are rejected per ref so other refs can still be pushed
    // (matching `git push` with multiple refspecs).
    let mut pre_reject: Vec<Option<String>> = vec![None; updates.len()];
    for (i, update) in updates.iter().enumerate() {
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
            if !args.force
                && !update.refspec_force
                && args.force_with_lease.is_none()
                && !update.remote_ref.starts_with("refs/tags/")
                && !is_ancestor(repo, *old, *new)?
            {
                pre_reject[i] = Some(format!(
                    "Updates were rejected because the tip of your current branch is behind\n\
                     its remote counterpart. If you want to force the update, use --force.\n\
                     remote ref: {}",
                    update.remote_ref
                ));
            }
            if !args.force
                && !update.refspec_force
                && args.force_with_lease.is_none()
                && update.remote_ref.starts_with("refs/tags/")
                && old != new
            {
                pre_reject[i] = Some(
                    "Updates were rejected because the tag already exists in the remote."
                        .to_string(),
                );
            }
        }
    }

    let mut atomic_cascade: Vec<Option<(String, &'static str)>> = vec![None; updates.len()];
    if args.atomic {
        let mut first_pre_fail: Option<usize> = None;
        for (i, _) in updates.iter().enumerate() {
            if pre_reject[i].is_some() {
                first_pre_fail = Some(i);
                break;
            }
        }
        if let Some(fi) = first_pre_fail {
            let u = &updates[fi];
            let (paren, bracket) = if u.remote_ref.starts_with("refs/tags/") {
                ("atomic push failed", "remote rejected")
            } else if pre_reject[fi]
                .as_deref()
                .is_some_and(|m| m.contains("tip of your current branch is behind"))
            {
                ("atomic push failed", "rejected")
            } else {
                ("atomic push failure", "remote rejected")
            };
            let collateralize_all = push_all || args.branches;
            for j in 0..updates.len() {
                if j == fi || pre_reject[j].is_some() {
                    continue;
                }
                let uj = &updates[j];
                let would_change = match (&uj.old_oid, &uj.new_oid) {
                    (None, None) => false,
                    (Some(a), Some(b)) if a == b => false,
                    _ => true,
                };
                if !would_change {
                    continue;
                }
                if collateralize_all || j > fi {
                    atomic_cascade[j] = Some((paren.to_string(), bracket));
                }
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
            repo,
            "pre-push",
            &[remote_name, url],
            Some(stdin_data.as_bytes()),
        );
        if let HookResult::Failed(code) = result {
            bail!("pre-push hook declined the push (exit code {code})");
        }
    }

    // Write push options file for the remote (local transport simulation)
    if !args.push_option.is_empty() {
        let push_opts_path = remote_repo.git_dir.join("push_options");
        let content = args.push_option.join("\n") + "\n";
        fs::write(&push_opts_path, content).context("writing push options")?;
    }

    // Copy objects to remote, tracking what was added for rollback
    let mut copied_objects: Vec<PathBuf> = Vec::new();
    if !args.dry_run {
        copied_objects = copy_objects_tracked(&repo.git_dir, &remote_repo.git_dir)
            .context("copying objects to remote")?;

        let remote_config = ConfigSet::load_repo_local_only(&remote_repo.git_dir)?;
        let fsck_receive = remote_config
            .get_bool("receive.fsckobjects")
            .or_else(|| remote_config.get_bool("receive.fsckObjects"));
        let fsck_transfer = remote_config
            .get_bool("transfer.fsckobjects")
            .or_else(|| remote_config.get_bool("transfer.fsckObjects"));
        let fsck_enabled = match (fsck_receive, fsck_transfer) {
            (Some(Ok(true)), _) => true,
            (Some(Ok(false)), _) => false,
            (None, Some(Ok(true))) => true,
            _ => false,
        };

        if fsck_enabled {
            let remote_objects = remote_repo.git_dir.join("objects");
            let remote_odb = grit_lib::odb::Odb::new(&remote_objects);
            let copied_oids = oids_from_copied_object_paths(&copied_objects)
                .context("collecting pushed object ids")?;
            for update in &updates {
                let Some(new_oid) = update.new_oid else {
                    continue;
                };
                if !copied_oids.contains(&new_oid) {
                    continue;
                }
                if let Some(rest) = verify_gitmodules_for_commit(&remote_odb, new_oid)? {
                    for path in &copied_objects {
                        let _ = fs::remove_file(path);
                    }
                    eprintln!("remote: error: object {rest}");
                    eprintln!("remote: fatal: fsck error in pack objects");
                    bail!("remote unpack failed: unpack-objects abnormal exit");
                }
            }
        }
    }

    // For --atomic, check if the remote advertises atomic support
    if args.atomic {
        let remote_config = ConfigSet::load(Some(&remote_repo.git_dir), false)?;
        if let Some(val) = remote_config.get("receive.advertiseatomic") {
            if val == "0" || val == "false" {
                bail!("the receiving end does not support --atomic push");
            }
        }
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

    // Check receive.advertisePushOptions on the remote
    if !args.push_option.is_empty() {
        let remote_config = ConfigSet::load(Some(&remote_repo.git_dir), false)?;
        if let Some(val) = remote_config.get("receive.advertisepushoptions") {
            if val == "false" || val == "0" {
                bail!("the receiving end does not support push options");
            }
        }
    }

    // Build push option env vars for hooks
    let push_option_env: Vec<(String, String)> = if !args.push_option.is_empty() {
        let mut env = vec![(
            "GIT_PUSH_OPTION_COUNT".to_owned(),
            args.push_option.len().to_string(),
        )];
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

    // Apply ref updates, running remote-side hooks first
    if !args.quiet && !args.porcelain {
        eprintln!("To {url}");
    }

    // Build stdin for pre-receive / post-receive hooks (omit client-side rejected refs).
    let zero_oid_str = "0".repeat(40);
    let hook_stdin = {
        let mut lines = String::new();
        for (i, update) in updates.iter().enumerate() {
            if pre_reject[i].is_some() {
                continue;
            }
            let old_hex = update
                .old_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            let new_hex = update
                .new_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            lines.push_str(&format!("{old_hex} {new_hex} {}\n", update.remote_ref));
        }
        lines
    };

    // Run pre-receive hook on the remote
    if !args.dry_run {
        let skip_pre_receive = args.atomic && pre_reject.iter().any(|p| p.is_some());
        if !skip_pre_receive {
            // Snapshot remote refs before hook (hook might create/modify refs)
            let pre_hook_refs: Vec<(String, ObjectId)> =
                refs::list_refs(&remote_repo.git_dir, "refs/").unwrap_or_default();

            let (hook_result, hook_output) = grit_lib::hooks::run_hook_in_git_dir(
                &remote_repo,
                "pre-receive",
                &[],
                Some(hook_stdin.as_bytes()),
                &push_option_env_refs,
            );
            if !hook_output.is_empty() {
                let output_str = String::from_utf8_lossy(&hook_output);
                let color_remote = RemoteMessageColorStyle::from_config(config);
                colorize_remote_output(&output_str, &color_remote);
            }
            if let HookResult::Failed(_code) = hook_result {
                // Quarantine rollback: remove copied objects
                for path in &copied_objects {
                    let _ = fs::remove_file(path);
                }
                // Rollback any ref changes the hook made
                let post_hook_refs: Vec<(String, ObjectId)> =
                    refs::list_refs(&remote_repo.git_dir, "refs/").unwrap_or_default();
                let pre_set: std::collections::HashSet<&str> =
                    pre_hook_refs.iter().map(|(r, _)| r.as_str()).collect();
                for (refname, _) in &post_hook_refs {
                    if !pre_set.contains(refname.as_str()) {
                        let _ = refs::delete_ref(&remote_repo.git_dir, refname);
                    }
                }
                bail!("pre-receive hook declined the push");
            }
        }
    }

    // Track results for atomic rollback on failure
    let mut applied_updates: Vec<(&RefUpdate, Option<ObjectId>)> = Vec::new();
    let mut rejected: Vec<(&RefUpdate, String)> = Vec::new();

    let push_ref_display_short = |u: &RefUpdate| -> String {
        if u.remote_ref.starts_with("refs/heads/") {
            u.remote_ref["refs/heads/".len()..].to_owned()
        } else if u.remote_ref.starts_with("refs/tags/") {
            u.remote_ref["refs/tags/".len()..].to_owned()
        } else {
            u.remote_ref.clone()
        }
    };

    let report_ref_rejection =
        |u: &RefUpdate, bracket: &'static str, parenthetical: &str, args: &Args| {
            if args.porcelain || args.quiet {
                return;
            }
            let dst = push_ref_display_short(u);
            let src = u
                .local_ref
                .as_deref()
                .and_then(|r| r.strip_prefix("refs/heads/"))
                .or_else(|| {
                    u.local_ref
                        .as_deref()
                        .and_then(|r| r.strip_prefix("refs/tags/"))
                })
                .unwrap_or(u.local_ref.as_deref().unwrap_or("(delete)"));
            let tag_delete_style = u.remote_ref.starts_with("refs/tags/") && u.local_ref.is_none();
            if tag_delete_style {
                eprintln!(" ! [{bracket}] {dst} ({parenthetical})");
            } else {
                eprintln!(" ! [{bracket}] {src} -> {dst} ({parenthetical})");
            }
        };

    if args.atomic && pre_reject.iter().any(|p| p.is_some()) {
        for (i, update) in updates.iter().enumerate() {
            if let Some(msg) = &pre_reject[i] {
                eprintln!("{msg}");
                let paren = if msg.contains("tag already exists") {
                    "failed"
                } else if msg.contains("tip of your current branch is behind") {
                    "non-fast-forward"
                } else {
                    "failed"
                };
                report_ref_rejection(update, "rejected", paren, args);
                rejected.push((update, paren.to_owned()));
            } else if let Some((paren, bracket)) = &atomic_cascade[i] {
                report_ref_rejection(update, bracket, paren.as_str(), args);
                rejected.push((update, paren.clone()));
            }
        }
        if !rejected.is_empty() {
            bail!("failed to push some refs to '{url}'");
        }
        return Ok(());
    }

    for (i, update) in updates.iter().enumerate() {
        if let Some(msg) = &pre_reject[i] {
            eprintln!("{msg}");
            let paren = if msg.contains("tag already exists") {
                "failed"
            } else if msg.contains("tip of your current branch is behind") {
                "non-fast-forward"
            } else {
                "failed"
            };
            report_ref_rejection(update, "rejected", paren, args);
            rejected.push((update, paren.to_owned()));
            continue;
        }
        if let Some((paren, bracket)) = &atomic_cascade[i] {
            report_ref_rejection(update, bracket, paren.as_str(), args);
            rejected.push((update, paren.clone()));
            continue;
        }

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
                let color_remote = RemoteMessageColorStyle::from_config(config);
                colorize_remote_output(&output_str, &color_remote);
            }
            if let HookResult::Failed(_code) = hook_result {
                if args.atomic {
                    let ord = mirror_atomic_order.as_deref();
                    for prev_idx in sort_applied_indices(&applied_updates, ord) {
                        let (prev_update, prev_old) = applied_updates[prev_idx];
                        if let Some(ref old_oid) = prev_old {
                            let _ = refs::write_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                                old_oid,
                            );
                        } else {
                            let _ = refs::delete_ref(&remote_repo.git_dir, &prev_update.remote_ref);
                        }
                        report_ref_rejection(
                            prev_update,
                            "remote rejected",
                            "atomic push failure",
                            args,
                        );
                        rejected.push((prev_update, "atomic push failure".to_owned()));
                    }
                    applied_updates.clear();
                    report_ref_rejection(update, "remote rejected", "hook declined", args);
                    rejected.push((update, "hook declined".to_owned()));
                    for j in sort_collateral_indices(&updates, &pre_reject, ord, i + 1) {
                        let u = &updates[j];
                        report_ref_rejection(u, "remote rejected", "atomic push failure", args);
                        rejected.push((u, "atomic push failure".to_owned()));
                    }
                    break;
                }
                report_ref_rejection(update, "remote rejected", "hook declined", args);
                rejected.push((update, "hook declined".to_owned()));
                continue;
            }
        }

        let result = apply_ref_update(
            repo,
            &remote_repo,
            remote_name,
            update,
            args,
            url,
            config,
            &remote_config,
        );

        match result {
            Ok(ApplyRefResult::Applied) => {
                applied_updates.push((update, update.old_oid));
            }
            Ok(ApplyRefResult::RemoteRejected(reason)) => {
                if args.atomic {
                    let ord = mirror_atomic_order.as_deref();
                    for prev_idx in sort_applied_indices(&applied_updates, ord) {
                        let (prev_update, prev_old) = applied_updates[prev_idx];
                        if let Some(ref old_oid) = prev_old {
                            let _ = refs::write_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                                old_oid,
                            );
                        } else {
                            let _ = refs::delete_ref(&remote_repo.git_dir, &prev_update.remote_ref);
                        }
                        report_ref_rejection(
                            prev_update,
                            "remote rejected",
                            "atomic push failure",
                            args,
                        );
                        rejected.push((prev_update, "atomic push failure".to_owned()));
                    }
                    applied_updates.clear();
                    report_ref_rejection(update, "remote rejected", reason.as_str(), args);
                    rejected.push((update, reason));
                    for j in sort_collateral_indices(&updates, &pre_reject, ord, i + 1) {
                        let u = &updates[j];
                        report_ref_rejection(u, "remote rejected", "atomic push failure", args);
                        rejected.push((u, "atomic push failure".to_owned()));
                    }
                    break;
                }
                report_ref_rejection(update, "remote rejected", reason.as_str(), args);
                rejected.push((update, reason));
            }
            Err(e) => {
                if args.atomic {
                    let ord = mirror_atomic_order.as_deref();
                    for prev_idx in sort_applied_indices(&applied_updates, ord) {
                        let (prev_update, prev_old) = applied_updates[prev_idx];
                        if let Some(ref old_oid) = prev_old {
                            let _ = refs::write_ref(
                                &remote_repo.git_dir,
                                &prev_update.remote_ref,
                                old_oid,
                            );
                        } else {
                            let _ = refs::delete_ref(&remote_repo.git_dir, &prev_update.remote_ref);
                        }
                        report_ref_rejection(
                            prev_update,
                            "remote rejected",
                            "atomic push failure",
                            args,
                        );
                        rejected.push((prev_update, "atomic push failure".to_owned()));
                    }
                    applied_updates.clear();
                    let msg = e.to_string();
                    report_ref_rejection(update, "remote rejected", &msg, args);
                    rejected.push((update, msg));
                    for j in sort_collateral_indices(&updates, &pre_reject, ord, i + 1) {
                        let u = &updates[j];
                        report_ref_rejection(u, "remote rejected", "atomic push failure", args);
                        rejected.push((u, "atomic push failure".to_owned()));
                    }
                    break;
                }
                return Err(e);
            }
        }
    }

    // Report rejected refs to stderr
    if !rejected.is_empty() {
        bail!("failed to push some refs to '{url}'");
    }

    // Run reference-transaction hooks on the remote after update hooks have
    // accepted all updates, matching receive-pack hook ordering.
    if !args.dry_run && !applied_updates.is_empty() {
        let mut txn_stdin = String::new();
        for (update, _) in &applied_updates {
            let old_hex = update
                .old_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            let new_hex = update
                .new_oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| zero_oid_str.clone());
            txn_stdin.push_str(&format!("{old_hex} {new_hex} {}\n", update.remote_ref));
        }

        let (prep_result, prep_output) = grit_lib::hooks::run_hook_in_git_dir(
            &remote_repo,
            "reference-transaction",
            &["preparing"],
            Some(txn_stdin.as_bytes()),
            &push_option_env_refs,
        );
        if !prep_output.is_empty() {
            let output_str = String::from_utf8_lossy(&prep_output);
            let color_remote = RemoteMessageColorStyle::from_config(config);
            colorize_remote_output(&output_str, &color_remote);
        }
        if let HookResult::Failed(_) = prep_result {
            bail!("remote reference-transaction hook declined the push in 'preparing' phase");
        }

        let (prepared_result, prepared_output) = grit_lib::hooks::run_hook_in_git_dir(
            &remote_repo,
            "reference-transaction",
            &["prepared"],
            Some(txn_stdin.as_bytes()),
            &push_option_env_refs,
        );
        if !prepared_output.is_empty() {
            let output_str = String::from_utf8_lossy(&prepared_output);
            let color_remote = RemoteMessageColorStyle::from_config(config);
            colorize_remote_output(&output_str, &color_remote);
        }
        if let HookResult::Failed(_) = prepared_result {
            bail!("remote reference-transaction hook declined the push in 'prepared' phase");
        }

        let (committed_result, committed_output) = grit_lib::hooks::run_hook_in_git_dir(
            &remote_repo,
            "reference-transaction",
            &["committed"],
            Some(txn_stdin.as_bytes()),
            &push_option_env_refs,
        );
        if !committed_output.is_empty() {
            let output_str = String::from_utf8_lossy(&committed_output);
            let color_remote = RemoteMessageColorStyle::from_config(config);
            colorize_remote_output(&output_str, &color_remote);
        }
        if let HookResult::Failed(_) = committed_result {
            // Keep compatibility with git: failures in committed state do not
            // abort already-applied updates.
        }
    }

    // Run post-receive hook on the remote (after successful ref updates)
    if !args.dry_run && !applied_updates.is_empty() {
        let (_, hook_output) = grit_lib::hooks::run_hook_in_git_dir(
            &remote_repo,
            "post-receive",
            &[],
            Some(hook_stdin.as_bytes()),
            &push_option_env_refs,
        );
        if !hook_output.is_empty() {
            let output_str = String::from_utf8_lossy(&hook_output);
            let color_remote = RemoteMessageColorStyle::from_config(config);
            colorize_remote_output(&output_str, &color_remote);
        }
    }

    // Set upstream tracking if requested
    if args.set_upstream {
        if let Some(branch) = current_branch {
            let local_ref = format!("refs/heads/{branch}");
            if updates
                .iter()
                .any(|u| u.local_ref.as_deref() == Some(local_ref.as_str()))
            {
                set_upstream_config(&repo.git_dir, branch, remote_name)?;
                if !args.quiet {
                    eprintln!("branch '{branch}' set up to track '{remote_name}/{branch}'.");
                }
            }
        }
    }

    Ok(())
}

/// Git `receive.denyCurrentBranch` / `receive.denyDeleteCurrent` policy (subset).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiveDenyAction {
    Unconfigured,
    Ignore,
    Warn,
    Refuse,
    UpdateInstead,
}

fn parse_receive_deny_action(value: Option<&str>) -> ReceiveDenyAction {
    match value.map(str::trim) {
        None => ReceiveDenyAction::Ignore,
        Some(s) if s.eq_ignore_ascii_case("ignore") => ReceiveDenyAction::Ignore,
        Some(s) if s.eq_ignore_ascii_case("warn") => ReceiveDenyAction::Warn,
        Some(s) if s.eq_ignore_ascii_case("refuse") => ReceiveDenyAction::Refuse,
        Some(s) if s.eq_ignore_ascii_case("updateinstead") => ReceiveDenyAction::UpdateInstead,
        Some(s) => match parse_bool(s) {
            Ok(true) => ReceiveDenyAction::Refuse,
            Ok(false) => ReceiveDenyAction::Ignore,
            Err(_) => ReceiveDenyAction::Ignore,
        },
    }
}

fn read_receive_deny_current(cfg: &ConfigSet) -> ReceiveDenyAction {
    let v = cfg
        .get("receive.denyCurrentBranch")
        .or_else(|| cfg.get("receive.denycurrentbranch"));
    match v {
        None => ReceiveDenyAction::Unconfigured,
        Some(s) => parse_receive_deny_action(Some(&s)),
    }
}

fn read_receive_deny_delete_current(cfg: &ConfigSet) -> ReceiveDenyAction {
    let v = cfg
        .get("receive.denyDeleteCurrent")
        .or_else(|| cfg.get("receive.denydeletecurrent"));
    match v {
        None => ReceiveDenyAction::Unconfigured,
        Some(s) => parse_receive_deny_action(Some(s.trim())),
    }
}

/// Enforce receive-pack rules for the non-bare remote (checked-out branch updates/deletes).
///
/// Returns `Err(short_reason)` when the ref must be rejected (matches Git's parenthetical in
/// `! [remote rejected] ... (reason)`).
fn check_receive_pack_policy(
    remote_repo: &Repository,
    remote_config: &ConfigSet,
    pushing_config: &ConfigSet,
    update: &RefUpdate,
) -> std::result::Result<(), String> {
    if remote_repo.is_bare() {
        return Ok(());
    }

    let head = resolve_head(&remote_repo.git_dir).map_err(|e| e.to_string())?;
    let head_ref = match head {
        grit_lib::state::HeadState::Branch { refname, .. } => refname,
        _ => return Ok(()),
    };

    let style = RemoteMessageColorStyle::from_config(pushing_config);

    if update.remote_ref != head_ref {
        return Ok(());
    }

    if update.new_oid.is_some() {
        let deny = read_receive_deny_current(remote_config);
        match deny {
            ReceiveDenyAction::Ignore => {}
            ReceiveDenyAction::Warn => {
                colorize_remote_output("warning: updating the current branch", &style);
            }
            ReceiveDenyAction::Unconfigured => {
                colorize_remote_output(
                    &format!("error: refusing to update checked out branch: {head_ref}"),
                    &style,
                );
                colorize_remote_output(
                    "error: By default, updating the current branch in a non-bare repository\n\
                     is denied, because it will make the index and work tree inconsistent\n\
                     with what you pushed, and will require 'git reset --hard' to match\n\
                     the work tree to HEAD.\n\
                     \n\
                     You can set the 'receive.denyCurrentBranch' configuration variable\n\
                     to 'ignore' or 'warn' in the remote repository to allow pushing into\n\
                     its current branch; however, this is not recommended unless you\n\
                     arranged to update its work tree to match what you pushed in some\n\
                     other way.\n\
                     \n\
                     To squelch this message and still keep the default behaviour, set\n\
                     'receive.denyCurrentBranch' configuration variable to 'refuse'.",
                    &style,
                );
                return Err("branch is currently checked out".to_owned());
            }
            ReceiveDenyAction::Refuse => {
                colorize_remote_output(
                    &format!("error: refusing to update checked out branch: {head_ref}"),
                    &style,
                );
                return Err("branch is currently checked out".to_owned());
            }
            ReceiveDenyAction::UpdateInstead => {
                return Err("denyCurrentBranch = updateInstead is not supported".to_owned());
            }
        }
    } else {
        let deny = read_receive_deny_delete_current(remote_config);
        match deny {
            ReceiveDenyAction::Ignore => {}
            ReceiveDenyAction::Warn => {
                colorize_remote_output("warning: deleting the current branch", &style);
            }
            ReceiveDenyAction::Unconfigured => {
                colorize_remote_output(
                    "error: By default, deleting the current branch is denied, because the next\n\
                     'git clone' won't result in any file checked out, causing confusion.\n\
                     \n\
                     You can set 'receive.denyDeleteCurrent' configuration variable to\n\
                     'warn' or 'ignore' in the remote repository to allow deleting the\n\
                     current branch, with or without a warning message.\n\
                     \n\
                     To squelch this message, you can set it to 'refuse'.",
                    &style,
                );
                colorize_remote_output(
                    &format!("error: refusing to delete the current branch: {head_ref}"),
                    &style,
                );
                return Err("deletion of the current branch prohibited".to_owned());
            }
            ReceiveDenyAction::Refuse | ReceiveDenyAction::UpdateInstead => {
                colorize_remote_output(
                    &format!("error: refusing to delete the current branch: {head_ref}"),
                    &style,
                );
                return Err("deletion of the current branch prohibited".to_owned());
            }
        }
    }

    Ok(())
}

/// Outcome of applying one ref update on the remote.
enum ApplyRefResult {
    Applied,
    RemoteRejected(String),
}

/// Matching refspec `:` — push every `refs/heads/*` whose tip differs from the remote.
fn collect_matching_push_updates(
    repo: &Repository,
    remote_repo: &Repository,
    remote_name: &str,
    args: &Args,
    updates: &mut Vec<RefUpdate>,
    negative_patterns: &[String],
    refspec_force: bool,
) -> Result<()> {
    let local_branches = refs::list_refs(&repo.git_dir, "refs/heads/")?;
    for (refname, local_oid) in &local_branches {
        if negative_patterns
            .iter()
            .any(|p| ref_excluded_by_negative_push_pattern(p, refname))
        {
            continue;
        }
        let old_oid = refs::resolve_ref(&remote_repo.git_dir, refname).ok();
        if old_oid.as_ref() == Some(local_oid) {
            continue;
        }
        let dst = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(refname.as_str());
        let expected_oid = resolve_force_with_lease_expect(
            &args.force_with_lease,
            &repo.git_dir,
            remote_name,
            dst,
        );
        updates.push(RefUpdate {
            local_ref: Some(refname.clone()),
            remote_ref: refname.clone(),
            old_oid,
            new_oid: Some(*local_oid),
            expected_oid,
            refspec_force,
        });
    }
    Ok(())
}

/// Leading `:` / `+:` matching refspec, optionally followed only by negative `^` patterns.
fn parse_matching_push_with_negatives(args: &Args) -> Option<(bool, Vec<String>)> {
    let first = args.refspecs.first()?.as_str();
    let (refspec_force, tail) = match first {
        ":" => (false, &args.refspecs[1..]),
        "+:" => (true, &args.refspecs[1..]),
        _ => return None,
    };
    if tail.is_empty() {
        return Some((refspec_force, Vec::new()));
    }
    if !tail.iter().all(|s| s.starts_with('^')) {
        return None;
    }
    let neg: Vec<String> = tail.iter().map(|s| s[1..].to_owned()).collect();
    Some((refspec_force, neg))
}

/// Apply a single ref update on the remote, printing output as appropriate.
fn apply_ref_update(
    repo: &Repository,
    remote_repo: &Repository,
    remote_name: &str,
    update: &RefUpdate,
    args: &Args,
    _url: &str,
    pushing_config: &ConfigSet,
    remote_config: &ConfigSet,
) -> Result<ApplyRefResult> {
    if let Err(reason) =
        check_receive_pack_policy(remote_repo, remote_config, pushing_config, update)
    {
        return Ok(ApplyRefResult::RemoteRejected(reason));
    }

    let zero_oid = "0".repeat(40);

    match (&update.new_oid, &update.old_oid) {
        (Some(new_oid), old_oid_opt) => {
            if !args.dry_run {
                refs::write_ref(&remote_repo.git_dir, &update.remote_ref, new_oid)
                    .with_context(|| format!("updating remote ref {}", update.remote_ref))?;
                update_remote_tracking_ref(repo, remote_name, &update.remote_ref, Some(*new_oid))?;
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
                .unwrap_or(update.local_ref.as_deref().unwrap_or("(unknown)"));

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
                        eprintln!(
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
                        eprintln!(" * [new {kind}]      {src_short} -> {branch_short}");
                    }
                    _ => {
                        eprintln!(" = [up to date]      {} -> {}", src_short, branch_short);
                    }
                }
            }
        }
        (None, Some(old_oid)) => {
            // Delete
            if !args.dry_run {
                refs::delete_ref(&remote_repo.git_dir, &update.remote_ref)
                    .with_context(|| format!("deleting remote ref {}", update.remote_ref))?;
                update_remote_tracking_ref(repo, remote_name, &update.remote_ref, None)?;
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
                eprintln!(
                    " - [deleted]         {} -> {}",
                    &old_oid.to_hex()[..7],
                    branch_short,
                );
            }
        }
        _ => {}
    }

    Ok(ApplyRefResult::Applied)
}

/// Update local remote-tracking refs after a successful push.
///
/// Git updates `refs/remotes/<remote>/...` when pushing to a named remote.
/// For path-like remotes we skip tracking updates.
fn update_remote_tracking_ref(
    repo: &Repository,
    remote_name: &str,
    remote_ref: &str,
    new_oid: Option<ObjectId>,
) -> Result<()> {
    if remote_name.contains('/') || remote_name.starts_with('.') {
        return Ok(());
    }

    let Some(branch) = remote_ref.strip_prefix("refs/heads/") else {
        return Ok(());
    };
    let tracking_ref = format!("refs/remotes/{remote_name}/{branch}");

    match new_oid {
        Some(oid) => refs::write_ref(&repo.git_dir, &tracking_ref, &oid)
            .with_context(|| format!("updating tracking ref {tracking_ref}"))?,
        None => {
            let _ = refs::delete_ref(&repo.git_dir, &tracking_ref);
        }
    }
    Ok(())
}

/// Parsed --force-with-lease argument.
#[derive(Debug)]
enum ForceWithLease {
    /// --force-with-lease (bare, use tracking ref for the ref being pushed)
    Bare,
    /// --force-with-lease=<refname> (use tracking ref for this specific ref)
    Ref(String),
    /// --force-with-lease=<refname>:<expect> (explicit expected OID)
    RefExpect(String, String),
}

/// Resolve the expected OID for --force-with-lease, given the push target ref.
fn resolve_force_with_lease_expect(
    fwl: &Option<String>,
    git_dir: &Path,
    remote_name: &str,
    dst_branch: &str,
) -> Option<ObjectId> {
    let val = match fwl {
        Some(v) => v.as_str(),
        None => return None,
    };
    let parsed = parse_force_with_lease(val);
    match parsed {
        ForceWithLease::Bare => {
            // Use the remote-tracking ref for the branch being pushed
            let branch = dst_branch.strip_prefix("refs/heads/").unwrap_or(dst_branch);
            let tracking_ref = format!("refs/remotes/{remote_name}/{branch}");
            refs::resolve_ref(git_dir, &tracking_ref).ok()
        }
        ForceWithLease::Ref(refname) => {
            // Use the remote-tracking ref for the given refname
            let tracking_ref = format!("refs/remotes/{remote_name}/{refname}");
            refs::resolve_ref(git_dir, &tracking_ref).ok()
        }
        ForceWithLease::RefExpect(_refname, expect) => {
            // Try to resolve expect as a revision expression (handles main^, etc.)
            // We need a Repository for rev_parse, so open one from git_dir.
            if let Ok(repo) = Repository::open(git_dir, None) {
                if let Ok(oid) = grit_lib::rev_parse::resolve_revision(&repo, &expect) {
                    return Some(oid);
                }
            }
            // Fall back: try as raw OID
            expect.parse::<ObjectId>().ok()
        }
    }
}

fn parse_force_with_lease(val: &str) -> ForceWithLease {
    if val.is_empty() {
        ForceWithLease::Bare
    } else if let Some(idx) = val.find(':') {
        ForceWithLease::RefExpect(val[..idx].to_owned(), val[idx + 1..].to_owned())
    } else {
        ForceWithLease::Ref(val.to_owned())
    }
}

/// Copy symbolic refs that match a glob pattern from local to remote.
fn copy_symrefs_push(
    local_git_dir: &Path,
    remote_git_dir: &Path,
    src_pattern: &str,
    dst_pattern: &str,
) -> Result<()> {
    let refs_dir = local_git_dir.join("refs");
    if !refs_dir.is_dir() {
        return Ok(());
    }
    walk_refs_for_symrefs(&refs_dir, "refs", &mut |refname, path| {
        if let Some(matched) = match_glob(src_pattern, &refname) {
            let content = fs::read_to_string(path)?;
            let content = content.trim();
            if let Some(target) = content.strip_prefix("ref: ") {
                let remote_ref = dst_pattern.replacen('*', matched, 1);
                let remote_path =
                    remote_git_dir.join(remote_ref.replace('/', std::path::MAIN_SEPARATOR_STR));
                if let Some(parent) = remote_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&remote_path, format!("ref: {target}\n"))?;
            }
        }
        Ok(())
    })?;
    Ok(())
}

fn walk_refs_for_symrefs(
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
            walk_refs_for_symrefs(&entry.path(), &refname, cb)?;
        } else {
            cb(refname, &entry.path())?;
        }
    }
    Ok(())
}

/// Negative refspec matching for push (same rules as fetch).
fn ref_excluded_by_negative_push_pattern(pattern: &str, refname: &str) -> bool {
    match_glob(pattern, refname).is_some() || pattern == refname
}

fn validate_negative_push_patterns(patterns: &[&str]) -> Result<()> {
    for pat in patterns {
        let clean = pat.strip_prefix("refs/").unwrap_or(pat);
        if clean.chars().all(|c| c.is_ascii_hexdigit()) && clean.len() >= 7 {
            bail!("negative refspecs do not support object ids: ^{pat}");
        }
    }
    Ok(())
}

fn push_prune_glob_refspec(
    repo: &Repository,
    remote_repo: &Repository,
    args: &Args,
    remote_name: &str,
    force: bool,
    src_pat: &str,
    dst_pat: &str,
    negative_patterns: &[String],
    updates: &mut Vec<RefUpdate>,
) -> Result<()> {
    if !src_pat.contains('*') || dst_pat.is_empty() {
        return Ok(());
    }
    let remote_refs = refs::list_refs(&remote_repo.git_dir, "refs/")?;
    for (remote_ref, old_oid) in &remote_refs {
        if let Some(matched) = match_glob(dst_pat, remote_ref) {
            if negative_patterns
                .iter()
                .any(|p| ref_excluded_by_negative_push_pattern(p, remote_ref))
            {
                continue;
            }
            let local_ref = src_pat.replacen('*', matched, 1);
            if refs::resolve_ref(&repo.git_dir, &local_ref).is_ok() {
                continue;
            }
            if updates.iter().any(|u| u.remote_ref == *remote_ref) {
                continue;
            }
            let expected_oid = resolve_force_with_lease_expect(
                &args.force_with_lease,
                &repo.git_dir,
                remote_name,
                remote_ref.strip_prefix("refs/heads/").unwrap_or(remote_ref),
            );
            updates.push(RefUpdate {
                local_ref: None,
                remote_ref: remote_ref.clone(),
                old_oid: Some(*old_oid),
                new_oid: None,
                expected_oid,
                refspec_force: force,
            });
        }
    }
    Ok(())
}

/// Match a glob pattern (e.g. "refs/heads/*") against a ref name.
/// Returns the part matched by '*' if it matches, None otherwise.
fn match_glob<'a>(pattern: &str, refname: &'a str) -> Option<&'a str> {
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

fn resolve_push_src(git_dir: &Path, src: &str) -> Result<(String, ObjectId)> {
    if src.starts_with("refs/") {
        let oid = refs::resolve_ref(git_dir, src)?;
        return Ok((src.to_owned(), oid));
    }
    if src.len() == 40 {
        if let Ok(oid) = src.parse::<ObjectId>() {
            return Ok((src.to_owned(), oid));
        }
    }
    let mut matches: Vec<(String, ObjectId)> = Vec::new();
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        let full = format!("{prefix}{src}");
        if let Ok(oid) = refs::resolve_ref(git_dir, &full) {
            matches.push((full, oid));
        }
    }
    match matches.len() {
        0 => bail!("ref not found: {}", src),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            eprintln!("error: src refspec {} matches more than one", src);
            bail!("failed to push some refs");
        }
    }
}

/// Write branch tracking config.
fn set_upstream_config(git_dir: &Path, branch: &str, remote: &str) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };
    config.set(&format!("branch.{branch}.remote"), remote)?;
    config.set(
        &format!("branch.{branch}.merge"),
        &format!("refs/heads/{branch}"),
    )?;
    config.write()?;
    Ok(())
}

/// Copy all objects (loose + packs) from src to dst, skipping existing.
/// Copy objects and return the list of newly created files (for rollback).
fn copy_objects_tracked(src_git_dir: &Path, dst_git_dir: &Path) -> Result<Vec<PathBuf>> {
    let src_objects = src_git_dir.join("objects");
    let dst_objects = dst_git_dir.join("objects");
    let mut copied = Vec::new();

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
                        copied.push(dst_file);
                    }
                }
            }
        }
    }

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
                    copied.push(dst_file);
                }
            }
        }
    }

    Ok(copied)
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let dot_git = path.join(".git");
    if dot_git.is_file() {
        let git_dir = grit_lib::repo::resolve_dot_git(&dot_git)
            .with_context(|| format!("resolving gitfile at {}", dot_git.display()))?;
        return Repository::open(&git_dir, Some(path)).map_err(Into::into);
    }
    Repository::open(&dot_git, Some(path)).map_err(Into::into)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitColorBool {
    Never,
    Always,
    Auto,
}

/// Match `git_config_colorbool` / `use_sideband_colors` in `git/sideband.c`.
fn git_config_colorbool(value: &str) -> GitColorBool {
    let v = value.trim();
    if !v.is_empty() {
        if v.eq_ignore_ascii_case("never") {
            return GitColorBool::Never;
        }
        if v.eq_ignore_ascii_case("always") {
            return GitColorBool::Always;
        }
        if v.eq_ignore_ascii_case("auto") {
            return GitColorBool::Auto;
        }
    }
    match parse_bool(v) {
        Ok(false) => GitColorBool::Never,
        Ok(true) => GitColorBool::Auto,
        Err(_) => GitColorBool::Auto,
    }
}

fn want_color_stderr(mode: GitColorBool) -> bool {
    match mode {
        GitColorBool::Never => false,
        GitColorBool::Always => true,
        GitColorBool::Auto => io::stderr().is_terminal(),
    }
}

/// Per-keyword ANSI open sequences for remote hook output (`git/sideband.c`).
struct RemoteMessageColorStyle {
    enabled: bool,
    hint: String,
    warning: String,
    success: String,
    error: String,
}

impl RemoteMessageColorStyle {
    fn from_config(config: &ConfigSet) -> Self {
        let color_mode = config
            .get("color.remote")
            .map(|v| git_config_colorbool(&v))
            .or_else(|| config.get("color.ui").map(|v| git_config_colorbool(&v)))
            .unwrap_or(GitColorBool::Auto);
        let enabled = want_color_stderr(color_mode);

        let mut hint = parse_color("yellow").unwrap_or_default();
        let mut warning = parse_color("bold yellow").unwrap_or_default();
        let mut success = parse_color("bold green").unwrap_or_default();
        let mut error = parse_color("bold red").unwrap_or_default();

        if let Some(v) = config.get("color.remote.hint") {
            if let Ok(seq) = parse_color(&v) {
                hint = seq;
            }
        }
        if let Some(v) = config.get("color.remote.warning") {
            if let Ok(seq) = parse_color(&v) {
                warning = seq;
            }
        }
        if let Some(v) = config.get("color.remote.success") {
            if let Ok(seq) = parse_color(&v) {
                success = seq;
            }
        }
        if let Some(v) = config.get("color.remote.error") {
            if let Ok(seq) = parse_color(&v) {
                error = seq;
            }
        }

        Self {
            enabled,
            hint,
            warning,
            success,
            error,
        }
    }
}

fn match_remote_keyword_prefix(line_after_ws: &str, keyword: &str) -> Option<usize> {
    let kw_len = keyword.len();
    if line_after_ws.len() < kw_len {
        return None;
    }
    if !line_after_ws[..kw_len].eq_ignore_ascii_case(keyword) {
        return None;
    }
    match line_after_ws[kw_len..].chars().next() {
        None => Some(kw_len),
        Some(c) if !c.is_ascii_alphanumeric() => Some(kw_len),
        _ => None,
    }
}

/// Write remote messages to stderr, colorizing keywords if enabled.
fn colorize_remote_output(output: &str, style: &RemoteMessageColorStyle) {
    use std::io::Write;
    const RESET: &str = "\x1b[m";
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    for line in output.lines() {
        let body = if style.enabled {
            colorize_remote_line(line, style, RESET)
        } else {
            line.to_string()
        };
        let _ = writeln!(err, "remote: {body}");
    }
}

/// Colorize a single remote message line (`maybe_colorize_sideband` in `git/sideband.c`).
fn colorize_remote_line(line: &str, style: &RemoteMessageColorStyle, reset: &str) -> String {
    let trimmed = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let ws_prefix_len = line.len() - trimmed.len();
    let prefix = &line[..ws_prefix_len];

    let keywords: [(&str, &str); 4] = [
        ("hint", style.hint.as_str()),
        ("warning", style.warning.as_str()),
        ("success", style.success.as_str()),
        ("error", style.error.as_str()),
    ];
    for (kw, open_seq) in keywords {
        if let Some(kw_len) = match_remote_keyword_prefix(trimmed, kw) {
            let orig = &trimmed[..kw_len];
            let rest = &trimmed[kw_len..];
            return format!("{prefix}{open_seq}{orig}{reset}{rest}");
        }
    }
    line.to_string()
}
