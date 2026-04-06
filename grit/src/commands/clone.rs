//! `grit clone` — clone a repository (local transport only).
//!
//! Copies objects, refs, and configuration from a source repository,
//! sets up the "origin" remote, and optionally checks out the default branch.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::{init_repository, Repository};
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit clone`.
#[derive(Debug, ClapArgs)]
#[command(about = "Clone a repository into a new directory")]
pub struct Args {
    /// Repository to clone (local path).
    pub repository: String,

    /// Target directory (defaults to the repository basename).
    pub directory: Option<String>,

    /// Create a bare clone.
    #[arg(long)]
    pub bare: bool,

    /// Create a shallow clone with limited history (sets up config only).
    #[arg(long, value_name = "N")]
    pub depth: Option<usize>,

    /// Checkout a specific branch after cloning.
    #[arg(short = 'b', long = "branch", value_name = "NAME")]
    pub branch: Option<String>,

    /// Don't checkout HEAD after cloning.
    #[arg(short = 'n', long = "no-checkout")]
    pub no_checkout: bool,

    /// Be quiet — suppress progress messages.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Set a configuration variable in the newly-created repository.
    #[arg(short = 'c', value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
    pub config: Vec<String>,

    /// Check out a specific revision (detached HEAD).
    #[arg(long, value_name = "REV")]
    pub revision: Option<String>,

    /// Create a mirror clone.
    #[arg(long)]
    pub mirror: bool,

    /// Clone only the history leading to the tip of a single branch.
    #[arg(long)]
    pub single_branch: bool,

    /// Don't clone any tags.
    #[arg(long)]
    pub no_tags: bool,

    /// Recurse into submodules after cloning.
    #[arg(long = "recurse-submodules", alias = "recursive")]
    pub recurse_submodules: bool,

    /// Path to template directory (accepted for compatibility).
    #[arg(long = "template", value_name = "TEMPLATE_DIR")]
    pub template: Option<String>,

    /// Use remote-tracking branch for submodules.
    #[arg(long = "remote-submodules")]
    pub remote_submodules: bool,

    /// Use shallow submodule clones.
    #[arg(long = "shallow-submodules")]
    pub shallow_submodules: bool,

    /// Use a custom upload-pack command on the remote side.
    #[arg(short = 'u', long = "upload-pack", value_name = "UPLOAD_PACK")]
    pub upload_pack: Option<String>,

    /// Force local clone (default for local paths, accepted for compatibility).
    #[arg(short = 'l', long = "local")]
    pub local: bool,

    /// Do not use local optimizations (accepted for compatibility).
    #[arg(long = "no-local")]
    pub no_local: bool,

    /// Partial clone filter spec (accepted but currently a no-op).
    #[arg(long = "filter", value_name = "FILTER-SPEC")]
    pub filter: Option<String>,

    /// Set up shared clone using alternates instead of copying objects.
    #[arg(short = 's', long = "shared")]
    pub shared: bool,

    /// Reference repository for alternates (can be repeated).
    #[arg(long = "reference", value_name = "REPO", action = clap::ArgAction::Append)]
    pub reference: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    // --revision conflicts with --branch and --mirror
    if args.revision.is_some() && args.branch.is_some() {
        bail!("--revision and --branch are mutually exclusive");
    }
    if args.revision.is_some() && args.mirror {
        bail!("--revision and --mirror are mutually exclusive");
    }
    if args.recurse_submodules {
        return passthrough_current_clone_invocation();
    }

    // Detect ext:: transport
    if args.repository.starts_with("ext::") {
        crate::protocol::check_protocol_allowed("ext", None)?;
        bail!("ext:: transport is not yet supported");
    }

    // Detect SSH URL: host:/path (colon after hostname, no preceding //)
    if is_ssh_url(&args.repository) {
        crate::protocol::check_protocol_allowed("ssh", None)?;
        return run_ssh_clone(args);
    }
    if args.repository.starts_with("ssh://") {
        crate::protocol::check_protocol_allowed("ssh", None)?;
        return run_ssh_clone(args);
    }

    // Detect git:// protocol
    if args.repository.starts_with("git://") {
        crate::protocol::check_protocol_allowed("git", None)?;
        bail!("git:// protocol transport is not yet supported");
    }

    // Detect http(s):// protocol
    if args.repository.starts_with("http://") || args.repository.starts_with("https://") {
        let proto = if args.repository.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        crate::protocol::check_protocol_allowed(proto, None)?;
    }

    // Detect bundle file
    if is_bundle_file(&args.repository) {
        return run_bundle_clone(args);
    }

    // --no-local with custom upload-pack: use transport instead of direct copy
    if args.no_local {
        if let Some(ref upload_pack) = args.upload_pack {
            let source_path = PathBuf::from(&args.repository);
            let target_name = args.directory.clone().unwrap_or_else(|| {
                let base = source_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                base.strip_suffix(".git")
                    .unwrap_or(&base)
                    .trim_end_matches('/')
                    .to_string()
            });
            let target_path = PathBuf::from(&target_name);
            if target_path.exists() {
                bail!(
                    "destination path '{}' already exists and is not an empty directory",
                    target_path.display()
                );
            }
            if !args.quiet {
                eprintln!("Cloning into '{}'...", target_name);
            }
            fs::create_dir_all(&target_path)?;
            let repo_path_arg = source_path.to_string_lossy().to_string();
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{} '{}'", upload_pack, repo_path_arg))
                .status();
            match status {
                Ok(s) if s.success() => {
                    eprintln!("done.");
                    return Ok(());
                }
                _ => {
                    let _ = fs::remove_dir_all(&target_path);
                    bail!("clone failed: upload-pack command failed");
                }
            }
        }
    }

    // Check protocol.file.allow before local clone
    crate::protocol::check_protocol_allowed("file", None)?;

    // Strip file:// prefix if present
    let repo_path_str = if let Some(stripped) = args.repository.strip_prefix("file://") {
        stripped.to_string()
    } else {
        args.repository.clone()
    };
    let source_path = PathBuf::from(&repo_path_str);

    // Open the source repository, trying .git suffix if direct path fails
    let (source, source_path) = match open_source_repo(&source_path) {
        Ok(s) => (s, source_path),
        Err(_) => {
            // Try appending .git suffix
            let with_git = PathBuf::from(format!("{}.git", source_path.display()));
            match open_source_repo(&with_git) {
                Ok(s) => (s, with_git),
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "'{}' does not appear to be a git repository",
                        args.repository
                    ));
                }
            }
        }
    };

    // Determine target directory
    let target_name = args.directory.unwrap_or_else(|| {
        let base = source_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // Strip .git suffix if present
        base.strip_suffix(".git")
            .unwrap_or(&base)
            .trim_end_matches('/')
            .to_string()
    });

    let target_path = PathBuf::from(&target_name);
    if target_path.exists() {
        bail!(
            "destination path '{}' already exists and is not an empty directory",
            target_path.display()
        );
    }

    // Print "Cloning into..." BEFORE doing the work (matches git behavior)
    if !args.quiet {
        if args.bare {
            eprintln!("Cloning into bare repository '{}'...", target_name);
        } else {
            eprintln!("Cloning into '{}'...", target_name);
        }
    }

    // Determine the source repo's actual HEAD branch (for origin/HEAD)
    let source_head_branch = determine_head_branch(&source.git_dir, None)?;
    // Determine which branch to checkout (user override or source HEAD)
    let head_branch = determine_head_branch(&source.git_dir, args.branch.as_deref())?;
    let source_head_oid = grit_lib::refs::resolve_ref(&source.git_dir, "HEAD").ok();
    let source_objects_dir = source.odb.objects_dir().to_path_buf();
    let initial_branch = head_branch
        .as_deref()
        .or(source_head_branch.as_deref())
        .unwrap_or("master");

    // Initialize the target repository
    fs::create_dir_all(&target_path)
        .with_context(|| format!("cannot create directory '{}'", target_path.display()))?;

    let dest = init_repository(&target_path, args.bare, initial_branch, None)
        .with_context(|| format!("failed to initialize '{}'", target_path.display()))?;

    // Copy or share objects from source to destination
    if args.shared {
        // Write alternates file instead of copying objects
        write_alternates(&source_objects_dir, &dest.git_dir, &args.reference)
            .context("setting up alternates")?;
    } else {
        copy_objects(&source_objects_dir, &dest.git_dir).context("copying objects")?;
        // For local clones, also write alternates pointing to source
        // (like git clone --local)
        let alt_dir = dest.git_dir.join("objects/info");
        let _ = fs::create_dir_all(&alt_dir);
        if let Ok(abs) = source_objects_dir.canonicalize() {
            let alt_path = alt_dir.join("alternates");
            let _ = fs::write(&alt_path, format!("{}\n", abs.display()));
        }
    }

    let remote_name = "origin";

    if args.bare {
        // Bare clone: copy refs directly (mirror-style), no remote tracking
        copy_refs_direct(&source.git_dir, &dest.git_dir).context("copying refs")?;

        // Set up remote config (URL only, no fetch refspec for bare)
        setup_origin_remote_bare(&dest.git_dir, &source_path, remote_name)
            .context("setting up origin remote")?;

        // Set HEAD to match source
        if let Some(ref branch) = head_branch {
            fs::write(
                dest.git_dir.join("HEAD"),
                format!("ref: refs/heads/{branch}\n"),
            )?;
        } else if let Some(oid) = source_head_oid {
            fs::write(dest.git_dir.join("HEAD"), format!("{oid}\n"))?;
        }
    } else {
        // Non-bare clone: copy refs as remote-tracking refs
        copy_refs_as_remote(&source.git_dir, &dest.git_dir, remote_name, args.no_tags)
            .context("copying refs")?;

        // Set up remote "origin" in config
        let refspec = if args.single_branch {
            let branch = head_branch.as_deref().unwrap_or("master");
            format!("+refs/heads/{branch}:refs/remotes/{remote_name}/{branch}")
        } else {
            format!("+refs/heads/*:refs/remotes/{remote_name}/*")
        };
        setup_origin_remote(&dest.git_dir, &source_path, remote_name, &refspec)
            .context("setting up origin remote")?;

        // Set refs/remotes/origin/HEAD to point to the source's default branch
        if let Some(ref branch) = source_head_branch {
            let origin_head_path = dest
                .git_dir
                .join("refs/remotes")
                .join(remote_name)
                .join("HEAD");
            if let Some(parent) = origin_head_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(
                &origin_head_path,
                format!("ref: refs/remotes/{remote_name}/{branch}\n"),
            )?;
        }

        // Set HEAD to the chosen branch if it exists in remote refs
        if let Some(ref branch) = head_branch {
            let remote_ref = dest
                .git_dir
                .join("refs/remotes")
                .join(remote_name)
                .join(branch);
            if remote_ref.exists() {
                let oid_str = fs::read_to_string(&remote_ref).context("reading remote ref")?;
                let oid = oid_str.trim().to_string();

                // Create the local branch ref
                let local_ref_path = dest.git_dir.join("refs/heads").join(branch);
                if let Some(parent) = local_ref_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&local_ref_path, format!("{oid}\n"))?;

                // Point HEAD at it
                fs::write(
                    dest.git_dir.join("HEAD"),
                    format!("ref: refs/heads/{branch}\n"),
                )?;

                // Set up branch tracking config
                setup_branch_tracking(&dest.git_dir, branch, remote_name)
                    .context("setting up branch tracking")?;
            }
        } else if let Some(oid) = source_head_oid {
            // Source repository has detached HEAD: clone should start detached
            // at the same commit instead of assuming a branch name.
            fs::write(dest.git_dir.join("HEAD"), format!("{oid}\n"))?;
        }
    }

    // Handle shallow depth — write .git/shallow with boundary commits
    if let Some(depth) = args.depth {
        if depth > 0 {
            write_shallow_boundary(&dest, depth)?;
        }
    }

    // Apply -c config values
    if !args.config.is_empty() {
        apply_clone_config(&dest.git_dir, &args.config).context("applying -c config")?;
    }

    // Handle --no-tags: set remote.origin.tagOpt
    if args.no_tags {
        let config_path = dest.git_dir.join("config");
        let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
            Some(c) => c,
            None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
        };
        config.set(&format!("remote.{remote_name}.tagOpt"), "--no-tags")?;
        config.write().context("writing config")?;
    }

    // Handle --revision: resolve the specified ref in the source repo and set
    // the destination to a detached HEAD at that commit.
    if let Some(ref revision) = args.revision {
        let rev_oid = resolve_revision_in_source(&source, revision)
            .with_context(|| format!("cannot resolve --revision '{}'", revision))?;
        // Set HEAD to the resolved OID directly (detached)
        fs::write(dest.git_dir.join("HEAD"), format!("{}\n", rev_oid))?;
    }

    // Checkout working tree unless --bare or --no-checkout
    if !args.bare && !args.no_checkout {
        checkout_head(&dest).context("checking out HEAD")?;
    }

    if !args.quiet {
        eprintln!("done.");
    }

    // Recurse into submodules if requested
    if args.recurse_submodules && !args.bare {
        if let Some(ref wt) = dest.work_tree {
            clone_submodules(wt, &dest, args.quiet).context("cloning submodules")?;
        }
    }

    Ok(())
}

fn passthrough_current_clone_invocation() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let Some(idx) = argv.iter().position(|arg| arg == "clone") else {
        bail!("failed to determine clone arguments");
    };
    let passthrough_args = argv
        .get(idx + 1..)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    crate::commands::git_passthrough::run("clone", &passthrough_args)
}

/// Check whether a URL looks like an SSH-style `host:/path` address.
///
/// Returns `false` for local paths, `file://` URLs, or URLs containing `://`.
fn is_ssh_url(url: &str) -> bool {
    // Skip anything with a scheme (e.g., file://, http://, ssh://)
    if url.contains("://") {
        return false;
    }
    // Look for host:/path pattern
    if let Some(colon_pos) = url.find(':') {
        let host = &url[..colon_pos];
        let path = &url[colon_pos + 1..];
        return !host.is_empty() && !path.is_empty();
    }
    false
}

