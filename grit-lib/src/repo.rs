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

        let mut current = start.as_path();
        loop {
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
        self.work_tree.is_none()
    }
}

/// Try to open a repository rooted exactly at `dir`.
///
/// Returns `Ok(None)` when `dir` is not a repository root (the caller should
/// walk up); returns `Err` on a structural problem.
fn try_open_at(dir: &Path) -> Result<Option<Repository>> {
    let dot_git = dir.join(".git");

    if dot_git.is_file() {
        // gitfile indirection: file contains "gitdir: <path>"
        let content =
            fs::read_to_string(&dot_git).map_err(|e| Error::NotARepository(e.to_string()))?;
        let git_dir = parse_gitfile(&content, dir)?;
        let repo = Repository::open(&git_dir, Some(dir))?;
        return Ok(Some(repo));
    }

    if dot_git.is_dir() {
        let repo = Repository::open(&dot_git, Some(dir))?;
        return Ok(Some(repo));
    }

    // Check if `dir` itself is a bare repo (has objects/ and HEAD directly)
    if dir.join("objects").is_dir() && dir.join("HEAD").is_file() {
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
