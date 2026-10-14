//! Repository handle sweeps for `[total]` coverage on `repo.rs` and handles.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::objects::ObjectKind;
use grit_lib::repo::{InitOptions, Repository};
use std::fs;
use tempfile::tempdir;

#[test]
fn repository_handles_and_discovery_sweep() -> grit_lib::error::Result<()> {
    let dir = tempdir().map_err(grit_lib::error::Error::Io)?;
    let mut repo = Repository::init(dir.path(), &InitOptions::default())?;
    fs::write(dir.path().join("probe.txt"), b"x\n").map_err(grit_lib::error::Error::Io)?;

    let _ = repo.is_bare();
    let _ = repo.git_dir();
    let _ = repo.work_tree();
    let _ = repo.discovery_info();
    let _ = repo.thread_policy();
    let _ = repo.environment();
    let _ = repo.reference_unix_time();
    let _ = repo.wall_clock_epoch();
    let _ = repo.config()?;
    repo.reload_config()?;
    let _ = repo.refs();
    let _ = repo.objects();
    let _ = repo.references();
    let _ = repo.index_handle().load()?;
    let _ = repo.config_handle();
    let _ = repo.rev_walk();
    let _ = repo.diff();
    let _ = repo.remotes();
    let _ = repo.worktrees();
    let _ = repo.stash();
    let _ = repo.notes();
    let _ = repo.bundles();
    let _ = repo.diagnostics();
    repo.set_network_trace(true);
    let _ = repo.network_trace_enabled();
    repo.set_network_trace(false);

    let discovered = Repository::discover(Some(dir.path()), &RepositoryOptions::empty())?;
    let _ = discovered.enforce_safe_directory();
    let _ = discovered.verify_safe_for_clone_source();

    let blob = repo.odb().write(ObjectKind::Blob, b"data\n")?;
    assert!(repo.objects().contains(&blob));

    Ok(())
}