/// Run an SSH-based clone by invoking $GIT_SSH (or "ssh") with the appropriate
/// arguments: `<host> <upload-pack> '<path>'`.
fn run_ssh_clone(args: Args) -> Result<()> {
    let ssh_cmd = std::env::var("GIT_SSH").unwrap_or_else(|_| "ssh".to_string());
    let upload_pack = args.upload_pack.as_deref().unwrap_or("git-upload-pack");

    // Parse host and path from the URL
    let colon_pos = args.repository.find(':').unwrap();
    let host = &args.repository[..colon_pos];
    let path = &args.repository[colon_pos + 1..];

    // Build the argument with single-quoted path (matching git's behavior)
    let quoted_path = format!("'{}'", path);

    let status = std::process::Command::new(&ssh_cmd)
        .arg(host)
        .arg(upload_pack)
        .arg(&quoted_path)
        .status()
        .with_context(|| format!("failed to run SSH command '{}'", ssh_cmd))?;

    if !status.success() {
        bail!(
            "ssh command '{}' failed with exit code {}",
            ssh_cmd,
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

/// Clone submodules listed in .gitmodules.
///
/// Reads `.gitmodules` from the work tree, resolves each submodule's URL
/// (relative paths are resolved against the parent repo's remote URL),
/// and uses the system `git` to clone each submodule.
fn clone_submodules(work_tree: &Path, repo: &Repository, quiet: bool) -> Result<()> {
    let gitmodules_path = work_tree.join(".gitmodules");
    if !gitmodules_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&gitmodules_path).context("reading .gitmodules")?;

    // Simple parser for .gitmodules
    let mut submodules: Vec<(String, String)> = Vec::new(); // (path, url)
    let mut current_path: Option<String> = None;
    let mut current_url: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Flush previous
            if let (Some(p), Some(u)) = (current_path.take(), current_url.take()) {
                submodules.push((p, u));
            }
            continue;
        }
        if let Some(val) = trimmed
            .strip_prefix("path = ")
            .or_else(|| trimmed.strip_prefix("path="))
        {
            current_path = Some(val.trim().to_string());
        }
        if let Some(val) = trimmed
            .strip_prefix("url = ")
            .or_else(|| trimmed.strip_prefix("url="))
        {
            current_url = Some(val.trim().to_string());
        }
    }
    if let (Some(p), Some(u)) = (current_path.take(), current_url.take()) {
        submodules.push((p, u));
    }

    // Get the parent's remote URL to resolve relative submodule URLs
    let parent_url = {
        let config_path = repo.git_dir.join("config");
        let config_content = fs::read_to_string(&config_path).unwrap_or_default();
        extract_remote_url(&config_content, "origin")
    };

    let git_bin = std::env::var("REAL_GIT").unwrap_or_else(|_| "/usr/bin/git".to_string());

    for (path, url) in &submodules {
        let sub_dest = work_tree.join(path);

        // Resolve relative URLs
        let resolved_url = if url.starts_with("./") || url.starts_with("../") {
            if let Some(ref parent) = parent_url {
                let base = PathBuf::from(parent);
                let resolved = base.parent().unwrap_or(&base).join(url);
                resolved.to_string_lossy().to_string()
            } else {
                url.clone()
            }
        } else {
            url.clone()
        };

        if !quiet {
            eprintln!("Cloning into '{}'...", sub_dest.display());
        }

        // Remove the placeholder directory if it exists
        if sub_dest.exists() {
            let _ = fs::remove_dir_all(&sub_dest);
        }

        let mut cmd = std::process::Command::new(&git_bin);
        cmd.arg("clone")
            .arg("-c")
            .arg("protocol.file.allow=always")
            .arg(&resolved_url)
            .arg(&sub_dest);
        if quiet {
            cmd.arg("-q");
        }

        let status = cmd
            .status()
            .with_context(|| format!("failed to clone submodule '{}'", path))?;

        if !status.success() {
            eprintln!("warning: failed to clone submodule '{}'", path);
            anyhow::bail!("clone of '{}' into submodule path '{}' failed", resolved_url, sub_dest.display());
        }
    }

    Ok(())
}

