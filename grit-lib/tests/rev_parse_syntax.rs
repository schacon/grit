//! Rev-parse syntax oracle tests (upstream t1500/t1503/t1505–t1514/t6101 subsets).

mod support;

use std::fs;
use std::path::Path;

use grit_lib::environment::{Environment, RepositoryOptions};
use grit_lib::error::Error;
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    resolve_revision, resolve_revision_for_verify, resolve_upstream_symbolic_name,
    split_double_dot_range, split_triple_dot_range,
};
use grit_lib::rev_parse_error::RevParseError;

use support::{
    build_commit_dag, commit_file, git, git_rev_parse_verify, oracle_abbrev,
    oracle_double_dot_range, oracle_one, oracle_table,
    oracle_triple_dot_range, HistoryRepo, ResolveMode,
};

fn open_grit(wt: &Path) -> Repository {
    let mut env = Environment::empty();
    env.cwd = wt.to_path_buf();
    env.pwd = Some(wt.to_string_lossy().into_owned());
    let options = RepositoryOptions {
        environment: env,
        ..RepositoryOptions::empty()
    };
    Repository::discover_with(&options, Some(wt)).expect("discover grit repo")
}

fn setup_linear_main() -> HistoryRepo {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    build_commit_dag(wt, &[("b", "a"), ("c", "b")], &[("c", "refs/heads/main")]);
    repo
}

fn setup_t1506_worktree() -> HistoryRepo {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    commit_file(wt, "file.txt", "one\n", "init");
    fs::create_dir_all(wt.join("subdir")).expect("mkdir");
    commit_file(wt, "subdir/file.txt", "two\n", "subdir");
    commit_file(wt, "subdir/file2.txt", "three\n", "file2");
    fs::write(wt.join("index-only.txt"), "four\n").expect("write");
    git(wt, &["add", "index-only.txt"]);
    fs::write(wt.join("disk-only.txt"), "five\n").expect("write");
    repo
}

fn setup_annotated_tags() -> (HistoryRepo, ObjectId) {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    fs::write(wt.join("a-blob"), "blob\n").expect("write");
    let blob = git(wt, &["hash-object", "-w", "a-blob"]).trim().to_string();
    git(wt, &["tag", "-a", "-m", "blob", "blob-tag", &blob]);
    fs::create_dir_all(wt.join("a-tree")).expect("mkdir");
    fs::write(wt.join("a-tree/another-blob"), "more\n").expect("write");
    git(wt, &["add", "."]);
    let tree = git(wt, &["write-tree"]).trim().to_string();
    git(wt, &["tag", "-a", "-m", "tree", "tree-tag", &tree]);
    commit_file(wt, "a-blob", "blob\n", "Initial");
    let tip = git_rev_parse_verify(wt, "HEAD").expect("HEAD");
    git(wt, &["tag", "-a", "-m", "commit", "commit-tag"]);
    git(wt, &["branch", "ref"]);
    commit_file(wt, "a-blob", "modified\n", "Modified");
    git(wt, &["branch", "modref"]);
    (repo, tip)
}

fn setup_upstream_remote() -> HistoryRepo {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    commit_file(wt, "f", "1\n", "c1");
    let bare = tempfile::tempdir().expect("bare temp");
    git(bare.path(), &["init", "--bare", "-q"]);
    git(
        wt,
        &[
            "remote",
            "add",
            "origin",
            bare.path().to_str().expect("bare path utf8"),
        ],
    );
    git(wt, &["push", "-u", "origin", "main"]);
    git(wt, &["config", "branch.main.remote", "origin"]);
    git(wt, &["config", "branch.main.merge", "refs/heads/main"]);
    repo
}

#[test]
fn t1500_core_specs_match_git_rev_parse() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    let specs = [
        "HEAD",
        "main",
        "refs/heads/main",
        "main^",
        "main~1",
        "main~2",
        "HEAD~0",
        "HEAD^0",
        "HEAD^1",
        "@",
        "@^{commit}",
        "main^{commit}",
        "main^{}",
    ];
    oracle_table(&repo, wt, ResolveMode::Verify, &specs);
}

#[test]
fn t1503_verify_rejects_missing_ref() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    assert!(git_rev_parse_verify(wt, "no-such-ref").is_none());
    assert!(resolve_revision_for_verify(&repo, "no-such-ref").is_err());
}

