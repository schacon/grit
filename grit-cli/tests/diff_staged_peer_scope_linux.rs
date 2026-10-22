//! Linux regression: `grit diff --staged` hunk-id peer load must not read unrelated paths.

#![cfg(target_os = "linux")]

use std::process::Command;

fn count_path_opens(strace_log: &str, needle: &str) -> usize {
    strace_log
        .lines()
        .filter(|line| {
            (line.contains("openat(") || line.contains("open(")) && line.contains(needle)
        })
        .count()
}

#[test]
fn staged_diff_does_not_open_unrelated_unstaged_files_for_hunk_peers() {
    let grit = env!("CARGO_BIN_EXE_grit");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    assert!(Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init")
        .success());
    for (key, val) in [("user.email", "t@example.com"), ("user.name", "t")] {
        Command::new("git")
            .args(["config", key, val])
            .current_dir(root)
            .status()
            .expect("git config");
    }

    std::fs::write(root.join("primary.txt"), b"a\n").expect("primary");
    std::fs::write(root.join("unrelated.txt"), b"b\n").expect("unrelated");
    Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()
        .expect("add");
    Command::new("git")
        .args(["commit", "-qm", "init"])
        .current_dir(root)
        .status()
        .expect("commit");

    std::fs::write(root.join("primary.txt"), b"A\n").expect("stage primary");
    std::fs::write(root.join("unrelated.txt"), b"B\n").expect("modify unrelated");
    assert!(Command::new("git")
        .args(["add", "primary.txt"])
        .current_dir(root)
        .status()
        .expect("add primary")
        .success());

    let strace_out = tmp.path().join("strace.txt");
    let status = Command::new("strace")
        .args([
            "-f",
            "-e",
            "openat,open",
            "-o",
            strace_out.to_str().expect("utf8 path"),
            grit,
            "--json",
            "diff",
            "--staged",
        ])
        .current_dir(root)
        .status()
        .expect("strace grit diff --staged");
    assert!(status.success(), "grit diff --staged failed under strace");

    let log = std::fs::read_to_string(&strace_out).expect("read strace log");
    let unrelated_needle = format!("{}/unrelated.txt", root.display());
    let opens = count_path_opens(&log, &unrelated_needle);
    assert_eq!(
        opens, 0,
        "expected no opens of unrelated unstaged file, got {opens}\n{log}"
    );

    let stdout = Command::new(grit)
        .args(["--json", "diff", "--staged"])
        .current_dir(root)
        .output()
        .expect("grit diff");
    assert!(stdout.status.success());
    let v: serde_json::Value = serde_json::from_slice(&stdout.stdout).expect("json diff --staged");
    let files = v["files"].as_array().expect("files");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "primary.txt");
}
