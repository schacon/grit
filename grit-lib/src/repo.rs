//! Repository discovery and the primary `Repository` handle.
//!
//! # Discovery
//!
//! [`Repository::discover`] walks up from a starting directory to find the
//! nearest `.git` directory (or bare repository), honouring `GIT_DIR` and
//! `GIT_WORK_TREE` environment variables and the `.git` gitfile indirection.
//!
//! # Structure
//!
//! A [`Repository`] owns:
//!
//! - `git_dir` — absolute path to the `.git` directory (or the repo root for
//!   bare repos).
//! - `work_tree` — `Some(path)` for non-bare repos, `None` for bare.
//! - [`Odb`] — the loose object database.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::odb::Odb;

/// A handle to an open Git repository.
#[derive(Debug)]
pub struct Repository {
    /// Absolute path to the git directory (`.git/` or bare repo root).
    pub git_dir: PathBuf,
    /// Absolute path to the working tree, or `None` for bare repos.
    pub work_tree: Option<PathBuf>,
    /// Loose object database.
    pub odb: Odb,
}

impl Repository {
    /// Open a repository from an explicit git-dir and optional work-tree.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotARepository`] if `git_dir` does not look like a
    /// valid git directory (missing `objects/`, `HEAD`, etc.).
    pub fn open(git_dir: &Path, work_tree: Option<&Path>) -> Result<Self> {
        let git_dir = git_dir
            .canonicalize()
            .map_err(|_| Error::NotARepository(git_dir.display().to_string()))?;

        if !git_dir.join("HEAD").exists() {
            return Err(Error::NotARepository(git_dir.display().to_string()));
        }

        // For git worktrees the `objects/` directory lives in the common git
        // directory pointed to by the `commondir` file.
        let objects_dir = if git_dir.join("objects").exists() {
            git_dir.join("objects")
        } else if let Ok(common_raw) = fs::read_to_string(git_dir.join("commondir")) {
            let common_rel = common_raw.trim();
            let common_dir = if Path::new(common_rel).is_absolute() {
                PathBuf::from(common_rel)
            } else {
                git_dir.join(common_rel)
            };
            let common_dir = common_dir
                .canonicalize()
                .map_err(|_| Error::NotARepository(git_dir.display().to_string()))?;
            common_dir.join("objects")
        } else {
            return Err(Error::NotARepository(git_dir.display().to_string()));
        };

        if !objects_dir.exists() {
            return Err(Error::NotARepository(git_dir.display().to_string()));
        }

        let work_tree = match work_tree {
            Some(p) => Some(
                p.canonicalize()
                    .map_err(|_| Error::PathError(p.display().to_string()))?,
            ),
            None => None,
        };

        let odb = if let Some(ref wt) = work_tree {
            Odb::with_work_tree(&objects_dir, wt)
        } else {
            Odb::new(&objects_dir)
        };

        Ok(Self {
            git_dir,
            work_tree,
            odb,
        })
    }

