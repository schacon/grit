//! Repository-level fsck compared to system `git fsck` (upstream t1450-style cases).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use grit_lib::maintenance::repository_fsck::{self as fsck, FsckIssue, FsckOptions};
use grit_lib::index::Index;
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::porcelain::add::stage_for_commit;
use grit_lib::porcelain::commit::{create_commit, CommitRequest};
use grit_lib::progress::{NeverCancel, NullProgress};
use grit_lib::repo::{InitOptions, Repository};
use grit_test_support::{git_fsck, write_loose_object, HashAlgo as FixtureAlgo};
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && args.first() != Some(&"fsck") {
        panic!("git {args:?} failed: {combined}");
    }
    combined
}

fn init_repo() -> (TempDir, Repository) {
    let dir = TempDir::new().expect("tempdir");
    let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
    (dir, repo)
}

fn one_commit(repo: &Repository) -> ObjectId {
    let wt = repo.work_tree().expect("wt");
    std::fs::write(wt.join("f.txt"), b"hello\n").expect("write");
    stage_for_commit(repo, &mut NullProgress).expect("stage");
    let ident = "T <t@example.com> 1 +0000".to_string();
    create_commit(
        repo,
        &CommitRequest {
            message: "c1".into(),
            author: ident.clone(),
            committer: ident,
            allow_empty: false,
            sign_override: None,
            amend: false,
        },
        &mut NullProgress,
    )
    .expect("commit")
    .oid
}

fn grit_fsck(repo: &Repository, opts: &FsckOptions) -> fsck::FsckReport {
    fsck::fsck(repo, opts, &mut NullProgress, &NeverCancel).expect("grit fsck")
}

fn git_dangling_set(root: &std::path::Path) -> Vec<(ObjectKind, ObjectId)> {
    let text = git(root, &["fsck"]);
    parse_dangling_lines(&text)
}

fn parse_dangling_lines(text: &str) -> Vec<(ObjectKind, ObjectId)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line
            .trim()
            .strip_prefix("dangling ")
            .or_else(|| line.trim().strip_prefix("notice: dangling "))
        else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let kind = match parts.next() {
            Some("commit") => ObjectKind::Commit,
            Some("blob") => ObjectKind::Blob,
            Some("tree") => ObjectKind::Tree,
            Some("tag") => ObjectKind::Tag,
            _ => continue,
        };
        let Some(hex) = parts.next() else {
            continue;
        };
        if let Ok(oid) = ObjectId::from_hex(hex) {
            out.push((kind, oid));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn grit_dangling_set(report: &fsck::FsckReport) -> Vec<(ObjectKind, ObjectId)> {
    let mut v = report.dangling.clone();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

fn has_msg_id(report: &fsck::FsckReport, id: &str) -> bool {
    report.issues.iter().any(|i| fsck::issue_msg_id(i) == id)
}

#[test]
fn clean_repo_has_no_issues_and_git_agrees() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    let git_out = git_fsck(dir.path(), true);
    assert!(git_out.msg_ids.is_empty(), "{:?}", git_out.msg_ids);
    let config = repo.config().expect("config");
    let report = grit_fsck(&repo, &FsckOptions::full_repository(true));
    assert!(report.ok(&config, &FsckOptions::full_repository(true)));
    assert!(report.issues.is_empty());
}

#[test]
fn missing_blob_reported_as_missing_object() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    let wt = repo.work_tree().expect("wt");
    std::fs::write(wt.join("orphan.txt"), b"x").expect("write");
    stage_for_commit(&repo, &mut NullProgress).expect("stage");
    let index = Index::load(&repo.git_dir().join("index")).expect("index");
    let staged = index
        .entries()
        .iter()
        .find(|e| e.path.ends_with(b"orphan.txt"))
        .expect("entry")
        .oid;
    let loose = repo
        .odb()
        .enumerate_local_loose_objects()
        .expect("loose")
        .into_iter()
        .find(|(oid, _)| *oid == staged)
        .expect("path");
    std::fs::remove_file(loose.1).expect("rm");

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.contains("missing") && git_text.contains(&staged.to_hex()),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(has_msg_id(&report, "missingObject"), "{:?}", report.issues);
}

