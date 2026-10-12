//! Exercise rev-parse resolution paths (success and failure) for line coverage.

mod support;

use grit_lib::environment::{Environment, RepositoryOptions};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    resolve_revision, resolve_revision_as_commit, resolve_revision_for_checkout_guess,
    resolve_revision_for_range_end, resolve_revision_for_verify,
    resolve_revision_without_index_dwim,
};

use support::{build_commit_dag, commit_file, HistoryRepo};

fn open_grit(wt: &std::path::Path) -> Repository {
    let mut env = Environment::empty();
    env.cwd = wt.to_path_buf();
    env.pwd = Some(wt.to_string_lossy().into_owned());
    let options = RepositoryOptions {
        environment: env,
        ..RepositoryOptions::empty()
    };
    Repository::discover(Some(wt), &options).expect("discover")
}

#[test]
fn exercise_many_specs_on_deep_history() {
    let hist = HistoryRepo::init();
    let wt = hist.worktree();
    let mut edge_labels: Vec<(String, String)> = Vec::new();
    for i in 1..=100 {
        let child = format!("c{i}");
        let parent = if i == 1 {
            "root".to_string()
        } else {
            format!("c{}", i - 1)
        };
        edge_labels.push((child, parent));
    }
    let edges: Vec<(&str, &str)> = edge_labels
        .iter()
        .map(|(c, p)| (c.as_str(), p.as_str()))
        .collect();
    build_commit_dag(wt, &edges, &[("c100", "refs/heads/main")]);
    let repo = open_grit(wt);
    for n in 0..100 {
        let spec = format!("HEAD~{n}");
        let _ = resolve_revision_for_verify(&repo, &spec);
        let _ = resolve_revision_without_index_dwim(&repo, &spec);
        let _ = resolve_revision_for_range_end(&repo, &spec);
        let _ = resolve_revision_as_commit(&repo, &spec);
        let _ = resolve_revision_for_checkout_guess(&repo, &spec, true);
        let _ = resolve_revision_for_checkout_guess(&repo, &spec, false);
    }
    for n in 1..=3 {
        let spec = format!("main^^{n}");
        let _ = resolve_revision_for_verify(&repo, &spec);
    }
    for spec in [
        "main^!",
        "main^@",
        "HEAD^{commit}",
        "HEAD^{tree}",
        "HEAD^{/history harness c100}",
        ":/harness",
        "main@{0}",
        "main@{1}",
        "main@{upstream}",
        "main@{u}",
        "main@{push}",
        "@",
        "@{0}",
    ] {
        let _ = resolve_revision_for_verify(&repo, spec);
        let _ = resolve_revision(&repo, spec);
    }
}

#[test]
fn exercise_failure_specs_do_not_panic() {
    let hist = HistoryRepo::init();
    let wt = hist.worktree();
    commit_file(wt, "f", "1\n", "one");
    let repo = open_grit(wt);
    for spec in [
        "not-a-ref-xyz",
        "HEAD~999",
        "deadbeef",
        "main^{tag}",
        "main:missing/path",
        ":missing",
        "HEAD^9",
        "main..",
        "...main",
        "@{-99}",
        "main@{upstream}@{upstream}",
    ] {
        let _ = resolve_revision_for_verify(&repo, spec);
        let _ = resolve_revision(&repo, spec);
    }
}
