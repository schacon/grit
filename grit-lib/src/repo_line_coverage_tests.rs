//! Line-coverage sweeps for [`crate::repo::Repository`] setup and index helpers.

#![allow(clippy::unwrap_used)]

use crate::config::environment::{Environment, RepositoryOptions};
use crate::index::{IndexEntry, MODE_REGULAR};
use crate::objects::ObjectId;
use crate::objects::ObjectKind;
use crate::parallel::ThreadPolicy;
use crate::ref_storage::RefStorageFormat;
use crate::repo::{
    init_bare_with_env_worktree, init_repository, init_repository_separate_git_dir,
    init_worktree_config, repository_format_warning, resolve_git_directory_arg,
    trace_repo_setup_if_requested, validate_repo_format, InitOptions, Repository,
    RepositoryBuilder,
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
    let blob = repo.odb().write(ObjectKind::Blob, b"seed\n")?;
    let tree_body = format!("100644 seed.txt\0").into_bytes();
    let mut tree_bytes = tree_body;
    tree_bytes.extend_from_slice(blob.as_bytes());
    let tree = repo.odb().write(ObjectKind::Tree, &tree_bytes)?;
    let commit_body =
        format!("tree {tree}\nauthor T <t@e.com> 1 +0000\ncommitter T <t@e.com> 1 +0000\n\ninit\n");
    let tip = repo
        .odb()
        .write(ObjectKind::Commit, commit_body.as_bytes())?;
    std::fs::write(git_dir.join("refs/heads/main"), format!("{tip}\n"))
        .map_err(crate::error::Error::Io)?;
    let _ = validate_repo_format(&git_dir)?;
    let _ = repository_format_warning(&git_dir)?;
    let _ = init_worktree_config(&git_dir)?;
    let _ = resolve_git_directory_arg(&git_dir)?;
    let repo =
        RepositoryBuilder::new(RepositoryOptions::empty()).open(&git_dir, Some(dir.path()))?;
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
    let _ = repo.bloom_pathspec_cwd();
    let _ = repo.pathspec_precompose_enabled();
    let discovered = Repository::discover(Some(dir.path()), &RepositoryOptions::empty())?;
    let _ = discovered.resolve_ref_name("HEAD")?;
    let mut index = discovered.load_index()?;
    let blob = discovered.odb().write(ObjectKind::Blob, b"indexed\n")?;
    index.entries_mut().push(index_entry("indexed.txt", blob));
    let _ = write_tree_from_index(discovered.odb(), &index, "")?;
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
    init_repository_separate_git_dir(wt.path(), &separate, "main", None, RefStorageFormat::Files)?;
    let repo =
        RepositoryBuilder::new(RepositoryOptions::empty()).open(&separate, Some(wt.path()))?;
    let _ = repo.load_index()?;

    let bare_dir = tempdir().map_err(crate::error::Error::Io)?;
    init_repository(bare_dir.path(), true, "main", None, RefStorageFormat::Files)?;
    let _bare = Repository::open(bare_dir.path(), &RepositoryOptions::empty())?;

    let bare_wt = tempdir().map_err(crate::error::Error::Io)?;
    let git = bare_wt.path().join(".git");
    init_bare_with_env_worktree(&git, bare_wt.path(), "main", None, RefStorageFormat::Files)?;

    let nested = tempdir().map_err(crate::error::Error::Io)?;
    let nested_git = nested.path().join("nested.git");
    Repository::init(
        nested.path(),
        &InitOptions {
            separate_git_dir: Some(nested_git),
            initial_branch: "main".into(),
            ..InitOptions::default()
        },
    )?;
    Ok(())
}

#[test]
fn open_for_serving_and_bare_worktree_at_path() -> crate::error::Result<()> {
    let wt = tempdir().map_err(crate::error::Error::Io)?;
    Repository::init(
        wt.path(),
        &InitOptions {
            bare_with_worktree_at_path: true,
            initial_branch: "main".into(),
            ..InitOptions::default()
        },
    )?;
    let mut serve_opts = RepositoryOptions::empty();
    serve_opts.open_for_serving = true;
    let _ = Repository::open(wt.path(), &serve_opts)?;

    let bare_git = tempdir().map_err(crate::error::Error::Io)?;
    let git_dir = bare_git.path().join("repo.git");
    std::fs::create_dir_all(&git_dir).map_err(crate::error::Error::Io)?;
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")
        .map_err(crate::error::Error::Io)?;
    std::fs::create_dir_all(git_dir.join("refs/heads")).map_err(crate::error::Error::Io)?;
    std::fs::create_dir_all(git_dir.join("objects")).map_err(crate::error::Error::Io)?;
    std::fs::write(
        git_dir.join("config"),
        "[core]\n\trepositoryformatversion = 0\n",
    )
    .map_err(crate::error::Error::Io)?;
    let objects = git_dir.join("objects");
    let blob = {
        let odb = crate::odb::Odb::new(&objects);
        odb.write(ObjectKind::Blob, b"root\n")?
    };
    let tree_body = format!("100644 f.txt\0").into_bytes();
    let mut tree_bytes = tree_body;
    tree_bytes.extend_from_slice(blob.as_bytes());
    let tree = {
        let odb = crate::odb::Odb::new(&objects);
        odb.write(ObjectKind::Tree, &tree_bytes)?
    };
    let commit_body =
        format!("tree {tree}\nauthor T <t@e.com> 1 +0000\ncommitter T <t@e.com> 1 +0000\n\ninit\n");
    let tip = {
        let odb = crate::odb::Odb::new(&objects);
        odb.write(ObjectKind::Commit, commit_body.as_bytes())?
    };
    std::fs::write(git_dir.join("refs/heads/main"), format!("{tip}\n"))
        .map_err(crate::error::Error::Io)?;
    let bare_path = bare_git.path().join("repo.git");
    let _ = Repository::open(&bare_path, &serve_opts)?;
    Ok(())
}

