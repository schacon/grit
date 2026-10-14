//! Repack/gc compatibility: grit maintenance vs system `git fsck`.

use std::path::{Path, PathBuf};
use std::process::Command;

use filetime::FileTime;
use grit_lib::config::environment::RepositoryOptions;
use grit_lib::error::Error;
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
use time::OffsetDateTime;

fn duplicate_repo_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst");
    let status = Command::new("cp")
        .args([
            "-a",
            &format!("{}/.", src.display()),
            &dst.to_string_lossy(),
        ])
        .status()
        .expect("spawn cp");
    assert!(status.success(), "cp -a failed");
}

fn git_commit_dated(dir: &Path, when: &str, message: &str) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["commit", "--allow-empty", "-m", message])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_AUTHOR_DATE", when)
        .env("GIT_COMMITTER_DATE", when)
        .output()
        .expect("git commit");
    assert!(
        out.status.success(),
        "git commit: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

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

    let report = repo_fsck(&repo, &FsckOptions::full_repository(true)).expect("grit fsck");
    assert!(report.ok, "{:?}", report.issues);
}

#[test]
fn gc_matches_repack_plus_prune_and_git_fsck() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    many_grit_commits(&repo, 20);

    let stats = gc(&repo, &GcOptions::with_reflog_policy(1_700_000_000, false)).expect("gc");
    assert!(stats.repack.pack_path.is_some());

    git(dir.path(), &["fsck", "--strict"]);
}

fn count_loose_head_ref_files(git_dir: &Path) -> usize {
    let heads = git_dir.join("refs/heads");
    if !heads.is_dir() {
        return 0;
    }
    std::fs::read_dir(heads)
        .expect("refs/heads")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .count()
}

#[test]
fn gc_pack_refs_packs_branch_refs() {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    many_grit_commits(&repo, 5);
    git(dir.path(), &["branch", "feature"]);

    let git_dir = dir.path().join(".git");
    let loose_before = count_loose_head_ref_files(&git_dir);
    assert!(
        loose_before >= 2,
        "expected at least main and feature as loose refs/heads, got {loose_before}"
    );

    let stats = gc(&repo, &GcOptions::with_reflog_policy(1_700_000_000, false)).expect("gc");
    let pack = stats.pack_refs.as_ref().expect("gc should run pack-refs");
    assert!(
        pack.refs_packed >= 2,
        "expected branch refs packed, got {}",
        pack.refs_packed
    );
    assert_eq!(
        count_loose_head_ref_files(&git_dir),
        0,
        "expected no loose refs/heads after gc pack-refs --all"
    );

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

    gc(&repo, &GcOptions::with_reflog_policy(1_700_000_000, false)).expect("gc");

    assert!(
        repo.odb().read(&staged_oid).is_ok(),
        "gc must not delete index-only blob"
    );
    git_fsck(dir.path(), true);
    let report = repo_fsck(&repo, &FsckOptions::full_repository(true)).expect("fsck");
    assert!(report.ok, "{:?}", report.issues);
}

