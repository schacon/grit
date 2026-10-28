//! Ctrl+C during `grit switch` rolls the checkout back: `HEAD`, the index and every
//! work-tree file stay as they were, no `index.lock` is left behind, and the same switch
//! can simply be run again (issue #2104). On Windows the interrupt is a console
//! Ctrl+Break sent to the child's process group.

#![cfg(any(unix, windows))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use grit_test_support::{configure_git_env, grit_cmd};

const GRIT: &str = env!("CARGO_BIN_EXE_grit");
const FILES: usize = 4000;
const DIRS: usize = 40;
const GONE: usize = 400;
const ATTEMPTS: usize = 5;

fn old_body(i: usize) -> String {
    format!("file {i}\n").repeat(200)
}

/// Commit `old` has `FILES` files in `d00..d39` plus `gone/`; commit `new` rewrites every
/// file, deletes `gone/` and adds `n00..n39`. Leaves `HEAD` on `old`.
fn fixture(base: &Path) -> (PathBuf, String) {
    let repo = base.join("repo");
    fs::create_dir_all(&repo).expect("mkdir repo");
    grit_cmd(&["init"]).in_dir(&repo).suc();
    for i in 0..FILES {
        let dir = repo.join(format!("d{:02}", i % DIRS));
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(format!("f{i:04}.txt")), old_body(i)).expect("write");
    }
    fs::create_dir_all(repo.join("gone")).expect("mkdir gone");
    for i in 0..GONE {
        fs::write(repo.join(format!("gone/g{i:03}.txt")), old_body(i)).expect("write");
    }
    grit_cmd(&["commit", "-m", "old"]).in_dir(&repo).suc();
    grit_cmd(&["branch", "old"]).in_dir(&repo).suc();

    for i in 0..FILES {
        let path = repo.join(format!("d{:02}/f{i:04}.txt", i % DIRS));
        fs::write(path, format!("changed {i}\n").repeat(200)).expect("rewrite");
        let added = repo.join(format!("n{:02}", i % DIRS));
        fs::create_dir_all(&added).expect("mkdir");
        fs::write(added.join(format!("a{i:04}.txt")), format!("added {i}\n")).expect("add");
    }
    fs::remove_dir_all(repo.join("gone")).expect("rm gone");
    grit_cmd(&["commit", "-m", "new"]).in_dir(&repo).suc();
    let new = head_oid(&repo);
    grit_cmd(&["switch", "old"]).in_dir(&repo).suc();
    (repo, new)
}

fn head_oid(repo: &Path) -> String {
    let out = grit_cmd(&["resolve", "HEAD", "--json"]).in_dir(repo).suc();
    let resolved: serde_json::Value = serde_json::from_str(&out.stdout).expect("resolve json");
    resolved["results"][0]["oid"]
        .as_str()
        .expect("HEAD oid")
        .to_owned()
}

fn spawn_switch(repo: &Path, new: &str) -> Child {
    let mut cmd = Command::new(GRIT);
    configure_git_env(&mut cmd);
    cmd.args(["switch", "--detach", new])
        .current_dir(repo)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Its own process group, so Ctrl+Break reaches only grit and not this test.
        cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().expect("spawn grit switch")
}

/// Whether the checkout has started changing the work tree.
fn checking_out(repo: &Path) -> bool {
    !repo.join("gone").exists() || (0..DIRS).any(|d| repo.join(format!("n{d:02}")).exists())
}

#[cfg(unix)]
fn interrupt(child: &Child) {
    let pid = libc::pid_t::try_from(child.id()).expect("pid");
    // SAFETY: `pid` is our own child, not yet reaped.
    unsafe {
        libc::kill(pid, libc::SIGINT);
    }
}

#[cfg(windows)]
fn interrupt(child: &Child) {
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    // SAFETY: the child leads its own process group, so only it receives the event.
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    assert_ne!(sent, 0, "GenerateConsoleCtrlEvent failed");
}

/// Every path in the work tree outside `.git`, relative to `repo`.
fn worktree_files(repo: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![repo.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path == repo.join(".git") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(repo).expect("under repo");
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

fn assert_worktree_is_old(repo: &Path) {
    let mut expected: Vec<String> = (0..FILES)
        .map(|i| format!("d{:02}/f{i:04}.txt", i % DIRS))
        .chain((0..GONE).map(|i| format!("gone/g{i:03}.txt")))
        .collect();
    expected.sort();
    assert_eq!(
        worktree_files(repo),
        expected,
        "cancelled switch left the wrong set of files"
    );
    for i in 0..FILES {
        let path = repo.join(format!("d{:02}/f{i:04}.txt", i % DIRS));
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            old_body(i),
            "{} was not restored",
            path.display()
        );
    }
}

fn assert_status_clean(repo: &Path) {
    let out = grit_cmd(&["status", "--json"]).in_dir(repo).suc();
    let status: serde_json::Value = serde_json::from_str(&out.stdout).expect("status json");
    assert_eq!(status["clean"], true, "status after the switch: {status}");
}

#[test]
fn ctrl_c_during_switch_leaves_head_index_and_worktree_unchanged() {
    let base = tempfile::tempdir().expect("tempdir");
    let (repo, new) = fixture(base.path());
    let old = head_oid(&repo);

    let mut cancelled = 0;
    for _ in 0..ATTEMPTS {
        let index_before = fs::read(repo.join(".git/index")).expect("read index");
        let mut child = spawn_switch(&repo, &new);
        let deadline = Instant::now() + Duration::from_secs(60);
        let finished_early = loop {
            if child.try_wait().expect("wait").is_some() {
                break true;
            }
            if checking_out(&repo) {
                break false;
            }
            assert!(
                Instant::now() < deadline,
                "switch never started checking out"
            );
            std::thread::sleep(Duration::from_micros(200));
        };
        if !finished_early {
            interrupt(&child);
        }
        let out = child.wait_with_output().expect("wait");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !repo.join(".git/index.lock").exists(),
            "interrupted switch left .git/index.lock ({:?}): {stderr}",
            out.status
        );

        if out.status.success() {
            assert_eq!(head_oid(&repo), new);
            assert_status_clean(&repo);
            grit_cmd(&["switch", "old"]).in_dir(&repo).suc();
            continue;
        }
        assert!(
            stderr.contains("operation cancelled"),
            "interrupted switch exited {:?}: {stderr}",
            out.status
        );
        cancelled += 1;
        assert_eq!(head_oid(&repo), old, "HEAD moved");
        assert_eq!(
            fs::read(repo.join(".git/index")).expect("read index"),
            index_before,
            "index changed"
        );
        assert_worktree_is_old(&repo);
        assert_status_clean(&repo);
        break;
    }
    assert!(cancelled > 0, "no switch was interrupted mid-checkout");

    grit_cmd(&["switch", "--detach", &new]).in_dir(&repo).suc();
    assert_eq!(head_oid(&repo), new);
    assert_status_clean(&repo);
}
