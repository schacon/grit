//! Raw Unix argv must reach clap without a UTF-8 conversion panic.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::{Command, Output};

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

fn command(program: &str, dir: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    cmd
}

fn git(dir: &Path, args: &[&str]) -> Output {
    let out = command("git", dir).args(args).output().unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    out
}

fn raw_names() -> [OsString; 3] {
    [
        OsString::from_vec(b"raw\xff.txt".to_vec()),
        OsString::from_vec(b"raw\xfe.txt".to_vec()),
        "raw\u{fffd}.txt".into(),
    ]
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Tool"]);
    git(dir.path(), &["config", "user.email", "tool@example.com"]);
    for (i, name) in raw_names().iter().enumerate() {
        fs::write(dir.path().join(name), format!("hello {i}\n")).unwrap();
    }
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "initial"]);
    assert!(git(dir.path(), &["status", "--porcelain=v1"])
        .stdout
        .is_empty());
    for name in raw_names() {
        let out = command("git", dir.path())
            .args(["blame", "--line-porcelain", "HEAD", "--"])
            .arg(&name)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert!(out.stdout.windows(6).any(|w| w == b"\thello"));
        let out = command("git", dir.path())
            .args(["status", "--porcelain=v1", "--"])
            .arg(&name)
            .output()
            .unwrap();
        assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
    }
    git(dir.path(), &["fsck", "--strict"]);
    dir
}

fn snapshot(dir: &Path) -> Vec<Vec<u8>> {
    let mut bytes = vec![
        fs::read(dir.join(".git/index")).unwrap(),
        git(dir, &["rev-parse", "HEAD"]).stdout,
        git(dir, &["ls-files", "--stage", "-z"]).stdout,
    ];
    bytes.extend(raw_names().map(|name| fs::read(dir.join(name)).unwrap()));
    bytes
}

fn assert_usage(out: &Output, mode: &str, expected: &str) {
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stdout.contains("panicked") && !stderr.contains("panicked"),
        "{out:?}"
    );
    let message = match mode {
        "--json" => {
            assert!(stderr.is_empty(), "{out:?}");
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["kind"], "usage", "{value}");
            value["error"].as_str().unwrap().to_owned()
        }
        "--markdown" => {
            assert!(stderr.is_empty(), "{out:?}");
            assert!(stdout.starts_with("**Error:**"), "{out:?}");
            stdout.into_owned()
        }
        _ => {
            assert!(stdout.is_empty(), "{out:?}");
            assert!(stderr.starts_with("error:"), "{out:?}");
            stderr.into_owned()
        }
    };
    assert!(message.contains(expected), "{message}");
}

#[test]
fn raw_path_operands_are_usage_errors_without_mutation() {
    let dir = fixture();
    // Each invalid name has a different byte spelling; lossy conversion would
    // select the third, valid Unicode name and could accidentally stage it.
    for name in raw_names() {
        fs::write(dir.path().join(name), b"changed\n").unwrap();
    }
    let before = snapshot(dir.path());
    for name in &raw_names()[..2] {
        for mode in ["", "--json", "--markdown"] {
            for global_first in [false, true] {
                for args in [
                    vec!["blame"],
                    vec!["blame", "HEAD", "--"],
                    vec!["status", "--"],
                    vec!["add", "--"],
                ] {
                    let mut cmd = command(GRIT, dir.path());
                    if global_first && !mode.is_empty() {
                        cmd.arg(mode);
                    }
                    cmd.arg(args[0]);
                    if !global_first && !mode.is_empty() {
                        cmd.arg(mode);
                    }
                    let out = cmd.args(&args[1..]).arg(name).output().unwrap();
                    assert_usage(&out, mode, "invalid UTF-8");
                    assert_eq!(snapshot(dir.path()), before, "{out:?}");
                }
            }
        }
    }
    git(dir.path(), &["fsck", "--strict"]);
}

