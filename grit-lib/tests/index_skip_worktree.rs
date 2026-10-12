//! Skip-worktree and assume-unchanged bits vs system git (t2104/t7011/t7012).

mod support;

use std::fs;

use grit_lib::index::Index;
use grit_lib::repo::Repository;

use support::{
    assert_git_fsck_strict, assert_porcelain_v2_matches_git, assert_skip_worktree_matches_git, git,
    git_ls_files_verbose_map, seed_multidir_repo, HistoryRepo,
};

fn open_grit(wt: &std::path::Path) -> Repository {
    Repository::discover(Some(wt)).expect("open")
}

#[test]
fn t7012_skip_worktree_bits_match_git_ls_files_v() {
    if !support::skip_unless_git_at_least(2, 25) {
        return;
    }
    let repo = HistoryRepo::init();
    seed_multidir_repo(&repo);
    let wt = repo.worktree();
    git(wt, &["sparse-checkout", "init", "--cone"]);
    git(wt, &["sparse-checkout", "set", "a"]);
    git(wt, &["read-tree", "-mu", "HEAD"]);

    let grit = open_grit(wt);
    let index = grit.load_index().expect("load");
    assert_skip_worktree_matches_git(wt, &index);

    // Grit applies the same skip-worktree pattern programmatically on a fresh index load.
    let mut idx2 = Index::load(&grit.index_path()).expect("reload");
    grit_lib::sparse_checkout::apply_sparse_checkout_skip_worktree(
        &grit.git_dir,
        Some(wt),
        &mut idx2,
        false,
    );
    grit.write_index(&mut idx2).expect("write");
    assert_git_fsck_strict(wt);
    assert_skip_worktree_matches_git(wt, &idx2);
}

#[test]
fn t7011_assume_unchanged_seen_by_git_ls_files_v() {
    let repo = HistoryRepo::init();
    seed_multidir_repo(&repo);
    let wt = repo.worktree();
    git(wt, &["update-index", "--assume-unchanged", "a/f0.txt"]);

    let grit = open_grit(wt);
    let mut index = grit.load_index().expect("load");
    let entry = index
        .entries_mut()
        .iter_mut()
        .find(|e| e.path.ends_with(b"a/f0.txt"))
        .expect("entry");
    entry.set_assume_unchanged(true);
    grit.write_index(&mut index).expect("write");
    assert_git_fsck_strict(wt);

    let tags = git_ls_files_verbose_map(wt);
    assert_eq!(tags.get("a/f0.txt").copied(), Some('h'));
}

#[test]
fn t2104_skip_worktree_excluded_from_status_like_git() {
    if !support::skip_unless_git_at_least(2, 25) {
        return;
    }
    let repo = HistoryRepo::init();
    seed_multidir_repo(&repo);
    let wt = repo.worktree();
    git(wt, &["sparse-checkout", "init", "--cone"]);
    git(wt, &["sparse-checkout", "set", "a"]);
    git(wt, &["read-tree", "-mu", "HEAD"]);

    // Modify an included path and a skipped path; git status only reports `a/`.
    fs::write(wt.join("a/f0.txt"), "changed\n").expect("touch included");
    if wt.join("b/f0.txt").exists() {
        fs::write(wt.join("b/f0.txt"), "changed\n").expect("touch excluded");
    }

    let grit = open_grit(wt);
    assert_porcelain_v2_matches_git(wt, &grit);
}
