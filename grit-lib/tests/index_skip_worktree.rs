//! Skip-worktree and assume-unchanged bits vs system git (t2104/t7011/t7012).

mod support;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

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

    // Modify an included path and mark an excluded index entry dirty without a worktree file.
    fs::write(wt.join("a/f0.txt"), "changed\n").expect("touch included");
    let mut hash = Command::new("git")
        .current_dir(wt)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("hash-object");
    hash.stdin
        .take()
        .expect("stdin")
        .write_all(b"changed excluded\n")
        .expect("write stdin");
    let hash_out = hash.wait_with_output().expect("wait hash-object");
    assert!(hash_out.status.success());
    let blob_hex = String::from_utf8_lossy(&hash_out.stdout).trim().to_owned();
    Command::new("git")
        .current_dir(wt)
        .args([
            "update-index",
            "--cacheinfo",
            "100644",
            &blob_hex,
            "b/f0.txt",
        ])
        .status()
        .expect("update excluded index entry");

    let grit = open_grit(wt);
    assert_porcelain_v2_matches_git(wt, &grit);
}