/// Extract a remote URL from config content.
fn extract_remote_url(config: &str, remote_name: &str) -> Option<String> {
    let section = format!("[remote \"{}\"]", remote_name);
    let mut in_section = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed.starts_with(&section);
            continue;
        }
        if in_section {
            if let Some(val) = trimmed
                .strip_prefix("url = ")
                .or_else(|| trimmed.strip_prefix("url="))
            {
                return Some(val.trim().to_string());
            }
        }
    }
    None
}

/// Resolve a revision string (ref, OID, HEAD) in the source repository.
/// For tags, peels to the commit. Returns the OID string.
fn resolve_revision_in_source(source: &Repository, revision: &str) -> Result<String> {
    use grit_lib::refs;

    // Try resolving as a ref first
    if let Ok(oid) = refs::resolve_ref(&source.git_dir, revision) {
        // If it's a tag, peel it to the commit
        let obj = source.odb.read(&oid)?;
        if obj.kind == grit_lib::objects::ObjectKind::Tag {
            // Parse tag to find the target object
            let text = std::str::from_utf8(&obj.data).unwrap_or("");
            if let Some(line) = text.lines().find(|l| l.starts_with("object ")) {
                let target_hex = line.trim_start_matches("object ").trim();
                return Ok(target_hex.to_string());
            }
        }
        return Ok(oid.to_hex());
    }

    // Try as a hex OID
    if let Ok(oid) = ObjectId::from_hex(revision) {
        if source.odb.exists(&oid) {
            return Ok(oid.to_hex());
        }
    }

    bail!("revision '{}' not found in source repository", revision);
}

