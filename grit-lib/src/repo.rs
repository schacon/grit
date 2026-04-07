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

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
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

        validate_repository_format(&git_dir)?;

        if !git_dir.join("HEAD").exists() {
            return Err(Error::NotARepository(git_dir.display().to_string()));
        }

        // For git worktrees the `objects/` directory lives in the common git
        // directory pointed to by the `commondir` file.
        let objects_dir = if git_dir.join("objects").exists() {
            git_dir.join("objects")
        } else if let Some(common_dir) = resolve_common_dir(&git_dir) {
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
            let work_tree = env::var("GIT_WORK_TREE").ok().map(PathBuf::from);
            if work_tree.is_some() {
                return Self::open(&git_dir, work_tree.as_deref());
            }
            // When GIT_DIR is set without GIT_WORK_TREE, infer the work tree
            // from the parent of the git directory (standard layout).
            let mut repo = Self::open(&git_dir, None)?;
            if repo.work_tree.is_none() {
                let canonical = git_dir.canonicalize().unwrap_or_else(|_| git_dir.clone());
                // Check core.bare config
                let config_path = canonical.join("config");
                let is_bare = if config_path.exists() {
                    fs::read_to_string(&config_path)
                        .ok()
                        .and_then(|c| {
                            c.lines()
                                .find(|l| {
                                    let trimmed = l.trim();
                                    trimmed.starts_with("bare") && trimmed.contains("true")
                                })
                                .map(|_| true)
                        })
                        .unwrap_or(false)
                } else {
                    false
                };
                if !is_bare {
                    repo.work_tree = canonical.parent().map(|p| p.to_path_buf());
                }
            }
            return Ok(repo);
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

/// Resolve the common git directory for linked worktrees.
fn resolve_common_dir(git_dir: &Path) -> Option<PathBuf> {
    let common_raw = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common_rel = common_raw.trim();
    if common_rel.is_empty() {
        return None;
    }
    let common_dir = if Path::new(common_rel).is_absolute() {
        PathBuf::from(common_rel)
    } else {
        git_dir.join(common_rel)
    };
    Some(common_dir.canonicalize().unwrap_or(common_dir))
}

/// Determine the config file path for a repository or linked worktree.
fn repository_config_path(git_dir: &Path) -> Option<PathBuf> {
    let local = git_dir.join("config");
    if local.exists() {
        return Some(local);
    }
    let common = resolve_common_dir(git_dir)?;
    let shared = common.join("config");
    if shared.exists() {
        Some(shared)
    } else {
        None
    }
}

/// Validate core repository format/version compatibility.
///
/// Supports repository format versions 0 and 1, with extension handling that
/// matches Git's compatibility expectations in upstream repo-version tests.
fn validate_repository_format(git_dir: &Path) -> Result<()> {
    let Some(config_path) = repository_config_path(git_dir) else {
        return Ok(());
    };

    let content = fs::read_to_string(&config_path).map_err(Error::Io)?;
    let mut in_core = false;
    let mut in_extensions = false;
    let mut repo_version = 0u32;
    let mut extensions = BTreeSet::new();

    for raw_line in content.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') {
            let Some(end_idx) = line.find(']') else {
                return Err(Error::ConfigError(format!(
                    "invalid config in {}",
                    config_path.display()
                )));
            };

            let section = line[1..end_idx].trim();
            let section_name = section
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            in_core = section_name == "core";
            in_extensions = section_name == "extensions";

            let remainder = line[end_idx + 1..].trim();
            if remainder.is_empty() || remainder.starts_with('#') || remainder.starts_with(';') {
                continue;
            }
            line = remainder;
        }

        if in_core {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("repositoryformatversion") {
                    repo_version = value.trim().parse::<u32>().map_err(|_| {
                        Error::ConfigError(format!(
                            "invalid core.repositoryformatversion in {}",
                            config_path.display()
                        ))
                    })?;
                }
            }
        }

        if in_extensions {
            let key = if let Some((key, _)) = line.split_once('=') {
                key.trim()
            } else {
                line
            };
            if !key.is_empty() {
                extensions.insert(key.to_ascii_lowercase());
            }
        }
    }

    if repo_version > 1 {
        return Err(Error::UnsupportedRepositoryFormatVersion(repo_version));
    }

    for extension in extensions {
        if repo_version == 0 {
            if extension.ends_with("-v1") {
                return Err(Error::UnsupportedRepositoryExtension(extension));
            }
            continue;
        }

        if matches!(
            extension.as_str(),
            "noop"
                | "noop-v1"
                | "preciousobjects"
                | "partialclone"
                | "worktreeconfig"
                | "objectformat"
                | "compatobjectformat"
                | "refstorage"
        ) {
            continue;
        }

        return Err(Error::UnsupportedRepositoryExtension(extension));
    }

    Ok(())
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
        let repo = Repository::open(&git_dir, Some(dir))?;
        if env::var("GIT_TEST_ASSUME_DIFFERENT_OWNER").ok().as_deref() == Some("1")
            && !safe_directory_allows_repo_path(dir, &repo.git_dir)
        {
            return Err(Error::DubiousOwnership(dir.display().to_string()));
        }
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
                if env::var("GIT_TEST_ASSUME_DIFFERENT_OWNER").ok().as_deref() == Some("1")
                    && !safe_directory_allows_repo_path(dir, &repo.git_dir)
                {
                    return Err(Error::DubiousOwnership(dir.display().to_string()));
                }
                return Ok(Some(repo));
            }
            Err(Error::NotARepository(_)) => return Ok(None),
            Err(e) => return Err(e),
        }
    }

    // Check if `dir` itself is a repository root candidate (objects/ + HEAD).
    if dir.join("objects").is_dir() && dir.join("HEAD").is_file() {
        emit_implicit_bare_repository_trace(dir);
        let repo = Repository::open(dir, None)?;
        // Check safe.bareRepository policy before opening implicit bare repos.
        // This is only respected in protected config (system/global/command).
        if repo.is_bare() && safe_bare_repository_explicit(&repo.git_dir) {
            return Err(Error::ForbiddenBareRepository(dir.display().to_string()));
        }
        if env::var("GIT_TEST_ASSUME_DIFFERENT_OWNER").ok().as_deref() == Some("1")
            && !safe_directory_allows_repo_path(dir, &repo.git_dir)
        {
            return Err(Error::DubiousOwnership(dir.display().to_string()));
        }
        return Ok(Some(repo));
    }

    Ok(None)
}

