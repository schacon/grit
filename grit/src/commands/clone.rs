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
}

pub fn run(args: Args) -> Result<()> {
    let source_path = PathBuf::from(&args.repository);

    // Open the source repository
    let source = open_source_repo(&source_path)
        .with_context(|| format!("'{}' does not appear to be a git repository", args.repository))?;

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
            eprintln!(
                "Cloning into bare repository '{}'...",
                target_name
            );
        } else {
            eprintln!("Cloning into '{}'...", target_name);
        }
    }

    // Determine the initial branch from source HEAD
    let head_branch = determine_head_branch(&source.git_dir, args.branch.as_deref())?;
    let initial_branch = head_branch.as_deref().unwrap_or("master");

    // Initialize the target repository
    fs::create_dir_all(&target_path)
        .with_context(|| format!("cannot create directory '{}'", target_path.display()))?;

    let dest = init_repository(&target_path, args.bare, initial_branch, None)
        .with_context(|| format!("failed to initialize '{}'", target_path.display()))?;

    // Copy all objects from source to destination
    copy_objects(&source.git_dir, &dest.git_dir)
        .context("copying objects")?;

    let remote_name = "origin";

    if args.bare {
        // Bare clone: copy refs directly (mirror-style), no remote tracking
        copy_refs_direct(&source.git_dir, &dest.git_dir)
            .context("copying refs")?;

        // Set up remote config (URL only, no fetch refspec for bare)
        setup_origin_remote_bare(&dest.git_dir, &source_path, remote_name)
            .context("setting up origin remote")?;

        // Set HEAD to match source
        if let Some(ref branch) = head_branch {
            fs::write(
                dest.git_dir.join("HEAD"),
                format!("ref: refs/heads/{branch}\n"),
            )?;
        }
    } else {
        // Non-bare clone: copy refs as remote-tracking refs
        copy_refs_as_remote(&source.git_dir, &dest.git_dir, remote_name)
            .context("copying refs")?;

        // Set up remote "origin" in config
        setup_origin_remote(&dest.git_dir, &source_path, remote_name)
            .context("setting up origin remote")?;

        // Set HEAD to the chosen branch if it exists in remote refs
        if let Some(ref branch) = head_branch {
            let remote_ref = dest.git_dir.join("refs/remotes").join(remote_name).join(branch);
            if remote_ref.exists() {
                let oid_str = fs::read_to_string(&remote_ref)
                    .context("reading remote ref")?;
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
        }
    }

    // Handle shallow depth config (informational only, not actual shallow)
    if let Some(_depth) = args.depth {
        let shallow_path = dest.git_dir.join("shallow");
        // Write the current HEAD as the shallow boundary
        if let Ok(head_content) = fs::read_to_string(dest.git_dir.join("refs/heads").join(head_branch.as_deref().unwrap_or("master"))) {
            fs::write(&shallow_path, head_content.trim())?;
        }
    }

    // Checkout working tree unless --bare or --no-checkout
    if !args.bare && !args.no_checkout {
        checkout_head(&dest).context("checking out HEAD")?;
    }

    if !args.quiet {
        eprintln!("done.");
    }

    Ok(())
}

