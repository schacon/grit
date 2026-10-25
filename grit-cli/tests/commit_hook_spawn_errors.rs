//! `grit commit` when a hook cannot be started (missing `#!` interpreter).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const GRIT: &str = env!("CARGO_BIN_EXE_grit");
const MISSING_INTERPRETER: &str = "#!/definitely/missing/interpreter\nexit 0\n";

fn repo_with_hook(hook: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let status = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(tmp.path())
        .status()
        .expect("git init");
    assert!(status.success());
    let path = tmp.path().join(".git/hooks").join(hook);
    fs::create_dir_all(path.parent().expect("hooks dir")).expect("mkdir");
    fs::write(&path, MISSING_INTERPRETER).expect("write hook");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    fs::write(tmp.path().join("f.txt"), "x\n").expect("write file");
    tmp
}

fn grit(dir: &Path, args: &[&str]) -> Output {
    Command::new(GRIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "A")
        .env("GIT_AUTHOR_EMAIL", "a@example.com")
        .env("GIT_COMMITTER_NAME", "A")
        .env("GIT_COMMITTER_EMAIL", "a@example.com")
        .output()
        .expect("run grit")
}

fn commit_count(dir: &Path) -> usize {
    let out = Command::new("git")
        .args(["rev-list", "--count", "--all"])
        .current_dir(dir)
        .output()
        .expect("git rev-list");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[test]
fn unstartable_pre_commit_reports_the_os_error_and_aborts() {
    let repo = repo_with_hook("pre-commit");
    let os_error = std::io::Error::from_raw_os_error(2).to_string();

    let out = grit(repo.path(), &["commit", "-m", "one", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    let error = json["error"].as_str().expect("error string");
    assert!(
        error.contains("failed to run hook pre-commit (.git/hooks/pre-commit)"),
        "{error}"
    );
    assert!(error.contains(&os_error), "{error}");
    assert_eq!(commit_count(repo.path()), 0);

    let human = grit(repo.path(), &["commit", "-m", "one"]);
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert_eq!(human.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(&os_error), "{stderr}");
}

#[test]
fn unstartable_post_commit_warns_and_keeps_the_commit() {
    let repo = repo_with_hook("post-commit");

    let out = grit(repo.path(), &["commit", "-m", "one", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert_eq!(commit_count(repo.path()), 1);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert!(json.get("error").is_none(), "{stdout}");
    assert!(
        stderr.contains("post-commit") && stderr.contains(".git/hooks/post-commit"),
        "the hook failure must still be reported: {stderr}"
    );
}

#[test]
fn unstartable_script_keeps_output_of_the_configured_hook_before_it() {
    let repo = repo_with_hook("pre-commit");
    fs::write(
        repo.path().join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n[hook \"lint\"]\n\tcommand = echo from-configured-hook\n\tevent = pre-commit\n",
    )
    .expect("config");

    let out = grit(repo.path(), &["commit", "-m", "one", "--markdown"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.starts_with("**Error:** failed to run hook pre-commit (.git/hooks/pre-commit)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("**Hook stdout:**\n\n```text\nfrom-configured-hook\n```"),
        "{stdout}"
    );
    assert_eq!(commit_count(repo.path()), 0);
}
