//! Tree, index, and worktree diffs with rename/copy/break vs system `git` raw output.

mod support;

use std::fs;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::diff::{diff_trees, DiffPostprocessOptions, GIT_DIFF_DEFAULT_BREAK_SCORE};
use grit_lib::error::Error;
use grit_lib::objects::ObjectId;

use support::{
    assert_raw_matches_git, git_quiet, git_raw_lines, head_tree_oid, index_to_tree_raw,
    index_to_worktree_raw, rev_parse_tree, tree_diff_raw, write_bogus_tree, DiffRepo,
};

fn default_opts() -> DiffPostprocessOptions {
    DiffPostprocessOptions {
        rename_threshold: 100,
        ..Default::default()
    }
}

fn rename_opts(threshold: u32) -> DiffPostprocessOptions {
    DiffPostprocessOptions {
        rename_threshold: threshold,
        ..Default::default()
    }
}

#[test]
fn t4002_tree_index_worktree_raw_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    fs::write(wt.join("a.txt"), "alpha\n").unwrap();
    fs::write(wt.join("b.txt"), "beta\n").unwrap();
    git_quiet(wt, &["add", "."]);
    git_quiet(wt, &["commit", "-qm", "base"]);

    let tree_o = rev_parse_tree(wt, "HEAD^{tree}");
    fs::write(wt.join("a.txt"), "alpha2\n").unwrap();
    fs::write(wt.join("c.txt"), "new\n").unwrap();
    git_quiet(wt, &["add", "a.txt", "c.txt"]);
    let tree_a: ObjectId = git_quiet(wt, &["write-tree"]).trim().parse().unwrap();

    let config = ConfigSet::new();
    let opts = default_opts();

    let git_tree = git_raw_lines(
        wt,
        &[
            "diff-tree",
            "-r",
            "--root",
            "--raw",
            &tree_o.to_hex(),
            &tree_a.to_hex(),
        ],
    );
    let grit_tree = tree_diff_raw(&dr.repo, Some(&tree_o), Some(&tree_a), &config, &opts);
    assert_raw_matches_git(&grit_tree, &git_tree, "tree↔tree");

    let git_index = git_raw_lines(
        wt,
        &[
            "diff-index",
            "-r",
            "--cached",
            "--root",
            "--raw",
            &tree_o.to_hex(),
        ],
    );
    let index = dr.load_index();
    let grit_index = index_to_tree_raw(&dr.repo, &index, &tree_o, &config, &opts);
    assert_raw_matches_git(&grit_index, &git_index, "index↔tree (cached)");

    fs::write(wt.join("b.txt"), "beta-worktree\n").unwrap();
    let git_files = git_raw_lines(wt, &["diff-files", "-r", "--raw"]);
    let mut index = dr.load_index();
    let head_tree = head_tree_oid(&dr.repo);
    let grit_files =
        index_to_worktree_raw(&dr.repo, wt, &mut index, head_tree.as_ref(), &config, &opts);
    assert_raw_matches_git(&grit_files, &git_files, "index↔worktree");
}