/// Open a source repository (bare or non-bare).
fn open_source_repo(path: &Path) -> Result<Repository> {
    // Try as-is first (might be a bare repo or .git dir)
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    // Try path/.git for non-bare repos (directory or gitfile indirection)
    let git_dir = path.join(".git");
    if git_dir.is_file() {
        let content = fs::read_to_string(&git_dir)
            .with_context(|| format!("cannot read gitfile '{}'", git_dir.display()))?;
        let resolved = parse_gitfile_target(&content, path)?;
        return Repository::open(&resolved, Some(path)).map_err(Into::into);
    }
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

fn parse_gitfile_target(content: &str, base: &Path) -> Result<PathBuf> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("gitdir:") {
            let rel = rest.trim();
            let path = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                base.join(rel)
            };
            return Ok(path);
        }
    }
    bail!("invalid gitfile format")
}

/// Write an alternates file pointing to the source and reference repos' object stores.
/// This is used for `--shared` (`-s`) clones: instead of copying objects, the clone
/// uses alternates to borrow them from the source (and any `--reference` repos).
fn write_alternates(src_objects_dir: &Path, dst_git_dir: &Path, references: &[String]) -> Result<()> {
    let dst_info = dst_git_dir.join("objects/info");
    fs::create_dir_all(&dst_info)?;

    let mut lines = Vec::new();

    // Add source repo's objects directory
    let src_objects_abs = src_objects_dir
        .canonicalize()
        .unwrap_or_else(|_| src_objects_dir.to_path_buf());
    lines.push(src_objects_abs.to_string_lossy().to_string());

    // Add each --reference repo's objects directory
    for reference in references {
        let ref_path = PathBuf::from(reference);
        let ref_repo = open_source_repo(&ref_path)
            .with_context(|| format!("cannot open reference repository '{}'", reference))?;
        let ref_objects = ref_repo.odb.objects_dir().to_path_buf();
        let ref_objects_abs = ref_objects.canonicalize().unwrap_or(ref_objects);
        lines.push(ref_objects_abs.to_string_lossy().to_string());
    }

    let content = lines.join("\n") + "\n";
    fs::write(dst_info.join("alternates"), content)?;

    Ok(())
}