#[test]
fn t1505_rev_parse_last_at_minus_n_matches_git() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    for spec in ["@{-1}", "@{-0}"] {
        oracle_one(&repo, wt, spec, ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn t1506_ambiguous_path_vs_rev_typed_errors() {
    let hist = setup_t1506_worktree();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    let ok_specs = [
        "HEAD:file.txt",
        "HEAD:subdir/file.txt",
        ":index-only.txt",
        ":0:file.txt",
        "HEAD:./file.txt",
    ];
    oracle_table(&repo, wt, ResolveMode::Verify, &ok_specs);
    for spec in ["file.txt", "subdir/file.txt"] {
        assert!(
            git_rev_parse_verify(wt, spec).is_none(),
            "git --verify should reject ambiguous path {spec}"
        );
        match resolve_revision(&repo, spec) {
            Err(Error::RevParse(RevParseError::AmbiguousArgument { .. })) => {}
            other => panic!("expected AmbiguousArgument for {spec}, got {other:?}"),
        }
    }
    assert!(git_rev_parse_verify(wt, "missing:path").is_none());
    match resolve_revision_for_verify(&repo, "missing:path") {
        Err(Error::RevParse(
            RevParseError::PathNotInRevision { .. } | RevParseError::InvalidObjectName { .. },
        )) => {}
        other => panic!("expected path/rev failure for missing:path, got {other:?}"),
    }
}

#[test]
fn t1507_upstream_and_push_shorthand_match_git() {
    let hist = setup_upstream_remote();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    for spec in [
        "main@{upstream}",
        "main@{u}",
        "@{upstream}",
        "@{u}",
        "main@{push}",
        "@{push}",
    ] {
        oracle_one(&repo, wt, spec, ResolveMode::Verify).unwrap_or_else(|e| panic!("{spec}: {e}"));
    }
    let sym = resolve_upstream_symbolic_name(&repo, "main@{u}").expect("upstream sym");
    assert!(sym.contains("refs/remotes/origin"));
}

#[test]
fn t1508_at_combinations_match_git() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    let specs = ["@{0}", "main@{0}", "HEAD@{0}"];
    oracle_table(&repo, wt, ResolveMode::Verify, &specs);
}

#[test]
fn t1511_caret_peel_forms_match_git() {
    let (hist, _tip) = setup_annotated_tags();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    let specs = [
        "ref^{}",
        "ref^{commit}",
        "commit-tag^{commit}",
        "ref^{tree}",
        "tree-tag^{tree}",
        "HEAD^{tree}",
        "HEAD^{/Initial}",
        "HEAD^!",
        "HEAD^@",
        "HEAD^-1",
    ];
    for spec in specs {
        oracle_one(&repo, wt, spec, ResolveMode::Verify).unwrap_or_else(|e| panic!("{spec}: {e}"));
    }
}

fn git_hash_stdin(wt: &Path, bytes: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("git")
        .current_dir(wt)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn hash-object");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(bytes)
        .expect("write stdin");
    let out = child.wait_with_output().expect("hash-object");
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn t1512_disambiguation_matches_git() {
    let hist = HistoryRepo::init();
    let wt = hist.worktree();
    let _blob1 = git_hash_stdin(wt, b"aaa\n");
    let _blob2 = git_hash_stdin(wt, b"bbb\n");
    commit_file(wt, "f", "x\n", "c");
    let repo = open_grit(wt);
    let full = git_rev_parse_verify(wt, "HEAD").expect("HEAD");
    let prefix: String = full.to_hex().chars().take(7).collect();
    if git_rev_parse_verify(wt, &prefix).is_some() {
        oracle_one(&repo, wt, &prefix, ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
        oracle_abbrev(&repo, wt, full, 7);
    }
    git(wt, &["config", "core.disambiguate", "commit"]);
    if git_rev_parse_verify(wt, &prefix).is_some() {
        oracle_one(&repo, wt, &prefix, ResolveMode::AsCommit).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn t1514_rev_parse_push_default_matches_git() {
    let hist = setup_upstream_remote();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    git(wt, &["config", "push.default", "simple"]);
    oracle_one(&repo, wt, "@{push}", ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn t6101_rev_parse_parents_match_git() {
    let hist = HistoryRepo::init();
    let wt = hist.worktree();
    build_commit_dag(
        wt,
        &[("b", "a"), ("c", "b"), ("m", "b"), ("m", "c")],
        &[("m", "refs/heads/main")],
    );
    let repo = open_grit(wt);
    for n in 1..=2 {
        let spec = format!("main^^{n}");
        oracle_one(&repo, wt, &spec, ResolveMode::Verify).unwrap_or_else(|e| panic!("{spec}: {e}"));
    }
    oracle_one(&repo, wt, "main^1", ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
    oracle_one(&repo, wt, "main^2", ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn colon_message_search_matches_git() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    oracle_one(&repo, wt, ":/history harness c", ResolveMode::Verify)
        .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn colon_path_and_index_stage_match_git() {
    let hist = setup_t1506_worktree();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    oracle_one(&repo, wt, ":file.txt", ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
    oracle_one(&repo, wt, ":0:file.txt", ResolveMode::Verify).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn rev_colon_path_treeish_blob_matches_git() {
    let hist = setup_t1506_worktree();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    oracle_one(&repo, wt, "HEAD:file.txt", ResolveMode::TreeishBlobAtPath)
        .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn reflog_at_n_and_date_match_git() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    for spec in ["main@{0}", "main@{1}"] {
        oracle_one(&repo, wt, spec, ResolveMode::Verify).unwrap_or_else(|e| panic!("{spec}: {e}"));
    }
}

#[test]
fn split_range_helpers_match_git_endpoints() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    git(wt, &["branch", "feature", "main~1"]);
    let repo = open_grit(wt);
    oracle_double_dot_range(&repo, wt, "main..feature");
    oracle_triple_dot_range(&repo, wt, "main...feature");
    assert!(split_double_dot_range("main..feature").is_some());
    assert!(split_triple_dot_range("main...feature").is_some());
}

#[test]
fn resolve_revision_and_as_commit_agree_with_git_on_main() {
    let hist = setup_linear_main();
    let wt = hist.worktree();
    let repo = open_grit(wt);
    for spec in ["main", "HEAD~1"] {
        oracle_one(&repo, wt, spec, ResolveMode::Revision).unwrap_or_else(|e| panic!("{e}"));
        oracle_one(&repo, wt, spec, ResolveMode::AsCommit).unwrap_or_else(|e| panic!("{e}"));
    }
}
