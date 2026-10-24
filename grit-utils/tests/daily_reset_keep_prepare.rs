//! Reset-keep daily prepare must start from a tracked unstaged modification.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use grit_utils::daily::matrix_helpers::{run_prepare_reset_keep, setup_synth10k};
use grit_utils::daily::plan::DailySetup;
use grit_utils::fixture_registry::FixtureKind;
use grit_utils::worktree_fixture::set_worktree_bench_cwd;
use tempfile::TempDir;

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn lock_fixtures() -> MutexGuard<'static, ()> {
    std::env::remove_var("GRIT_BENCH_DAILY_ROOT");
    FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn reset_keep_prepare_leaves_tracked_unstaged_edit() {
    let _guard = lock_fixtures();
    let git = grit_utils::binary::resolve_binary("git", None).expect("git");
    let grit = {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        Command::new(env!("CARGO"))
            .args(["build", "-p", "grit-cli", "--bin", "grit", "-q"])
            .current_dir(&workspace)
            .status()
            .expect("build grit");
        workspace.join("target/debug/grit")
    };
    let cache = TempDir::new().expect("cache");
    std::env::set_var("GRIT_BENCH_CACHE", cache.path());
    grit_utils::fixture_registry::ensure_cached(&git, FixtureKind::Synth10kSmoke, true)
        .expect("fixture");

    let bench = PathBuf::from(env!("CARGO_BIN_EXE_grit-bench"));
    let setup = DailySetup {
        git: &git,
        grit: &grit,
        bench: &bench,
        smoke: true,
        offline: true,
    };
    let ctx = setup_synth10k(&setup).expect("setup");
    set_worktree_bench_cwd(&ctx.repo).expect("cwd");
    run_prepare_reset_keep(&git).expect("reset-keep prepare");

    let out = Command::new(&git)
        .args(["status", "--porcelain"])
        .current_dir(&ctx.repo)
        .output()
        .expect("git status");
    assert!(out.status.success());
    let status = String::from_utf8_lossy(&out.stdout);
    assert!(
        status
            .lines()
            .any(|line| line.starts_with(" M ") || line.starts_with(" M\t")),
        "expected unstaged modification on a tracked path, got:\n{status}"
    );
    assert!(
        !status.lines().any(|line| line.starts_with("??")),
        "reset-keep prepare must not rely on untracked files:\n{status}"
    );
}
