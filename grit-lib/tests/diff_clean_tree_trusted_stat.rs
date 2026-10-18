//! Index↔worktree diff on a clean tree matches system `git diff-files`.

use std::process::Command;

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::diff::{diff_index_to_worktree_with_options, DiffIndexToWorktreeOptions};
use grit_lib::repo::Repository;
use grit_test_support::git;

fn assert_git_diff_files_clean(repo_root: &std::path::Path) {
    let root = repo_root.to_str().expect("utf-8 path");
    let status = Command::new("git")
        .args(["-C", root, "diff-files", "--quiet"])
        .status()
        .expect("spawn git diff-files");
    assert!(
        status.success(),
        "git diff-files --quiet must succeed on clean tree"
    );
}

#[test]
fn grit_diff_index_worktree_empty_on_deep_clean_tree() {
    const N: usize = 2_000;
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main", "."]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    for i in 0..N {
        let dir = format!("d{}/sub{}", i % 40, i % 17);
        std::fs::create_dir_all(root.join(&dir)).expect("mkdir");
        std::fs::write(
            root.join(dir).join(format!("f{i:05}.txt")),
            format!("body {i}\n"),
        )
        .expect("write");
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);

    let repo = Repository::open(
        &root.join(".git"),
        &RepositoryOptions::empty().with_work_tree(root),
    )
    .expect("open");
    let mut index = repo.index_handle().load().expect("index");
    let index_mtime = grit_lib::diff::index_mtime_for_diff(&index, None);
    let config = repo.config().expect("config");
    let (entries, _) = diff_index_to_worktree_with_options(
        repo.odb(),
        &mut index,
        root,
        DiffIndexToWorktreeOptions {
            repository_git_dir: Some(repo.git_dir().to_path_buf()),
            index_mtime,
            refresh_index_stat_in_pass: false,
            for_status: true,
            config: Some(config),
            thread_policy: Some(repo.thread_policy()),
            ..DiffIndexToWorktreeOptions::default()
        },
    )
    .expect("diff");
    assert!(
        entries.is_empty(),
        "expected no unstaged diff, got {}",
        entries.len()
    );
    assert_git_diff_files_clean(root);
}

#[test]
#[ignore = "manual clean-tree diff latency vs system git"]
fn clean_tree_diff_latency_within_git_on_large_fixture() {
    use std::time::{Duration, Instant};

    const N: usize = 10_000;
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main", "."]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    for i in 0..N {
        let dir = format!("d{}/sub{}", i % 80, i % 31);
        std::fs::create_dir_all(root.join(&dir)).expect("mkdir");
        std::fs::write(
            root.join(dir).join(format!("f{i:05}.txt")),
            format!("body {i}\n"),
        )
        .expect("write");
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);

    let grit_bin = std::env::var("GRIT_BIN").unwrap_or_else(|_| {
        std::env::var("CARGO_BIN_EXE_grit").expect("GRIT_BIN or CARGO_BIN_EXE_grit")
    });
    let root_s = root.to_str().expect("utf-8");

    let mut git_times = Vec::new();
    let mut grit_times = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        let git_status = Command::new("git")
            .args(["-C", root_s, "diff"])
            .status()
            .expect("git diff");
        assert!(git_status.success());
        git_times.push(t0.elapsed());

        let t0 = Instant::now();
        let grit_status = Command::new(&grit_bin)
            .args(["diff"])
            .current_dir(root)
            .status()
            .expect("grit diff");
        assert!(grit_status.success());
        grit_times.push(t0.elapsed());
    }
    git_times.sort();
    grit_times.sort();
    let git_med = git_times[git_times.len() / 2];
    let grit_med = grit_times[grit_times.len() / 2];
    let ratio = grit_med.as_secs_f64() / git_med.as_secs_f64();
    eprintln!("git median {git_med:?}, grit median {grit_med:?}, ratio {ratio:.2}");
    assert!(
        ratio < 3.0,
        "grit diff should stay within 3× system git on a warm clean tree (got {ratio:.2}×)"
    );
    assert!(git_med < Duration::from_secs(30));
}
