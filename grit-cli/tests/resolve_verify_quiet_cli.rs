//! `grit resolve --verify` and `--quiet` exit codes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn grit(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_grit");
    let out = Command::new(bin)
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn grit");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {} failed", args.join(" "));
}

#[test]
fn resolve_verify_quiet_cli() {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["commit", "--allow-empty", "-q", "-m", "seed"]);

    let (ok_code, ok_out, ok_err) = grit(dir.path(), &["resolve", "--verify", "HEAD", "--quiet"]);
    assert_eq!(ok_code, 0);
    assert!(ok_out.is_empty(), "stdout: {ok_out:?}");
    assert!(ok_err.is_empty(), "stderr: {ok_err:?}");

    let (bad_code, bad_out, bad_err) = grit(
        dir.path(),
        &["resolve", "--verify", "missing-ref", "--quiet"],
    );
    assert_eq!(bad_code, 1);
    assert!(bad_out.is_empty(), "stdout: {bad_out:?}");
    assert!(bad_err.is_empty(), "stderr: {bad_err:?}");

    let (json_code, json_out, json_err) = grit(
        dir.path(),
        &["resolve", "--verify", "HEAD", "--json", "--quiet"],
    );
    assert_eq!(json_code, 0);
    assert!(json_out.is_empty(), "stdout: {json_out:?}");
    assert!(json_err.is_empty(), "stderr: {json_err:?}");

    let (show_code, show_out, _show_err) = grit(dir.path(), &["resolve", "--verify", "HEAD"]);
    assert_eq!(show_code, 0);
    assert!(
        !show_out.trim().is_empty(),
        "non-quiet verify should print oid"
    );

    let missing = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let (miss_code, miss_out, miss_err) =
        grit(dir.path(), &["resolve", "--verify", missing, "--quiet"]);
    assert_eq!(miss_code, 1, "missing full hex must exit 1");
    assert!(miss_out.is_empty());
    assert!(miss_err.is_empty());

    let (usage_code, usage_out, usage_err) =
        grit(dir.path(), &["resolve", "--verify", "HEAD", "main"]);
    assert_eq!(usage_code, 2, "multiple specs with --verify must exit 2");
    assert!(usage_out.is_empty());
    assert!(
        usage_err.is_empty() || usage_err.contains("error"),
        "{usage_err}"
    );
}
