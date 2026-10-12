//! Repack/gc compatibility: grit maintenance vs system `git fsck`.

use std::process::Command;

use grit_lib::maintenance::{
    fsck as repo_fsck, gc, repack_all, FsckOptions, GcOptions, RepackOptions,
};
use grit_lib::porcelain::add::stage_for_commit;
use grit_lib::porcelain::commit::{create_commit, CommitRequest};
use grit_lib::progress::NullProgress;
use grit_lib::repo::{InitOptions, Repository};
use grit_test_support::git_fsck;
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn many_grit_commits(repo: &Repository, n: usize) {
    let wt = repo.work_tree().expect("worktree");
    let ident = "Test <t@example.com> 1 +0000".to_string();
    for i in 0..n {
        let path = wt.join(format!("k_{}.txt", i % 9));
        std::fs::write(&path, format!("payload-{i}\n")).expect("write");
        stage_for_commit(repo, &mut NullProgress).expect("stage");
        create_commit(
            repo,
            &CommitRequest {
                message: format!("m {i}"),
                author: ident.clone(),
                committer: ident.clone(),
                allow_empty: false,
                sign_override: None,
                amend: false,
            },
            &mut NullProgress,
        )
        .expect("commit");
    }
}

#[test]
fn repack_after_many_commits_single_pack_and_git_fsck_clean() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    many_grit_commits(&repo, 30);

    let git_dir = dir.path().join(".git");
    let loose_before = repo
        .odb()
        .enumerate_local_loose_objects()
        .expect("loose")
        .len();
    assert!(loose_before > 10, "expected many loose objects, got {loose_before}");

    let stats = repack_all(&repo, &RepackOptions::default()).expect("repack");
    assert!(stats.pack_path.is_some());
    assert!(stats.objects_packed.unwrap_or(0) > 0);

    let packs = std::fs::read_dir(git_dir.join("objects/pack"))
        .expect("pack dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "pack"))
        .count();
    assert_eq!(packs, 1, "expected one pack after repack -a");

    let git_outcome = git_fsck(dir.path(), true);
    assert!(
        git_outcome.msg_ids.is_empty(),
        "git fsck --strict msg-ids: {:?}",
        git_outcome.msg_ids
    );

    let report = repo_fsck(
        &repo,
        &FsckOptions {
            strict: true,
            check_objects: true,
            check_refs: true,
            check_midx: true,
        },
    )
    .expect("grit fsck");
    assert!(report.ok, "{:?}", report.issues);
}

#[test]
fn gc_matches_repack_plus_prune_and_git_fsck() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    many_grit_commits(&repo, 20);

    let stats = gc(
        &repo,
        &GcOptions {
            repack: RepackOptions::default(),
            expire_reflog: false,
            now_unix: 1_700_000_000,
        },
    )
    .expect("gc");
    assert!(stats.repack.pack_path.is_some());

    git(dir.path(), &["fsck", "--strict"]);
}
