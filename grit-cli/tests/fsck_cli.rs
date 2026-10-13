//! Exit codes and integrity reporting for `grit fsck`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::index::Index;
use grit_lib::porcelain::add::stage_for_commit;
use grit_lib::progress::NullProgress;
use grit_lib::repo::Repository;
use grit_test_support::CliFixture;

fn grit_fsck(fixture: &CliFixture, extra: &[&str]) -> i32 {
    let mut args = vec!["fsck"];
    args.extend(extra);
    fixture.run(&args).status.unwrap_or(1)
}

#[test]
fn fsck_cli_exit_codes() {
    let clean = CliFixture::new();
    assert_eq!(grit_fsck(&clean, &[]), 0, "clean repo should exit 0");

    let dangling = CliFixture::empty();
    let wt = dangling.path();
    let mut child = Command::new("git")
        .current_dir(wt)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .spawn()
        .expect("hash-object");
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(b"unreferenced\n").expect("write");
    }
    let out = child.wait_with_output().expect("hash-object wait");
    assert!(out.status.success(), "hash-object: {:?}", out.stderr);
    assert_eq!(
        grit_fsck(&dangling, &[]),
        0,
        "dangling object alone should exit 0"
    );

    let corrupt = CliFixture::empty();
    one_commit_with_staged_blob(&corrupt);
    let repo = Repository::discover(Some(corrupt.path()), &RepositoryOptions::default())
        .expect("discover");
    let index = Index::load(&repo.git_dir().join("index")).expect("index");
    let staged_oid = index
        .entries()
        .iter()
        .find(|e| e.path.ends_with(b"missing.txt"))
        .expect("entry")
        .oid;
    let loose = repo
        .odb()
        .enumerate_local_loose_objects()
        .expect("loose")
        .into_iter()
        .find(|(oid, _)| *oid == staged_oid)
        .expect("loose path");
    fs::remove_file(loose.1).expect("remove blob");

    assert_eq!(
        grit_fsck(&corrupt, &[]),
        1,
        "missing index blob should exit 1"
    );
}

fn one_commit_with_staged_blob(fixture: &CliFixture) {
    let repo = Repository::discover(Some(fixture.path()), &RepositoryOptions::default())
        .expect("discover");
    fs::write(repo.work_tree().expect("wt").join("missing.txt"), b"x").expect("write");
    stage_for_commit(&repo, &mut NullProgress).expect("stage");
}
