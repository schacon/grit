//! Serial vs parallel rename/copy detection must match exactly.

use std::path::Path;
use std::process::Command;

use grit_lib::blame::{compute_blame, BlameDiffAlgorithm};
use grit_lib::config::ConfigSet;
use grit_lib::diff::{
    detect_copies_with_policy, detect_renames_with_policy, diff_trees, DiffEntry, DiffStatus,
};
use grit_lib::objects::ObjectId;
use grit_lib::parallel::ThreadPolicy;
use grit_lib::repo::Repository;
use grit_test_support::forced_parallel_policy;

fn git_run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Diff Test")
        .env("GIT_AUTHOR_EMAIL", "diff@example.com")
        .env("GIT_COMMITTER_NAME", "Diff Test")
        .env("GIT_COMMITTER_EMAIL", "diff@example.com")
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

fn rev_parse_tree(dir: &Path, rev: &str) -> ObjectId {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", rev])
        .output()
        .expect("rev-parse");
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("oid")
}

fn serial_policy() -> ThreadPolicy {
    ThreadPolicy::with_threads_override(Some(1))
        .min_items_override(Some(0))
        .min_total_bytes_override(Some(0))
}

fn parallel_policy() -> ThreadPolicy {
    forced_parallel_policy()
}

fn assert_serial_parallel_rename_match(
    odb: &grit_lib::odb::Odb,
    entries: Vec<DiffEntry>,
    threshold: u32,
    config: &ConfigSet,
) {
    let serial = detect_renames_with_policy(
        odb,
        None,
        entries.clone(),
        threshold,
        config,
        &serial_policy(),
    );
    let parallel =
        detect_renames_with_policy(odb, None, entries, threshold, config, &parallel_policy());
    assert_eq!(
        serial, parallel,
        "parallel rename detection must match serial output"
    );
}

fn git_name_status(dir: &Path, old_tree: &ObjectId, new_tree: &ObjectId) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "diff-tree",
            "-M50",
            "-C50",
            "--name-status",
            "--root",
            "-r",
            &old_tree.to_hex(),
            &new_tree.to_hex(),
        ])
        .output()
        .expect("diff-tree");
    assert!(
        out.status.success(),
        "git diff-tree: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn grit_name_status(entries: &[DiffEntry]) -> String {
    let mut lines: Vec<String> = entries
        .iter()
        .filter(|e| {
            matches!(
                e.status,
                DiffStatus::Renamed | DiffStatus::Copied | DiffStatus::Modified
            )
        })
        .map(|e| {
            let tag = match e.status {
                DiffStatus::Renamed => "R100",
                DiffStatus::Copied => "C100",
                DiffStatus::Modified => "M",
                _ => "?",
            };
            match (&e.old_path, &e.new_path) {
                (Some(old), Some(new)) if old != new => format!("{tag}\t{old}\t{new}"),
                (_, Some(new)) => format!("{tag}\t{new}"),
                (Some(old), None) => format!("D\t{old}"),
                _ => String::new(),
            }
        })
        .collect();
    lines.sort();
    lines.join("")
}

#[test]
fn parallel_rename_matches_serial_exact_and_inexact() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git_run(root, &["init", "-q", "-b", "main"]);

    std::fs::write(root.join("exact.txt"), b"same content\n").unwrap();
    let body: String = (1..=30)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(root.join("inexact.txt"), format!("{body}\n")).unwrap();
    git_run(root, &["add", "."]);
    git_run(root, &["commit", "-m", "base"]);

    std::fs::write(root.join("exact.txt"), b"same content\n").unwrap();
    let mut edited = body;
    edited = edited.replacen("line 10", "line TEN", 1);
    std::fs::remove_file(root.join("inexact.txt")).unwrap();
    std::fs::write(root.join("exact-renamed.txt"), b"same content\n").unwrap();
    std::fs::write(root.join("inexact-renamed.txt"), format!("{edited}\n")).unwrap();
    git_run(root, &["add", "-A"]);
    git_run(root, &["commit", "-m", "renames"]);

    let parent = rev_parse_tree(root, "HEAD~1^{tree}");
    let commit = rev_parse_tree(root, "HEAD^{tree}");
    let repo = Repository::discover(Some(root)).expect("open");
    let raw = diff_trees(&repo.odb, Some(&parent), Some(&commit), "").expect("diff");
    let config = ConfigSet::new();
    assert_serial_parallel_rename_match(&repo.odb, raw.clone(), 50, &config);

    let detected =
        detect_renames_with_policy(&repo.odb, None, raw, 50, &config, &parallel_policy());
    let git_lines = git_name_status(root, &parent, &commit);
    let grit_lines = grit_name_status(&detected);
    assert!(
        git_lines.contains("exact.txt") && git_lines.contains("exact-renamed.txt"),
        "git diff-tree should show exact rename: {git_lines}"
    );
    assert!(
        grit_lines.contains("exact.txt") && grit_lines.contains("exact-renamed.txt"),
        "grit should show exact rename: {grit_lines:?}"
    );
}

