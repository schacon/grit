//! `GIT_OBJECT_DIRECTORY` selects where `grit` reads and writes objects (issue #1907).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn command(program: &str, dir: &Path, objects: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Odb Env")
        .env("GIT_AUTHOR_EMAIL", "odb-env@example.com")
        .env("GIT_COMMITTER_NAME", "Odb Env")
        .env("GIT_COMMITTER_EMAIL", "odb-env@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env("GIT_OBJECT_DIRECTORY", objects);
    cmd
}

fn ok(mut cmd: Command) -> String {
    let out: Output = cmd.output().expect("spawn");
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn loose_count(objects: &Path) -> usize {
    fs::read_dir(objects)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().len() == 2 && e.path().is_dir())
                .map(|e| fs::read_dir(e.path()).map(|d| d.count()).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(unix)]
#[test]
fn cat_add_and_commit_use_env_object_directory() {
    let grit = env!("CARGO_BIN_EXE_grit");
    let tmp = tempfile::tempdir().expect("tempdir");
    let work = tmp.path().join("r");
    let objq = tmp.path().join("objq");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&objq).unwrap();
    let mut init = Command::new("git");
    init.current_dir(&work).args(["init", "-q", "-b", "main"]);
    ok(init);
    let dot_git_objects = work.join(".git/objects");

    fs::write(work.join("fb"), "fresh blob\n").unwrap();
    let hex = ok(command("git", &work, &objq, &["hash-object", "-w", "fb"]));
    assert_eq!(
        ok(command(grit, &work, &objq, &["cat", &hex])),
        "fresh blob"
    );

    fs::write(work.join("nb"), "new\n").unwrap();
    ok(command(grit, &work, &objq, &["add", "nb"]));
    assert_eq!(loose_count(&objq), 2);
    assert_eq!(loose_count(&dot_git_objects), 0);

    ok(command(grit, &work, &objq, &["commit", "-m", "x"]));
    assert_eq!(
        loose_count(&dot_git_objects),
        0,
        "nothing leaks into .git/objects"
    );
    ok(command(
        "git",
        &work,
        &objq,
        &["fsck", "--strict", "--no-dangling"],
    ));
    assert_eq!(
        ok(command("git", &work, &objq, &["cat-file", "-p", "HEAD:nb"])),
        "new"
    );
}

#[cfg(unix)]
#[test]
fn empty_env_object_directory_is_not_a_repository() {
    let grit = env!("CARGO_BIN_EXE_grit");
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut init = Command::new("git");
    init.current_dir(tmp.path()).args(["init", "-q"]);
    ok(init);
    let empty = Path::new("");
    for program in ["git", grit] {
        let out = command(program, tmp.path(), empty, &["status"])
            .output()
            .expect("spawn");
        assert!(!out.status.success(), "{program} status with empty value");
    }
}

#[cfg(unix)]
#[test]
fn fetch_and_push_use_env_object_directory() {
    let grit = env!("CARGO_BIN_EXE_grit");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().unwrap();
    let up = root.join("up");
    fs::create_dir_all(&up).unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Odb Env")
            .env("GIT_AUTHOR_EMAIL", "odb-env@example.com")
            .env("GIT_COMMITTER_NAME", "Odb Env")
            .env("GIT_COMMITTER_EMAIL", "odb-env@example.com")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
        ok(cmd)
    };
    git(&up, &["init", "-q", "-b", "main"]);
    fs::write(up.join("base"), "base\n").unwrap();
    git(&up, &["add", "base"]);
    git(&up, &["commit", "-q", "-m", "base"]);
    git(&root, &["clone", "-q", "--bare", "up", "remote.git"]);
    git(&root, &["clone", "-q", "remote.git", "r"]);
    fs::write(up.join("next"), "next\n").unwrap();
    git(&up, &["add", "next"]);
    git(&up, &["commit", "-q", "-m", "next"]);
    git(&up, &["push", "-q", "../remote.git", "main"]);

    let work = root.join("r");
    let objq = root.join("objq");
    fs::create_dir_all(&objq).unwrap();
    let main_store = work.join(".git/objects");
    let quarantined = |program: &str, args: &[&str]| {
        let mut cmd = command(program, &work, &objq, args);
        cmd.env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &main_store);
        cmd
    };
    let main_files = || {
        let out = Command::new("find")
            .arg(&main_store)
            .arg("-type")
            .arg("f")
            .output()
            .expect("find");
        String::from_utf8_lossy(&out.stdout).lines().count()
    };
    let before = main_files();

    ok(quarantined(grit, &["fetch", "origin"]));
    assert_eq!(main_files(), before, "fetch leaves .git/objects alone");
    ok(quarantined("git", &["fsck", "--strict", "--no-dangling"]));
    ok(quarantined(
        "git",
        &["merge", "-q", "--ff-only", "origin/main"],
    ));

    fs::write(work.join("local"), "local\n").unwrap();
    ok(quarantined(grit, &["commit", "-m", "local"]));
    ok(quarantined(grit, &["push", "origin", "main"]));
    assert_eq!(
        main_files(),
        before,
        "commit and push leave .git/objects alone"
    );
    let pushed = ok(quarantined("git", &["rev-parse", "HEAD"]));
    let remote = root.join("remote.git");
    assert_eq!(git(&remote, &["rev-parse", "refs/heads/main"]), pushed);
    git(&remote, &["fsck", "--strict", "--no-dangling"]);
}
