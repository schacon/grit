//! Regression: invalid `-o` must not delete an existing nonempty destination.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

type TestResult = Result<(), Box<dyn Error>>;

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

fn git_in(dir: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()?;
    assert!(status.success(), "git {:?} failed", args);
    Ok(())
}

#[test]
fn invalid_origin_preserves_nonempty_destination() -> TestResult {
    let base = tempfile::tempdir()?;
    let origin = base.path().join("origin.git");
    fs::create_dir_all(&origin)?;
    git_in(
        base.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    )?;

    let dest = base.path().join("dest");
    fs::create_dir(&dest)?;
    fs::write(dest.join("keep.txt"), b"stay")?;

    let out = Command::new(GRIT)
        .args([
            "clone",
            "-o",
            "bad name",
            origin.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .output()?;
    assert!(!out.status.success(), "clone must fail");
    assert!(
        dest.join("keep.txt").is_file(),
        "nonempty destination must be preserved; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}
