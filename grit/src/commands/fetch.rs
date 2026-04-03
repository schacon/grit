//! `grit fetch` — download objects and refs from a local repository.
//!
//! Only the **local (file://)** transport is supported.  Reads the remote
//! URL from `remote.<name>.url` in the local config, opens the remote
//! repository, copies missing objects (loose + packs), and updates
//! remote-tracking refs under `refs/remotes/<remote>/`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigSet};
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit fetch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download objects and refs from another repository")]
pub struct Args {
    /// Remote name or path to fetch from (defaults to "origin").
    #[arg(value_name = "REMOTE")]
    pub remote: Option<String>,

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

    /// Number of parallel children for fetching (accepted but ignored).
    #[arg(short = 'j', long = "jobs", value_name = "N")]
    pub jobs: Option<usize>,
}

pub fn run(args: Args) -> Result<()> {
    let git_dir = resolve_git_dir()?;
    let config = ConfigSet::load(Some(&git_dir), true)?;

    if args.all {
        let remotes = collect_remote_names(&config);
        if remotes.is_empty() {
            bail!("no remotes configured");
        }
        for name in &remotes {
            fetch_remote(&git_dir, &config, name, &args)?;
        }
        Ok(())
    } else {
        let remote_name = args.remote.as_deref().unwrap_or("origin");
        fetch_remote(&git_dir, &config, remote_name, &args)
    }
}

/// Fetch from a single remote.
fn fetch_remote(
    git_dir: &Path,
    config: &ConfigSet,
    remote_name: &str,
    args: &Args,
) -> Result<()> {
    // Read remote URL from config
    let url_key = format!("remote.{remote_name}.url");
    let url = config
        .get(&url_key)
        .with_context(|| format!("remote '{remote_name}' not found; no such remote"))?;

    // Strip file:// prefix if present
    let remote_path = if let Some(stripped) = url.strip_prefix("file://") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(&url)
    };

    // Open the remote repository
    let remote_repo = open_repo(&remote_path)
        .with_context(|| format!("could not open remote repository at '{}'", remote_path.display()))?;

    // Read the refspec(s) from config (e.g. +refs/heads/*:refs/remotes/origin/*)
    let fetch_key = format!("remote.{remote_name}.fetch");
    let refspecs = collect_refspecs(config, &fetch_key);

    // Enumerate remote refs
    let remote_heads = refs::list_refs(&remote_repo.git_dir, "refs/heads/")?;
    let remote_tags = refs::list_refs(&remote_repo.git_dir, "refs/tags/")?;

    // Copy objects from remote → local
    copy_objects(&remote_repo.git_dir, git_dir, args.refetch)
        .context("copying objects from remote")?;

    // Handle --depth / --deepen: write shallow graft info
    let effective_depth = args.depth.or(args.deepen);
    if let Some(depth) = effective_depth {
        write_shallow_info(git_dir, &remote_heads, &remote_repo, depth)?;
    }

    // Determine the destination prefix for remote-tracking refs
    // Default: refs/heads/* → refs/remotes/<remote>/*
    let dst_prefix = format!("refs/remotes/{remote_name}/");

    // Track which remote-tracking refs we updated (for prune)
    let mut updated_refs: Vec<String> = Vec::new();
    let mut has_updates = false;

    // Determine the remote's HEAD branch for FETCH_HEAD
    let remote_head_branch = determine_remote_head(&remote_repo.git_dir);

    // Collect FETCH_HEAD entries
    let mut fetch_head_entries: Vec<String> = Vec::new();

    // Update remote-tracking refs from remote heads
    for (refname, remote_oid) in &remote_heads {
        // refname is like "refs/heads/main"
        let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);

        // Map through refspecs if configured, otherwise use default mapping
        let local_ref = if refspecs.is_empty() {
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

        if !args.quiet {
            print_update(&old_oid, remote_oid, branch, remote_name);
        }
    }

    // Fetch tags if requested (or by default unless --no-tags)
    if args.tags || !args.no_tags {
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
                    let tag_name = local_tag_ref.strip_prefix("refs/tags/").unwrap_or(local_tag_ref);
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

    // Write FETCH_HEAD (default branch first, then not-for-merge entries)
    if !fetch_head_entries.is_empty() {
        // Sort so entries without "not-for-merge" come first
        fetch_head_entries.sort_by(|a, b| {
            let a_nfm = a.contains("not-for-merge");
            let b_nfm = b.contains("not-for-merge");
            a_nfm.cmp(&b_nfm)
        });
        let fetch_head_path = git_dir.join("FETCH_HEAD");
        let content = fetch_head_entries.join("\n") + "\n";
        fs::write(&fetch_head_path, content).context("writing FETCH_HEAD")?;
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
            lines.push(format!(
                "{flag} {} {} {local_ref}",
                old_hex, remote_oid,
            ));
        }
        let content = lines.join("\n") + "\n";
        fs::write(output_path, content).context("writing --output file")?;
    }

    Ok(())
}

/// Print a ref update line (to stderr, matching git).
fn print_update(
    old_oid: &Option<ObjectId>,
    new_oid: &ObjectId,
    branch: &str,
    remote_name: &str,
) {
    let tracking = format!("{remote_name}/{branch}");
    match old_oid {
        None => {
            eprintln!(
                " * [new branch]      {branch:<17} -> {tracking}"
            );
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

/// Copy all objects (loose + packs) from remote to local.
/// If `refetch` is true, re-copy objects even if they already exist locally.
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
            refs::delete_ref(git_dir, refname)
                .with_context(|| format!("pruning {refname}"))?;
            if !quiet {
                // Show short name: "origin/branch" instead of "refs/remotes/origin/branch"
                let short = refname
                    .strip_prefix("refs/remotes/")
                    .unwrap_or(refname);
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
    use grit_lib::objects::{ObjectKind, parse_commit};
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
                Ok(obj) if obj.kind == ObjectKind::Commit => {
                    match parse_commit(&obj.data) {
                        Ok(c) => {
                            if c.parents.is_empty() {
                                break;
                            }
                            oid = c.parents[0];
                        }
                        Err(_) => break,
                    }
                }
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

/// A parsed refspec (e.g. "+refs/heads/*:refs/remotes/origin/*").
struct FetchRefspec {
    /// Source pattern (remote side), e.g. "refs/heads/*".
    src: String,
    /// Destination pattern (local side), e.g. "refs/remotes/origin/*".
    dst: String,
    /// Whether this is a force refspec (leading '+').
    #[allow(dead_code)]
    force: bool,
}

/// Collect all fetch refspecs from a config key (may be multi-valued).
fn collect_refspecs(config: &ConfigSet, key: &str) -> Vec<FetchRefspec> {
    let mut result = Vec::new();
    for entry in config.entries() {
        if entry.key == key {
            if let Some(ref val) = entry.value {
                let val = val.trim();
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
    for rs in refspecs {
        if let Some(mapped) = match_refspec_pattern(&rs.src, &rs.dst, remote_ref) {
            return Some(mapped);
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
