//! Ctrl+C during `grit clone` removes everything the clone created, so the
//! same command can simply be run again (issue #2162).

#![cfg(unix)]

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use grit_test_support::{configure_git_env, grit_cmd};

const GRIT: &str = env!("CARGO_BIN_EXE_grit");
const FILES: usize = 4000;

/// A source repository with enough files that copying its objects and
/// checking them out each take a while.
fn source_repo(base: &Path) -> std::path::PathBuf {
    let src = base.join("src");
    fs::create_dir_all(&src).expect("mkdir src");
    grit_cmd(&["init"]).in_dir(&src).suc();
    for i in 0..FILES {
        let dir = src.join(format!("d{:02}", i % 40));
        fs::create_dir_all(&dir).expect("mkdir");
        let body = format!("file {i}\n").repeat(200);
        fs::write(dir.join(format!("f{i:04}.txt")), body).expect("write");
    }
    grit_cmd(&["commit", "-m", "many files"]).in_dir(&src).suc();
    src
}

fn spawn_clone(base: &Path, src: &Path, dest: &Path) -> Child {
    let mut cmd = Command::new(GRIT);
    configure_git_env(&mut cmd);
    cmd.env("HOME", base)
        .env_remove("XDG_CACHE_HOME")
        .args([
            "clone",
            src.to_str().expect("utf-8"),
            dest.to_str().expect("utf-8"),
        ])
        .current_dir(base)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn().expect("spawn grit clone")
}

/// Whether the clone has started writing objects into `dest/.git/objects`.
fn copying_objects(dest: &Path) -> bool {
    let objects = dest.join(".git/objects");
    let loose = fs::read_dir(&objects).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| !matches!(e.file_name().to_str(), Some("info" | "pack")))
    });
    loose || fs::read_dir(objects.join("pack")).is_ok_and(|mut entries| entries.next().is_some())
}

/// Whether the clone has started writing work-tree files.
fn checking_out(dest: &Path) -> bool {
    fs::read_dir(dest).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_str() != Some(".git"))
    })
}

fn sigint(child: &Child) {
    let pid = libc::pid_t::try_from(child.id()).expect("pid");
    // SAFETY: `pid` is our own child, not yet reaped.
    unsafe {
        libc::kill(pid, libc::SIGINT);
    }
}

/// Start a clone, send `signals` back-to-back SIGINTs once `reached` holds,
/// and return its exit code.
fn interrupt_clone(
    base: &Path,
    src: &Path,
    dest: &Path,
    reached: fn(&Path) -> bool,
    signals: usize,
) -> i32 {
    let mut child = spawn_clone(base, src, dest);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            return status.code().unwrap_or(-1);
        }
        if reached(dest) {
            break;
        }
        assert!(Instant::now() < deadline, "clone never reached the phase");
        std::thread::sleep(Duration::from_micros(200));
    }
    for _ in 0..signals {
        sigint(&child);
    }
    child.wait().expect("wait").code().unwrap_or(-1)
}

/// After an interrupted clone the destination is either gone (or empty, when
/// it existed empty before) or a complete repository; and cloning again works.
fn assert_clean_and_retry(base: &Path, src: &Path, dest: &Path, code: i32, kept_dir: bool) {
    match code {
        130 => {
            if kept_dir {
                let left: Vec<_> = fs::read_dir(dest)
                    .expect("pre-existing destination is kept")
                    .flatten()
                    .map(|e| e.file_name())
                    .collect();
                assert!(left.is_empty(), "left behind: {left:?}");
            } else {
                assert!(!dest.exists(), "interrupted clone left {}", dest.display());
            }
        }
        0 => assert!(dest.join(".git/HEAD").is_file(), "finished clone has HEAD"),
        other => panic!("unexpected exit code {other}"),
    }
    if code == 0 {
        fs::remove_dir_all(dest).expect("remove finished clone");
    }

    let retry = spawn_clone(base, src, dest).wait().expect("wait");
    assert!(retry.success(), "retrying the same clone failed: {retry:?}");
    assert!(dest.join(".git/HEAD").is_file());
    let files = fs::read_dir(dest)
        .expect("read dest")
        .flatten()
        .filter(|e| e.file_name().to_str() != Some(".git"))
        .map(|e| fs::read_dir(e.path()).map_or(0, |d| d.count()))
        .sum::<usize>();
    assert_eq!(files, FILES, "retried clone checks out every file");
    grit_cmd(&["status"]).in_dir(dest).suc();
    fs::remove_dir_all(dest).expect("cleanup");
}

#[test]
fn ctrl_c_during_clone_removes_the_destination_and_retry_succeeds() {
    let base = tempfile::tempdir().expect("tempdir");
    let src = source_repo(base.path());
    let dest = base.path().join("dest");

    let phases: [(&str, fn(&Path) -> bool); 2] =
        [("object copy", copying_objects), ("checkout", checking_out)];
    for (phase, reached) in phases {
        let code = interrupt_clone(base.path(), &src, &dest, reached, 1);
        assert!(
            code == 130 || code == 0,
            "{phase}: exit code {code}, expected 130"
        );
        assert_clean_and_retry(base.path(), &src, &dest, code, false);

        fs::create_dir(&dest).expect("mkdir empty dest");
        let code = interrupt_clone(base.path(), &src, &dest, reached, 1);
        assert_clean_and_retry(base.path(), &src, &dest, code, true);

        // `timeout -s INT` signals both the child and its process group.
        let code = interrupt_clone(base.path(), &src, &dest, reached, 2);
        assert_clean_and_retry(base.path(), &src, &dest, code, false);
    }
}

#[test]
fn a_later_second_ctrl_c_terminates_a_clone_stuck_on_the_network() {
    let base = tempfile::tempdir().expect("tempdir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}/repo.git", listener.local_addr().expect("addr"));
    let (accepted, connected) = mpsc::channel();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            held.push(stream);
            let _ = accepted.send(());
        }
    });

    let mut cmd = Command::new(GRIT);
    configure_git_env(&mut cmd);
    let mut child = cmd
        .env("HOME", base.path())
        .args(["clone", &url, "dest"])
        .current_dir(base.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn grit clone");
    connected
        .recv_timeout(Duration::from_secs(30))
        .expect("clone connects to the server");

    sigint(&child);
    std::thread::sleep(Duration::from_millis(300));
    if child.try_wait().expect("wait").is_none() {
        sigint(&child);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().expect("wait").is_none() {
        assert!(
            Instant::now() < deadline,
            "a second Ctrl+C must end the clone"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