#[test]
fn fsck_skips_object_checks_when_check_objects_disabled() {
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

    let mut opts = FsckOptions::default();
    opts.strict = false;
    opts.check_objects = false;
    opts.check_refs = false;
    opts.check_midx = false;
    let report = repo_fsck(&repo, &opts).expect("fsck");
    assert!(report.ok, "{:?}", report.issues);
    assert!(
        !report.issues.iter().any(|i| i.msg_id == "missingObject"),
        "{:?}",
        report.issues
    );
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

    let report = repo_fsck(&repo, &FsckOptions::objects_and_refs(true)).expect("fsck");
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
fn gc_default_options_uses_wall_clock_for_reflog_expiry() {
    let dir = TempDir::new().expect("tempdir");
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "gc.auto", "0"]);
    git_commit_dated(dir.path(), "2000-01-01T00:00:00 +0000", "old");
    git_commit_dated(dir.path(), "2026-01-01T12:00:00 +0000", "new");
    git(dir.path(), &["reset", "--hard", "HEAD~1"]);

    let grit_copy = TempDir::new().expect("grit copy");
    duplicate_repo_tree(dir.path(), grit_copy.path());
    let explicit_copy = TempDir::new().expect("explicit copy");
    duplicate_repo_tree(dir.path(), explicit_copy.path());
    let git_copy = TempDir::new().expect("git copy");
    duplicate_repo_tree(dir.path(), git_copy.path());

    let repo_default =
        Repository::discover(Some(grit_copy.path()), &RepositoryOptions::empty()).expect("open");
    let repo_explicit =
        Repository::discover(Some(explicit_copy.path()), &RepositoryOptions::empty())
            .expect("open");

    let wall = OffsetDateTime::now_utc().unix_timestamp();
    let default_stats = gc(&repo_default, &GcOptions::default()).expect("default gc");
    let explicit_stats =
        gc(&repo_explicit, &GcOptions::with_reflog_policy(wall, true)).expect("explicit-now gc");

    assert_eq!(
        default_stats.reflog_entries_pruned, explicit_stats.reflog_entries_pruned,
        "GcOptions::default() must normalize now_unix before reflog expiry"
    );
    assert!(
        default_stats.reflog_entries_pruned > 0,
        "expected aged reflog entries to be pruned"
    );

    let grit_reflog = grit_copy.path().join(".git/logs/HEAD");
    let grit_body = std::fs::read_to_string(&grit_reflog).expect("grit HEAD reflog");
    assert!(
        !grit_body.contains("old"),
        "grit gc must expire aged reflog entries with default options"
    );

    let git_gc = Command::new("git")
        .current_dir(git_copy.path())
        .args(["gc", "--quiet"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git gc");
    assert!(
        git_gc.status.success(),
        "git gc: {}",
        String::from_utf8_lossy(&git_gc.stderr)
    );
    let git_reflog = git_copy.path().join(".git/logs/HEAD");
    let git_body = std::fs::read_to_string(&git_reflog).expect("git HEAD reflog");
    assert!(
        !git_body.contains("old"),
        "system git gc must expire the same aged reflog entries"
    );
    assert!(
        default_stats.reflog_entries_pruned >= 1,
        "grit must report pruned reflog entries when git gc also expires them"
    );
}

#[test]
fn gc_invalid_prune_expire_rejects_before_deleting_loose() {
    let dir = TempDir::new().expect("tempdir");
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "gc.auto", "0"]);
    git_commit_dated(dir.path(), "2000-01-01T00:00:00 +0000", "old");
    git_commit_dated(dir.path(), "2026-01-01T12:00:00 +0000", "new");
    git(dir.path(), &["reset", "--hard", "HEAD~1"]);

    let repo =
        Repository::discover(Some(dir.path()), &RepositoryOptions::empty()).expect("open repo");

    let orphan = repo
        .odb()
        .write_loose_materialize(ObjectKind::Blob, b"unreachable orphan payload\n")
        .expect("write loose");
    let loose_path: PathBuf = repo
        .odb()
        .enumerate_local_loose_objects()
        .expect("loose")
        .into_iter()
        .find(|(oid, _)| *oid == orphan)
        .map(|(_, path)| path)
        .expect("orphan loose path");
    filetime::set_file_mtime(&loose_path, FileTime::from_unix_time(978_307_200, 0))
        .expect("mtime 2001");

    git(dir.path(), &["config", "gc.pruneExpire", "bogus-value"]);

    let head_reflog = dir.path().join(".git/logs/HEAD");
    let reflog_before = std::fs::read(&head_reflog).expect("read HEAD reflog");
    assert_eq!(
        std::str::from_utf8(&reflog_before)
            .expect("utf8 reflog")
            .lines()
            .filter(|l| !l.is_empty())
            .count(),
        3,
        "fixture must start with a three-entry HEAD reflog"
    );

    let err = gc(&repo, &GcOptions::default())
        .expect_err("invalid gc.pruneExpire must fail before mutating");
    assert!(
        matches!(err, Error::Config(_)),
        "expected config error, got {err:?}"
    );
    let reflog_after = std::fs::read(&head_reflog).expect("read HEAD reflog after failed gc");
    assert_eq!(
        reflog_before, reflog_after,
        "failed grit gc must not mutate HEAD reflog"
    );
    assert!(
        repo.odb().read(&orphan).is_ok(),
        "invalid config must not delete unreachable loose objects"
    );

    let git_reflog_before = reflog_before.clone();
    let git_gc = Command::new("git")
        .current_dir(dir.path())
        .args(["gc", "--quiet"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git gc");
    assert!(
        !git_gc.status.success(),
        "system git gc must reject bogus gc.pruneExpire"
    );
    assert_eq!(git_gc.status.code(), Some(128));
    let git_reflog_after = std::fs::read(&head_reflog).expect("read HEAD reflog after git gc");
    assert_eq!(
        git_reflog_before, git_reflog_after,
        "system git gc must not mutate HEAD reflog on invalid gc.pruneExpire"
    );
    let out = Command::new("git")
        .current_dir(dir.path())
        .args(["cat-file", "-e", &orphan.to_hex()])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("cat-file");
    assert!(
        out.status.success(),
        "system git gc must preserve orphan when config is invalid"
    );
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

    let stats = gc(&repo, &GcOptions::with_reflog_policy(1_700_000_000, false)).expect("grit gc");
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

    let report = repo_fsck(&repo, &FsckOptions::objects_and_refs(true)).expect("grit fsck");
    assert!(
        report.ok,
        "gitlink to absent submodule commit must not fail fsck: {:?}",
        report.issues
    );
}
