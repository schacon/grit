//! [`TargetTipLists`] ahead/behind counts and bounded OID lists vs system `git`.

use std::path::Path;
use std::process::Command;

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::objects::ObjectId;
use grit_lib::refs::branch_tracking::{TargetTipListOptions, TargetTipLists};
use grit_lib::repo::Repository;
use grit_test_support::{git, unique_tmp, write_file};

fn commit(dir: &Path, file: &str, message: &str) {
    write_file(dir, file, message);
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
            message,
        ],
    );
}

fn rev(dir: &Path, spec: &str) -> ObjectId {
    git(dir, &["rev-parse", spec])
        .trim()
        .parse()
        .expect("rev-parse prints an oid")
}

fn open(dir: &Path) -> Repository {
    Repository::discover(Some(dir), &RepositoryOptions::empty()).expect("open repo")
}

fn git_rev_list_count(dir: &Path, range: &str) -> usize {
    git(dir, &["rev-list", "--count", range])
        .trim()
        .parse()
        .expect("count")
}

fn git_rev_list_n(dir: &Path, n: usize, range: &str) -> Vec<ObjectId> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["rev-list", "-n", &n.to_string(), range])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git rev-list");
    assert!(out.status.success(), "git rev-list failed");
    out.stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            std::str::from_utf8(line)
                .expect("utf-8 oid")
                .parse()
                .expect("oid")
        })
        .collect()
}

fn diverged_clone(tag: &str) -> std::path::PathBuf {
    let root = unique_tmp("target-compare", tag);
    let origin = root.join("origin");
    std::fs::create_dir_all(&origin).expect("mkdir origin");
    git(&origin, &["init", "-q", "-b", "main"]);
    commit(&origin, "a", "a1");
    commit(&origin, "a", "a2");
    git(&root, &["clone", "-q", "origin", "clone"]);
    let clone = root.join("clone");
    commit(&clone, "b", "b1");
    commit(&clone, "b", "b2");
    commit(&origin, "a", "a3");
    git(&clone, &["fetch", "-q"]);
    clone
}

#[test]
fn target_comparison_behind_git_compat() -> grit_lib::error::Result<()> {
    let clone = diverged_clone("behind");
    let repo = open(&clone);
    let head = rev(&clone, "HEAD");
    let target = rev(&clone, "origin/main");

    let opts = TargetTipListOptions {
        list_limit: 1,
        include_behind: true,
    };
    let cmp = TargetTipLists::compute(&repo, head, target, opts)?;
    assert_eq!(cmp.ahead, git_rev_list_count(&clone, "origin/main..HEAD"));
    assert_eq!(cmp.behind, git_rev_list_count(&clone, "HEAD..origin/main"));
    assert_eq!(
        cmp.ahead_oids,
        git_rev_list_n(&clone, 1, "origin/main..HEAD")
    );
    assert_eq!(
        cmp.behind_oids,
        git_rev_list_n(&clone, 1, "HEAD..origin/main")
    );

    let bounded = TargetTipLists::compute(
        &repo,
        head,
        target,
        TargetTipListOptions {
            list_limit: 100,
            include_behind: false,
        },
    )?;
    assert_eq!(bounded.ahead, 2);
    assert_eq!(bounded.behind, 1);
    assert_eq!(bounded.ahead_oids.len(), 2);
    assert!(bounded.behind_oids.is_empty());
    Ok(())
}

#[test]
fn target_comparison_list_limit_bounds_lists_not_counts() -> grit_lib::error::Result<()> {
    let clone = diverged_clone("limit");
    let repo = open(&clone);
    let head = rev(&clone, "HEAD");
    let target = rev(&clone, "origin/main");

    let cmp = TargetTipLists::compute(
        &repo,
        head,
        target,
        TargetTipListOptions {
            list_limit: 1,
            include_behind: true,
        },
    )?;
    assert_eq!(cmp.ahead, 2);
    assert_eq!(cmp.behind, 1);
    assert_eq!(cmp.ahead_oids.len(), 1);
    assert_eq!(cmp.behind_oids.len(), 1);
    Ok(())
}
