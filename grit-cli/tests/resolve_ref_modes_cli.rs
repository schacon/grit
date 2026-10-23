//! `grit resolve --abbrev-ref` and `--symbolic-full-name`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use serde_json::Value;

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

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn parse_json_stdout(stdout: &str) -> Value {
    serde_json::from_str(stdout.trim()).expect("parse grit --json stdout")
}

#[test]
fn resolve_abbrev_and_symbolic_full_name_match_git() {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["commit", "--allow-empty", "-q", "-m", "seed"]);

    let (code, stdout, _stderr) = grit(dir.path(), &["resolve", "--abbrev-ref", "HEAD"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]).trim()
    );

    let (code, stdout, _stderr) = grit(dir.path(), &["resolve", "--symbolic-full-name", "HEAD"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        git(dir.path(), &["rev-parse", "--symbolic-full-name", "HEAD"]).trim()
    );

    let (code, stdout, _stderr) = grit(dir.path(), &["resolve", "--json", "--abbrev-ref", "main"]);
    assert_eq!(code, 0);
    let json = parse_json_stdout(&stdout);
    assert_eq!(
        json,
        serde_json::json!({
            "results": [{
                "spec": "main",
                "ref_name": "main",
                "full_ref": "refs/heads/main",
                "ref_display": "main",
            }]
        })
    );

    let (code, stdout, _stderr) = grit(
        dir.path(),
        &["resolve", "--json", "--symbolic-full-name", "main"],
    );
    assert_eq!(code, 0);
    let json = parse_json_stdout(&stdout);
    assert_eq!(
        json,
        serde_json::json!({
            "results": [{
                "spec": "main",
                "ref_name": "main",
                "full_ref": "refs/heads/main",
                "ref_display": "refs/heads/main",
            }]
        })
    );

    git(dir.path(), &["branch", "dup"]);
    git(dir.path(), &["tag", "dup"]);
    let (_, grit_out, grit_err) = grit(dir.path(), &["resolve", "--abbrev-ref", "dup"]);
    assert_eq!(grit_out.trim(), "");
    assert!(
        grit_err.contains("ambiguous"),
        "stderr should carry ambiguous ref warning: {grit_err}"
    );
}