#[test]
fn setup_trace_and_replace_ref_read_paths() -> crate::error::Result<()> {
    let dir = tempdir().map_err(crate::error::Error::Io)?;
    std::fs::create_dir_all(dir.path().join("nested/deep")).map_err(crate::error::Error::Io)?;
    let trace = dir.path().join("setup-trace.txt");
    std::fs::write(&trace, b"").map_err(crate::error::Error::Io)?;
    let trace_abs = trace.canonicalize().unwrap_or(trace);
    let mut env = Environment::empty();
    env.git_trace_setup = Some(trace_abs.to_string_lossy().into_owned());
    env.cwd = dir.path().join("nested/deep");
    let mut opts = RepositoryOptions::empty();
    opts.environment = env;
    let repo = init_repository(dir.path(), false, "main", None, RefStorageFormat::Files)?;
    let git_dir = dir.path().join(".git");
    let blob = repo.odb().write(ObjectKind::Blob, b"payload\n")?;
    let tree_body = format!("100644 x.txt\0").into_bytes();
    let mut tree_bytes = tree_body;
    tree_bytes.extend_from_slice(blob.as_bytes());
    let tree = repo.odb().write(ObjectKind::Tree, &tree_bytes)?;
    let commit_body =
        format!("tree {tree}\nauthor T <t@e.com> 1 +0000\ncommitter T <t@e.com> 1 +0000\n\nm\n");
    let tip = repo
        .odb()
        .write(ObjectKind::Commit, commit_body.as_bytes())?;
    std::fs::write(git_dir.join("refs/heads/main"), format!("{tip}\n"))
        .map_err(crate::error::Error::Io)?;
    let replacement = repo.odb().write(ObjectKind::Blob, b"replacement\n")?;
    std::fs::create_dir_all(git_dir.join("refs/replace")).map_err(crate::error::Error::Io)?;
    std::fs::write(
        git_dir.join(format!("refs/replace/{}", tip.to_hex())),
        format!("{replacement}\n"),
    )
    .map_err(crate::error::Error::Io)?;
    let discovered = Repository::discover(Some(dir.path()), &opts)?;
    trace_repo_setup_if_requested(&discovered).map_err(crate::error::Error::Io)?;
    let contents = std::fs::read_to_string(&trace_abs).map_err(crate::error::Error::Io)?;
    assert!(contents.contains("setup:"));
    let replaced = discovered.read_replaced(&tip)?;
    assert_eq!(replaced.kind, ObjectKind::Blob);
    let _oid: ObjectId = tip;
    Ok(())
}

#[test]
fn discover_honors_git_dir_and_ceiling_env() -> crate::error::Result<()> {
    let dir = tempdir().map_err(crate::error::Error::Io)?;
    let repo = init_repository(dir.path(), false, "main", None, RefStorageFormat::Files)?;
    let git_dir = dir.path().join(".git");
    let blob = repo.odb().write(ObjectKind::Blob, b"x\n")?;
    let tree_body = format!("100644 x.txt\0").into_bytes();
    let mut tree_bytes = tree_body;
    tree_bytes.extend_from_slice(blob.as_bytes());
    let tree = repo.odb().write(ObjectKind::Tree, &tree_bytes)?;
    let commit_body =
        format!("tree {tree}\nauthor T <t@e.com> 1 +0000\ncommitter T <t@e.com> 1 +0000\n\nm\n");
    let tip = repo
        .odb()
        .write(ObjectKind::Commit, commit_body.as_bytes())?;
    std::fs::write(git_dir.join("refs/heads/main"), format!("{tip}\n"))
        .map_err(crate::error::Error::Io)?;

    let mut env = Environment::empty();
    env.cwd = dir.path().to_path_buf();
    env.git_dir = Some(
        git_dir
            .to_string_lossy()
            .into_owned(),
    );
    let mut opts = RepositoryOptions::empty();
    opts.environment = env.clone();
    let explicit = Repository::discover(None, &opts)?;
    assert_eq!(explicit.git_dir(), git_dir);

    let mut env_wt = env.clone();
    env_wt.git_work_tree = Some(dir.path().to_string_lossy().into_owned());
    opts.environment = env_wt;
    let with_wt = Repository::discover(None, &opts)?;
    assert!(with_wt.work_tree().is_some());

    let parent = dir
        .path()
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_owned());
    let mut env_ceil = Environment::empty();
    env_ceil.cwd = dir.path().to_path_buf();
    env_ceil.git_ceiling_directories = Some(format!(":{parent}"));
    opts.environment = env_ceil;
    let from_sub = Repository::discover(Some(dir.path()), &opts)?;
    assert!(from_sub.work_tree().is_some());
    Ok(())
}