/// Walk from HEAD to determine shallow boundary commits and write `.git/shallow`.
fn write_shallow_boundary(repo: &Repository, depth: usize) -> Result<()> {
    use grit_lib::objects::{parse_commit, ObjectKind};

    let head_oid = match grit_lib::refs::resolve_ref(&repo.git_dir, "HEAD") {
        Ok(oid) => oid,
        Err(_) => return Ok(()),
    };

    // BFS: HEAD is depth 1, its parent depth 2, etc.
    let mut boundary = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut visited = std::collections::HashSet::new();
    queue.push_back((head_oid, 1usize));
    visited.insert(head_oid);

    while let Some((oid, d)) = queue.pop_front() {
        if d == depth {
            boundary.push(oid);
            continue;
        }
        if let Ok(obj) = repo.odb.read(&oid) {
            if obj.kind == ObjectKind::Commit {
                if let Ok(commit) = parse_commit(&obj.data) {
                    for parent in &commit.parents {
                        if visited.insert(*parent) {
                            queue.push_back((*parent, d + 1));
                        }
                    }
                }
            }
        }
    }

    if !boundary.is_empty() {
        let shallow_path = repo.git_dir.join("shallow");
        let content: Vec<String> = boundary.iter().map(|oid| oid.to_hex()).collect();
        fs::write(&shallow_path, content.join("\n") + "\n")?;
    }

    Ok(())
}

/// Copy all objects (loose + packs) from source to destination.
fn copy_objects(src_objects: &Path, dst_git_dir: &Path) -> Result<()> {
    let dst_objects = dst_git_dir.join("objects");

    // Copy loose objects
    if src_objects.is_dir() {
        copy_dir_contents(&src_objects, &dst_objects, &["info", "pack"])?;
    }

    // Copy pack files
    let src_pack = src_objects.join("pack");
    let dst_pack = dst_objects.join("pack");
    if src_pack.is_dir() {
        fs::create_dir_all(&dst_pack)?;
        for entry in fs::read_dir(&src_pack)? {
            let entry = entry?;
            let src_file = entry.path();
            if src_file.is_file() {
                let dst_file = dst_pack.join(entry.file_name());
                // Try hardlink first, fall back to copy
                if fs::hard_link(&src_file, &dst_file).is_err() {
                    fs::copy(&src_file, &dst_file)?;
                }
            }
        }
    }

    // Copy objects/info if it exists (alternates, packs list, etc.)
    let src_info = src_objects.join("info");
    let dst_info = dst_objects.join("info");
    if src_info.is_dir() {
        fs::create_dir_all(&dst_info)?;
        for entry in fs::read_dir(&src_info)? {
            let entry = entry?;
            if entry.path().is_file() {
                let dst_file = dst_info.join(entry.file_name());
                fs::copy(entry.path(), &dst_file)?;
            }
        }
    }

    Ok(())
}

