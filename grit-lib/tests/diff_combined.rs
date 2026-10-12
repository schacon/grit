//! Combined merge diffs vs `git diff-tree -c/--cc`.

mod support;

use std::io::Write;

use grit_lib::combined_tree_diff::{
    combined_diff_paths_filtered, format_combined_raw_line, CombinedTreeDiffOptions,
};
use grit_lib::merge_diff::combined_diff_paths;
use grit_lib::objects::{parse_commit, ObjectId};

use support::{git_quiet, git_raw_lines, DiffRepo};

fn read_merge(wt: &std::path::Path, commit: &ObjectId) -> (ObjectId, Vec<ObjectId>) {
    let repo = grit_lib::repo::Repository::discover(Some(wt)).expect("open");
    let data = repo.odb.read(commit).expect("read").data;
    let parsed = parse_commit(&data).expect("parse");
    (parsed.tree, parsed.parents)
}

#[test]
fn t4038_combined_raw_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    std::fs::write(wt.join("file"), "base\n").unwrap();
    git_quiet(wt, &["add", "file"]);
    git_quiet(wt, &["commit", "-qm", "root"]);

    git_quiet(wt, &["checkout", "-q", "-b", "p1"]);
    std::fs::write(wt.join("file"), "one\n").unwrap();
    git_quiet(wt, &["add", "file"]);
    git_quiet(wt, &["commit", "-qm", "p1"]);
    let p1: ObjectId = git_quiet(wt, &["rev-parse", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    git_quiet(wt, &["checkout", "-q", "main"]);
    std::fs::write(wt.join("file"), "two\n").unwrap();
    git_quiet(wt, &["add", "file"]);
    git_quiet(wt, &["commit", "-qm", "p2"]);
    let p2: ObjectId = git_quiet(wt, &["rev-parse", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    git_quiet(wt, &["checkout", "-q", "-b", "merge", "p1"]);
    std::fs::write(wt.join("file"), "merged\n").unwrap();
    git_quiet(wt, &["add", "file"]);
    let merge_tree: ObjectId = git_quiet(wt, &["write-tree"]).trim().parse().unwrap();
    let merge_commit: ObjectId = git_quiet(
        wt,
        &[
            "commit-tree",
            &merge_tree.to_hex(),
            "-p",
            &p1.to_hex(),
            "-p",
            &p2.to_hex(),
            "-m",
            "merge",
        ],
    )
    .trim()
    .parse()
    .unwrap();

    let git_lines = git_raw_lines(
        wt,
        &["diff-tree", "--cc", "-r", "--raw", &merge_commit.to_hex()],
    );

    let (result_tree, parents) = read_merge(wt, &merge_commit);
    assert_eq!(parents.len(), 2);
    let walk = CombinedTreeDiffOptions::default();
    let paths = combined_diff_paths_filtered(&dr.repo.odb, &result_tree, &parents, &walk, None)
        .expect("combined paths");
    let mut grit_lines: Vec<String> = paths
        .iter()
        .map(|p| format_combined_raw_line(p, None))
        .collect();
    grit_lines.sort();

    assert_eq!(
        git_lines, grit_lines,
        "combined raw lines must match git diff-tree --cc --raw"
    );
}

#[test]
fn t4057_combined_paths_octopus_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    for i in 1..=9 {
        std::fs::write(wt.join(format!("{i}.txt")), format!("{i}\n")).unwrap();
    }
    git_quiet(wt, &["add", "."]);
    git_quiet(wt, &["commit", "-qm", "init"]);

    git_quiet(wt, &["checkout", "-q", "-b", "side"]);
    for i in 2..=9 {
        std::fs::OpenOptions::new()
            .append(true)
            .open(wt.join(format!("{i}.txt")))
            .unwrap()
            .write_all(format!("{i}/2\n").as_bytes())
            .unwrap();
    }
    git_quiet(wt, &["commit", "-am", "side"]);

    git_quiet(wt, &["checkout", "-q", "main"]);
    std::fs::OpenOptions::new()
        .append(true)
        .open(wt.join("1.txt"))
        .unwrap()
        .write_all(b"1/2\n")
        .unwrap();
    git_quiet(wt, &["commit", "-am", "main"]);

    git_quiet(wt, &["merge", "side", "-m", "merge"]);
    let merge: ObjectId = git_quiet(wt, &["rev-parse", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let git_names = {
        let out = support::git_output(
            wt,
            &["diff", "-c", "--name-only", "HEAD", "HEAD^", "HEAD^2"],
        );
        assert!(out.status.success());
        let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        names.sort();
        names
    };

    let (tree, parents) = read_merge(wt, &merge);
    let mut grit_sorted = combined_diff_paths(&dr.repo.odb, &tree, &parents);
    grit_sorted.sort();

    assert_eq!(
        git_names, grit_sorted,
        "combined path list must match git diff -c --name-only"
    );
}
