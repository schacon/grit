//! Linked worktree add/remove matches Git layout and interoperates with `git worktree list`.

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::porcelain::add::{stage, StageOptions};
use grit_lib::porcelain::commit::{create_commit, CommitRequest};
use grit_lib::progress::NullProgress;
use grit_lib::repo::Repository;
use grit_lib::worktree::{
    add_linked_worktree, list_worktrees, remove_linked_worktree, AddLinkedWorktreeOptions,
};
use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn grit_add_and_remove_linked_worktree_git_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let main = root.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("tracked.txt"), "1\n").unwrap();
    git(&main, &["add", "tracked.txt"]);
    git(&main, &["commit", "-qm", "seed"]);

    let repo =
        Repository::discover(Some(main.as_path()), &RepositoryOptions::empty()).expect("discover");
    let linked = root.join("linked");
    let entry = add_linked_worktree(
        &repo,
        &AddLinkedWorktreeOptions {
            path: linked.clone(),
            branch: Some("linked".to_owned()),
            new_branch: true,
            detach: false,
            start_point: Some("HEAD".to_owned()),
            force: false,
        },
    )
    .expect("add worktree");

    let gitdir_line = std::fs::read_to_string(linked.join(".git")).expect("linked .git");
    assert!(
        gitdir_line.starts_with("gitdir: "),
        "unexpected .git file: {gitdir_line}"
    );
    let stored_admin = gitdir_line.trim_start_matches("gitdir: ").trim();
    assert!(
        !stored_admin.contains(r"\\?\") && !stored_admin.contains("//?/"),
        "worktree .git must not store verbatim paths: {stored_admin}"
    );
    let admin_gitdir =
        std::fs::read_to_string(entry.admin_dir.join("gitdir")).expect("admin gitdir");
    let stored_wt_git = admin_gitdir.trim();
    assert!(
        !stored_wt_git.contains(r"\\?\") && !stored_wt_git.contains("//?/"),
        "admin gitdir must not store verbatim paths: {stored_wt_git}"
    );
    #[cfg(windows)]
    {
        assert!(
            stored_admin.contains('/'),
            "gitdir should use forward slashes: {stored_admin}"
        );
        assert!(
            stored_wt_git.contains('/'),
            "gitdir should use forward slashes: {stored_wt_git}"
        );
    }

    let list = list_worktrees(&repo).expect("list");
    assert_eq!(list.len(), 2);
    assert!(linked.join("tracked.txt").is_file());
    let status = Command::new("git")
        .args(["status"])
        .current_dir(&linked)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "git status in linked worktree: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let porcelain = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&linked)
        .output()
        .unwrap();
    assert!(porcelain.status.success());
    assert!(String::from_utf8_lossy(&porcelain.stdout).trim().is_empty());

    let git_list = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&main)
        .output()
        .unwrap();
    assert!(git_list.status.success());
    let text = String::from_utf8_lossy(&git_list.stdout);
    assert!(text.contains("linked"));

    remove_linked_worktree(&repo, &linked, false).expect("remove clean worktree");
    assert_eq!(list_worktrees(&repo).unwrap().len(), 1);
    assert!(
        !linked.exists(),
        "clean non-force removal must delete the worktree directory and tracked files"
    );
}

/// After a grit commit in a linked worktree, removal must succeed without a prior status run
/// (issue #1001: stale index stat must not make `git diff-files` or remove think the tree is dirty).
#[test]
fn linked_worktree_remove_after_grit_commit_without_prior_status() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let main = root.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "t@example.com"]);
    git(&main, &["config", "user.name", "Test"]);
    git(&main, &["config", "core.autocrlf", "true"]);
    std::fs::write(main.join("tracked.txt"), "1\n").unwrap();
    git(&main, &["add", "tracked.txt"]);
    git(&main, &["commit", "-qm", "seed"]);

    let main_repo =
        Repository::discover(Some(main.as_path()), &RepositoryOptions::empty()).expect("discover");
    let linked = root.join("linked");
    add_linked_worktree(
        &main_repo,
        &AddLinkedWorktreeOptions {
            path: linked.clone(),
            branch: Some("linked".to_owned()),
            new_branch: true,
            detach: false,
            start_point: Some("HEAD".to_owned()),
            force: false,
        },
    )
    .expect("add worktree");

    let linked_repo =
        Repository::discover(Some(linked.as_path()), &RepositoryOptions::empty()).expect("linked");
    std::fs::write(linked.join("new.txt"), "new\n").unwrap();
    stage(&linked_repo, &StageOptions::default(), &mut NullProgress).expect("stage");
    create_commit(
        &linked_repo,
        &CommitRequest {
            message: "commit in linked wt".into(),
            author: "Test <t@example.com> 1000000000 +0000".into(),
            committer: "Test <t@example.com> 1000000000 +0000".into(),
            allow_empty: false,
            sign_override: None,
            amend: false,
        },
        &mut NullProgress,
    )
    .expect("commit");

    let diff_out = Command::new("git")
        .args(["diff-files"])
        .current_dir(&linked)
        .output()
        .expect("git diff-files");
    assert!(
        diff_out.status.success(),
        "git diff-files failed: {}",
        String::from_utf8_lossy(&diff_out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&diff_out.stdout).trim().is_empty(),
        "git diff-files must be clean after grit commit"
    );

    remove_linked_worktree(&main_repo, &linked, false).expect("remove after commit");
    assert_eq!(list_worktrees(&main_repo).unwrap().len(), 1);
    assert!(!linked.exists());
}
