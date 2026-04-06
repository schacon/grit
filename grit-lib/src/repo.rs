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
use std::fs::OpenOptions;
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
    /// Discovery provenance: true when opened via `GIT_DIR` env or explicit API.
    ///
    /// This suppresses safe.bareRepository implicit checks.
    pub explicit_git_dir: bool,
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
            explicit_git_dir: false,
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
                let mut repo = Self::open(&git_dir, work_tree.as_deref())?;
                repo.explicit_git_dir = true;
                return Ok(repo);
            }
            // When GIT_DIR is set without GIT_WORK_TREE, Git treats the
            // current directory as the work tree for non-bare repositories.
            let mut repo = Self::open(&git_dir, None)?;
            if repo.work_tree.is_none() {
                // Check core.bare config
                let config_path = repo.git_dir.join("config");
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
                    let cwd = env::current_dir()?;
                    repo.work_tree = Some(cwd.canonicalize().unwrap_or(cwd));
                }
            }
            repo.explicit_git_dir = true;
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
                repo.enforce_safe_directory()?;
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
        let replace_base = std::env::var("GIT_REPLACE_REF_BASE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "refs/replace/".to_owned());
        let replace_base = if replace_base.ends_with('/') {
            replace_base
        } else {
            format!("{replace_base}/")
        };
        let replace_ref = self
            .git_dir
            .join(format!("{}{}", replace_base, oid.to_hex()));
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
/// Public wrapper for validate_repository_format.
pub fn validate_repo_format(git_dir: &Path) -> Result<()> {
    validate_repository_format(git_dir)
}

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
                    "invalid gitfile format: {} is not a regular file",
                    dot_git.display()
                )));
            }
            if ft.is_symlink() {
                if let Ok(target_meta) = fs::metadata(&dot_git) {
                    let tft = target_meta.file_type();
                    if tft.is_fifo()
                        || tft.is_socket()
                        || tft.is_block_device()
                        || tft.is_char_device()
                    {
                        return Err(Error::NotARepository(format!(
                            "invalid gitfile format: {} is not a regular file",
                            dot_git.display()
                        )));
                    }
                }
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

    // Linked-worktree gitdir/admin directories contain HEAD and commondir,
    // and can be opened as repositories even without a local objects/ dir.
    if dir.join("HEAD").is_file() && dir.join("commondir").is_file() {
        maybe_trace_implicit_bare_repository(dir);
        let repo = Repository::open(dir, None)?;
        return Ok(Some(repo));
    }

    // Check if `dir` itself is a bare repo (has objects/ and HEAD directly)
    if dir.join("objects").is_dir() && dir.join("HEAD").is_file() {
        maybe_trace_implicit_bare_repository(dir);
        // Check safe.bareRepository policy before opening bare repos.
        // When set to "explicit", implicit bare repo discovery is forbidden
        // unless GIT_DIR was set (handled earlier in discover()).
        if !is_inside_dot_git(dir) {
            if let Ok(cfg) = crate::config::ConfigSet::load(None, true) {
                if let Some(val) = cfg.get("safe.bareRepository") {
                    if val.eq_ignore_ascii_case("explicit") {
                        return Err(Error::ForbiddenBareRepository(dir.display().to_string()));
                    }
                }
            }
        }
        let repo = Repository::open(dir, None)?;
        return Ok(Some(repo));
    }

    Ok(None)
}

fn is_inside_dot_git(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".git")
}

fn maybe_trace_implicit_bare_repository(dir: &Path) {
    let path = match std::env::var("GIT_TRACE2_PERF") {
        Ok(p) if !p.is_empty() => p,
        _ => return,
    };

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "setup: implicit-bare-repository:{}", dir.display());
    }
}

impl Repository {
    /// Enforce `safe.directory` ownership checks, matching upstream behavior.
    ///
    /// When `GIT_TEST_ASSUME_DIFFERENT_OWNER=1`, ownership is considered unsafe
    /// unless a matching `safe.directory` value is configured in system/global/
    /// command scopes (repository-local config is ignored).
    pub fn enforce_safe_directory(&self) -> Result<()> {
        let assume_different = std::env::var("GIT_TEST_ASSUME_DIFFERENT_OWNER")
            .ok()
            .map(|v| {
                let lower = v.to_ascii_lowercase();
                v == "1" || lower == "true" || lower == "yes" || lower == "on"
            })
            .unwrap_or(false);
        if !assume_different {
            return Ok(());
        }

        if self.explicit_git_dir {
            return Ok(());
        }

        // In normal discovery, ownership is checked against worktree paths
        // unless invocation starts inside the gitdir, in which case gitdir is
        // checked.
        let checked = if let Some(wt) = &self.work_tree {
            let cwd = std::env::current_dir().ok();
            if let Some(cwd) = cwd {
                if cwd
                    .canonicalize()
                    .ok()
                    .is_some_and(|c| c.starts_with(&self.git_dir))
                {
                    self.git_dir
                        .canonicalize()
                        .unwrap_or_else(|_| self.git_dir.clone())
                } else {
                    wt.canonicalize().unwrap_or_else(|_| wt.clone())
                }
            } else {
                wt.canonicalize().unwrap_or_else(|_| wt.clone())
            }
        } else {
            self.git_dir
                .canonicalize()
                .unwrap_or_else(|_| self.git_dir.clone())
        };

        if std::env::var("GRIT_DEBUG_SAFE_DIR").is_ok() {
            eprintln!(
                "debug-safe-directory checked={} git_dir={} work_tree={:?} cwd={:?}",
                checked.display(),
                self.git_dir.display(),
                self.work_tree,
                std::env::current_dir().ok()
            );
        }
        self.enforce_safe_directory_checked(&checked)
    }