/// Open a source repository (bare or non-bare).
fn open_source_repo(path: &Path) -> Result<Repository> {
    // Try as-is first (might be a bare repo or .git dir)
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    // Try path/.git for non-bare repos
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

/// Copy all objects (loose + packs) from source to destination.
fn copy_objects(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    let src_objects = src_git_dir.join("objects");
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
) -> Result<()> {
    let src_refs_heads = src_git_dir.join("refs/heads");
    let dst_remotes = dst_git_dir.join("refs/remotes").join(remote_name);

    // Copy refs/heads/* → refs/remotes/<remote>/*
    if src_refs_heads.is_dir() {
        copy_refs_recursive(&src_refs_heads, &dst_remotes)?;
    }

    // Copy refs/tags/* → refs/tags/* (tags are shared)
    let src_tags = src_git_dir.join("refs/tags");
    let dst_tags = dst_git_dir.join("refs/tags");
    if src_tags.is_dir() {
        copy_refs_recursive(&src_tags, &dst_tags)?;
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
            let Some(refname) = parts.next() else { continue };

            if let Some(branch) = refname.strip_prefix("refs/heads/") {
                let dst_ref = dst_remotes.join(branch);
                if let Some(parent) = dst_ref.parent() {
                    fs::create_dir_all(parent)?;
                }
                // Don't overwrite loose refs (they're more up-to-date)
                if !dst_ref.exists() {
                    fs::write(&dst_ref, format!("{oid}\n"))?;
                }
            } else if refname.starts_with("refs/tags/") {
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

/// Recursively copy ref files from src to dst.
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
fn copy_refs_direct(
    src_git_dir: &Path,
    dst_git_dir: &Path,
) -> Result<()> {
    // Copy refs/heads/* → refs/heads/*
    let src_refs_heads = src_git_dir.join("refs/heads");
    let dst_refs_heads = dst_git_dir.join("refs/heads");
    if src_refs_heads.is_dir() {
        copy_refs_recursive(&src_refs_heads, &dst_refs_heads)?;
    }

    // Copy refs/tags/* → refs/tags/*
    let src_tags = src_git_dir.join("refs/tags");
    let dst_tags = dst_git_dir.join("refs/tags");
    if src_tags.is_dir() {
        copy_refs_recursive(&src_tags, &dst_tags)?;
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
            let Some(refname) = parts.next() else { continue };

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
    config.set(
        &format!("remote.{remote_name}.fetch"),
        &format!("+refs/heads/*:refs/remotes/{remote_name}/*"),
    )?;
    config.write().context("writing config")?;

    Ok(())
}

/// Set up the "origin" remote for a bare clone (URL only, no fetch refspec).
fn setup_origin_remote_bare(
    git_dir: &Path,
    source_path: &Path,
    remote_name: &str,
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
    config.write().context("writing config")?;

    Ok(())
}

/// Set up branch tracking configuration (branch.<name>.remote and branch.<name>.merge).
fn setup_branch_tracking(
    git_dir: &Path,
    branch: &str,
    remote_name: &str,
) -> Result<()> {
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

    // Default to master
    Ok(Some("master".to_string()))
}

/// Perform a basic checkout of HEAD into the working tree.
fn checkout_head(repo: &Repository) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(wt) => wt,
        None => return Ok(()), // Bare repo
    };

    // Read HEAD
    let head_content = fs::read_to_string(repo.git_dir.join("HEAD"))
        .context("reading HEAD")?;
    let head = head_content.trim();

    // Resolve to an OID
    let oid = if let Some(refname) = head.strip_prefix("ref: ") {
        let ref_path = repo.git_dir.join(refname);
        let oid_str = fs::read_to_string(&ref_path)
            .with_context(|| format!("reading ref {refname}"))?;
        ObjectId::from_hex(oid_str.trim())
            .with_context(|| format!("invalid OID in {refname}"))?
    } else {
        ObjectId::from_hex(head)
            .context("invalid OID in HEAD")?
    };

    // Read the commit to get the tree
    let obj = repo.odb.read(&oid).context("reading HEAD commit")?;
    let commit = parse_commit(&obj.data).context("parsing HEAD commit")?;

    // Checkout the tree recursively
    checkout_tree(&repo.odb, &commit.tree, work_tree, "")?;

    // Write the index
    // Use grit's checkout-index style — we'll build a simple index
    // For now just write files; a proper index update would use the Index type
    write_index_from_tree(&repo, &commit.tree)?;

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
        if is_tree {
            fs::create_dir_all(&full_path)?;
            checkout_tree(odb, &entry.oid, work_tree, &path)?;
        } else {
            // Regular file or symlink
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let blob = odb.read(&entry.oid)
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
    add_tree_to_index(&repo.odb, tree_oid, "", &mut index, repo.work_tree.as_deref())?;
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
        if is_tree {
            add_tree_to_index(odb, &entry.oid, &path, index, work_tree)?;
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
