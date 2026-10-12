//! `format_porcelain_v2` vs system `git status --porcelain=v2`.

mod support;

use grit_lib::repo::Repository;

use support::{assert_porcelain_v2_matches_git, git, HistoryRepo};

#[test]
fn t_porcelain_v2_tracked_file_deletion_matches_git() {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    std::fs::write(wt.join("file.txt"), "payload\n").expect("write");
    git(wt, &["add", "file.txt"]);
    git(wt, &["commit", "-m", "add file"]);
    std::fs::remove_file(wt.join("file.txt")).expect("delete worktree file");

    let grit = Repository::discover(Some(wt)).expect("open");
    assert_porcelain_v2_matches_git(wt, &grit);
}

#[test]
fn t_porcelain_v2_modified_file_matches_git() {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    std::fs::write(wt.join("file.txt"), "a\n").expect("write");
    git(wt, &["add", "file.txt"]);
    git(wt, &["commit", "-m", "init"]);
    std::fs::write(wt.join("file.txt"), "b\n").expect("modify");

    let grit = Repository::discover(Some(wt)).expect("open");
    assert_porcelain_v2_matches_git(wt, &grit);
}
