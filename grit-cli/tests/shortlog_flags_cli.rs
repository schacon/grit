//! `grit shortlog` flags: `-n`, `--behind`, `--stat`, `--name-only`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn grit_bin() -> std::path::PathBuf {
    std::env::var_os("CARGO_BIN_EXE_grit")
        .map(std::path::PathBuf::from)
        .expect("CARGO_BIN_EXE_grit")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_grit(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(grit_bin())
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("spawn grit");
    assert!(
        out.status.success(),
        "grit {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(dir: &Path, file: &str, body: &str, message: &str) {
    fs::write(dir.join(file), body).expect("write");
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn diverged_worktree() -> tempfile::TempDir {
    diverged_worktree_with_behind_commits(1)
}

fn diverged_worktree_with_behind_commits(extra_on_origin: usize) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    let origin = root.path().join("origin");
    fs::create_dir_all(&origin).expect("mkdir");
    git(&origin, &["init", "-q", "-b", "main"]);
    commit(&origin, "a.txt", "1\n", "a1");
    commit(&origin, "a.txt", "2\n", "a2");
    git(root.path(), &["clone", "-q", "origin", "clone"]);
    let clone = root.path().join("clone");
    commit(&clone, "b.txt", "b1\n", "b1");
    commit(&clone, "b.txt", "b2\n", "b2");
    for i in 0..extra_on_origin {
        commit(
            &origin,
            "a.txt",
            &format!("{}\n", i + 3),
            &format!("a{}", i + 3),
        );
    }
    git(&clone, &["fetch", "-q"]);
    root
}

#[test]
fn shortlog_flags_cli() {
    let root = diverged_worktree();
    let repo = root.path().join("clone");

    let limited: Value =
        serde_json::from_str(run_grit(&repo, &["shortlog", "-n", "1", "--json"]).trim())
            .expect("json");
    assert_eq!(limited["ahead"].as_u64(), Some(2));
    assert_eq!(limited["commits"].as_array().map(Vec::len), Some(1));
    assert_eq!(limited["truncated"].as_bool(), Some(true));
    assert_eq!(limited["behind"].as_u64(), Some(1));

    let behind: Value = serde_json::from_str(
        run_grit(&repo, &["shortlog", "--behind", "-n", "5", "--json"]).trim(),
    )
    .expect("json");
    assert_eq!(behind["behind"].as_u64(), Some(1));
    let behind_commits = behind["behind_commits"].as_array().expect("behind_commits");
    assert_eq!(behind_commits.len(), 1);
    assert_eq!(behind["behind_truncated"].as_bool(), Some(false));

    let stat: Value =
        serde_json::from_str(run_grit(&repo, &["shortlog", "--stat", "--json"]).trim())
            .expect("json");
    let commits = stat["commits"].as_array().expect("commits");
    assert!(!commits.is_empty());
    let grit_stat = commits[0]["stat"].as_object().expect("stat object");
    let git_numstat = git(
        &repo,
        &["log", "-1", "--numstat", "origin/main..HEAD", "--format="],
    );
    let mut git_lines: Vec<&str> = git_numstat.lines().filter(|l| !l.is_empty()).collect();
    git_lines.sort_unstable();
    let grit_files = grit_stat["files"].as_array().expect("stat.files");
    let mut grit_lines: Vec<String> = grit_files
        .iter()
        .map(|f| {
            format!(
                "{}\t{}\t{}",
                f["insertions"].as_u64().unwrap_or(0),
                f["deletions"].as_u64().unwrap_or(0),
                f["path"].as_str().unwrap_or("")
            )
        })
        .collect();
    grit_lines.sort_unstable();
    assert_eq!(
        git_lines,
        grit_lines.iter().map(String::as_str).collect::<Vec<_>>()
    );

    let names: Value =
        serde_json::from_str(run_grit(&repo, &["shortlog", "--name-only", "--json"]).trim())
            .expect("json");
    let files = names["commits"][0]["files"]
        .as_array()
        .expect("files on first commit");
    assert!(
        files.iter().any(|f| f["path"].as_str() == Some("b.txt")),
        "expected b.txt in files: {files:?}"
    );

    let plain_md = run_grit(&repo, &["shortlog", "--markdown"]);
    let stat_md = run_grit(&repo, &["shortlog", "--stat", "--markdown"]);
    assert!(
        stat_md.contains("b.txt"),
        "stat markdown should list changed paths: {stat_md}"
    );
    assert!(
        stat_md.contains("### `") || stat_md.contains('|'),
        "stat markdown should include per-commit diff section: {stat_md}"
    );
    assert_ne!(
        stat_md, plain_md,
        "stat markdown should differ from plain shortlog markdown"
    );

    let name_md = run_grit(&repo, &["shortlog", "--name-only", "--markdown"]);
    assert!(
        name_md.contains("b.txt"),
        "name-only markdown should list b.txt: {name_md}"
    );
    assert_ne!(name_md, plain_md);

    let many = diverged_worktree_with_behind_commits(3);
    let many_repo = many.path().join("clone");
    let truncated: Value = serde_json::from_str(
        run_grit(&many_repo, &["shortlog", "--behind", "-n", "1", "--json"]).trim(),
    )
    .expect("json");
    assert_eq!(truncated["behind"].as_u64(), Some(3));
    assert_eq!(
        truncated["behind_commits"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(truncated["behind_truncated"].as_bool(), Some(true));
}
