//! `git push --receive-pack='grit receive-pack'`: the system Git client pushes
//! through grit's server side, and `git fsck` accepts the result.

use std::path::Path;
use std::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

fn git(dir: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

fn commit_files(dir: &Path, from: usize, to: usize) -> TestResult {
    for i in from..to {
        std::fs::write(
            dir.join(format!("f{i}")),
            format!("line {i}\n").repeat(i + 1),
        )?;
    }
    git(dir, &["add", "."])?;
    git(dir, &["commit", "-q", "-m", &format!("files {from}..{to}")])?;
    Ok(())
}

fn push(source: &Path, remote: &Path, refspecs: &[&str]) -> TestResult {
    let receive_pack = format!("--receive-pack={GRIT} receive-pack");
    let remote = remote.to_str().ok_or("non-utf8 path")?;
    let mut args = vec!["push", "-q", receive_pack.as_str(), remote];
    args.extend_from_slice(refspecs);
    git(source, &args)?;
    Ok(())
}

/// A first push large enough to be indexed as a pack, then an incremental
/// (thin) push small enough to be stored loose, then a branch deletion.
#[test]
fn git_push_through_grit_receive_pack_is_fsck_clean() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("src");
    std::fs::create_dir_all(&source)?;
    git(&source, &["init", "-q", "-b", "main", "."])?;
    for batch in 0..5 {
        commit_files(&source, batch * 30, batch * 30 + 30)?;
    }
    git(&source, &["branch", "topic"])?;
    git(&source, &["tag", "-a", "-m", "release", "v1"])?;
    let remote = tmp.path().join("remote.git");
    std::fs::create_dir_all(&remote)?;
    git(&remote, &["init", "-q", "--bare", "."])?;

    push(&source, &remote, &["--all"])?;
    push(&source, &remote, &["--tags"])?;
    let packs = std::fs::read_dir(remote.join("objects/pack"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "pack"))
        .count();
    assert_eq!(
        packs, 1,
        "a push above receive.unpackLimit is kept as a pack"
    );
    git(&remote, &["fsck", "--strict", "--no-progress"])?;
    for rev in ["main", "topic", "v1"] {
        assert_eq!(
            git(&remote, &["rev-parse", rev])?,
            git(&source, &["rev-parse", rev])?
        );
    }

    std::fs::write(source.join("f0"), "line 0, edited\n".repeat(20))?;
    git(&source, &["commit", "-q", "-am", "edit f0"])?;
    push(&source, &remote, &["main"])?;
    push(&source, &remote, &[":topic"])?;
    git(&remote, &["fsck", "--strict", "--no-progress"])?;
    assert_eq!(
        git(&remote, &["rev-parse", "main"])?,
        git(&source, &["rev-parse", "main"])?
    );
    assert!(git(&remote, &["rev-parse", "--verify", "-q", "topic"]).is_err());
    let leftovers: Vec<_> = std::fs::read_dir(remote.join("objects"))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("tmp_objdir") || n == "incoming")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    Ok(())
}

/// A rejected update is reported to the Git client as `[remote rejected]` with
/// the refusal reason.
#[test]
fn git_push_shows_grit_refusal_reason() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("src");
    std::fs::create_dir_all(&source)?;
    git(&source, &["init", "-q", "-b", "main", "."])?;
    commit_files(&source, 0, 2)?;
    let remote = tmp.path().join("remote.git");
    std::fs::create_dir_all(&remote)?;
    git(&remote, &["init", "-q", "--bare", "."])?;
    git(&remote, &["config", "receive.denyDeletes", "true"])?;
    push(&source, &remote, &["main"])?;

    let receive_pack = format!("--receive-pack={GRIT} receive-pack");
    let out = Command::new("git")
        .args(["push", receive_pack.as_str()])
        .arg(&remote)
        .arg(":main")
        .current_dir(&source)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()?;
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[remote rejected]") && stderr.contains("deletion prohibited"),
        "{stderr}"
    );
    Ok(())
}

/// Issue #2136: `git push` from a `--depth 1` clone sends `shallow <oid>`
/// before its commands. Onto a server with the full history the push applies
/// and the server stays complete; into an empty server it would need a shallow
/// update, which is refused unless `receive.shallowUpdate` is set.
#[test]
fn git_push_from_shallow_clone_through_grit_receive_pack() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("src");
    std::fs::create_dir_all(&source)?;
    git(&source, &["init", "-q", "-b", "main", "."])?;
    for batch in 0..3 {
        commit_files(&source, batch * 2, batch * 2 + 2)?;
    }
    let full = tmp.path().join("full.git");
    let empty = tmp.path().join("empty.git");
    let src = source.to_str().ok_or("non-utf8 path")?;
    git(
        tmp.path(),
        &["clone", "-q", "--bare", src, full.to_str().ok_or("path")?],
    )?;
    std::fs::create_dir_all(&empty)?;
    git(&empty, &["init", "-q", "--bare", "."])?;

    let client = tmp.path().join("sh1");
    let url = format!("file://{src}");
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            &url,
            client.to_str().ok_or("path")?,
        ],
    )?;
    git(&client, &["commit", "-q", "--allow-empty", "-m", "x"])?;
    let tip = git(&client, &["rev-parse", "HEAD"])?;

    push(&client, &full, &["HEAD:refs/heads/t1"])?;
    assert_eq!(git(&full, &["rev-parse", "t1"])?, tip);
    assert!(!full.join("shallow").exists());
    git(&full, &["fsck", "--strict", "--no-progress"])?;

    let receive_pack = format!("--receive-pack={GRIT} receive-pack");
    let out = Command::new("git")
        .args(["push", receive_pack.as_str()])
        .arg(&empty)
        .arg("HEAD:refs/heads/t2")
        .current_dir(&client)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("shallow update not allowed"), "{stderr}");
    assert!(git(&empty, &["rev-parse", "--verify", "-q", "t2"]).is_err());

    git(&empty, &["config", "receive.shallowUpdate", "true"])?;
    push(&client, &empty, &["HEAD:refs/heads/t2"])?;
    assert_eq!(git(&empty, &["rev-parse", "t2"])?, tip);
    assert_eq!(
        std::fs::read_to_string(empty.join("shallow"))?,
        std::fs::read_to_string(client.join(".git/shallow"))?
    );
    git(&empty, &["fsck", "--strict", "--no-progress"])?;
    Ok(())
}