    /// Discover the repository starting from `start` (defaults to cwd if `None`).
    ///
    /// Checks `GIT_DIR` first; if set, uses it directly.  Otherwise walks up
    /// the directory tree looking for `.git` (regular directory or gitfile).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotARepository`] if no repository can be found.
    pub fn discover(start: Option<&Path>) -> Result<Self> {
        // GIT_DIR override
        if let Ok(dir) = env::var("GIT_DIR") {
            let git_dir = PathBuf::from(&dir);
            let cwd = env::current_dir()?;
            let git_dir_abs = if git_dir.is_absolute() {
                git_dir.clone()
            } else {
                cwd.join(&git_dir)
            };
            let mut effective_git_dir = git_dir.clone();
            let mut inferred_work_tree_from_gitfile: Option<PathBuf> = None;
            if git_dir_abs.is_file() {
                let content =
                    fs::read_to_string(&git_dir_abs).map_err(|e| Error::NotARepository(e.to_string()))?;
                let base = git_dir_abs.parent().unwrap_or(&cwd);
                effective_git_dir = parse_gitfile(&content, base)?;
                inferred_work_tree_from_gitfile = git_dir_abs.parent().map(PathBuf::from);
            }
            if let Ok(wt_raw) = env::var("GIT_WORK_TREE") {
                let wt = if Path::new(&wt_raw).is_absolute() {
                    PathBuf::from(wt_raw)
                } else {
                    cwd.join(wt_raw)
                };
                return Self::open(&effective_git_dir, Some(&wt));
            }

            // With GIT_DIR set and no explicit GIT_WORK_TREE:
            // - if core.worktree is set, honor it
            // - else if core.bare=true, treat as bare
            // - else default worktree is the current working directory
            //   (matches Git's --git-dir behavior).
            let canonical_git = effective_git_dir
                .canonicalize()
                .unwrap_or_else(|_| effective_git_dir.clone());
            let (core_bare, core_worktree) = read_core_bare_and_worktree(&canonical_git);
            if let Some(wt) = core_worktree {
                return Self::open(&effective_git_dir, Some(&wt));
            }
            if core_bare.unwrap_or(false) {
                return Self::open(&effective_git_dir, None);
            }
            if let Some(wt) = inferred_work_tree_from_gitfile {
                return Self::open(&effective_git_dir, Some(&wt));
            }
            return Self::open(&effective_git_dir, Some(&cwd));
        }

        // If GIT_WORK_TREE is set without GIT_DIR, we still need to honor it
        // after discovery.
        let env_work_tree = env::var("GIT_WORK_TREE").ok().map(PathBuf::from);

        let cwd = env::current_dir()?;
        let start = start.unwrap_or(&cwd);
        let start = if start.is_absolute() {
            start.to_path_buf()
        } else {
            cwd.join(start)
        };

        // Parse GIT_CEILING_DIRECTORIES — colon-separated list of absolute
        // directory paths that limit upward repository discovery.
        let ceiling_dirs = parse_ceiling_directories();

        let mut current = start.as_path();
        let mut first = true;
        loop {
            // On the first iteration we always check the starting directory.
            // On subsequent iterations, check whether this parent directory is
            // blocked by a ceiling entry before probing for .git.
            if !first && is_ceiling_blocked(current, &ceiling_dirs) {
                break;
            }
            first = false;

            if let Some(mut repo) = try_open_at(current)? {
                // Override work_tree with GIT_WORK_TREE env if set
                if let Some(ref wt) = env_work_tree {
                    repo.work_tree = Some(wt.canonicalize().unwrap_or_else(|_| wt.clone()));
                }
                return Ok(repo);
            }
            match current.parent() {
                Some(p) => current = p,
                None => break,
            }
        }

        Err(Error::NotARepository(start.display().to_string()))
    }

    /// Path to the index file.
    #[must_use]
    pub fn index_path(&self) -> PathBuf {
        self.git_dir.join("index")
    }

    /// Path to the `refs/` directory.
    #[must_use]
    pub fn refs_dir(&self) -> PathBuf {
        self.git_dir.join("refs")
    }

    /// Path to `HEAD`.
    #[must_use]
    pub fn head_path(&self) -> PathBuf {
        self.git_dir.join("HEAD")
    }

    /// Whether this is a bare repository (no working tree).
    #[must_use]
    pub fn is_bare(&self) -> bool {
        if let Ok(cfg) = crate::config::ConfigSet::load(Some(&self.git_dir), true) {
            if let Some(Ok(v)) = cfg.get_bool("core.bare") {
                return v;
            }
        }
        if self.work_tree.is_some() {
            return false;
        }
        // No core.bare setting — if work_tree is None, assume bare
        true
    }

    /// Read an object, transparently following replace refs.
    ///
    /// If `refs/replace/<hex>` exists for the requested OID and
    /// `GIT_NO_REPLACE_OBJECTS` is **not** set, this reads the
    /// replacement object instead.  Otherwise it behaves identically
    /// to `self.odb.read(oid)`.
    pub fn read_replaced(&self, oid: &crate::objects::ObjectId) -> Result<crate::objects::Object> {
        if std::env::var_os("GIT_NO_REPLACE_OBJECTS").is_some() {
            return self.odb.read(oid);
        }
        let replace_ref = self.git_dir.join(format!("refs/replace/{}", oid.to_hex()));
        if replace_ref.is_file() {
            if let Ok(content) = std::fs::read_to_string(&replace_ref) {
                let hex = content.trim();
                if let Ok(replacement_oid) = hex.parse::<crate::objects::ObjectId>() {
                    if let Ok(obj) = self.odb.read(&replacement_oid) {
                        return Ok(obj);
                    }
                }
            }
        }
        self.odb.read(oid)
    }
}