#[test]
fn non_utf8_text_options_use_the_selected_error_renderer() {
    let dir = fixture();
    let before = snapshot(dir.path());
    for mode in ["", "--json", "--markdown"] {
        for global_first in [false, true] {
            for (subcommand, flag) in [("status", "--filter"), ("blame", "--rev")] {
                let mut cmd = command(GRIT, dir.path());
                if global_first && !mode.is_empty() {
                    cmd.arg(mode);
                }
                cmd.arg(subcommand);
                if !global_first && !mode.is_empty() {
                    cmd.arg(mode);
                }
                for attached in [false, true] {
                    let mut attempt = command(GRIT, dir.path());
                    attempt.args(cmd.get_args());
                    if attached {
                        let mut value = OsString::from(format!("{flag}="));
                        value.push(&raw_names()[0]);
                        attempt.arg(value);
                    } else {
                        attempt.arg(flag).arg(&raw_names()[0]);
                    }
                    let out = attempt.output().unwrap();
                    assert_usage(&out, mode, "invalid UTF-8");
                    assert_eq!(snapshot(dir.path()), before);
                }
            }
        }
    }
}

#[test]
fn raw_directory_global_preserves_bytes_and_validation_does_not_reread_utf8_argv() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q", "-b", "main"]);
    for name in &raw_names()[..2] {
        let dir = root.path().join(name);
        fs::create_dir(&dir).unwrap();
        let mut attached_c = OsString::from("-C");
        attached_c.push(name);
        // Visit the raw directory, then return to the repository. This isolates
        // argv handling from repository discovery's separate cwd encoding.
        for globals in [
            vec!["-C".into(), name.clone(), "-C".into(), "..".into()],
            vec![attached_c, "-C..".into()],
        ] {
            let out = command(GRIT, root.path())
                .args(&globals)
                .args(["--json", "status"])
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["branch"], "main");

            // Parsing succeeds; validate_cli then rejects excessive copy detection.
            for mode in ["", "--json", "--markdown"] {
                let mut cmd = command(GRIT, root.path());
                cmd.args(&globals).arg("blame");
                if !mode.is_empty() {
                    cmd.arg(mode);
                }
                let out = cmd
                    .args(["-C", "-C", "-C", "-C", "file.txt"])
                    .output()
                    .unwrap();
                assert_usage(&out, mode, "at most three -C");
            }
        }
    }
}