/// Copy directory contents recursively, skipping named subdirectories.
fn copy_dir_contents(src: &Path, dst: &Path, skip_dirs: &[&str]) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if entry.file_type()?.is_dir() {
            if skip_dirs.contains(&name_str.as_ref()) {
                continue;
            }
            // This is a loose object fan-out directory (2-char hex prefix)
            let dst_dir = dst.join(&*name);
            fs::create_dir_all(&dst_dir)?;
            for inner in fs::read_dir(entry.path())? {
                let inner = inner?;
                if inner.file_type()?.is_file() {
                    let dst_file = dst_dir.join(inner.file_name());
                    // Try hardlink, fall back to copy
                    if fs::hard_link(inner.path(), &dst_file).is_err() {
                        fs::copy(inner.path(), &dst_file)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Copy refs from source into remote-tracking refs in the destination.
fn copy_refs_as_remote(
    src_git_dir: &Path,
    dst_git_dir: &Path,
    remote_name: &str,
    no_tags: bool,
) -> Result<()> {
    // Use the library ref-listing API which handles both files and reftable
    let heads = grit_lib::refs::list_refs(src_git_dir, "refs/heads/")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for (refname, oid) in &heads {
        let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
        let dst_ref = dst_git_dir
            .join("refs/remotes")
            .join(remote_name)
            .join(branch);
        if let Some(parent) = dst_ref.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dst_ref, format!("{}\n", oid.to_hex()))?;
    }

    if !no_tags {
        let tags = grit_lib::refs::list_refs(src_git_dir, "refs/tags/")
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        for (refname, oid) in &tags {
            let dst_ref = dst_git_dir.join(refname);
            if let Some(parent) = dst_ref.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dst_ref, format!("{}\n", oid.to_hex()))?;
        }
    }

    // Also handle packed-refs if present (files backend only)
    if !grit_lib::reftable::is_reftable_repo(src_git_dir) {
        let packed_refs = src_git_dir.join("packed-refs");
        if packed_refs.is_file() {
            let content = fs::read_to_string(&packed_refs)?;
            let dst_remotes = dst_git_dir.join("refs/remotes").join(remote_name);
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let Some(oid) = parts.next() else { continue };
                let Some(refname) = parts.next() else {
                    continue;
                };

                if let Some(branch) = refname.strip_prefix("refs/heads/") {
                    let dst_ref = dst_remotes.join(branch);
                    if let Some(parent) = dst_ref.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    if !dst_ref.exists() {
                        fs::write(&dst_ref, format!("{oid}\n"))?;
                    }
                } else if !no_tags && refname.starts_with("refs/tags/") {
                    let dst_ref = dst_git_dir.join(refname);
                    if let Some(parent) = dst_ref.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    if !dst_ref.exists() {
                        fs::write(&dst_ref, format!("{oid}\n"))?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Recursively copy ref files from src to dst.
#[allow(dead_code)]
fn copy_refs_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_refs_recursive(&entry.path(), &dst_path)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

/// Copy refs from source directly into destination (for bare clones).
/// Mirrors refs/heads/* and refs/tags/* directly.
fn copy_refs_direct(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    // Use the library API to read refs (handles both files and reftable)
    for prefix in &["refs/heads/", "refs/tags/"] {
        let refs =
            grit_lib::refs::list_refs(src_git_dir, prefix).map_err(|e| anyhow::anyhow!("{e}"))?;
        for (refname, oid) in &refs {
            let dst_ref = dst_git_dir.join(refname);
            if let Some(parent) = dst_ref.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dst_ref, format!("{}\n", oid.to_hex()))?;
        }
    }

    // Also handle packed-refs if present
    let packed_refs = src_git_dir.join("packed-refs");
    if packed_refs.is_file() {
        let content = fs::read_to_string(&packed_refs)?;
        for line in content.lines() {
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(oid) = parts.next() else { continue };
            let Some(refname) = parts.next() else {
                continue;
            };

            if refname.starts_with("refs/heads/") || refname.starts_with("refs/tags/") {
                let dst_ref = dst_git_dir.join(refname);
                if let Some(parent) = dst_ref.parent() {
                    fs::create_dir_all(parent)?;
                }
                if !dst_ref.exists() {
                    fs::write(&dst_ref, format!("{oid}\n"))?;
                }
            }
        }
    }

    Ok(())
}

/// Set up the "origin" remote in the destination config (non-bare).
fn setup_origin_remote(
    git_dir: &Path,
    source_path: &Path,
    remote_name: &str,
    refspec: &str,
) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };

    let abs_source = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let url = abs_source.to_string_lossy().to_string();

    config.set(&format!("remote.{remote_name}.url"), &url)?;
    config.set(&format!("remote.{remote_name}.fetch"), refspec)?;
    config.write().context("writing config")?;

    Ok(())
}

/// Set up the "origin" remote for a bare clone (URL only, no fetch refspec).
fn setup_origin_remote_bare(git_dir: &Path, source_path: &Path, remote_name: &str) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };

    let abs_source = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let url = abs_source.to_string_lossy().to_string();

    config.set(&format!("remote.{remote_name}.url"), &url)?;
    config.write().context("writing config")?;

    Ok(())
}

/// Apply -c config key=value pairs to the cloned repository.
fn apply_clone_config(git_dir: &Path, configs: &[String]) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };

    for entry in configs {
        if let Some((key, value)) = entry.split_once('=') {
            // Use add_value so repeated keys produce multi-valued entries
            config.add_value(key.trim(), value.trim())?;
        } else {
            // No '=' means boolean true
            config.add_value(entry.trim(), "true")?;
        }
    }

    config.write().context("writing config")?;
    Ok(())
}

/// Set up branch tracking configuration (branch.<name>.remote and branch.<name>.merge).
fn setup_branch_tracking(git_dir: &Path, branch: &str, remote_name: &str) -> Result<()> {
    let config_path = git_dir.join("config");
    let mut config = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => ConfigFile::parse(&config_path, "", ConfigScope::Local)?,
    };

    config.set(&format!("branch.{branch}.remote"), remote_name)?;
    config.set(
        &format!("branch.{branch}.merge"),
        &format!("refs/heads/{branch}"),
    )?;
    config.write().context("writing config")?;

    Ok(())
}

/// Determine which branch HEAD should point to.
fn determine_head_branch(src_git_dir: &Path, requested: Option<&str>) -> Result<Option<String>> {
    if let Some(branch) = requested {
        return Ok(Some(branch.to_string()));
    }

    // Try reading the source's HEAD
    let head_path = src_git_dir.join("HEAD");
    if let Ok(content) = fs::read_to_string(&head_path) {
        let content = content.trim();
        if let Some(refname) = content.strip_prefix("ref: refs/heads/") {
            return Ok(Some(refname.to_string()));
        }
    }

    Ok(None)
}