#[test]
fn corrupt_loose_object() {
    let (dir, repo) = init_repo();
    let blob = one_commit(&repo);
    let _ = blob;
    let loose = repo
        .odb()
        .enumerate_local_loose_objects()
        .expect("loose")
        .into_iter()
        .find(|(_, p)| p.to_string_lossy().contains("/objects/"))
        .expect("any loose");
    let mut perms = std::fs::metadata(&loose.1).expect("meta").permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&loose.1, perms).expect("chmod");
    let mut bytes = std::fs::read(&loose.1).expect("read");
    if bytes.len() > 10 {
        bytes[10] ^= 0xff;
    }
    std::fs::write(&loose.1, &bytes).expect("corrupt");

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.to_lowercase().contains("corrupt") || git_text.contains("badObject"),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(report.issues.iter().any(|i| {
        matches!(
            i,
            FsckIssue::CorruptLoose { .. } | FsckIssue::BadObject { .. }
        )
    }));
}

#[test]
fn hash_mismatch() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    let odb = repo.odb();
    let body = b"payload-for-mismatch";
    let real_oid = odb.hash(ObjectKind::Blob, body);
    let decoy_oid = odb.hash(ObjectKind::Blob, b"decoy");
    let real_path = odb.object_path(&real_oid);
    write_loose_object(
        &dir.path().join(".git/objects"),
        FixtureAlgo::Sha1,
        "blob",
        body,
    )
    .expect("write");
    let decoy_path = odb.object_path(&decoy_oid);
    std::fs::create_dir_all(decoy_path.parent().expect("pfx")).expect("mkdir");
    std::fs::copy(&real_path, &decoy_path).expect("copy");
    let _ = std::fs::remove_file(&real_path);

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.contains("hasSha1")
            || git_text.to_lowercase().contains("hash")
            || git_text.to_lowercase().contains("mismatch"),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, FsckIssue::HashMismatch { .. })));
}

#[test]
fn truncated_pack_checksum() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    git(dir.path(), &["repack", "-a", "-d"]);
    let pack_dir = dir.path().join(".git/objects/pack");
    let pack = fs::read_dir(&pack_dir)
        .expect("rd")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "pack"))
        .expect("pack");
    let mut perms = std::fs::metadata(&pack).expect("meta").permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&pack, perms).expect("chmod");
    let mut bytes = std::fs::read(&pack).expect("read pack");
    bytes.truncate(bytes.len().saturating_sub(8));
    std::fs::write(&pack, &bytes).expect("truncate");

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.to_lowercase().contains("checksum")
            || git_text.contains("badPack")
            || git_text.contains("does not match index"),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(report.issues.iter().any(|i| {
        matches!(
            i,
            FsckIssue::PackChecksum { .. } | FsckIssue::IdxMismatch { .. }
        )
    }));
}

#[test]
fn dangling_commit_and_blob() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    let mut child = Command::new("git")
        .current_dir(dir.path())
        .args(["hash-object", "-w", "--stdin"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"dangling blob body\n");
    }
    child.wait().expect("wait");
    // Dangling commit
    let tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"])
        .trim()
        .to_string();
    let mut commit_cmd = Command::new("git")
        .current_dir(dir.path())
        .args(["commit-tree", &tree, "-m", "dangling"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("commit-tree");
    assert!(commit_cmd.status.success());

    let git_set = git_dangling_set(dir.path());
    let report = grit_fsck(
        &repo,
        &FsckOptions::full_repository(false).with_check_dangling(true),
    );
    let grit_set = grit_dangling_set(&report);
    assert_eq!(
        grit_set, git_set,
        "dangling mismatch git={git_set:?} grit={grit_set:?}"
    );
}

#[test]
fn gitlink_to_absent_commit_not_missing_object() {
    let (dir, repo) = init_repo();
    let head = one_commit(&repo);
    let absent = ObjectId::from_hex("1111111111111111111111111111111111111111").expect("hex");
    let mut tree_body = Vec::from(b"160000 sub\0" as &[u8]);
    tree_body.extend_from_slice(absent.as_bytes());
    let tree = repo
        .odb()
        .write(ObjectKind::Tree, &tree_body)
        .expect("tree");
    let commit_body = format!(
        "tree {}\nauthor T <t@example.com> 1 +0000\ncommitter T <t@example.com> 1 +0000\n\n",
        tree.to_hex()
    );
    let commit = repo
        .odb()
        .write(ObjectKind::Commit, commit_body.as_bytes())
        .expect("commit");
    let _ = head;
    git(
        dir.path(),
        &["update-ref", "refs/heads/main", &commit.to_hex()],
    );

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        !git_text.contains(&absent.to_hex()),
        "git must not report absent gitlink oid: {git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(
        !report
            .issues
            .iter()
            .any(|i| matches!(i, FsckIssue::MissingObject { oid, .. } if oid == &absent)),
        "gitlink must not require submodule commit in superproject ODB: {:?}",
        report.issues
    );
}

