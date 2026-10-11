//! Hermetic system-`git` helpers for history (rev-list, rev-parse, index) tests.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use grit_lib::objects::ObjectId;

use super::refs_harness::{git as refs_git, null_device_for_tests, AUTHOR_EMAIL, AUTHOR_NAME};

static COMMIT_TIME: AtomicU64 = AtomicU64::new(1_700_000_000);

fn next_commit_date() -> String {
    let secs = COMMIT_TIME.fetch_add(60, Ordering::SeqCst);
    format!("{secs} +0000")
}

fn hermetic_git(dir: &Path, args: &[&str]) -> std::process::Output {
    let date = next_commit_date();
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
        .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_CONFIG_GLOBAL", null_device_for_tests())
        .env("GIT_CONFIG_SYSTEM", null_device_for_tests())
        .output()
        .expect("spawn git")
}

/// Run `git` in `dir` with hermetic config and monotonically advancing author/committer dates.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = hermetic_git(dir, args);
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Parsed `(major, minor)` from `git --version`, or `None` if git is missing.
pub fn git_version() -> Option<(u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let out = Command::new("git").arg("--version").output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        // "git version 2.45.0"
        let token = text.split_whitespace().nth(2)?;
        let mut parts = token.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next().unwrap_or("0").parse().ok()?;
        Some((major, minor))
    })
}

/// Returns `true` when system git is at least `major.minor`.
#[must_use]
pub fn git_version_at_least(major: u32, minor: u32) -> bool {
    git_version().is_some_and(|(maj, min)| (maj, min) >= (major, minor))
}

/// Skip (return `false`) when system git is older than `major.minor`.
#[must_use]
pub fn skip_unless_git_at_least(major: u32, minor: u32) -> bool {
    if git_version_at_least(major, minor) {
        return true;
    }
    eprintln!("SKIP: system git >= {major}.{minor} required");
    false
}

/// Temporary repository initialized with system `git`.
pub struct HistoryRepo {
    _root: tempfile::TempDir,
    worktree: PathBuf,
}

impl HistoryRepo {
    /// Working tree path (contains `.git`).
    #[must_use]
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }

    /// Initialize a new repo under a temp directory (`main` as initial branch when supported).
    pub fn init() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let worktree = root.path().to_path_buf();
        git(&worktree, &["init", "-q"]);
        if git_version_at_least(2, 28) {
            let _ = hermetic_git(&worktree, &["branch", "-M", "main"]);
        }
        Self {
            _root: root,
            worktree,
        }
    }
}

/// Write `content` at `path`, stage, and commit with `msg`; returns the new commit oid.
pub fn commit_file(worktree: &Path, path: &str, content: &str, msg: &str) -> ObjectId {
    let full = worktree.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&full, content).expect("write");
    git(worktree, &["add", path]);
    git(worktree, &["commit", "-m", msg]);
    git(worktree, &["rev-parse", "HEAD"])
        .trim()
        .parse()
        .expect("HEAD oid")
}

/// Build a commit DAG from `edges` (`(child_label, parent_label)`); optional `tags` map labels to ref names.
///
/// Root commits have no incoming edge as a child whose parent is missing; labels only referenced as
/// parents are created as empty commits first.
pub fn build_commit_dag(
    worktree: &Path,
    edges: &[(&str, &str)],
    tags: &[(&str, &str)],
) -> HashMap<String, ObjectId> {
    let mut labels: HashSet<&str> = HashSet::new();
    for (child, parent) in edges {
        labels.insert(child);
        labels.insert(parent);
    }
    for (label, _) in tags {
        labels.insert(label);
    }

    let mut oids: HashMap<String, ObjectId> = HashMap::new();
    let mut remaining: HashSet<&str> = labels.iter().copied().collect();

    while !remaining.is_empty() {
        let ready: Vec<&str> = remaining
            .iter()
            .copied()
            .filter(|label| {
                let parents: Vec<&str> = edges
                    .iter()
                    .filter(|(child, _)| child == label)
                    .map(|(_, parent)| *parent)
                    .collect();
                parents.is_empty() || parents.iter().all(|p| oids.contains_key(*p))
            })
            .collect();
        if ready.is_empty() {
            panic!("commit DAG has unsatisfiable edges: {edges:?}");
        }
        for label in ready {
            remaining.remove(label);
            let parents: Vec<ObjectId> = edges
                .iter()
                .filter(|(child, _)| *child == label)
                .map(|(_, parent_label)| oids[*parent_label])
                .collect();
            let oid = commit_with_parents(worktree, label, &parents);
            oids.insert(label.to_owned(), oid);
        }
    }

    for (label, refname) in tags {
        let oid = oids
            .get(*label)
            .unwrap_or_else(|| panic!("unknown label {label} for tag"));
        git(worktree, &["update-ref", refname, &oid.to_hex()]);
    }

    oids
}

fn commit_with_parents(worktree: &Path, label: &str, parents: &[ObjectId]) -> ObjectId {
    let msg = format!("history harness {label}");
    let tree = git(worktree, &["mktree"]).trim().to_string();
    let mut args: Vec<String> = vec!["commit-tree".to_owned(), tree, "-m".to_owned(), msg];
    let parent_hex: Vec<String> = parents.iter().map(ObjectId::to_hex).collect();
    for hex in &parent_hex {
        args.push("-p".to_owned());
        args.push(hex.clone());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let commit = git(worktree, &arg_refs).trim().to_string();
    let refname = format!("refs/history/{label}");
    git(worktree, &["update-ref", &refname, &commit]);
    commit.parse().expect("commit oid")
}

/// `git rev-list` in `dir`, returning object ids in git's order (one oid per line).
pub fn git_rev_list(dir: &Path, args: &[&str]) -> Vec<ObjectId> {
    let mut full: Vec<&str> = vec!["rev-list"];
    full.extend_from_slice(args);
    let out = refs_git(dir, &full);
    out.lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// `git rev-parse` for `spec`, or `None` when git rejects the spec.
pub fn git_rev_parse(dir: &Path, spec: &str) -> Option<ObjectId> {
    let out = hermetic_git(dir, &["rev-parse", spec]);
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `git ls-files -s` as `(path, stage, mode, oid)` tuples.
pub fn git_ls_files_stage(dir: &Path) -> Vec<(String, u32, String, ObjectId)> {
    let out = git(dir, &["ls-files", "-s"]);
    let mut rows = Vec::new();
    for line in out.lines() {
        let mut parts = line.split_whitespace();
        let mode = parts.next().unwrap_or("").to_string();
        let oid: ObjectId = parts.next().unwrap_or("").parse().expect("oid");
        let stage: u32 = parts.next().unwrap_or("0").parse().expect("stage");
        let path = parts.collect::<Vec<_>>().join(" ");
        rows.push((path, stage, mode, oid));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    rows
}

/// Index stage map keyed by path (stage 0 only), for quick comparisons.
pub fn git_ls_files_stage0_map(dir: &Path) -> BTreeMap<String, ObjectId> {
    git_ls_files_stage(dir)
        .into_iter()
        .filter(|(_, stage, _, _)| *stage == 0)
        .map(|(path, _, _, oid)| (path, oid))
        .collect()
}
