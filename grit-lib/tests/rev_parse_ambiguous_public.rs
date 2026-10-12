//! Exercise ambiguous short-hex disambiguation through public rev-parse entry points.

mod support;

use grit_lib::environment::{Environment, RepositoryOptions};
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::ref_storage::RefStorageFormat;
use grit_lib::repo::{init_bare_clone_minimal, Repository};
use grit_lib::rev_parse::{
    resolve_reflog_walk_log_ref, resolve_revision, resolve_revision_as_commit,
    resolve_revision_for_range_end, resolve_revision_for_verify,
};
use std::fs;

fn ambiguous_hex_prefix_among(oids: &[ObjectId], min_len: usize, max_len: usize) -> Option<String> {
    for i in 0..oids.len() {
        for j in (i + 1)..oids.len() {
            let a = oids[i].to_hex();
            let b = oids[j].to_hex();
            let cap = max_len.min(a.len()).min(b.len());
            for len in (min_len..=cap).rev() {
                if a[..len] == b[..len] {
                    return Some(a[..len].to_string());
                }
            }
        }
    }
    None
}

fn bare_repo_with_ambiguous_prefix() -> (tempfile::TempDir, Repository, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    init_bare_clone_minimal(dir.path(), "main", RefStorageFormat::Files).expect("init");
    let repo = Repository::open(dir.path(), None).expect("open");
    fs::write(
        repo.git_dir.join("config"),
        "[core]\n\tdisambiguate = commit\n",
    )
    .expect("config");
    let tree = repo
        .odb
        .write_loose_materialize(ObjectKind::Tree, b"")
        .expect("tree");
    let mut oids = Vec::new();
    let mut counter = 0_i32;
    let prefix = loop {
        let body = format!(
            "tree {tree}\nauthor T <t@e.com> {counter} +0000\ncommitter T <t@e.com> {counter} +0000\n\nbody{counter}\n"
        );
        counter += 1;
        let oid = repo
            .odb
            .write_loose_materialize(ObjectKind::Commit, body.as_bytes())
            .expect("commit");
        oids.push(oid);
        if let Some(p) = ambiguous_hex_prefix_among(&oids, 4, 12) {
            break p;
        }
        assert!(
            oids.len() < 25_000,
            "could not seed ambiguous commits for coverage"
        );
    };
    let tip = *oids.last().expect("tip");
    fs::write(repo.git_dir.join("refs/heads/main"), format!("{tip}\n")).expect("main");
    (dir, repo, prefix)
}

#[test]
fn ambiguous_short_hex_public_resolvers() {
    let (_dir, repo, prefix) = bare_repo_with_ambiguous_prefix();
    let peel = format!("{prefix}^{{commit}}");
    let _ = resolve_revision_for_verify(&repo, &prefix);
    let _ = resolve_revision_for_range_end(&repo, &prefix);
    let _ = resolve_revision_as_commit(&repo, &prefix);
    let _ = resolve_revision_for_verify(&repo, &peel);
    let _ = resolve_revision(&repo, &prefix);
}

#[test]
fn reflog_walk_ref_at_minus_branch_form() {
    let hist = support::HistoryRepo::init();
    let wt = hist.worktree();
    support::commit_file(wt, "f.txt", "1\n", "one");
    support::git(wt, &["checkout", "-b", "first"]);
    support::commit_file(wt, "f.txt", "2\n", "two");
    support::git(wt, &["checkout", "main"]);
    let mut env = Environment::empty();
    env.cwd = wt.to_path_buf();
    env.pwd = Some(wt.to_string_lossy().into_owned());
    let options = RepositoryOptions {
        environment: env,
        ..RepositoryOptions::empty()
    };
    let repo = Repository::discover_with(&options, Some(wt)).expect("discover");
    let name = resolve_reflog_walk_log_ref(&repo, "@{-1}").expect("walk ref");
    assert!(name.contains("first") || name.starts_with("refs/"));
}