#[test]
fn commit_tree_wrong_kind_matches_git() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    let blob = repo
        .odb()
        .write(ObjectKind::Blob, b"not-a-tree")
        .expect("blob");
    let commit_body = format!(
        "tree {}\nauthor T <t@example.com> 1 +0000\ncommitter T <t@example.com> 1 +0000\n\n",
        blob.to_hex()
    );
    let bad_commit = repo
        .odb()
        .write(ObjectKind::Commit, commit_body.as_bytes())
        .expect("commit");
    git(
        dir.path(),
        &["update-ref", "refs/heads/main", &bad_commit.to_hex()],
    );

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.contains("not a tree") || git_text.contains("bad tree"),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(
        report.issues.iter().any(|i| {
            matches!(
                i,
                FsckIssue::BrokenLink { target, .. } if target == &blob
            ) || matches!(i, FsckIssue::BadObject { .. })
        }),
        "wrong tree kind must be reported: {:?}",
        report.issues
    );
}

#[test]
fn broken_link_from_tree() {
    let (dir, repo) = init_repo();
    let head = one_commit(&repo);
    let mut tree_body = Vec::from(b"100644 link\0" as &[u8]);
    tree_body.extend_from_slice(head.as_bytes());
    let new_tree = repo
        .odb()
        .write(ObjectKind::Tree, &tree_body)
        .expect("tree");
    let commit_body = format!(
        "tree {}\nauthor T <t@example.com> 1 +0000\ncommitter T <t@example.com> 1 +0000\n\n",
        new_tree.to_hex()
    );
    let bad_commit = repo
        .odb()
        .write(ObjectKind::Commit, commit_body.as_bytes())
        .expect("commit");
    git(
        dir.path(),
        &["update-ref", "refs/heads/main", &bad_commit.to_hex()],
    );

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.contains("brokenLink")
            || git_text.contains("missing")
            || git_text.to_lowercase().contains("broken link"),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(report.issues.iter().any(|i| matches!(
        i,
        FsckIssue::BrokenLink { .. } | FsckIssue::MissingObject { .. }
    )));
}

#[test]
fn bad_ref_name() {
    let (dir, repo) = init_repo();
    let tip = one_commit(&repo);
    std::fs::write(
        dir.path().join(".git/refs/heads/bad ref"),
        format!("{}\n", tip.to_hex()).as_bytes(),
    )
    .expect("write ref");

    let git_text = git(dir.path(), &["fsck"]);
    assert!(
        git_text.contains("badRefName")
            || git_text.contains("badRefContent")
            || git_text.contains("invalid sha1 pointer")
            || git_text.contains("bad ref"),
        "{git_text}"
    );
    let report = grit_fsck(&repo, &FsckOptions::full_repository(false));
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, FsckIssue::BadRef(_))));
}

#[test]
fn reflog_and_index_roots_are_not_dangling() {
    let (dir, repo) = init_repo();
    one_commit(&repo);
    git(dir.path(), &["commit", "--allow-empty", "-m", "e2"]);
    git(dir.path(), &["reset", "--hard", "HEAD~1"]);
    std::fs::write(repo.work_tree().expect("wt").join("idx-only"), b"z").expect("write");
    stage_for_commit(&repo, &mut NullProgress).expect("stage");

    let git_set = git_dangling_set(dir.path());
    let report = grit_fsck(
        &repo,
        &FsckOptions::full_repository(false).with_check_dangling(true),
    );
    let index = Index::load(&repo.git_dir().join("index")).expect("index");
    let staged = index
        .entries()
        .iter()
        .find(|e| e.path.ends_with(b"idx-only"))
        .expect("entry")
        .oid;
    assert!(
        !report.dangling.iter().any(|(_, oid)| *oid == staged),
        "index-only blob must not be dangling"
    );
    let reflog_out = git(dir.path(), &["rev-parse", "HEAD@{1}"])
        .trim()
        .to_string();
    if let Ok(reflog_oid) = ObjectId::from_hex(&reflog_out) {
        assert!(
            !report.dangling.iter().any(|(_, oid)| *oid == reflog_oid),
            "reflog commit must not be dangling"
        );
    }
    let grit_set = grit_dangling_set(&report);
    assert_eq!(grit_set, git_set);
}

use std::fs;
