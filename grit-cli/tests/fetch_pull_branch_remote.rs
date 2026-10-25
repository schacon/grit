//! `grit fetch` and `grit pull` with no remote argument use the current branch's
//! `branch.<name>.remote`, falling back to `origin` only when none is configured.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use grit_test_support::{git, unique_tmp, write_file};
use serde_json::Value;

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

fn grit(dir: &Path, args: &[&str]) -> Output {
    Command::new(GRIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("spawn grit")
}

fn grit_ok(dir: &Path, args: &[&str]) -> String {
    let out = grit(dir, args);
    assert!(
        out.status.success(),
        "grit {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn grit_json(dir: &Path, args: &[&str]) -> Value {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    serde_json::from_str(&grit_ok(dir, &full)).expect("json")
}

fn commit(dir: &Path, file: &str, msg: &str) {
    write_file(dir, file, msg);
    git(dir, &["add", file]);
    git(
        dir,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            msg,
        ],
    );
}

struct Fixture {
    work: PathBuf,
    other: PathBuf,
}

/// `work` has remotes `origin` (a.git) and `backup` (b.git) with `main` tracking
/// `backup/main`; `other` is a clone of b.git that can push new commits there.
fn setup(tag: &str) -> Fixture {
    let root = unique_tmp("fetch-pull-branch-remote", tag);
    fs::create_dir_all(&root).expect("root");
    git(&root, &["init", "-q", "--bare", "-b", "main", "a.git"]);
    git(&root, &["init", "-q", "--bare", "-b", "main", "b.git"]);
    let work = root.join("work");
    fs::create_dir_all(&work).expect("work");
    git(&work, &["init", "-q", "-b", "main"]);
    commit(&work, "f", "one");
    git(&work, &["remote", "add", "origin", "../a.git"]);
    git(&work, &["remote", "add", "backup", "../b.git"]);
    git(&work, &["push", "-q", "origin", "main"]);
    git(&work, &["push", "-q", "-u", "backup", "main"]);
    git(&root, &["clone", "-q", "-b", "main", "b.git", "other"]);
    Fixture {
        work,
        other: root.join("other"),
    }
}

fn push_new_commit_to_backup(fx: &Fixture) -> String {
    commit(&fx.other, "f", "three");
    git(&fx.other, &["push", "-q", "origin", "main"]);
    git(&fx.other, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn rev(dir: &Path, spec: &str) -> String {
    git(dir, &["rev-parse", spec]).trim().to_owned()
}

#[test]
fn fetch_without_remote_uses_branch_remote() {
    let fx = setup("fetch");
    let tip = push_new_commit_to_backup(&fx);
    let json = grit_json(&fx.work, &["fetch"]);
    assert_eq!(json["remote"], "backup");
    assert_eq!(rev(&fx.work, "refs/remotes/backup/main"), tip);
    git(&fx.work, &["fsck", "--strict"]);
}

#[test]
fn fetch_human_names_branch_remote() {
    let fx = setup("fetch-human");
    push_new_commit_to_backup(&fx);
    let out = grit_ok(&fx.work, &["fetch"]);
    assert!(out.contains("from backup"), "{out}");
}

#[test]
fn fetch_without_branch_remote_falls_back_to_origin() {
    let fx = setup("fetch-origin");
    git(&fx.work, &["config", "--unset", "branch.main.remote"]);
    let json = grit_json(&fx.work, &["fetch"]);
    assert_eq!(json["remote"], "origin");
}

/// `feat` tracks the local `main` (`branch.feat.remote = .`), which tracks `backup`.
fn checkout_local_tracking_branch(fx: &Fixture) {
    git(
        &fx.work,
        &["checkout", "-q", "-b", "feat", "--track", "main"],
    );
    assert_eq!(git(&fx.work, &["config", "branch.feat.remote"]).trim(), ".");
}

#[test]
fn fetch_on_local_tracking_branch_fetches_origin() {
    let fx = setup("fetch-local-upstream");
    checkout_local_tracking_branch(&fx);
    commit(&fx.other, "f", "on-origin");
    let a_git = fx.work.parent().unwrap().join("a.git");
    git(
        &fx.other,
        &["push", "-q", a_git.to_str().unwrap(), "HEAD:main"],
    );
    let tip = rev(&fx.other, "HEAD");
    let json = grit_json(&fx.work, &["fetch"]);
    assert_eq!(json["remote"], "origin");
    assert_eq!(rev(&fx.work, "refs/remotes/origin/main"), tip);
}

#[test]
fn pull_on_local_tracking_branch_integrates_local_upstream() {
    let fx = setup("pull-local-upstream");
    checkout_local_tracking_branch(&fx);
    git(&fx.work, &["checkout", "-q", "main"]);
    commit(&fx.work, "g", "local-main");
    let main_tip = rev(&fx.work, "main");
    git(&fx.work, &["checkout", "-q", "feat"]);
    let json = grit_json(&fx.work, &["pull"]);
    assert_eq!(json["result"], "fast_forward", "{json}");
    assert_eq!(rev(&fx.work, "HEAD"), main_tip);
}

#[test]
fn pull_without_remote_fast_forwards_from_branch_remote() {
    let fx = setup("pull");
    let tip = push_new_commit_to_backup(&fx);
    let json = grit_json(&fx.work, &["pull"]);
    assert_eq!(json["result"], "fast_forward");
    assert_eq!(json["branch"], "backup/main");
    assert_eq!(rev(&fx.work, "HEAD"), tip);
    git(&fx.work, &["fsck", "--strict"]);
}

#[test]
fn pull_ff_only_without_remote_uses_branch_remote() {
    let fx = setup("pull-ff-only");
    let tip = push_new_commit_to_backup(&fx);
    grit_ok(&fx.work, &["pull", "--ff-only"]);
    assert_eq!(rev(&fx.work, "HEAD"), tip);
}

#[test]
fn pull_check_without_remote_sees_new_upstream_commits() {
    let fx = setup("pull-check");
    let before = rev(&fx.work, "HEAD");
    push_new_commit_to_backup(&fx);
    let json = grit_json(&fx.work, &["pull", "--check"]);
    assert_eq!(json["check"]["behind"], 1, "{json}");
    assert_eq!(json["check"]["fast_forward_possible"], true);
    assert_eq!(rev(&fx.work, "HEAD"), before, "--check must not move HEAD");
}
