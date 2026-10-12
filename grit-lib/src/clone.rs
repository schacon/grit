//! Clone a remote repository: initialize, configure `origin`, fetch, and set up the default branch.
//!
//! File checkout is left to the caller (typically [`crate::porcelain::checkout`]).

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::{ConfigFile, ConfigScope, ConfigSet};
use crate::environment::Environment;
use crate::error::{Error, Result};
use crate::fetch::{fetch_operation_identity, Progress};
use crate::objects::ObjectId;
use crate::ref_storage::RefStorageFormat;
use crate::refs;
use crate::remote::{HttpClientFactory, Remote, DEFAULT_REMOTE};
use crate::repo::{init_repository, Repository};
use crate::transfer::{CloneReflog, FetchOptions, TagMode, TransferStats};
use crate::transport_path::{
    absolute_local_clone_source_url, should_store_absolute_local_clone_url,
};

/// Errors during [`clone`].
#[derive(Debug, Error)]
pub enum CloneError {
    /// Destination exists and is not empty.
    ///
    /// Grit does not create or modify the destination in this case; callers must not run
    /// [`cleanup_failed_clone`] on the path.
    #[error("destination '{0}' already exists and is not empty")]
    DestNotEmpty(String),
    /// Underlying library failure.
    #[error(transparent)]
    Library(#[from] Error),
    /// Remote resolution or transport failure.
    #[error(transparent)]
    Remote(#[from] crate::remote::RemoteError),
}

impl CloneError {
    /// Whether [`cleanup_failed_clone`] may run after this failure.
    ///
    /// Returns `false` for [`CloneError::DestNotEmpty`], which rejects the destination before
    /// any repository is initialized.
    #[must_use]
    pub fn should_cleanup_failed_clone(&self) -> bool {
        !matches!(self, Self::DestNotEmpty(_))
    }
}

/// Options for [`clone`].
pub struct CloneOptions {
    /// Source URL or path (as passed on the command line).
    pub url: String,
    /// Directory to create or use for the new repository.
    pub dest: PathBuf,
    /// Remote name to configure (default [`DEFAULT_REMOTE`]).
    pub remote_name: String,
    /// Process environment for config and reflog identity.
    pub environment: Environment,
    /// Initial branch name used when initializing the empty repo (`main`).
    pub initial_branch: String,
    /// Global thread policy for fetch pack ingest and the opened repository (`--threads`).
    pub thread_policy: crate::parallel::ThreadPolicy,
}

impl CloneOptions {
    /// Options for cloning into `dest` from `url`, using the current process environment.
    #[must_use]
    pub fn new(url: impl Into<String>, dest: PathBuf) -> Self {
        Self {
            url: url.into(),
            dest,
            remote_name: DEFAULT_REMOTE.to_owned(),
            environment: Environment::capture_process(), // hygiene: CLI boundary — snapshot process environment for clone
            initial_branch: "main".to_owned(),
            thread_policy: crate::parallel::ThreadPolicy::default(),
        }
    }
}

/// Result of a successful [`clone`] before working-tree checkout.
#[derive(Debug)]
pub struct CloneOutcome {
    /// Source URL string from the options.
    pub url: String,
    /// Destination path.
    pub path: PathBuf,
    /// Short name of the default branch that was created.
    pub branch: String,
    /// Commit to check out on that branch.
    pub checkout_oid: ObjectId,
    /// Open handle to the new repository.
    pub repo: Repository,
    /// Objects and pack bytes received during the initial fetch.
    pub transfer: TransferStats,
    /// Remote branches fetched (from the fetch ref updates).
    pub branches: u64,
    /// Remote tags fetched (from the fetch ref updates).
    pub tags: u64,
}

/// Derive a destination directory from a clone URL (last path component, minus `.git`).
#[must_use]
pub fn derive_clone_dir(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or(trimmed);
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

/// Whether `path` is an existing empty directory (used for failed-clone cleanup policy).
#[must_use]
pub fn dest_preexisted_empty(path: &Path) -> bool {
    path.is_dir()
        && path
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
}

/// Remove a partially created clone destination.
///
/// When `keep_toplevel` is true, only contents of `path` are removed (empty dir existed before clone).
pub fn cleanup_failed_clone(path: &Path, keep_toplevel: bool) {
    if keep_toplevel {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let _ = fs::remove_dir_all(p);
            } else {
                let _ = fs::remove_file(p);
            }
        }
    } else {
        let _ = fs::remove_dir_all(path);
    }
}

/// Initialize, fetch, and configure the default branch; does not check out files.
///
/// # Errors
///
/// Returns [`CloneError`] when the destination is invalid, fetch fails, or no branch exists.
pub fn clone(
    opts: CloneOptions,
    progress: &mut dyn Progress,
    http_factory: Option<&dyn HttpClientFactory>,
) -> std::result::Result<CloneOutcome, CloneError> {
    let dir = opts
        .dest
        .to_str()
        .ok_or_else(|| CloneError::Library(Error::Message("non-UTF-8 destination path".into())))?;
    if opts.dest.is_dir()
        && opts
            .dest
            .read_dir()
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false)
    {
        return Err(CloneError::DestNotEmpty(dir.to_owned()));
    }