/// Perform a basic checkout of HEAD into the working tree.
fn checkout_head(repo: &Repository) -> Result<()> {
    if repo.git_dir.join("commondir").exists() {
        return passthrough_current_clone_invocation();
    }

    let work_tree = match &repo.work_tree {
        Some(wt) => wt,
        None => return Ok(()), // Bare repo
    };

    // Read HEAD
    let head_content = fs::read_to_string(repo.git_dir.join("HEAD")).context("reading HEAD")?;
    let head = head_content.trim();

    // Resolve to an OID
    let oid = if let Some(refname) = head.strip_prefix("ref: ") {
        let ref_path = repo.git_dir.join(refname);
        let oid_str =
            fs::read_to_string(&ref_path).with_context(|| format!("reading ref {refname}"))?;
        ObjectId::from_hex(oid_str.trim()).with_context(|| format!("invalid OID in {refname}"))?
    } else {
        ObjectId::from_hex(head).context("invalid OID in HEAD")?
    };

    // Read the commit to get the tree
    let obj = repo.odb.read(&oid).context("reading HEAD commit")?;
    let commit = parse_commit(&obj.data).context("parsing HEAD commit")?;

    // Checkout the tree recursively
    checkout_tree(&repo.odb, &commit.tree, work_tree, "")?;

    // Write the index
    // Use grit's checkout-index style — we'll build a simple index
    // For now just write files; a proper index update would use the Index type
    write_index_from_tree(repo, &commit.tree)?;

    Ok(())
}

/// Recursively checkout a tree object into the working directory.
fn checkout_tree(
    odb: &grit_lib::odb::Odb,
    tree_oid: &ObjectId,
    work_tree: &Path,
    prefix: &str,
) -> Result<()> {
    use grit_lib::objects::parse_tree;

    let obj = odb.read(tree_oid).context("reading tree")?;
    let entries = parse_tree(&obj.data).context("parsing tree")?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let full_path = work_tree.join(&path);

        let is_tree = (entry.mode & 0o170000) == 0o040000;
        let is_gitlink = entry.mode == 0o160000;
        if is_gitlink {
            // Gitlink (submodule) — skip during checkout; the submodule
            // directory will be populated by `git submodule update` later.
            continue;
        } else if is_tree {
            fs::create_dir_all(&full_path)?;
            checkout_tree(odb, &entry.oid, work_tree, &path)?;
        } else {
            // Regular file or symlink
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let blob = odb
                .read(&entry.oid)
                .with_context(|| format!("reading blob for {path}"))?;
            fs::write(&full_path, &blob.data)?;

            // Set executable bit if mode is 100755
            #[cfg(unix)]
            if entry.mode == 0o100755 {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(0o755);
                fs::set_permissions(&full_path, perms)?;
            }
        }
    }

    Ok(())
}

/// Write the index file from a tree (simple version).
fn write_index_from_tree(repo: &Repository, tree_oid: &ObjectId) -> Result<()> {
    use grit_lib::index::Index;

    // Try to build the index by reading the tree
    // Use grit's read-tree equivalent
    let index_path = repo.index_path();

    // We'll create a minimal approach: run the equivalent of `read-tree`
    // by adding entries from the tree
    let mut index = Index::new();
    add_tree_to_index(
        &repo.odb,
        tree_oid,
        "",
        &mut index,
        repo.work_tree.as_deref(),
    )?;
    index.write(&index_path).context("writing index")?;

    Ok(())
}

/// Recursively add tree entries to an index.
fn add_tree_to_index(
    odb: &grit_lib::odb::Odb,
    tree_oid: &ObjectId,
    prefix: &str,
    index: &mut grit_lib::index::Index,
    work_tree: Option<&Path>,
) -> Result<()> {
    use grit_lib::objects::parse_tree;

    let obj = odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let is_tree = (entry.mode & 0o170000) == 0o040000;
        let is_gitlink = entry.mode == 0o160000;
        if is_tree {
            add_tree_to_index(odb, &entry.oid, &path, index, work_tree)?;
        } else if is_gitlink {
            // Gitlink (submodule) — add to index with mode 160000 and
            // the commit OID, but no stat info (not checked out).
            index.add_or_replace(grit_lib::index::IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: 0o160000,
                uid: 0,
                gid: 0,
                size: 0,
                oid: entry.oid,
                flags: path.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path.as_bytes().to_vec(),
            });
        } else {
            // Get file stat info from the working tree if available
            let (ctime_sec, ctime_nsec, mtime_sec, mtime_nsec, dev, ino, uid, gid, size) =
                if let Some(wt) = work_tree {
                    let full = wt.join(&path);
                    if let Ok(meta) = fs::metadata(&full) {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            (
                                meta.ctime() as u32,
                                meta.ctime_nsec() as u32,
                                meta.mtime() as u32,
                                meta.mtime_nsec() as u32,
                                meta.dev() as u32,
                                meta.ino() as u32,
                                meta.uid(),
                                meta.gid(),
                                meta.size() as u32,
                            )
                        }
                        #[cfg(not(unix))]
                        (0, 0, 0, 0, 0, 0, 0, 0, 0)
                    } else {
                        (0, 0, 0, 0, 0, 0, 0, 0, 0)
                    }
                } else {
                    (0, 0, 0, 0, 0, 0, 0, 0, 0)
                };

            index.add_or_replace(grit_lib::index::IndexEntry {
                ctime_sec,
                ctime_nsec,
                mtime_sec,
                mtime_nsec,
                dev,
                ino,
                mode: entry.mode,
                uid,
                gid,
                size,
                oid: entry.oid,
                flags: path.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path.as_bytes().to_vec(),
            });
        }
    }

    Ok(())
}

