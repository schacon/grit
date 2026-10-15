//! Stable public signature for [`grit_lib::blame::compute_blame`].

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use grit_lib::blame::{compute_blame, BlameDiffAlgorithm};
use grit_lib::config::environment::RepositoryOptions;
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;

fn git_run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Blame API Test")
        .env("GIT_AUTHOR_EMAIL", "blame-api@example.com")
        .env("GIT_COMMITTER_NAME", "Blame API Test")
        .env("GIT_COMMITTER_EMAIL", "blame-api@example.com")
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

fn head_oid(dir: &Path) -> ObjectId {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("oid")
}

#[test]
fn compute_blame_public_entry_point_attributes_lines() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git_run(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("f.txt"), "one\n").unwrap();
    git_run(root, &["add", "f.txt"]);
    git_run(root, &["commit", "-m", "init"]);
    std::fs::write(root.join("f.txt"), "one\ntwo\n").unwrap();
    git_run(root, &["add", "f.txt"]);
    git_run(root, &["commit", "-m", "add second line"]);

    let start = head_oid(root);
    let repo = Repository::discover(Some(root), &RepositoryOptions::empty()).expect("open");
    let ignore_revs = HashSet::new();
    let grafts = HashMap::new();

    let lines = compute_blame(
        repo.odb(),
        start,
        "f.txt",
        &ignore_revs,
        BlameDiffAlgorithm::Myers,
        None,
        false,
        0,
        false,
        &grafts,
        false,
        None,
        &grit_lib::parallel::ThreadPolicy::default(),
    )
    .expect("compute_blame");

    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].final_lineno, 1);
    assert_eq!(lines[1].final_lineno, 2);
    assert_ne!(
        lines[0].oid, lines[1].oid,
        "second line should be a newer commit"
    );
}

#[test]
fn compute_blame_line_range_limits_output_and_work() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git_run(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("f.txt"), "stable\n").unwrap();
    git_run(root, &["add", "f.txt"]);
    git_run(root, &["commit", "-m", "intro"]);
    let root_commit = head_oid(root);
    for i in 0..80 {
        std::fs::write(root.join("other.txt"), format!("noise {i}\n")).unwrap();
        git_run(root, &["add", "other.txt"]);
        git_run(root, &["commit", "-m", &format!("touch {i}")]);
    }

    let start = head_oid(root);
    let repo = Repository::discover(Some(root), &RepositoryOptions::empty()).expect("open");
    let ignore_revs = HashSet::new();
    let grafts = HashMap::new();
    let policy = grit_lib::parallel::ThreadPolicy::default();

    let ranged = compute_blame(
        repo.odb(),
        start,
        "f.txt",
        &ignore_revs,
        BlameDiffAlgorithm::Myers,
        None,
        false,
        0,
        true,
        &grafts,
        false,
        Some(1..=1),
        &policy,
    )
    .expect("ranged blame");

    assert_eq!(ranged.len(), 1);
    assert_eq!(ranged[0].final_lineno, 1);
    assert_eq!(ranged[0].oid, root_commit);
}
