//! Rename detection for index↔HEAD and index↔worktree diffs (issue #1012).

use std::path::Path;
use std::process::Command;

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::diff::{
    apply_rename_detection_to_diff_entries, diff_index_to_tree_with_renames,
    DiffStatus,
};
use grit_lib::diff::tree_oid_of_commit;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;

fn git_run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Diff Test")
        .env("GIT_AUTHOR_EMAIL", "diff@example.com")
        .env("GIT_COMMITTER_NAME", "Diff Test")
        .env("GIT_COMMITTER_EMAIL", "diff@example.com")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo_with_large_file(root: &Path) {
    git_run(root, &["init", "-q", "-b", "main"]);
    let body: String = (1..=40)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(root.join("a.txt"), format!("{body}\n")).unwrap();
    git_run(root, &["add", "."]);
    git_run(root, &["commit", "-qm", "one"]);
}

#[test]
fn diff_index_to_tree_with_renames_reports_staged_mv() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    init_repo_with_large_file(root);
    git_run(root, &["mv", "a.txt", "b.txt"]);

    let repo = Repository::discover(Some(root), &RepositoryOptions::empty()).expect("open");
    let head = resolve_head(repo.git_dir()).expect("head");
    let head_commit = head.oid().expect("commit");
    let head_tree = tree_oid_of_commit(repo.odb(), head_commit).expect("tree");
    let index = repo.index_handle().load().expect("index");
    let config = repo.config().expect("config");
    let policy = repo.thread_policy();

    let entries = diff_index_to_tree_with_renames(
        repo.odb(),
        &index,
        Some(&head_tree),
        false,
        50,
        config.as_ref(),
        &policy,
    )
    .expect("staged diff");

    let renames: Vec<_> = entries
        .iter()
        .filter(|e| e.status == DiffStatus::Renamed)
        .collect();
    assert_eq!(
        renames.len(),
        1,
        "expected one staged rename, got: {entries:?}"
    );
    assert_eq!(renames[0].old_path.as_deref(), Some("a.txt"));
    assert_eq!(renames[0].new_path.as_deref(), Some("b.txt"));
}

#[test]
fn apply_rename_detection_matches_git_cached_name_status() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    init_repo_with_large_file(root);
    git_run(root, &["mv", "a.txt", "b.txt"]);

    let git_out = Command::new("git")
        .current_dir(root)
        .args(["diff", "--cached", "--name-status", "-M"])
        .output()
        .expect("git diff");
    assert!(git_out.status.success());
    let git_line = String::from_utf8_lossy(&git_out.stdout);
    assert!(
        git_line.contains("R100\t") || git_line.contains("R"),
        "git should report rename: {git_line}"
    );

    let repo = Repository::discover(Some(root), &RepositoryOptions::empty()).expect("open");
    let head = resolve_head(repo.git_dir()).expect("head");
    let head_commit = head.oid().expect("commit");
    let head_tree = tree_oid_of_commit(repo.odb(), head_commit).expect("tree");
    let index = repo.index_handle().load().expect("index");
    let config = repo.config().expect("config");
    let policy = repo.thread_policy();
    let raw =
        grit_lib::diff::diff_index_to_tree(repo.odb(), &index, Some(&head_tree), false)
        .expect("raw");
    let entries =
        apply_rename_detection_to_diff_entries(repo.odb(), raw, None, 50, config.as_ref(), &policy)
            .expect("renames");
    assert!(
        entries.iter().any(|e| e.status == DiffStatus::Renamed),
        "grit should pair rename like git: {entries:?}"
    );
}
