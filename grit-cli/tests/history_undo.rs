//! CLI tests for `grit history undo`.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

type TestResult = Result<(), Box<dyn Error>>;

const GS: &str = env!("CARGO_BIN_EXE_grit");

struct CmdOutput {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

impl CmdOutput {
    fn success(&self) -> bool {
        self.status == Some(0)
    }
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "grit-cli-history-undo-{tag}-{}-{n}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn null_device() -> OsString {
    if cfg!(windows) {
        OsString::from("NUL")
    } else {
        OsString::from("/dev/null")
    }
}

fn gs<I, S>(dir: &Path, args: I) -> Result<CmdOutput, Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect();
    let out = Command::new(GS)
        .args(&args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test User")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Test User")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .output()?;
    Ok(CmdOutput {
        status: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn git(dir: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test User")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Test User")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .status()?;
    assert!(status.success());
    Ok(())
}

fn init_linear_repo(dir: &Path) -> Result<(), Box<dyn Error>> {
    git(dir, &["init", "-q", "-b", "main"])?;
    for (msg, content) in [("one", "1\n"), ("two", "2\n"), ("three", "3\n")] {
        fs::write(dir.join("f.txt"), content)?;
        git(dir, &["add", "f.txt"])?;
        git(dir, &["commit", "-m", msg])?;
    }
    Ok(())
}

#[test]
fn history_undo_json_and_dry_run() -> TestResult {
    let dir = Scratch::new("json")?;
    init_linear_repo(dir.path())?;
    let before = Command::new("git")
        .current_dir(dir.path())
        .args(["rev-parse", "HEAD"])
        .output()?
        .stdout;
    let before = String::from_utf8_lossy(&before).trim().to_string();

    let drop = gs(
        dir.path(),
        ["history", "drop", "HEAD", "--update-refs", "head"],
    )?;
    assert!(drop.success(), "{}\n{}", drop.stdout, drop.stderr);
    let dry = gs(dir.path(), ["--json", "history", "undo", "--dry-run"])?;
    assert!(dry.success(), "{}\n{}", dry.stdout, dry.stderr);
    let v: Value = serde_json::from_str(dry.stdout.trim())?;
    assert_eq!(v["operation"], "undo");
    assert_eq!(v["dry_run"], true);
    assert!(v["refs"].as_array().is_some());

    gs(dir.path(), ["history", "undo"])?;
    let after = Command::new("git")
        .current_dir(dir.path())
        .args(["rev-parse", "HEAD"])
        .output()?
        .stdout;
    let after = String::from_utf8_lossy(&after).trim().to_string();
    assert_eq!(before, after);

    let applied = gs(dir.path(), ["--json", "history", "undo"])?;
    assert!(applied.success());
    let applied_v: Value = serde_json::from_str(applied.stdout.trim())?;
    assert_eq!(applied_v["operation"], "undo");
    assert_eq!(applied_v["dry_run"], false);
    Ok(())
}

fn git_stdout(dir: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .output()?;
    assert!(out.status.success(), "git {args:?}");
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The issue's reproduction: a staged edit folded into `HEAD~2` comes back on undo.
fn fixup_staged_edit_into_root(dir: &Path) -> TestResult {
    git(dir, &["init", "-q", "-b", "main"])?;
    for (path, content, msg) in [("a.txt", "a\n", "c1"), ("b.txt", "b\n", "c2")] {
        fs::write(dir.join(path), content)?;
        git(dir, &["add", path])?;
        git(dir, &["commit", "-q", "-m", msg])?;
    }
    fs::write(dir.join("a.txt"), "a\na-fixed\n")?;
    git(dir, &["add", "a.txt"])?;
    let fixup = gs(dir, ["history", "fixup", "HEAD~1"])?;
    assert!(fixup.success(), "{}\n{}", fixup.stdout, fixup.stderr);
    Ok(())
}

#[test]
fn history_undo_after_fixup_reports_restored_paths() -> TestResult {
    let dir = Scratch::new("fixup-human")?;
    fixup_staged_edit_into_root(dir.path())?;
    let undo = gs(dir.path(), ["history", "undo"])?;
    assert!(undo.success(), "{}\n{}", undo.stdout, undo.stderr);
    assert!(
        undo.stdout.contains("restored a.txt (index and work tree)"),
        "{}",
        undo.stdout
    );
    assert_eq!(
        git_stdout(dir.path(), &["status", "--porcelain"])?,
        "M  a.txt\n"
    );

    let redo = gs(dir.path(), ["--markdown", "history", "undo"])?;
    assert!(redo.success(), "{}\n{}", redo.stdout, redo.stderr);
    assert!(
        redo.stdout
            .contains("- restored `a.txt` (index and work tree)"),
        "{}",
        redo.stdout
    );
    assert_eq!(git_stdout(dir.path(), &["status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn history_undo_json_lists_restored_paths_and_unrestored_edits() -> TestResult {
    let dir = Scratch::new("fixup-json")?;
    fixup_staged_edit_into_root(dir.path())?;
    let fixed_tip = git_stdout(dir.path(), &["rev-parse", "HEAD"])?;

    let dry = gs(dir.path(), ["--json", "history", "undo", "--dry-run"])?;
    assert!(dry.success(), "{}\n{}", dry.stdout, dry.stderr);
    let v: Value = serde_json::from_str(dry.stdout.trim())?;
    assert_eq!(v["restored_paths"], serde_json::json!(["a.txt"]));
    assert!(v.get("unrestored_edits").is_none());

    fs::remove_file(dir.path().join(".git/grit/history-last-work"))?;
    let undo = gs(dir.path(), ["--json", "history", "undo"])?;
    assert!(undo.success(), "{}\n{}", undo.stdout, undo.stderr);
    let v: Value = serde_json::from_str(undo.stdout.trim())?;
    assert!(v.get("restored_paths").is_none());
    assert_eq!(v["unrestored_edits"], fixed_tip.trim());
    Ok(())
}
