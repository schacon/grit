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
fn notes_remove_dry_run_predicts_batch_tip() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("f.txt"), "x\n")?;
    fixture.run_ok(&["commit", "-m", "one"]);
    std::fs::write(fixture.path().join("g.txt"), "y\n")?;
    fixture.run_ok(&["commit", "-m", "two"]);
    fixture.run_ok(&["notes", "add", "HEAD~1", "-m", "first"]);
    fixture.run_ok(&["notes", "add", "HEAD", "-m", "second"]);
    let parent = git(fixture.path(), &["rev-parse", "HEAD~1"])
        .trim()
        .to_string();
    let head = git(fixture.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    let tip_before = git(fixture.path(), &["rev-parse", "refs/notes/commits"])
        .trim()
        .to_string();

    let git_dir = fixture.path().join(".git");
    let before = git_dir_state_digest(&git_dir);
    let dry = fixture.run(&["--json", "notes", "remove", "HEAD~1", "HEAD", "--dry-run"]);
    assert_eq!(dry.status, Some(0), "{}", dry.dump("dry-run"));
    assert_eq!(before, git_dir_state_digest(&git_dir));
    let v = parse_json(&dry.stdout)?;
    assert_eq!(v["objects"], serde_json::json!([parent, head]));
    assert_eq!(v["notes_ref_old"].as_str(), Some(tip_before.as_str()));

    fixture.run_ok(&["notes", "remove", "HEAD~1", "HEAD"]);
    let tip_after = git(fixture.path(), &["rev-parse", "refs/notes/commits"])
        .trim()
        .to_string();
    assert_eq!(v["notes_ref_new"].as_str(), Some(tip_after.as_str()));
    assert_eq!(git(fixture.path(), &["notes", "list"]).trim(), "");
    let fsck = git_fsck(fixture.path(), true);
    assert!(fsck.ok, "fsck: {:?}", fsck.msg_ids);
    Ok(())
}

#[test]
fn notes_dry_run_markdown_says_would() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("f.txt"), "x\n")?;
    fixture.run_ok(&["commit", "-m", "init"]);

    let add = fixture.run_ok(&["--markdown", "notes", "add", "HEAD", "-m", "n", "--dry-run"]);
    assert!(
        add.stdout.starts_with("Would add note for `"),
        "{}",
        add.stdout
    );
    fixture.run_ok(&["notes", "add", "HEAD", "-m", "n"]);
    let replace = fixture.run_ok(&[
        "--markdown",
        "notes",
        "add",
        "HEAD",
        "-m",
        "m",
        "--force",
        "--dry-run",
    ]);
    assert!(
        replace.stdout.starts_with("Would replace note for `"),
        "{}",
        replace.stdout
    );
    let remove = fixture.run_ok(&["--markdown", "notes", "remove", "HEAD", "--dry-run"]);
    assert!(
        remove
            .stdout
            .starts_with("Would remove notes on `refs/notes/commits`:"),
        "{}",
        remove.stdout
    );
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
