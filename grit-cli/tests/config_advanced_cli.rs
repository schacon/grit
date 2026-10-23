//! CLI JSON tests for config --all, --add, --dry-run, and section ops.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!("grit-config-adv-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("mkdir");
        Self { path }
    }

    fn child(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn grit(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(GRIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("spawn grit")
}

fn grit_ok(dir: &Path, args: &[&str]) {
    let out = grit(dir, args);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn config_add_and_get_all_json() -> TestResult {
    let scratch = Scratch::new("add-all");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    grit_ok(&repo, &["config", "--add", "s.k", "one"]);
    grit_ok(&repo, &["config", "--add", "s.k", "two"]);
    let out = grit(&repo, &["--json", "config", "--all", "s.k"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(v["action"], "get_all");
    assert_eq!(v["values"].as_array().map(|a| a.len()), Some(2));
    Ok(())
}

#[test]
fn config_set_dry_run_leaves_file_unchanged() -> TestResult {
    let scratch = Scratch::new("dry-run");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    let cfg_path = repo.join(".git/config");
    let before = fs::read(&cfg_path)?;
    let out = grit(
        &repo,
        &["--json", "config", "--dry-run", "user.email", "x@y.z"],
    );
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(v["action"], "set");
    assert_eq!(v["dry_run"], true);
    assert_eq!(fs::read(&cfg_path)?, before);
    Ok(())
}

#[test]
fn config_unset_all_json() -> TestResult {
    let scratch = Scratch::new("unset-all");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    grit_ok(&repo, &["config", "--add", "m.k", "a"]);
    grit_ok(&repo, &["config", "--add", "m.k", "b"]);
    let out = grit(&repo, &["--json", "config", "--unset-all", "m.k"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(v["action"], "unset_all");
    assert_eq!(v["removed"], 2);
    Ok(())
}

#[test]
fn config_rename_section_with_file_scope() -> TestResult {
    let scratch = Scratch::new("rename-file");
    let cfg = scratch.child("external.cfg");
    fs::write(&cfg, "[old]\n\tx = 1\n")?;
    grit_ok(
        &scratch.path,
        &[
            "config",
            "-f",
            cfg.to_str().unwrap(),
            "--rename-section",
            "old",
            "--rename-section-new",
            "new",
        ],
    );
    let text = fs::read_to_string(&cfg)?;
    assert!(text.contains("[new]"));
    Ok(())
}

#[test]
fn config_remove_section_with_file_scope() -> TestResult {
    let scratch = Scratch::new("remove-file");
    let cfg = scratch.child("external.cfg");
    fs::write(&cfg, "[stay]\n\tk = 1\n[go]\n\tk = 2\n")?;
    grit_ok(
        &scratch.path,
        &[
            "config",
            "-f",
            cfg.to_str().unwrap(),
            "--remove-section",
            "go",
        ],
    );
    let text = fs::read_to_string(&cfg)?;
    assert!(text.contains("[stay]"));
    assert!(!text.contains("[go]"));
    Ok(())
}

#[test]
fn config_dry_run_unset_missing_key_errors() -> TestResult {
    let scratch = Scratch::new("dry-unset-missing");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    let out = grit(&repo, &["config", "--dry-run", "--unset", "missing.key"]);
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_get_all_with_value_is_rejected() -> TestResult {
    let scratch = Scratch::new("all-value");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    let out = grit(&repo, &["config", "--all", "user.name", "extra"]);
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_add_without_value_is_rejected() -> TestResult {
    let scratch = Scratch::new("add-no-value");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    let out = grit(&repo, &["config", "--add", "k.v"]);
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_default_still_validates_key() -> TestResult {
    let scratch = Scratch::new("default-bad-key");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    let out = grit(&repo, &["config", "--default", "x", "bad key with spaces"]);
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_worktree_read_without_extension_ignored() -> TestResult {
    let scratch = Scratch::new("wt-gate");
    let main = scratch.child("main");
    let wt = scratch.child("wt");
    fs::create_dir_all(&main)?;
    grit_ok(&main, &["init", "."]);
    grit_ok(
        &main,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "seed",
            "--author",
            "t <t@e>",
        ],
    );
    grit_ok(&main, &["worktree", "add", wt.to_str().unwrap()]);
    let wt_git = wt.join(".git");
    let git_dir = if wt_git.is_file() {
        let content = fs::read_to_string(&wt_git)?;
        let path = content.strip_prefix("gitdir: ").expect("gitdir").trim();
        PathBuf::from(path)
    } else {
        wt.join(".git")
    };
    fs::write(git_dir.join("config.worktree"), "[x]\n\tk = sneaky\n")?;
    let git = Command::new("git")
        .args(["config", "--worktree", "--get", "x.k"])
        .current_dir(&wt)
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()?;
    assert!(!git.status.success());
    let out = grit(&wt, &["config", "--worktree", "x.k"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    Ok(())
}

#[test]
fn config_dry_run_without_write_action_is_rejected() -> TestResult {
    let scratch = Scratch::new("dry-run-read");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    grit_ok(&repo, &["config", "core.answer", "42"]);
    let out = grit(&repo, &["config", "--dry-run", "core.answer"]);
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_type_conflicts_with_unset() -> TestResult {
    let scratch = Scratch::new("type-unset");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    grit_ok(&repo, &["config", "core.answer", "42"]);
    let out = grit(
        &repo,
        &["config", "--type", "bool", "--unset", "core.answer"],
    );
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_list_conflicts_with_remove_section() -> TestResult {
    let scratch = Scratch::new("list-remove-conflict");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    let out = grit(&repo, &["config", "--list", "--remove-section", "core"]);
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn config_rename_section_json() -> TestResult {
    let scratch = Scratch::new("rename");
    let repo = scratch.child("repo");
    fs::create_dir_all(&repo)?;
    grit_ok(&repo, &["init", "."]);
    grit_ok(&repo, &["config", "sec.k", "v"]);
    let out = grit(
        &repo,
        &[
            "--json",
            "config",
            "--rename-section",
            "sec",
            "--rename-section-new",
            "renamed",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(v["action"], "rename_section");
    Ok(())
}
