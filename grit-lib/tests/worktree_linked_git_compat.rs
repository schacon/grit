//! Linked worktree add/remove matches Git layout and interoperates with `git worktree list`.

use grit_lib::repo::Repository;
use grit_lib::worktree::{add_linked_worktree, list_worktrees, remove_linked_worktree, AddLinkedWorktreeOptions};
use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
}

#[test]
fn grit_add_and_remove_linked_worktree_git_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let main = root.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("f"), "1\n").unwrap();
    git(&main, &["add", "f"]);
    git(&main, &["commit", "-qm", "seed"]);

    let repo = Repository::discover(Some(&main)).expect("discover");
    let linked = root.join("linked");
    add_linked_worktree(
        &repo,
        &AddLinkedWorktreeOptions {
            path: linked.clone(),
            branch: Some("linked".to_owned()),
            new_branch: true,
            detach: false,
            start_point: Some("HEAD".to_owned()),
        },
    )
    .expect("add worktree");

    let list = list_worktrees(&repo).expect("list");
    assert_eq!(list.len(), 2);
    assert!(linked.join("f").is_file());

    let git_list = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&main)
        .output()
        .unwrap();
    assert!(git_list.status.success());
    let text = String::from_utf8_lossy(&git_list.stdout);
    assert!(text.contains("linked"));

    remove_linked_worktree(&repo, &linked, true).expect("remove");
    assert_eq!(list_worktrees(&repo).unwrap().len(), 1);
}
