//! Repack/gc compatibility: grit maintenance vs system `git fsck`.

use std::process::Command;

use grit_lib::index::Index;
use grit_lib::maintenance::{
    fsck as repo_fsck, gc, repack_all, FsckOptions, GcOptions, RepackOptions,
};
use grit_lib::objects::{ObjectId, ObjectKind};
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
    assert!(
        loose_before > 10,
        "expected many loose objects, got {loose_before}"
    );

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

fn one_commit(repo: &Repository) {
    many_grit_commits(repo, 1);
}

#[test]
fn gc_preserves_staged_only_blob() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    one_commit(&repo);
    let wt = repo.work_tree().expect("wt");
    std::fs::write(wt.join("staged-only.txt"), b"staged payload\n").expect("write");
    stage_for_commit(&repo, &mut NullProgress).expect("stage");
    let index = Index::load(&repo.git_dir().join("index")).expect("index");
    let staged_oid = index
        .entries()
        .iter()
        .find(|e| e.path.ends_with(b"staged-only.txt"))
        .expect("staged entry")
        .oid;

    gc(
        &repo,
        &GcOptions {
            repack: RepackOptions::default(),
            expire_reflog: false,
            now_unix: 1_700_000_000,
        },
    )
    .expect("gc");

    assert!(
        repo.odb().read(&staged_oid).is_ok(),
        "gc must not delete index-only blob"
    );
    git_fsck(dir.path(), true);
    let report = repo_fsck(
        &repo,
        &FsckOptions {
            strict: true,
            check_objects: true,
            check_refs: true,
            check_midx: true,
        },
    )
    .expect("fsck");
    assert!(report.ok, "{:?}", report.issues);
}

#[test]
fn fsck_reports_missing_index_blob_after_corruption() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    one_commit(&repo);
    let wt = repo.work_tree().expect("wt");
    std::fs::write(wt.join("orphan-index.txt"), b"x").expect("write");
    stage_for_commit(&repo, &mut NullProgress).expect("stage");
    let index = Index::load(&repo.git_dir().join("index")).expect("index");
    let staged_oid = index
        .entries()
        .iter()
        .find(|e| e.path.ends_with(b"orphan-index.txt"))
        .expect("entry")
        .oid;
    let loose = repo
        .odb()
        .enumerate_local_loose_objects()
        .expect("loose")
        .into_iter()
        .find(|(oid, _)| *oid == staged_oid)
        .expect("loose path");
    std::fs::remove_file(loose.1).expect("remove blob");

    let report = repo_fsck(
        &repo,
        &FsckOptions {
            strict: true,
            check_objects: true,
            check_refs: true,
            check_midx: false,
        },
    )
    .expect("fsck");
    assert!(
        !report.ok,
        "fsck must not report clean when index references a missing blob"
    );
    assert!(
        report.issues.iter().any(|i| i.msg_id == "missingObject"),
        "{:?}",
        report.issues
    );
}

#[test]
fn repack_keeps_reflog_only_commit_and_kept_pack() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    many_grit_commits(&repo, 2);
    git(dir.path(), &["reset", "--hard", "HEAD~1"]);

    let reflog_only: ObjectId = {
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", "HEAD@{1}"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("rev-parse");
        assert!(out.status.success());
        let hex = String::from_utf8_lossy(&out.stdout);
        ObjectId::from_hex(hex.trim()).expect("oid")
    };

    git(dir.path(), &["repack", "-a", "-d"]);
    let git_dir = dir.path().join(".git");
    let pack_dir = git_dir.join("objects/pack");
    let old_pack = std::fs::read_dir(&pack_dir)
        .expect("pack dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "pack"))
        .expect("pack");
    std::fs::write(old_pack.with_extension("keep"), b"keep").expect("keep");

    repack_all(&repo, &RepackOptions::default()).expect("repack");

    assert!(
        repo.odb().read(&reflog_only).is_ok(),
        "reflog-only commit must remain readable after repack"
    );
    assert!(old_pack.is_file(), "pack with .keep must not be deleted");
    git(dir.path(), &["fsck", "--strict"]);
}

#[test]
fn gc_prune_expire_never_preserves_unreachable_loose_blob() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    one_commit(&repo);

    let orphan = repo
        .odb()
        .write_loose_materialize(ObjectKind::Blob, b"unreachable orphan payload\n")
        .expect("write loose");

    git(dir.path(), &["config", "gc.pruneExpire", "never"]);

    let stats = gc(
        &repo,
        &GcOptions {
            repack: RepackOptions::default(),
            expire_reflog: false,
            now_unix: 1_700_000_000,
        },
    )
    .expect("grit gc");
    assert_eq!(
        stats.prune.pruned, 0,
        "gc.pruneExpire=never must disable loose prune"
    );
    assert!(
        repo.odb().read(&orphan).is_ok(),
        "unreachable loose blob must remain after grit gc"
    );

    git(dir.path(), &["gc"]);
    let out = Command::new("git")
        .current_dir(dir.path())
        .args(["cat-file", "-e", &orphan.to_hex()])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("cat-file");
    assert!(
        out.status.success(),
        "system git gc with pruneExpire=never must preserve orphan blob"
    );
}

#[test]
fn index_gitlink_absent_commit_does_not_break_repack_or_fsck() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    one_commit(&repo);

    let absent = "aabbccddeeff00112233445566778899aabbccdd";
    git(
        dir.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{absent},submodule"),
        ],
    );

    repack_all(&repo, &RepackOptions::default()).expect("grit repack");
    git(dir.path(), &["gc"]);
    git(dir.path(), &["fsck", "--strict"]);

    let report = repo_fsck(
        &repo,
        &FsckOptions {
            strict: true,
            check_objects: true,
            check_refs: true,
            check_midx: false,
        },
    )
    .expect("grit fsck");
    assert!(
        report.ok,
        "gitlink to absent submodule commit must not fail fsck: {:?}",
        report.issues
    );
}