fn read_core_bare_and_worktree(git_dir: &Path) -> (Option<bool>, Option<PathBuf>) {
    let config_path = git_dir.join("config");
    let Ok(content) = fs::read_to_string(&config_path) else {
        return (None, None);
    };

    let mut in_core = false;
    let mut bare = None;
    let mut worktree = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_core = trimmed.eq_ignore_ascii_case("[core]");
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if key.eq_ignore_ascii_case("bare") {
            let v = value.to_ascii_lowercase();
            bare = match v.as_str() {
                "true" | "yes" | "on" | "1" => Some(true),
                "false" | "no" | "off" | "0" => Some(false),
                _ => bare,
            };
        } else if key.eq_ignore_ascii_case("worktree") && !value.is_empty() {
            let path = PathBuf::from(value);
            let abs = if path.is_absolute() {
                path
            } else {
                git_dir.join(path)
            };
            worktree = Some(abs.canonicalize().unwrap_or(abs));
        }
    }

    (bare, worktree)
}

/// Try to open a repository rooted exactly at `dir`.
///
/// Returns `Ok(None)` when `dir` is not a repository root (the caller should
/// walk up); returns `Err` on a structural problem.
fn try_open_at(dir: &Path) -> Result<Option<Repository>> {
    let dot_git = dir.join(".git");

    // Check for special file types (FIFO, socket, etc.) — reject them
    // instead of walking up to a parent repository.
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(meta) = fs::symlink_metadata(&dot_git) {
            let ft = meta.file_type();
            if ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device() {
                return Err(Error::NotARepository(format!(
                    "invalid gitfile format: '{}' is not a regular file",
                    dot_git.display()
                )));
            }
            // A symlink may point to an unsupported file type (e.g. FIFO).
            // Reject it explicitly instead of silently walking up.
            if ft.is_symlink() {
                if let Ok(target_meta) = fs::metadata(&dot_git) {
                    let tft = target_meta.file_type();
                    if tft.is_fifo()
                        || tft.is_socket()
                        || tft.is_block_device()
                        || tft.is_char_device()
                    {
                        return Err(Error::NotARepository(format!(
                            "invalid gitfile format: '{}' is not a regular file",
                            dot_git.display()
                        )));
                    }
                }
            }
            if ft.is_symlink() && !dot_git.exists() {
                return Err(Error::NotARepository(format!(
                    "invalid gitfile format: '{}' is not a regular file",
                    dot_git.display()
                )));
            }
        }
    }

    if dot_git.is_file() {
        // gitfile indirection: file contains "gitdir: <path>"
        let content =
            fs::read_to_string(&dot_git).map_err(|e| Error::NotARepository(e.to_string()))?;
        let git_dir = parse_gitfile(&content, dir)?;
        let mut repo = Repository::open(&git_dir, Some(dir))?;
        apply_core_worktree_override(&mut repo);
        return Ok(Some(repo));
    }

    if dot_git.is_dir() {
        // If .git is a symlink to a directory, resolve the symlink target
        // for validation but keep the original .git path for user-facing output
        // (matches real git behavior: `rev-parse --git-dir` shows `.git`).
        let open_path = if dot_git.is_symlink() {
            // Resolve the symlink target for validation
            dot_git.read_link().unwrap_or_else(|_| dot_git.clone())
        } else {
            dot_git.clone()
        };
        // Try to open; if the directory is empty or invalid, continue
        // walking up (e.g. an empty .git/ directory should be ignored).
        match Repository::open(&open_path, Some(dir)) {
            Ok(mut repo) => {
                // Restore the original path so rev-parse shows .git not the
                // resolved symlink target.
                if dot_git.is_symlink() {
                    let abs_dot_git = if dot_git.is_absolute() {
                        dot_git
                    } else {
                        dir.join(".git")
                    };
                    repo.git_dir = abs_dot_git;
                }
                apply_core_worktree_override(&mut repo);
                return Ok(Some(repo));
            }
            Err(Error::NotARepository(_)) => return Ok(None),
            Err(e) => return Err(e),
        }
    }

    // Check if `dir` itself is a bare repo (has objects/ and HEAD directly)
    if dir.join("objects").is_dir() && dir.join("HEAD").is_file() {
        // Check safe.bareRepository policy before opening bare repos.
        // When set to "explicit", implicit bare repo discovery is forbidden
        // unless GIT_DIR was set (handled earlier in discover()).
        if let Ok(cfg) = crate::config::ConfigSet::load(None, true) {
            if let Some(val) = cfg.get("safe.bareRepository") {
                if val == "explicit" {
                    return Err(Error::ForbiddenBareRepository(dir.display().to_string()));
                }
            }
        }
        let repo = Repository::open(dir, None)?;
        return Ok(Some(repo));
    }

    Ok(None)
}