    let repo = init_repository(
        &opts.dest,
        false,
        &opts.initial_branch,
        None,
        RefStorageFormat::default(),
    )
    .map_err(|e| {
        CloneError::Library(Error::Message(format!("could not initialize '{dir}': {e}")))
    })?;

    let origin_url = stored_clone_remote_url(&opts.url);
    set_local_config(
        &repo,
        &[
            (
                &format!("remote.{}.url", opts.remote_name),
                origin_url.clone(),
            ),
            (
                &format!("remote.{}.fetch", opts.remote_name),
                format!("+refs/heads/*:refs/remotes/{}/*", opts.remote_name),
            ),
        ],
    )?;

    let config = ConfigSet::load(&opts.environment, Some(repo.git_dir()), true)
        .map_err(CloneError::Library)?;
    let remote = Remote::from_config(&config, &opts.remote_name)?;
    let clone_message = format!("clone: from {}", opts.url);
    let identity =
        fetch_operation_identity(&opts.environment, repo.git_dir()).map_err(CloneError::Library)?;
    let clone_reflog = CloneReflog {
        identity: identity.clone(),
        message: clone_message.clone(),
    };
    let mut fetch_opts = FetchOptions {
        tags: TagMode::All,
        initial_remote_fetch: true,
        remote_name: Some(opts.remote_name.clone()),
        clone_reflog: Some(clone_reflog),
        environment: opts.environment.clone(),
        thread_policy: opts.thread_policy,
        remote_progress: progress.remote_sideband_progress(),
        ..Default::default()
    };
    if let Ok(v) = crate::protocol::try_effective_client_protocol_version_from_config(&config) {
        fetch_opts.http_protocol_version = Some(v);
    }
    let fetch_outcome = remote.fetch(&repo, fetch_opts, progress, http_factory)?;

    let default = fetch_outcome
        .default_branch
        .as_deref()
        .map(|d| d.strip_prefix("refs/heads/").unwrap_or(d).to_owned())
        .or_else(|| pick_default_branch(&repo, &opts.remote_name))
        .ok_or_else(|| {
            CloneError::Library(Error::Message(
                "the remote has no branches to check out".into(),
            ))
        })?;

    let tracking = format!("refs/remotes/{}/{default}", opts.remote_name);
    let oid = refs::resolve_ref(repo.git_dir(), &tracking).map_err(CloneError::Library)?;

    let branch_ref = format!("refs/heads/{default}");
    refs::write_ref(repo.git_dir(), &branch_ref, &oid).map_err(CloneError::Library)?;
    let zero = ObjectId::zero();
    refs::append_reflog(
        repo.git_dir(),
        &branch_ref,
        &zero,
        &oid,
        &identity,
        &clone_message,
        false,
    )
    .map_err(CloneError::Library)?;
    refs::write_symbolic_ref(repo.git_dir(), "HEAD", &branch_ref).map_err(CloneError::Library)?;
    refs::append_reflog(
        repo.git_dir(),
        "HEAD",
        &zero,
        &oid,
        &identity,
        &clone_message,
        false,
    )
    .map_err(CloneError::Library)?;
    set_local_config(
        &repo,
        &[
            (
                &format!("branch.{default}.remote"),
                opts.remote_name.clone(),
            ),
            (
                &format!("branch.{default}.merge"),
                format!("refs/heads/{default}"),
            ),
        ],
    )?;

    Ok(CloneOutcome {
        url: opts.url.clone(),
        path: opts.dest.clone(),
        branch: default,
        checkout_oid: oid,
        repo: repo.with_thread_policy(opts.thread_policy),
        transfer: fetch_outcome.transfer,
        branches: fetch_outcome.branch_count(),
        tags: fetch_outcome.tag_count(),
    })
}

fn stored_clone_remote_url(url: &str) -> String {
    if should_store_absolute_local_clone_url(url) {
        absolute_local_clone_source_url(Path::new(url.trim()))
    } else {
        url.to_owned()
    }
}

