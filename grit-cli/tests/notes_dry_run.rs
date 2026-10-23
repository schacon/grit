//! `grit notes add/remove --dry-run` leaves `.git` unchanged.

use grit_test_support::{git, git_dir_state_digest, git_fsck, CliFixture};
use serde_json::Value;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

fn parse_json(stdout: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(stdout.trim())?)
}

#[test]
fn notes_add_dry_run_then_real() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("f.txt"), "x\n")?;
    fixture.run_ok(&["commit", "-m", "init"]);
    let head = git(fixture.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();

    let git_dir = fixture.path().join(".git");
    let before = git_dir_state_digest(&git_dir);
    let dry = fixture.run(&[
        "--json",
        "notes",
        "add",
        "HEAD",
        "-m",
        "note body",
        "--dry-run",
    ]);
    assert_eq!(dry.status, Some(0), "{}", dry.dump("dry-run"));
    assert_eq!(before, git_dir_state_digest(&git_dir));
    let fsck = git_fsck(fixture.path(), true);
    assert!(fsck.ok, "fsck: {:?}", fsck.msg_ids);

    let v = parse_json(&dry.stdout)?;
    assert_eq!(v["action"], "add");
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["object"].as_str(), Some(head.as_str()));
    assert!(v["notes_ref_new"].as_str().is_some());

    let real = fixture.run(&["--json", "notes", "add", "HEAD", "-m", "note body"]);
    assert_eq!(real.status, Some(0), "{}", real.dump("add"));
    let r = parse_json(&real.stdout)?;
    assert_eq!(r["replaced"], false);
    Ok(())
}

#[test]
fn notes_remove_dry_run_missing_note() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("f.txt"), "x\n")?;
    fixture.run_ok(&["commit", "-m", "init"]);

    let git_dir = fixture.path().join(".git");
    let before = git_dir_state_digest(&git_dir);
    let dry = fixture.run(&["--json", "notes", "remove", "HEAD", "--dry-run"]);
    assert_eq!(dry.status, Some(0), "{}", dry.dump("dry-run"));
    assert_eq!(before, git_dir_state_digest(&git_dir));

    let v = parse_json(&dry.stdout)?;
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["objects"].as_array().map(Vec::len), Some(0));
    Ok(())
}