fn apply_core_worktree_override(repo: &mut Repository) {
    let (core_bare, core_worktree) = read_core_bare_and_worktree(&repo.git_dir);
    if let Some(wt) = core_worktree {
        repo.work_tree = Some(wt);
    } else if core_bare == Some(true) {
        repo.work_tree = None;
    }
}

/// Parse a gitfile's `"gitdir: <path>"` line.
fn parse_gitfile(content: &str, base: &Path) -> Result<PathBuf> {
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
    Err(Error::NotARepository("invalid gitfile format".to_owned()))
}

/// Initialise a new Git repository at the given path.
///
/// Creates the standard directory skeleton (objects/, refs/heads/, refs/tags/,
/// info/, hooks/) and a default `HEAD` pointing to `refs/heads/<initial_branch>`.
///
/// # Parameters
///
/// - `path` — root directory to initialise (created if absent).
/// - `bare` — if true, `path` itself becomes the git-dir; otherwise `path/.git`.
/// - `initial_branch` — branch name for `HEAD` (e.g. `"main"`).
/// - `template_dir` — optional template directory; if `None`, a minimal skeleton
///   is created.
///
/// # Errors
///
/// Returns [`Error::Io`] on filesystem failures.
pub fn init_repository(
    path: &Path,
    bare: bool,
    initial_branch: &str,
    template_dir: Option<&Path>,
) -> Result<Repository> {
    let git_dir = if bare {
        path.to_path_buf()
    } else {
        path.join(".git")
    };

    // Create directory structure
    for sub in &[
        "objects",
        "objects/info",
        "objects/pack",
        "refs",
        "refs/heads",
        "refs/tags",
        "info",
        "hooks",
    ] {
        fs::create_dir_all(git_dir.join(sub))?;
    }

    // Copy template files if a template dir was given
    if let Some(tmpl) = template_dir {
        if tmpl.is_dir() {
            copy_template(tmpl, &git_dir)?;
        }
    }

    // Write HEAD
    let head_content = format!("ref: refs/heads/{initial_branch}\n");
    fs::write(git_dir.join("HEAD"), head_content)?;

    // Write config (minimal)
    let config_content = if bare {
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n"
    } else {
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n"
    };
    fs::write(git_dir.join("config"), config_content)?;

    // Write description
    fs::write(
        git_dir.join("description"),
        "Unnamed repository; edit this file 'description' to name the repository.\n",
    )?;

    let work_tree = if bare { None } else { Some(path) };
    Repository::open(&git_dir, work_tree)
}

/// Recursively copy template files from `src` to `dst`.
fn copy_template(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_template(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Parse `GIT_CEILING_DIRECTORIES` into a list of canonical absolute paths.
///
/// The variable is colon-separated (`:`) on Unix.  Empty entries and
/// non-absolute paths are silently skipped, matching Git's behaviour.
fn parse_ceiling_directories() -> Vec<PathBuf> {
    let raw = match env::var("GIT_CEILING_DIRECTORIES") {
        Ok(val) => val,
        Err(_) => return Vec::new(),
    };
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(':')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let p = PathBuf::from(s);
            if !p.is_absolute() {
                return None;
            }
            // Canonicalize to resolve symlinks; fall back to the raw path
            // (with trailing slashes stripped) when the directory doesn't exist.
            Some(p.canonicalize().unwrap_or_else(|_| {
                // Strip trailing slashes for consistent comparison
                let s = s.trim_end_matches('/');
                PathBuf::from(s)
            }))
        })
        .collect()
}

/// Check whether `dir` is blocked by any ceiling directory.
///
/// A ceiling directory `C` prevents looking at `C` itself and any of its
/// ancestors during the upward walk.  Directories strictly below `C` are
/// not blocked — i.e. if `dir` is a child of `C`, the walk may still look
/// there.
///
/// In path terms: `dir` is blocked when the ceiling IS `dir` or IS a
/// descendant of `dir` (meaning `ceil.starts_with(dir)`).
fn is_ceiling_blocked(dir: &Path, ceilings: &[PathBuf]) -> bool {
    if ceilings.is_empty() {
        return false;
    }
    // Canonicalize `dir` for reliable comparison; if it fails (e.g. the path
    // doesn't exist) fall back to the raw path.
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    for ceil in ceilings {
        // Block when the walk has reached exactly the ceiling directory.
        // Git's semantics: the ceiling prevents looking at the ceiling
        // itself and anything above it.  Since we walk upward, once we hit
        // the ceiling we stop.
        if canon == *ceil {
            return true;
        }
    }
    false
}