#[test]
fn parallel_copy_many_to_one_matches_serial() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git_run(root, &["init", "-q", "-b", "main"]);

    let shared = b"shared block one\nshared block two\nunique tail\n";
    std::fs::write(root.join("source.txt"), shared).unwrap();
    git_run(root, &["add", "source.txt"]);
    git_run(root, &["commit", "-m", "base"]);

    std::fs::remove_file(root.join("source.txt")).unwrap();
    for name in ["copy-a.txt", "copy-b.txt", "copy-c.txt"] {
        std::fs::write(root.join(name), shared).unwrap();
    }
    git_run(root, &["add", "-A"]);
    git_run(root, &["commit", "-m", "copies"]);

    let parent = rev_parse_tree(root, "HEAD~1^{tree}");
    let commit = rev_parse_tree(root, "HEAD^{tree}");
    let repo = Repository::discover(Some(root)).expect("open");
    let raw = diff_trees(&repo.odb, Some(&parent), Some(&commit), "").expect("diff");
    let source_tree = flatten_tree(&repo.odb, &parent);

    let config = ConfigSet::new();
    let serial = detect_copies_with_policy(
        &repo.odb,
        None,
        raw.clone(),
        50,
        false,
        &source_tree,
        &config,
        &serial_policy(),
    );
    let parallel = detect_copies_with_policy(
        &repo.odb,
        None,
        raw,
        50,
        false,
        &source_tree,
        &config,
        &parallel_policy(),
    );
    assert_eq!(
        serial, parallel,
        "parallel copy detection must match serial"
    );
    assert!(
        serial.iter().any(|e| e.status == DiffStatus::Renamed),
        "one destination should be rename: {serial:?}"
    );
    assert!(
        serial
            .iter()
            .filter(|e| e.status == DiffStatus::Copied)
            .count()
            >= 2,
        "remaining destinations should be copies: {serial:?}"
    );
}

fn flatten_tree(odb: &grit_lib::odb::Odb, tree_oid: &ObjectId) -> Vec<(String, String, ObjectId)> {
    use grit_lib::objects::parse_tree;
    fn walk(
        odb: &grit_lib::odb::Odb,
        tree_oid: &ObjectId,
        prefix: &str,
        out: &mut Vec<(String, String, ObjectId)>,
    ) {
        let obj = odb.read(tree_oid).expect("tree");
        let entries = parse_tree(&obj.data).expect("parse");
        for e in entries {
            let name = String::from_utf8_lossy(&e.name);
            let path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            if e.mode == 0o040000 {
                walk(odb, &e.oid, &path, out);
            } else {
                out.push((path, format!("{:o}", e.mode), e.oid));
            }
        }
    }
    let mut out = Vec::new();
    walk(odb, tree_oid, "", &mut out);
    out
}

#[test]
fn blame_copy_serial_matches_parallel_scoring() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git_run(root, &["init", "-q", "-b", "main"]);

    let shared = "alpha line\nbeta line\ngamma line\n";
    std::fs::write(root.join("source.txt"), shared).unwrap();
    git_run(root, &["add", "source.txt"]);
    git_run(root, &["commit", "-m", "source"]);

    std::fs::write(root.join("target.txt"), shared).unwrap();
    git_run(root, &["add", "target.txt"]);
    git_run(root, &["commit", "-m", "target"]);

    let repo = Repository::discover(Some(root)).expect("open");
    let start_commit = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse");
    let start_oid: ObjectId = String::from_utf8_lossy(&start_commit.stdout)
        .trim()
        .parse()
        .expect("oid");
    let grafts = std::collections::HashMap::new();
    let ignore = std::collections::HashSet::new();

    let serial = compute_blame(
        &repo.odb,
        start_oid,
        "target.txt",
        &ignore,
        BlameDiffAlgorithm::Myers,
        None,
        false,
        3,
        false,
        &grafts,
        false,
        &serial_policy(),
    )
    .expect("serial blame");
    let parallel = compute_blame(
        &repo.odb,
        start_oid,
        "target.txt",
        &ignore,
        BlameDiffAlgorithm::Myers,
        None,
        false,
        3,
        false,
        &grafts,
        false,
        &parallel_policy(),
    )
    .expect("parallel blame");
    assert_eq!(
        serial, parallel,
        "blame -C scoring must match across thread counts"
    );
}

#[test]
fn parallel_rename_matrix_with_many_pairs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git_run(root, &["init", "-q", "-b", "main"]);
    for i in 0..32 {
        std::fs::write(root.join(format!("old{i}.txt")), format!("payload {i}\n")).unwrap();
    }
    git_run(root, &["add", "."]);
    git_run(root, &["commit", "-m", "many"]);

    for i in 0..32 {
        let _ = std::fs::remove_file(root.join(format!("old{i}.txt")));
        std::fs::write(root.join(format!("new{i}.txt")), format!("payload {i}\n")).unwrap();
    }
    git_run(root, &["add", "-A"]);
    git_run(root, &["commit", "-m", "many renames"]);

    let parent = rev_parse_tree(root, "HEAD~1^{tree}");
    let commit = rev_parse_tree(root, "HEAD^{tree}");
    let repo = Repository::discover(Some(root)).expect("open");
    let raw = diff_trees(&repo.odb, Some(&parent), Some(&commit), "").expect("diff");
    let config = ConfigSet::new();
    assert_serial_parallel_rename_match(&repo.odb, raw, 50, &config);
}