fn safe_bare_repository_explicit(git_dir: &Path) -> bool {
    if let Ok(cfg) = crate::config::ConfigSet::load(Some(git_dir), true) {
        if let Some(val) = cfg.get("safe.bareRepository") {
            return val.eq_ignore_ascii_case("explicit");
        }
    }
    false
}

fn emit_implicit_bare_repository_trace(path: &Path) {
    let Ok(dest) = env::var("GIT_TRACE2_PERF") else {
        return;
    };
    let line = format!("implicit-bare-repository:{}\n", path.display());
    match dest.as_str() {
        "1" | "2" | "true" => {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        _ => {
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(dest) {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }
}

fn normalize_safe_directory_candidate(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn safe_directory_pattern_matches(candidate: &Path, pattern: &str, cwd: &Path) -> bool {
    let candidate = normalize_safe_directory_candidate(candidate);
    let mut configured = pattern.trim().to_owned();
    if configured.is_empty() {
        return false;
    }
    if configured == "*" {
        return true;
    }
    if let Some(rest) = configured.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            configured = Path::new(&home).join(rest).to_string_lossy().into_owned();
        }
    }
    let wildcard = configured.ends_with("/*");
    let configured_path = if wildcard {
        PathBuf::from(configured.trim_end_matches("/*"))
    } else if configured == "." {
        cwd.to_path_buf()
    } else {
        PathBuf::from(&configured)
    };
    let configured_path = normalize_safe_directory_candidate(&configured_path);
    if wildcard {
        candidate.starts_with(&configured_path)
    } else {
        candidate == configured_path
    }
}

fn safe_directory_allows_repo_path(repo_path: &Path, _git_dir: &Path) -> bool {
    let Ok(cfg) = crate::config::ConfigSet::load(None, true) else {
        return false;
    };
    let mut values = cfg.get_all("safe.directory");
    if values.is_empty() {
        return false;
    }

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd = normalize_safe_directory_candidate(&cwd);
    let repo_path = normalize_safe_directory_candidate(repo_path);

    let mut allowed = false;
    for value in values.drain(..) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            allowed = false;
            continue;
        }
        if safe_directory_pattern_matches(&repo_path, trimmed, &cwd) {
            allowed = true;
        }
    }
    allowed
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
