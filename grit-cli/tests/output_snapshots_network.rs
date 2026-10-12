//! Golden snapshots for network commands (clone, fetch, pull, push, remote refs).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use grit_test_support::{
    git, git_fsck, grit, CliFixture, CliFixtureWithUpstream, NetworkSnapshotOptions,
};

fn snapshot_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots"))
}

fn push_opts() -> NetworkSnapshotOptions {
    NetworkSnapshotOptions {
        fsck_upstream: true,
    }
}

fn noop_opts() -> NetworkSnapshotOptions {
    NetworkSnapshotOptions::default()
}

fn empty_commit_on_bare(upstream: &Path, message: &str) {
    let tree = git(upstream, &["rev-parse", "HEAD^{tree}"]);
    let parent = git(upstream, &["rev-parse", "HEAD"]);
    let mut cmd = std::process::Command::new("git");
    cmd.args([
        "commit-tree",
        tree.trim(),
        "-p",
        parent.trim(),
        "-m",
        message,
    ])
    .current_dir(upstream);
    grit_test_support::configure_git_env(&mut cmd);
    let out = cmd.output().expect("git commit-tree");
    assert!(
        out.status.success(),
        "commit-tree: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    git(upstream, &["update-ref", "refs/heads/main", &oid]);
}

#[test]
fn snapshot_clone_local() {
    let net = CliFixture::new().with_upstream();
    let upstream = net.upstream_path().to_str().expect("upstream utf8");
    net.snapshot_network(
        net.workspace_path(),
        "clone_local",
        &["clone", upstream, "cloned"],
        snapshot_dir(),
        noop_opts(),
    );
}

#[test]
fn snapshot_fetch_noop() {
    let net = CliFixture::new().with_upstream();
    let client = net.git_clone_client("client");
    git(&client, &["fetch", "-q", "origin"]);
    git(&client, &["reset", "--hard", "origin/main"]);
    net.snapshot_network(
        &client,
        "fetch_noop",
        &["fetch"],
        snapshot_dir(),
        noop_opts(),
    );
}

#[test]
fn snapshot_fetch_new() {
    let net = CliFixture::new().with_upstream();
    let client = net.git_clone_client("client");
    git(&client, &["fetch", "-q", "origin"]);
    git(&client, &["reset", "--hard", "origin/main"]);
    empty_commit_on_bare(net.upstream_path(), "upstream-ahead");
    net.snapshot_network(
        &client,
        "fetch_new",
        &["fetch"],
        snapshot_dir(),
        noop_opts(),
    );
}

#[test]
fn snapshot_fetch_prune() {
    let net = CliFixture::new().with_upstream();
    let client = net.git_clone_client("client");
    git(
        net.upstream_path(),
        &["update-ref", "refs/heads/prune-me", "HEAD"],
    );
    git(&client, &["fetch", "-q", "origin"]);
    git(
        net.upstream_path(),
        &["update-ref", "-d", "refs/heads/prune-me"],
    );
    net.snapshot_network(
        &client,
        "fetch_prune",
        &["fetch", "--prune"],
        snapshot_dir(),
        noop_opts(),
    );
}

#[test]
fn snapshot_pull_ff() {
    let net = CliFixture::new().with_upstream();
    let client = net.git_clone_client("client");
    git(&client, &["fetch", "-q", "origin"]);
    git(&client, &["reset", "--hard", "origin/main"]);
    empty_commit_on_bare(net.upstream_path(), "pull-ff-tip");
    net.snapshot_network(&client, "pull_ff", &["pull"], snapshot_dir(), noop_opts());
}

#[test]
fn snapshot_push_ok() {
    let net = CliFixture::new().with_upstream();
    let client = synced_client(&net);
    std::fs::write(client.join("push.txt"), "push ok\n").unwrap();
    let add = grit(&client, &["add", "push.txt"]);
    assert!(add.ok(), "grit add: {}", add.stderr);
    let commit = grit(&client, &["commit", "-m", "push ok"]);
    assert!(commit.ok(), "grit commit: {}", commit.stderr);
    net.snapshot_network(&client, "push_ok", &["push"], snapshot_dir(), push_opts());
}

#[test]
fn snapshot_push_up_to_date() {
    let net = CliFixture::new().with_upstream();
    let client = synced_client(&net);
    net.snapshot_network(
        &client,
        "push_up_to_date",
        &["push"],
        snapshot_dir(),
        push_opts(),
    );
}

#[test]
fn snapshot_push_rejected() {
    let net = CliFixture::new().with_upstream();
    let client = synced_client(&net);
    empty_commit_on_bare(net.upstream_path(), "remote-first");
    std::fs::write(client.join("local.txt"), "local only\n").unwrap();
    let add = grit(&client, &["add", "local.txt"]);
    assert!(add.ok(), "grit add: {}", add.stderr);
    let commit = grit(&client, &["commit", "-m", "local"]);
    assert!(commit.ok(), "grit commit: {}", commit.stderr);
    net.snapshot_network(
        &client,
        "push_rejected",
        &["push"],
        snapshot_dir(),
        noop_opts(),
    );
}

#[test]
fn snapshot_push_tags() {
    let net = CliFixture::new().with_upstream();
    let client = synced_client(&net);
    git(&client, &["tag", "-a", "v0.2", "-m", "two"]);
    net.snapshot_network(
        &client,
        "push_tags",
        &["push", "--tags"],
        snapshot_dir(),
        push_opts(),
    );
}

#[test]
fn snapshot_push_delete() {
    let net = CliFixture::new().with_upstream();
    let client = synced_client(&net);
    git(&client, &["checkout", "-b", "old-topic"]);
    git(&client, &["commit", "--allow-empty", "-qm", "topic"]);
    git(&client, &["push", "-u", "origin", "old-topic"]);
    git(&client, &["checkout", "main"]);
    git(&client, &["branch", "-D", "old-topic"]);
    let fsck_before = git_fsck(net.upstream_path(), true);
    assert!(fsck_before.ok);
    net.snapshot_network(
        &client,
        "push_delete",
        &["push", "--delete", "old-topic", "--remote", "origin"],
        snapshot_dir(),
        push_opts(),
    );
}

#[test]
fn snapshot_remote_refs_heads() {
    let net = CliFixture::new().with_upstream();
    let upstream = net.upstream_path().to_str().expect("upstream utf8");
    net.snapshot_network(
        net.workspace_path(),
        "remote_refs_heads",
        &["remote", "refs", upstream, "--heads"],
        snapshot_dir(),
        noop_opts(),
    );
}

#[test]
fn snapshot_remote_refs_tags() {
    let net = CliFixture::new().with_upstream();
    let upstream = net.upstream_path().to_str().expect("upstream utf8");
    net.snapshot_network(
        net.workspace_path(),
        "remote_refs_tags",
        &["remote", "refs", upstream, "--tags"],
        snapshot_dir(),
        noop_opts(),
    );
}

fn synced_client(net: &CliFixtureWithUpstream) -> PathBuf {
    let client = net.git_clone_client("client");
    git(&client, &["fetch", "-q", "origin"]);
    git(&client, &["reset", "--hard", "origin/main"]);
    client
}
