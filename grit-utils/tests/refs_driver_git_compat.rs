//! Refs driver `for-each-ref` output must match system `git` on a 1k-ref fixture.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const REF_COUNT: usize = 1000;

fn bench_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_grit-bench"))
}

fn system_git() -> PathBuf {
    PathBuf::from("git")
}

fn git_env(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
}

fn init_refs_fixture(git: &Path, dir: &Path) {
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    assert!(Command::new(git)
        .args([
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            dir.file_name().unwrap().to_str().unwrap(),
        ])
        .current_dir(dir.parent().unwrap())
        .status()
        .unwrap()
        .success());
    let commit = Command::new(git)
        .args(["commit-tree", EMPTY_TREE, "-m", "base"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(commit.status.success());
    let commit_oid = String::from_utf8_lossy(&commit.stdout).trim().to_owned();
    let mut child = Command::new(git)
        .args(["update-ref", "--stdin"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        for i in 0..REF_COUNT {
            writeln!(stdin, "create refs/heads/b{i:05} {commit_oid}").unwrap();
        }
    }
    assert!(child.wait().unwrap().success());
}

fn git_for_each_ref(git: &Path, dir: &Path) -> Vec<u8> {
    let mut cmd = Command::new(git);
    cmd.args(["for-each-ref"]).current_dir(dir);
    git_env(&mut cmd);
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn grit_for_each_ref(dir: &Path) -> Vec<u8> {
    let out = Command::new(bench_exe())
        .args(["refs-driver", "for-each-ref", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

#[test]
fn refs_driver_for_each_ref_matches_git_on_1k_fixture() {
    let git = system_git();
    let root = TempDir::new().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_refs_fixture(&git, &repo);
    let git_out = git_for_each_ref(&git, &repo);
    let grit_out = grit_for_each_ref(&repo);
    assert_eq!(grit_out, git_out);
}
