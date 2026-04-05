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

        let odb = Odb::new(&objects_dir);

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
            let git_dir = PathBuf::from(dir);
            let work_tree = env::var("GIT_WORK_TREE").ok().map(PathBuf::from);
            return Self::open(&git_dir, work_tree.as_deref());
        }

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

            if let Some(repo) = try_open_at(current)? {
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
        if self.work_tree.is_some() {
            return false;
        }
        // Check core.bare in the repo config.  A .git directory of a
        // non-bare repo has objects/ and HEAD but core.bare=false.
        let config_path = self.git_dir.join("config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            let mut in_core = false;
            for line in content.lines() {
                let t = line.trim();
                if t.starts_with('[') {
                    in_core = t.eq_ignore_ascii_case("[core]");
                    continue;
                }
                if in_core {
                    if let Some((k, v)) = t.split_once('=') {
                        if k.trim().eq_ignore_ascii_case("bare") {
                            return v.trim().eq_ignore_ascii_case("true");
                        }
                    }
                }
            }
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
                    ".git exists but is not a valid file or directory: {}",
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
        let repo = Repository::open(&git_dir, Some(dir))?;
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
    Err(Error::NotARepository(
        "gitfile does not contain 'gitdir:' line".to_owned(),
    ))
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
