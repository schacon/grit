//! Regression for issue #1653: loose-object gc/repack pack size vs system `git`.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

const GIT_ENV: &[(&str, &str)] = &[
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_SYSTEM", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_AUTHOR_NAME", "T"),
    ("GIT_AUTHOR_EMAIL", "t@example.com"),
    ("GIT_COMMITTER_NAME", "T"),
    ("GIT_COMMITTER_EMAIL", "t@example.com"),
];

fn git(dir: &Path, args: &[&str]) {
    git_with_commit_index(dir, args, None);
}

fn git_with_commit_index(dir: &Path, args: &[&str], commit_index: Option<usize>) {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    for (k, v) in GIT_ENV {
        cmd.env(k, v);
    }
    if let Some(i) = commit_index {
        let when = format!("@{} +0000", 1_700_000_000 + i);
        cmd.env("GIT_AUTHOR_DATE", &when);
        cmd.env("GIT_COMMITTER_DATE", &when);
    }
    let out = cmd.output().expect("git");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn grit_gc(dir: &Path) {
    let mut cmd = Command::new(GRIT);
    cmd.current_dir(dir).args(["gc", "-q"]);
    for (k, v) in GIT_ENV {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("grit gc");
    assert!(
        out.status.success(),
        "grit gc: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn populate_loose_history(dir: &Path, commits: usize) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "gc.auto", "0"]);
    git(dir, &["config", "pack.threads", "1"]);
    for i in 0..commits {
        for name in ["a.txt", "b.txt", "c.txt"] {
            let path = dir.join(name);
            let mut body = String::new();
            if path.is_file() {
                body = std::fs::read_to_string(&path).expect("read");
            }
            let extra = 5 + (i * 17 + name.len()) % 56;
            for j in 0..extra {
                body.push_str(&format!("{name}: line {i}-{j} lorem ipsum dolor\n"));
            }
            std::fs::write(&path, body).expect("write blob");
        }
        git(dir, &["add", "a.txt", "b.txt", "c.txt"]);
        git_with_commit_index(dir, &["commit", "-q", "-m", &format!("c{i}")], Some(i));
    }
}

fn loose_object_count(git_dir: &Path) -> usize {
    let objects = git_dir.join("objects");
    std::fs::read_dir(&objects)
        .expect("objects dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().len() == 2)
        .map(|e| {
            std::fs::read_dir(e.path())
                .expect("loose shard")
                .filter_map(|f| f.ok())
                .count()
        })
        .sum()
}

fn pack_bytes(git_dir: &Path) -> usize {
    let pack_dir = git_dir.join("objects/pack");
    std::fs::read_dir(&pack_dir)
        .expect("pack dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "pack"))
        .map(|e| std::fs::metadata(e.path()).expect("meta").len() as usize)
        .sum()
}

#[test]
fn issue_1653_loose_gc_pack_near_git_size_and_fsck_clean() {
    let grit_dir = TempDir::new().expect("grit tempdir");
    let git_dir = TempDir::new().expect("git tempdir");
    populate_loose_history(grit_dir.path(), 120);
    populate_loose_history(git_dir.path(), 120);

    let grit_git = grit_dir.path().join(".git");
    assert_eq!(
        loose_object_count(&grit_git),
        600,
        "issue #1653 fixture must have 600 distinct loose objects before gc"
    );

    git(git_dir.path(), &["config", "pack.threads", "1"]);

    grit_gc(grit_dir.path());
    git(git_dir.path(), &["gc", "-q"]);

    git(grit_dir.path(), &["fsck", "--strict"]);
    let grit_pack = pack_bytes(&grit_dir.path().join(".git"));
    let git_pack = pack_bytes(&git_dir.path().join(".git"));
    assert!(
        grit_pack <= git_pack * 15 / 10,
        "issue #1653: grit pack {grit_pack} vs git pack {git_pack} (ratio {:.2})",
        grit_pack as f64 / git_pack as f64
    );
}