#[test]
fn t4001_rename_detection_scores_match_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    let path0: String = (1..=15)
        .map(|i| {
            if i == 11 {
                "line 11".to_string()
            } else {
                format!("Line {i}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(wt.join("path0"), format!("{path0}\n")).unwrap();
    git_quiet(wt, &["add", "path0"]);
    git_quiet(wt, &["commit", "-qm", "base"]);

    let mut edited = path0.replace("line 11", "Line 11");
    fs::remove_file(wt.join("path0")).unwrap();
    fs::write(wt.join("path1"), format!("{edited}\n")).unwrap();
    git_quiet(wt, &["add", "-A"]);
    git_quiet(wt, &["commit", "-qm", "rename"]);

    let parent = rev_parse_tree(wt, "HEAD~1^{tree}");
    let commit = rev_parse_tree(wt, "HEAD^{tree}");
    let config = ConfigSet::new();
    let opts = rename_opts(50);

    let git_lines = git_raw_lines(
        wt,
        &[
            "diff-tree",
            "-r",
            "-M50",
            "--root",
            "--raw",
            &parent.to_hex(),
            &commit.to_hex(),
        ],
    );
    let grit_lines = tree_diff_raw(&dr.repo, Some(&parent), Some(&commit), &config, &opts);
    assert_raw_matches_git(&grit_lines, &git_lines, "t4001 rename scores");
}

#[test]
fn t4007_copy_detection_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    fs::create_dir_all(wt.join("path0")).unwrap();
    fs::create_dir_all(wt.join("path1")).unwrap();
    let content = "copy me\nline two\n";
    fs::write(wt.join("path0/COPYING"), content).unwrap();
    git_quiet(wt, &["add", "path0/COPYING"]);
    let tree = git_quiet(wt, &["write-tree"])
        .trim()
        .parse::<ObjectId>()
        .unwrap();

    fs::write(wt.join("path1/COPYING"), content).unwrap();
    git_quiet(wt, &["add", "path1/COPYING"]);

    let config = ConfigSet::new();
    let opts = DiffPostprocessOptions {
        rename_threshold: 50,
        copy_threshold: Some(50),
        find_copies_harder: true,
        ..Default::default()
    };

    let git_lines = git_raw_lines(
        wt,
        &[
            "diff-index",
            "-r",
            "-C50",
            "--find-copies-harder",
            "--root",
            "--raw",
            &tree.to_hex(),
        ],
    );
    let index = dr.load_index();
    let grit_lines = index_to_tree_raw(&dr.repo, &index, &tree, &config, &opts);
    assert_raw_matches_git(&grit_lines, &git_lines, "t4007 copy harder");
}

#[test]
fn t4008_break_rewrite_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    fs::write(wt.join("file0"), "some dissimilar content\n").unwrap();
    fs::write(
        wt.join("file1"),
        "totally different long body\nsecond line\n",
    )
    .unwrap();
    git_quiet(wt, &["add", "file0", "file1"]);
    let reference = git_quiet(wt, &["write-tree"])
        .trim()
        .parse::<ObjectId>()
        .unwrap();

    let body = fs::read_to_string(wt.join("file0"))
        .unwrap()
        .replace("some", "SOME");
    fs::write(wt.join("file1"), body).unwrap();
    fs::remove_file(wt.join("file0")).unwrap();
    git_quiet(wt, &["add", "-u"]);

    let config = ConfigSet::new();
    let break_only = DiffPostprocessOptions {
        break_score: Some(GIT_DIFF_DEFAULT_BREAK_SCORE),
        rename_threshold: 100,
        ..Default::default()
    };
    let git_b = git_raw_lines(
        wt,
        &[
            "diff-index",
            "-r",
            "-B",
            "--cached",
            "--root",
            "--raw",
            &reference.to_hex(),
        ],
    );
    let index = dr.load_index();
    let grit_b = index_to_tree_raw(&dr.repo, &index, &reference, &config, &break_only);
    assert_raw_matches_git(&grit_b, &git_b, "t4008 break without rename");

    let break_rename = DiffPostprocessOptions {
        break_score: Some(GIT_DIFF_DEFAULT_BREAK_SCORE),
        rename_threshold: 50,
        ..Default::default()
    };
    let git_br = git_raw_lines(
        wt,
        &[
            "diff-index",
            "-r",
            "-B",
            "-M50",
            "--cached",
            "--root",
            "--raw",
            &reference.to_hex(),
        ],
    );
    let grit_br = index_to_tree_raw(&dr.repo, &index, &reference, &config, &break_rename);
    assert_raw_matches_git(&grit_br, &git_br, "t4008 break with rename");
}

#[test]
fn t4023_typechange_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    fs::write(wt.join("f"), "blob\n").unwrap();
    git_quiet(wt, &["add", "f"]);
    git_quiet(wt, &["commit", "-qm", "blob"]);
    let parent = rev_parse_tree(wt, "HEAD^{tree}");

    fs::remove_file(wt.join("f")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("target", wt.join("f")).unwrap();
    #[cfg(not(unix))]
    {
        eprintln!("SKIP: t4023_typechange requires unix symlinks");
        return;
    }
    git_quiet(wt, &["add", "f"]);
    let commit: ObjectId = git_quiet(wt, &["write-tree"]).trim().parse().unwrap();

    let config = ConfigSet::new();
    let opts = default_opts();
    let git_lines = git_raw_lines(
        wt,
        &[
            "diff-tree",
            "-r",
            "--root",
            "--raw",
            &parent.to_hex(),
            &commit.to_hex(),
        ],
    );
    let grit_lines = tree_diff_raw(&dr.repo, Some(&parent), Some(&commit), &config, &opts);
    assert_raw_matches_git(&grit_lines, &git_lines, "t4023 typechange");
}

#[test]
fn t4054_bogus_tree_typed_error() {
    let dr = DiffRepo::init();
    git_quiet(&dr.worktree, &["commit", "--allow-empty", "-qm", "empty"]);
    let bogus = write_bogus_tree(&dr.repo.odb);
    let good = rev_parse_tree(&dr.worktree, "HEAD^{tree}");
    let err = diff_trees(&dr.repo.odb, Some(&good), Some(&bogus), "").unwrap_err();
    assert!(
        matches!(err, Error::CorruptObject(_)),
        "expected CorruptObject, got {err:?}"
    );
}

#[test]
fn rename_limit_wide_tree_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    const N: usize = 1001;
    for i in 0..N {
        fs::write(wt.join(format!("old{i}.txt")), format!("payload {i}\n")).unwrap();
    }
    git_quiet(wt, &["add", "."]);
    git_quiet(wt, &["commit", "-qm", "many"]);

    for i in 0..N {
        fs::remove_file(wt.join(format!("old{i}.txt"))).unwrap();
        fs::write(wt.join(format!("new{i}.txt")), format!("payload {i}\n")).unwrap();
    }
    git_quiet(wt, &["add", "-A"]);
    git_quiet(wt, &["commit", "-qm", "renamed"]);

    let parent = rev_parse_tree(wt, "HEAD~1^{tree}");
    let commit = rev_parse_tree(wt, "HEAD^{tree}");
    let config = ConfigSet::new();
    let opts = rename_opts(50);

    let git_lines = git_raw_lines(
        wt,
        &[
            "diff-tree",
            "-r",
            "-M50",
            "--root",
            "--raw",
            &parent.to_hex(),
            &commit.to_hex(),
        ],
    );
    let grit_lines = tree_diff_raw(&dr.repo, Some(&parent), Some(&commit), &config, &opts);
    assert_eq!(git_lines.len(), N);
    assert_raw_matches_git(&grit_lines, &git_lines, "renameLimit wide tree");
}

#[test]
fn t4006_diff_mode_raw_matches_git() {
    let dr = DiffRepo::init();
    let wt = &dr.worktree;
    fs::write(wt.join("x"), "v1\n").unwrap();
    git_quiet(wt, &["add", "x"]);
    git_quiet(wt, &["commit", "-qm", "c1"]);
    let old = rev_parse_tree(wt, "HEAD^{tree}");
    fs::write(wt.join("x"), "v2\n").unwrap();
    git_quiet(wt, &["add", "x"]);
    let new = git_quiet(wt, &["write-tree"])
        .trim()
        .parse::<ObjectId>()
        .unwrap();
    let config = ConfigSet::new();
    let opts = default_opts();
    let git_lines = git_raw_lines(
        wt,
        &[
            "diff-tree",
            "-r",
            "--root",
            "--raw",
            &old.to_hex(),
            &new.to_hex(),
        ],
    );
    let grit_lines = tree_diff_raw(&dr.repo, Some(&old), Some(&new), &config, &opts);
    assert_raw_matches_git(&grit_lines, &git_lines, "t4006 mode");
}