/// Check if a path looks like a git bundle file.
fn is_bundle_file(path: &str) -> bool {
    let p = Path::new(path);
    if let Ok(mut f) = fs::File::open(p) {
        let mut buf = [0u8; 20];
        if let Ok(n) = std::io::Read::read(&mut f, &mut buf) {
            return buf[..n].starts_with(b"# v2 git bundle");
        }
    }
    false
}

/// Clone from a bundle file.
fn run_bundle_clone(args: Args) -> Result<()> {
    let bundle_path = PathBuf::from(&args.repository);
    let data = fs::read(&bundle_path)
        .with_context(|| format!("cannot read bundle '{}'", args.repository))?;

    // Parse bundle header
    let header_line = b"# v2 git bundle\n";
    if !data.starts_with(header_line) {
        bail!("not a v2 git bundle");
    }
    let mut pos = header_line.len();
    let mut refs: Vec<(String, grit_lib::objects::ObjectId)> = Vec::new();
    loop {
        let eol = data[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| pos + i)
            .ok_or_else(|| anyhow::anyhow!("truncated bundle header"))?;
        let line = &data[pos..eol];
        if line.is_empty() {
            pos = eol + 1;
            break;
        }
        let line_str = std::str::from_utf8(line)?;
        if line_str.starts_with('-') {
            pos = eol + 1;
            continue;
        }
        if let Some((hex, refname)) = line_str.split_once(' ') {
            let oid = grit_lib::objects::ObjectId::from_hex(hex)
                .map_err(|e| anyhow::anyhow!("bad oid in bundle: {e}"))?;
            refs.push((refname.to_string(), oid));
        }
        pos = eol + 1;
    }

    // Determine target directory
    let target_name = args.directory.unwrap_or_else(|| {
        let base = bundle_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        base
    });
    let target_path = PathBuf::from(&target_name);
    if target_path.exists() {
        bail!(
            "destination path '{}' already exists",
            target_path.display()
        );
    }

    if !args.quiet {
        eprintln!("Cloning into '{}'...", target_name);
    }

    // Figure out default branch from refs
    let head_branch = refs
        .iter()
        .find(|(r, _)| r == "HEAD")
        .and_then(|(_, head_oid)| {
            refs.iter()
                .find(|(r, oid)| r.starts_with("refs/heads/") && oid == head_oid)
                .map(|(r, _)| r.strip_prefix("refs/heads/").unwrap_or(r).to_string())
        })
        .or_else(|| {
            // If no HEAD, pick the first (or only) branch ref
            let branches: Vec<_> = refs
                .iter()
                .filter(|(r, _)| r.starts_with("refs/heads/"))
                .collect();
            if branches.len() == 1 {
                Some(
                    branches[0]
                        .0
                        .strip_prefix("refs/heads/")
                        .unwrap_or(&branches[0].0)
                        .to_string(),
                )
            } else {
                // Prefer main, then master, then first
                branches
                    .iter()
                    .find(|(r, _)| r.ends_with("/main"))
                    .or_else(|| branches.iter().find(|(r, _)| r.ends_with("/master")))
                    .or(branches.first())
                    .map(|(r, _)| r.strip_prefix("refs/heads/").unwrap_or(r).to_string())
            }
        })
        .unwrap_or_else(|| "master".to_string());

    // Initialize target repo
    fs::create_dir_all(&target_path)?;
    let dest = init_repository(&target_path, args.bare, &head_branch, None)
        .with_context(|| format!("failed to initialize '{}'", target_path.display()))?;

    // Unbundle pack data
    let pack_data = &data[pos..];
    if pack_data.len() >= 12 + 20 {
        let opts = grit_lib::unpack_objects::UnpackOptions {
            dry_run: false,
            quiet: true,
        };
        grit_lib::unpack_objects::unpack_objects(&mut &pack_data[..], &dest.odb, &opts)
            .map_err(|e| anyhow::anyhow!("unbundle failed: {e}"))?;
    }

    // Write refs as remote tracking refs under origin/
    for (refname, oid) in &refs {
        if refname == "HEAD" {
            continue;
        }
        // Write as remote tracking ref
        if let Some(branch) = refname.strip_prefix("refs/heads/") {
            let remote_ref_path = dest.git_dir.join("refs/remotes/origin").join(branch);
            if let Some(parent) = remote_ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&remote_ref_path, format!("{}\n", oid.to_hex()))?;
        }
        // Also write tags directly
        if refname.starts_with("refs/tags/") {
            let ref_path = dest.git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", oid.to_hex()))?;
        }
    }

    // Set HEAD to point to the default branch
    if let Some((_, oid)) = refs
        .iter()
        .find(|(r, _)| r == &format!("refs/heads/{}", head_branch))
    {
        let branch_ref_path = dest.git_dir.join("refs/heads").join(&head_branch);
        if let Some(parent) = branch_ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&branch_ref_path, format!("{}\n", oid.to_hex()))?;
    }

    // Set up origin remote config
    let bundle_abs = fs::canonicalize(&bundle_path).unwrap_or(bundle_path);
    let refspec = "+refs/heads/*:refs/remotes/origin/*".to_string();
    setup_origin_remote(&dest.git_dir, &bundle_abs, "origin", &refspec)?;

    // Checkout if not bare
    if !args.bare {
        checkout_head(&dest)?;
    }

    Ok(())
}
