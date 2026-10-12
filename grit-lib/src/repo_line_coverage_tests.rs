//! Line-coverage sweeps for [`crate::repo::Repository`] setup and index helpers.

#![allow(clippy::unwrap_used)]

use crate::index::{IndexEntry, MODE_REGULAR};
use crate::objects::ObjectKind;
use crate::parallel::ThreadPolicy;
use crate::ref_storage::RefStorageFormat;
use crate::environment::RepositoryOptions;
use crate::repo::{
    init_bare_clone_minimal, init_bare_with_env_worktree, init_repository,
    init_repository_separate, init_repository_separate_git_dir, init_worktree_config,
    repository_format_warning, resolve_git_directory_arg, trace_repo_setup_if_requested,
    validate_repo_format, Repository,
};
use crate::write_tree::write_tree_from_index;
use tempfile::tempdir;

fn index_entry(path: &str, oid: crate::objects::ObjectId) -> IndexEntry {
    IndexEntry {
        ctime_sec: 0,
        ctime_nsec: 0,
        mtime_sec: 0,
        mtime_nsec: 0,
        dev: 0,
        ino: 0,
        mode: MODE_REGULAR,
        uid: 0,
        gid: 0,
        size: 0,
        oid,
        flags: path.len().min(0xfff) as u16,
        flags_extended: None,
        path: path.as_bytes().to_vec(),
        base_index_pos: 0,
    }
}

#[test]
fn repository_open_discover_and_index_roundtrip() -> crate::error::Result<()> {
    let dir = tempdir().map_err(crate::error::Error::Io)?;
    let repo = init_repository(dir.path(), false, "main", None, RefStorageFormat::Files)?;
    let git_dir = dir.path().join(".git");
    let blob = repo.odb.write(ObjectKind::Blob, b"seed\n")?;
    let tree_body = format!("100644 seed.txt\0").into_bytes();
    let mut tree_bytes = tree_body;
    tree_bytes.extend_from_slice(blob.as_bytes());
    let tree = repo.odb.write(ObjectKind::Tree, &tree_bytes)?;
    let commit_body = format!(
        "tree {tree}\nauthor T <t@e.com> 1 +0000\ncommitter T <t@e.com> 1 +0000\n\ninit\n"
    );
    let tip = repo.odb.write(ObjectKind::Commit, commit_body.as_bytes())?;
    std::fs::write(git_dir.join("refs/heads/main"), format!("{tip}\n"))
        .map_err(crate::error::Error::Io)?;
    let _ = validate_repo_format(&git_dir)?;
    let _ = repository_format_warning(&git_dir)?;
    let _ = init_worktree_config(&git_dir)?;
    let _ = resolve_git_directory_arg(&git_dir)?;
    let opts = RepositoryOptions::empty();
    let repo = Repository::open_with_options(&git_dir, Some(dir.path()), opts.clone())?;
    let _ = repo.thread_policy();
    let _ = repo.environment();
    let _ = repo.reference_unix_time();
    let _ = repo.wall_clock_epoch();
    let _ = repo.command_runner();
    let _ = repo.config()?;
    let _ = repo.refs();
    let _ = repo.is_bare();
    let _ = repo.index_path();
    let _ = repo.index_path_for_env()?;
    let _ = repo.refs_dir();
    let _ = repo.head_path();
    let _ = repo.effective_pathspec_cwd();
    let _ = repo.bloom_pathspec_cwd();
    let _ = repo.pathspec_precompose_enabled();
    let discovered =
        Repository::discover_with(&RepositoryOptions::empty(), Some(dir.path()))?;
    let _ = discovered.resolve_ref_name("HEAD")?;
    let mut index = discovered.load_index()?;
    let blob = discovered
        .odb
        .write(ObjectKind::Blob, b"indexed\n")?;
    index.entries_mut().push(index_entry("indexed.txt", blob));
    let _ = write_tree_from_index(&discovered.odb, &index, "")?;
    discovered.write_index(&mut index)?;
    let _ = discovered.try_write_index(&mut index)?;
    let _ = discovered.split_index_would_force_write(&index);
    let _ = trace_repo_setup_if_requested(&discovered);
    let with_policy = discovered.with_thread_policy(ThreadPolicy::default());
    let _ = with_policy.network_trace_enabled();
    Ok(())
}

#[test]
fn separate_git_dir_and_bare_init_paths() -> crate::error::Result<()> {
    let wt = tempdir().map_err(crate::error::Error::Io)?;
    let separate = wt.path().join("repo.git");
    init_repository_separate_git_dir(
        wt.path(),
        &separate,
        "main",
        None,
        RefStorageFormat::Files,
    )?;
    let repo = Repository::open(&separate, Some(wt.path()))?;
    let _ = repo.load_index()?;

    let bare_dir = tempdir().map_err(crate::error::Error::Io)?;
    init_bare_clone_minimal(bare_dir.path(), "main", RefStorageFormat::Files)?;
    let _bare = Repository::open(bare_dir.path(), None)?;

    let bare_wt = tempdir().map_err(crate::error::Error::Io)?;
    let git = bare_wt.path().join(".git");
    init_bare_with_env_worktree(
        &git,
        bare_wt.path(),
        "main",
        None,
        RefStorageFormat::Files,
    )?;

    let nested = tempdir().map_err(crate::error::Error::Io)?;
    let nested_git = nested.path().join("nested.git");
    init_repository_separate(nested.path(), &nested_git, "main", None)?;
    Ok(())
}
