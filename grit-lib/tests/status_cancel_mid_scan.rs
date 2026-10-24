//! Status honours cancellation during untracked scanning (not only before start).

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::error::Error;
use grit_lib::porcelain::status::{status, StatusOptions, UntrackedMode};
use grit_lib::progress::{FlagCancel, NullProgress};
use grit_lib::repo::Repository;
use grit_test_support::git;
use std::thread;
use std::time::Duration;

#[test]
fn status_returns_cancelled_when_signalled_during_untracked_walk() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "seed"]);

    for i in 0..800 {
        let dir = root.join(format!("many/{i}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("file.txt"), format!("payload {i}\n")).expect("write");
    }

    let repo = Repository::discover(Some(root), &RepositoryOptions::empty()).expect("open");
    let flag = FlagCancel::new();
    let flag_for_thread = flag.clone();
    let sleeper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(5));
        flag_for_thread.cancel();
    });

    let mut opts = StatusOptions::default();
    opts.untracked = UntrackedMode::All;
    let err =
        status(&repo, &opts, &mut NullProgress, &flag).expect_err("cancel during untracked walk");
    sleeper.join().expect("join");
    assert!(matches!(err, Error::Cancelled));
}