fn set_local_config(repo: &Repository, entries: &[(&str, String)]) -> Result<()> {
    let path = repo.git_dir().join("config");
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut config = ConfigFile::parse(&path, &content, ConfigScope::Local)?;
    for (key, value) in entries {
        config.set(key, value)?;
    }
    config.write()?;
    Ok(())
}

fn pick_default_branch(repo: &Repository, remote_name: &str) -> Option<String> {
    for candidate in ["main", "master"] {
        if refs::resolve_ref(
            repo.git_dir(),
            &format!("refs/remotes/{remote_name}/{candidate}"),
        )
        .is_ok()
        {
            return Some(candidate.to_owned());
        }
    }
    refs::list_refs(repo.git_dir(), &format!("refs/remotes/{remote_name}/"))
        .ok()?
        .into_iter()
        .find_map(|(name, _)| {
            name.strip_prefix(&format!("refs/remotes/{remote_name}/"))
                .filter(|b| *b != "HEAD")
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn dest_preexisted_empty_missing_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope");
        assert!(!dest_preexisted_empty(&missing));
    }

    #[test]
    fn dest_preexisted_empty_empty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(dest_preexisted_empty(dir.path()));
    }

    #[test]
    fn dest_preexisted_empty_nonempty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("file"), b"x").expect("write");
        assert!(!dest_preexisted_empty(dir.path()));
    }

    #[test]
    fn dest_not_empty_skips_failed_clone_cleanup() {
        assert!(!CloneError::DestNotEmpty("x".into()).should_cleanup_failed_clone());
        assert!(CloneError::Library(Error::Message("x".into())).should_cleanup_failed_clone());
    }

    #[test]
    fn outcome_reports_transfer_stats() {
        use std::process::Command;

        let upstream = tempfile::tempdir().expect("upstream");
        let upstream_wt = upstream.path().join("src");
        std::fs::create_dir_all(&upstream_wt).expect("mkdir");
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(&upstream_wt)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {:?}: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(upstream_wt.join("README.md"), b"hello\n").expect("write");
        git(&["add", "README.md"]);
        git(&["commit", "-m", "init"]);
        git(&["tag", "v1"]);
        git(&["branch", "feature"]);
        std::fs::write(upstream_wt.join("extra.txt"), b"x\n").expect("write");
        git(&["add", "extra.txt"]);
        git(&["commit", "-m", "on feature"]);
        git(&["checkout", "main"]);

        let dest = tempfile::tempdir().expect("dest");
        let outcome = clone(
            CloneOptions::new(upstream_wt.to_string_lossy(), dest.path().to_path_buf()),
            &mut crate::fetch::NoProgress,
            None,
        )
        .expect("clone");

        let count_out = Command::new("git")
            .args(["count-objects", "-v"])
            .current_dir(dest.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("count-objects");
        assert!(
            count_out.status.success(),
            "{}",
            String::from_utf8_lossy(&count_out.stderr)
        );
        let count_text = String::from_utf8_lossy(&count_out.stdout);
        let mut in_pack = None::<u64>;
        for line in count_text.lines() {
            if let Some(v) = line.strip_prefix("in-pack: ") {
                in_pack = Some(v.trim().parse().expect("in-pack"));
            }
        }
        let git_objects = in_pack.expect("in-pack line");
        let git_dir = dest.path().join(".git");
        let pack_dir = git_dir.join("objects/pack");
        let pack_bytes = std::fs::read_dir(&pack_dir)
            .expect("pack dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pack"))
            .map(|p| std::fs::metadata(&p).expect("pack stat").len())
            .sum::<u64>();
        assert_eq!(
            outcome.transfer.objects, git_objects,
            "object count must match git count-objects -v in-pack"
        );
        assert_eq!(
            outcome.transfer.bytes, pack_bytes,
            "pack bytes must match on-disk .pack file size"
        );
        assert!(outcome.branches >= 2, "expected at least main and feature");
        assert!(outcome.tags >= 1, "expected tag v1");
    }

    #[test]
    fn clone_rejects_nonempty_destination_without_git_dir() {
        let upstream = tempfile::tempdir().expect("upstream");
        let dest = tempfile::tempdir().expect("dest");
        fs::write(dest.path().join("notes.txt"), b"keep").expect("write");
        let err = clone(
            CloneOptions::new(upstream.path().to_string_lossy(), dest.path().to_path_buf()),
            &mut crate::fetch::NoProgress,
            None,
        )
        .expect_err("non-empty dest");
        assert!(matches!(err, CloneError::DestNotEmpty(_)));
        assert!(dest.path().join("notes.txt").is_file());
        assert!(!dest.path().join(".git").exists());
    }
}
