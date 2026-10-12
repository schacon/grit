//! Clone an empty bare remote and compare layout with system `git`.

use std::process::Command;

use grit_lib::clone::{clone, CloneOptions};
use grit_lib::fetch::NoProgress;

fn git(cwd: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
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
fn clone_empty_bare_matches_git_fsck_and_config() {
    let root = tempfile::tempdir().expect("tempdir");
    let bare = root.path().join("origin.git");
    git(
        root.path(),
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );

    let grit_dest = root.path().join("grit-clone");
    clone(
        CloneOptions::new(bare.to_string_lossy(), grit_dest.clone()),
        &mut NoProgress,
        None,
    )
    .expect("grit clone");

    let git_dest = root.path().join("git-clone");
    git(
        root.path(),
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            git_dest.to_str().unwrap(),
        ],
    );

    for dest in [&grit_dest, &git_dest] {
        git(dest, &["fsck", "--strict"]);
    }

    let grit_head = std::fs::read_to_string(grit_dest.join(".git/HEAD")).expect("grit HEAD");
    let git_head = std::fs::read_to_string(git_dest.join(".git/HEAD")).expect("git HEAD");
    assert_eq!(grit_head.trim(), git_head.trim());

    for key in [
        "branch.main.remote",
        "branch.main.merge",
        "remote.origin.url",
    ] {
        let grit_val = Command::new("git")
            .args(["config", "--get", key])
            .current_dir(&grit_dest)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git config grit");
        let git_val = Command::new("git")
            .args(["config", "--get", key])
            .current_dir(&git_dest)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git config git");
        assert_eq!(
            String::from_utf8_lossy(&grit_val.stdout).trim(),
            String::from_utf8_lossy(&git_val.stdout).trim(),
            "{key}"
        );
    }

    assert!(
        !grit_dest.join(".git/refs/heads/main").exists(),
        "unborn branch must not create refs/heads/main"
    );
}