    /// Enforce safe.directory checks using the repository git-dir path.
    ///
    /// Used by operations that explicitly open another repository by path
    /// (e.g. local clone source).
    pub fn enforce_safe_directory_git_dir(&self) -> Result<()> {
        let assume_different = std::env::var("GIT_TEST_ASSUME_DIFFERENT_OWNER")
            .ok()
            .map(|v| {
                let lower = v.to_ascii_lowercase();
                v == "1" || lower == "true" || lower == "yes" || lower == "on"
            })
            .unwrap_or(false);
        if !assume_different {
            return Ok(());
        }
        let checked = self
            .git_dir
            .canonicalize()
            .unwrap_or_else(|_| self.git_dir.clone());
        if std::env::var("GRIT_DEBUG_SAFE_DIR").is_ok() {
            eprintln!(
                "debug-safe-directory(gitdir) checked={} git_dir={} work_tree={:?}",
                checked.display(),
                self.git_dir.display(),
                self.work_tree
            );
        }
        self.enforce_safe_directory_checked(&checked)
    }

    /// Enforce safe.directory checks against an explicit checked path.
    pub fn enforce_safe_directory_git_dir_with_path(&self, checked: &Path) -> Result<()> {
        let assume_different = std::env::var("GIT_TEST_ASSUME_DIFFERENT_OWNER")
            .ok()
            .map(|v| {
                let lower = v.to_ascii_lowercase();
                v == "1" || lower == "true" || lower == "yes" || lower == "on"
            })
            .unwrap_or(false);
        if !assume_different {
            return Ok(());
        }
        self.enforce_safe_directory_checked(checked)
    }

    fn enforce_safe_directory_checked(&self, checked: &Path) -> Result<()> {
        let cfg = crate::config::ConfigSet::load(Some(&self.git_dir), true)
            .unwrap_or_else(|_| crate::config::ConfigSet::new());
        let mut values: Vec<String> = Vec::new();
        for e in cfg.entries() {
            if e.key == "safe.directory"
                && e.scope != crate::config::ConfigScope::Local
                && e.scope != crate::config::ConfigScope::Worktree
            {
                values.push(e.value.clone().unwrap_or_else(|| "true".to_owned()));
            }
        }

        // Last empty assignment resets the list.
        let mut effective: Vec<String> = Vec::new();
        for v in values {
            if v.is_empty() {
                effective.clear();
            } else {
                effective.push(v);
            }
        }

        let checked_s = checked.to_string_lossy().to_string();
        if std::env::var("GRIT_DEBUG_SAFE_DIR").is_ok() {
            eprintln!("debug-safe-directory values={:?}", effective);
        }
        if effective
            .iter()
            .any(|v| safe_directory_matches(v, &checked_s))
        {
            return Ok(());
        }

        Err(Error::DubiousOwnership(checked_s))
    }
}

fn normalize_fs_path(raw: &str) -> String {
    use std::path::Component;
    let p = std::path::Path::new(raw);
    let mut parts: Vec<String> = Vec::new();
    let mut absolute = false;
    for c in p.components() {
        match c {
            Component::RootDir => {
                absolute = true;
                parts.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::Prefix(_) => {}
        }
    }
    let mut out = if absolute {
        String::from("/")
    } else {
        String::new()
    };
    out.push_str(&parts.join("/"));
    out
}

fn safe_directory_matches(config_value: &str, checked: &str) -> bool {
    if config_value == "*" {
        return true;
    }
    if config_value == "." {
        // CWD only.
        if let Ok(cwd) = std::env::current_dir() {
            let cwd_s = normalize_fs_path(&cwd.to_string_lossy());
            let checked_s = normalize_fs_path(checked);
            return cwd_s == checked_s;
        }
        return false;
    }

    let canonicalize_or_normalize = |raw: &str| -> String {
        let p = std::path::Path::new(raw);
        if p.exists() {
            p.canonicalize()
                .map(|c| c.to_string_lossy().to_string())
                .map(|s| normalize_fs_path(&s))
                .unwrap_or_else(|_| normalize_fs_path(raw))
        } else {
            normalize_fs_path(raw)
        }
    };

    let config_norm = canonicalize_or_normalize(config_value);
    let checked_norm = normalize_fs_path(checked);

    if config_norm.ends_with("/*") {
        let prefix_raw = &config_norm[..config_norm.len() - 2];
        let prefix_norm = canonicalize_or_normalize(prefix_raw);
        let mut prefix = prefix_norm;
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        return checked_norm.starts_with(&prefix);
    }

    config_norm == checked_norm
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
            if !path.exists() {
                return Err(Error::NotARepository(path.display().to_string()));
            }
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

/// Validate the repository format version from config text.
/// Returns Ok if the format is supported, Err with message if not.
pub fn validate_repo_config(config_text: &str) -> std::result::Result<(), String> {
    let mut version: u32 = 0;
    let mut in_core = false;
    for line in config_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_core = trimmed.to_lowercase().starts_with("[core");
            continue;
        }
        if in_core {
            if let Some(rest) = trimmed.strip_prefix("repositoryformatversion") {
                let val = rest
                    .trim_start_matches(|c: char| c == ' ' || c == '=')
                    .trim();
                if let Ok(v) = val.parse::<u32>() {
                    version = v;
                }
            }
        }
    }
    if version >= 2 {
        return Err(format!("unknown repository format version: {version}"));
    }
    Ok(())
}