#[test]
fn unfiltered_status_retains_distinct_non_utf8_paths() {
    let dir = fixture();
    let clean = command(GRIT, dir.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(clean.status.success(), "{clean:?}");
    let value: serde_json::Value = serde_json::from_slice(&clean.stdout).unwrap();
    for field in ["staged", "unstaged", "untracked"] {
        assert!(value[field].as_array().unwrap().is_empty(), "{value}");
    }
    for name in raw_names() {
        fs::write(dir.path().join(name), b"changed\n").unwrap();
    }
    let out = command(GRIT, dir.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let paths: Vec<_> = value["unstaged"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    for name in raw_names() {
        use std::os::unix::ffi::OsStrExt;
        assert!(
            paths.contains(
                &grit_lib::repo_relpath::storage_string(OsStr::new(&name).as_bytes()).as_str()
            ),
            "{value}"
        );
    }
    git(dir.path(), &["fsck", "--strict"]);
}

#[test]
fn raw_repository_selection_cannot_mutate_a_unicode_sibling() {
    let root = tempfile::tempdir().unwrap();
    let names = [
        OsString::from_vec(b"repo\xff".to_vec()),
        OsString::from_vec(b"repo\xfe".to_vec()),
        OsString::from("repo\u{fffd}"),
    ];
    let repos: Vec<_> = names.iter().map(|name| root.path().join(name)).collect();
    for (i, repo) in repos.iter().enumerate() {
        fs::create_dir(repo).unwrap();
        git(repo, &["init", "-q", "-b", &format!("branch-{i}")]);
        git(repo, &["config", "user.name", "Tool"]);
        git(repo, &["config", "user.email", "tool@example.com"]);
        fs::write(repo.join("file"), format!("initial {i}\n")).unwrap();
        git(repo, &["add", "file"]);
        git(repo, &["commit", "-qm", "initial"]);
        assert!(git(repo, &["status", "--porcelain=v1"]).stdout.is_empty());
        fs::write(repo.join("file"), format!("changed {i}\n")).unwrap();
    }
    let snapshot = || {
        repos
            .iter()
            .map(|repo| {
                [
                    fs::read(repo.join(".git/index")).unwrap(),
                    git(repo, &["rev-parse", "HEAD"]).stdout,
                    fs::read(repo.join("file")).unwrap(),
                ]
            })
            .collect::<Vec<_>>()
    };
    let before = snapshot();
    for (name, repo) in names[..2].iter().zip(&repos) {
        let mut attached_c = OsString::from("-C");
        attached_c.push(name);
        let mut attached_git_dir = OsString::from("--git-dir=");
        attached_git_dir.push(repo.join(".git"));
        let mut attached_work_tree = OsString::from("--work-tree=");
        attached_work_tree.push(repo);
        let unicode = &repos[2];
        let selections = [
            vec!["-C".into(), name.clone()],
            vec![attached_c],
            vec!["-C".into(), repo.as_os_str().to_owned()],
            vec![
                "--git-dir".into(),
                repo.join(".git").into_os_string(),
                "--work-tree".into(),
                repo.as_os_str().to_owned(),
            ],
            vec![attached_git_dir, attached_work_tree],
            vec![
                "--git-dir".into(),
                repo.join(".git").into_os_string(),
                "--work-tree".into(),
                unicode.as_os_str().to_owned(),
            ],
            vec![
                "--git-dir".into(),
                unicode.join(".git").into_os_string(),
                "--work-tree".into(),
                repo.as_os_str().to_owned(),
            ],
        ];
        for mode in ["", "--json", "--markdown"] {
            for global_first in [false, true] {
                for subcommand in ["add", "status"] {
                    for selection in &selections {
                        let mut cmd = command(GRIT, root.path());
                        cmd.args(selection);
                        if global_first && !mode.is_empty() {
                            cmd.arg(mode);
                        }
                        cmd.arg(subcommand);
                        if !global_first && !mode.is_empty() {
                            cmd.arg(mode);
                        }
                        let out = cmd.output().unwrap();
                        assert!(
                            snapshot() == before,
                            "repository state changed: {selection:?}: {out:?}"
                        );
                        assert_usage(&out, mode, "UTF-8");
                    }
                    // A native cwd also needs validation, including a UTF-8 symlink
                    // whose resolved directory contains invalid bytes.
                    let alias = root.path().join("alias");
                    std::os::unix::fs::symlink(repo, &alias).unwrap();
                    for cwd in [repo, &alias] {
                        let mut cmd = command(GRIT, cwd);
                        if global_first && !mode.is_empty() {
                            cmd.arg(mode);
                        }
                        cmd.arg(subcommand);
                        if !global_first && !mode.is_empty() {
                            cmd.arg(mode);
                        }
                        let out = cmd.output().unwrap();
                        assert!(
                            snapshot() == before,
                            "repository state changed: {cwd:?}: {out:?}"
                        );
                        assert_usage(&out, mode, "UTF-8");
                    }
                    fs::remove_file(alias).unwrap();
                }
            }
        }
    }
    // Valid Unicode selection still works. System Git can stage raw roots by
    // their bytes, and all three repositories remain valid on disk.
    let out = command(GRIT, root.path())
        .arg("-C")
        .arg(&repos[2])
        .args(["add", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(&snapshot()[..2], &before[..2]);
    for repo in &repos[..2] {
        git(repo, &["add", "file"]);
    }
    for repo in &repos {
        assert!(git(repo, &["diff", "--name-only"]).stdout.is_empty());
        assert_eq!(
            git(repo, &["diff", "--cached", "--name-only"]).stdout,
            b"file\n"
        );
        git(repo, &["fsck", "--strict"]);
    }
}
