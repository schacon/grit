//! `grit push` — update remote refs and associated objects.
//!
//! Only **local (file://)** transports are supported.  Copies objects from
//! the local repository to the remote and updates remote refs.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
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
    #[arg(long = "force-with-lease")]
    pub force_with_lease: bool,

    /// Request an atomic push: either all refs update or none do.
    #[arg(long)]
    pub atomic: bool,

    /// Send a push option string to the server.
    #[arg(long = "push-option", short = 'o', value_name = "OPTION")]
    pub push_option: Vec<String>,

    /// Machine-readable output (one line per ref update).
    #[arg(long)]
    pub porcelain: bool,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,
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

    let head = resolve_head(&repo.git_dir)?;
    let current_branch = head.branch_name().map(|s| s.to_owned());

    // Determine remote name
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

    // Get remote URL
    let url_key = format!("remote.{remote_name}.url");
    let url = config
        .get(&url_key)
        .with_context(|| format!("remote '{remote_name}' not found"))?;

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

    if args.delete {
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
            updates.push(RefUpdate {
                local_ref: None,
                remote_ref,
                old_oid,
                new_oid: None,
                expected_oid: None,
            });
        }
    } else if !args.refspecs.is_empty() {
        // Explicit refspecs
        for spec in &args.refspecs {
            let (src, dst) = parse_refspec(spec);
            let local_ref = normalize_ref(&src);
            let remote_ref = normalize_ref(&dst);

            let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
                .with_context(|| format!("src refspec '{}' does not match any", src))?;
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

            // For --force-with-lease, the expected oid comes from the local
            // remote-tracking ref (what `fetch` last stored), NOT re-reading
            // the remote. This detects when someone else pushed since our
            // last fetch.
            let expected_oid = if args.force_with_lease {
                let tracking_ref = format!(
                    "refs/remotes/{}/{}",
                    remote_name,
                    dst.strip_prefix("refs/heads/").unwrap_or(&dst)
                );
                refs::resolve_ref(&repo.git_dir, &tracking_ref).ok()
            } else {
                None
            };

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

        // For --force-with-lease, the expected oid comes from the local
        // remote-tracking ref.
        let expected_oid = if args.force_with_lease {
            let tracking_ref = format!("refs/remotes/{remote_name}/{branch}");
            refs::resolve_ref(&repo.git_dir, &tracking_ref).ok()
        } else {
            None
        };

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
            if !args.force && !args.force_with_lease && !is_ancestor(&repo, *old, *new)? {
                bail!(
                    "Updates were rejected because the tip of your current branch is behind\n\
                     its remote counterpart. If you want to force the update, use --force.\n\
                     remote ref: {}",
                    update.remote_ref
                );
            }
        }
    }

    // Run pre-push hook
    {
        use grit_lib::hooks::{run_hook, HookResult};
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

    // Apply ref updates
    if !args.quiet && !args.porcelain {
        println!("To {url}");
    }

    // Track results for atomic rollback on failure
    let mut applied_updates: Vec<(&RefUpdate, Option<ObjectId>)> = Vec::new();

    for update in &updates {
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
