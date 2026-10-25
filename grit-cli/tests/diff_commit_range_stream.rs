//! Commit-range patch streaming: `--exit-code`, human, and `--markdown` outcomes.

use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

type TestResult = Result<(), Box<dyn Error>>;

const GS: &str = env!("CARGO_BIN_EXE_grit");

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "grit-diff-range-stream-{tag}-{}-{n}",
            std::process::id()
        ));
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

fn gs<I, S>(dir: &Path, args: I) -> Result<(Option<i32>, String, String), Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Command::new(GS)
        .args(args.into_iter().map(|a| a.as_ref().to_os_string()))
        .current_dir(dir)
        .output()?;
    Ok((
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn git_ok(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {:?} failed", args);
}

fn two_commit_repo_with_changes(scratch: &Scratch) -> Result<(), Box<dyn Error>> {
    let repo = scratch.path();
    git_ok(repo, &["init", "-q", "-b", "main"]);
    git_ok(repo, &["config", "user.name", "T"]);
    git_ok(repo, &["config", "user.email", "t@e.com"]);
    for i in 0..4 {
        fs::write(repo.join(format!("f{i}.txt")), format!("base {i}\n"))?;
    }
    git_ok(repo, &["add", "-A"]);
    git_ok(repo, &["commit", "-qm", "base"]);
    for i in 0..4 {
        fs::write(repo.join(format!("f{i}.txt")), format!("tip {i}\n"))?;
    }
    git_ok(repo, &["add", "-A"]);
    git_ok(repo, &["commit", "-qm", "tip"]);
    Ok(())
}

#[test]
fn commit_range_exit_code_and_human_no_spurious_no_changes() -> TestResult {
    let scratch = Scratch::new("human-exit")?;
    two_commit_repo_with_changes(&scratch)?;
    let repo = scratch.path();

    let (code, stdout, _) = gs(repo, ["diff", "HEAD~1", "HEAD", "--exit-code"])?;
    assert_eq!(code, Some(1), "changed commit range should exit 1");
    assert!(stdout.contains("f0.txt"), "expected patch body: {stdout}");
    assert!(
        !stdout.contains("No changes."),
        "streamed diff must not append empty message: {stdout}"
    );

    let (clean_code, clean_out, _) = gs(repo, ["diff", "HEAD", "HEAD", "--exit-code"])?;
    assert_eq!(clean_code, Some(0));
    assert!(clean_out.contains("No changes.") || clean_out.trim().is_empty());

    let (quiet_code, quiet_out, quiet_err) = gs(repo, ["diff", "HEAD~1", "HEAD", "--quiet"])?;
    assert_eq!(
        quiet_code,
        Some(1),
        "changed commit range --quiet should exit 1"
    );
    assert!(
        quiet_out.is_empty() && quiet_err.is_empty(),
        "quiet must produce no output: stdout={quiet_out:?} stderr={quiet_err:?}"
    );

    Ok(())
}

#[test]
fn commit_range_markdown_stream_preserves_file_separators() -> TestResult {
    let scratch = Scratch::new("markdown")?;
    two_commit_repo_with_changes(&scratch)?;
    let repo = scratch.path();

    let (_, stdout, _) = gs(repo, ["diff", "HEAD~1", "HEAD", "--markdown"])?;
    assert!(stdout.contains("## f0.txt"), "{stdout}");
    assert!(stdout.contains("## f1.txt"), "{stdout}");
    assert!(
        !stdout.contains("No changes."),
        "changed range must not say no changes: {stdout}"
    );
    let sep = stdout.find("## f0.txt").expect("f0 header");
    let f1 = stdout.find("## f1.txt").expect("f1 header");
    assert!(
        stdout[sep..f1].contains("\n\n"),
        "markdown files should be separated by a blank line: {:?}",
        &stdout[sep..f1.min(sep + 80)]
    );

    Ok(())
}
