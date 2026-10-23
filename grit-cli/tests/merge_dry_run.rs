//! `grit merge --dry-run` leaves `.git` unchanged and matches a real merge.

use grit_test_support::{git_dir_state_digest, git_fsck, CliFixture};
use serde_json::Value;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

fn parse_json(stdout: &str) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(stdout.trim())?)
}

#[test]
fn merge_dry_run_ff_then_real() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("a.txt"), "a\n")?;
    fixture.run_ok(&["add", "a.txt"]);
    fixture.run_ok(&["commit", "-m", "base"]);
    fixture.run_ok(&["switch", "--create", "topic"]);
    std::fs::write(fixture.path().join("b.txt"), "b\n")?;
    fixture.run_ok(&["add", "b.txt"]);
    fixture.run_ok(&["commit", "-m", "topic"]);
    fixture.run_ok(&["switch", "main"]);

    let git_dir = fixture.path().join(".git");
    let before = git_dir_state_digest(&git_dir);
    let dry = fixture.run(&["--json", "merge", "--dry-run", "topic"]);
    assert_eq!(dry.status, Some(0), "{}", dry.dump("dry-run"));
    assert_eq!(before, git_dir_state_digest(&git_dir));
    let fsck = git_fsck(fixture.path(), true);
    assert!(fsck.ok, "fsck: {:?}", fsck.msg_ids);

    let v = parse_json(&dry.stdout)?;
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["result"], "fast_forward");
    assert!(v["oid"].as_str().is_some());

    let real = fixture.run(&["--json", "merge", "topic"]);
    assert_eq!(real.status, Some(0), "{}", real.dump("merge"));
    let r = parse_json(&real.stdout)?;
    assert_eq!(r["result"], "fast_forward");
    assert_eq!(r["oid"], v["oid"]);
    Ok(())
}

#[test]
fn merge_dry_run_merge_commit_then_real() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("a.txt"), "base\n")?;
    fixture.run_ok(&["add", "a.txt"]);
    fixture.run_ok(&["commit", "-m", "base"]);
    fixture.run_ok(&["switch", "--create", "topic"]);
    std::fs::write(fixture.path().join("a.txt"), "side\n")?;
    fixture.run_ok(&["commit", "-am", "side"]);
    fixture.run_ok(&["switch", "main"]);
    std::fs::write(fixture.path().join("b.txt"), "main\n")?;
    fixture.run_ok(&["add", "b.txt"]);
    fixture.run_ok(&["commit", "-m", "main"]);

    let git_dir = fixture.path().join(".git");
    let before = git_dir_state_digest(&git_dir);
    let dry = fixture.run(&["--json", "merge", "--dry-run", "topic"]);
    assert_eq!(dry.status, Some(0), "{}", dry.dump("dry-run"));
    assert_eq!(before, git_dir_state_digest(&git_dir));

    let v = parse_json(&dry.stdout)?;
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["result"], "merged");
    assert!(v.get("oid").is_none());
    let parents = v["parents"].as_array().expect("parents");
    assert_eq!(parents.len(), 2);

    let real = fixture.run(&["--json", "merge", "topic"]);
    assert_eq!(real.status, Some(0), "{}", real.dump("merge"));
    let r = parse_json(&real.stdout)?;
    assert_eq!(r["result"], "merged");
    assert!(r["oid"].as_str().is_some());
    Ok(())
}

#[test]
fn merge_dry_run_conflict_exits_zero() -> TestResult {
    let fixture = CliFixture::empty();
    std::fs::write(fixture.path().join("a.txt"), "base\n")?;
    fixture.run_ok(&["add", "a.txt"]);
    fixture.run_ok(&["commit", "-m", "base"]);
    fixture.run_ok(&["switch", "--create", "topic"]);
    std::fs::write(fixture.path().join("a.txt"), "side\n")?;
    fixture.run_ok(&["commit", "-am", "side"]);
    fixture.run_ok(&["switch", "main"]);
    std::fs::write(fixture.path().join("a.txt"), "main\n")?;
    fixture.run_ok(&["commit", "-am", "main"]);

    let dry = fixture.run(&["--json", "merge", "--dry-run", "topic"]);
    assert_eq!(dry.status, Some(0), "{}", dry.dump("dry-run"));
    let v = parse_json(&dry.stdout)?;
    assert_eq!(v["result"], "conflicts");
    assert_eq!(v["dry_run"], true);

    let check = fixture.run(&["merge", "--check", "topic"]);
    assert_eq!(check.status, Some(1), "{}", check.dump("check"));
    Ok(())
}
