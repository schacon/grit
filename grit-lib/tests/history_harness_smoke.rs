//! Smoke test for [`support::history_harness`].

mod support;

use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, RevListOptions};

use support::{build_commit_dag, git_rev_list, HistoryRepo};

#[test]
fn history_harness_smoke_three_commit_dag_matches_grit_rev_list() {
    let repo = HistoryRepo::init();
    let wt = repo.worktree();
    build_commit_dag(
        wt,
        &[("b", "a"), ("c", "b")],
        &[("c", "refs/heads/main")],
    );

    let git_oids = git_rev_list(wt, &["--reverse", "main"]);
    assert_eq!(git_oids.len(), 3, "expected three commits on main");

    let grit_repo = Repository::discover(Some(wt)).expect("discover");
    let opts = RevListOptions {
        reverse: true,
        ..RevListOptions::default()
    };
    let grit_result = rev_list(&grit_repo, &[String::from("main")], &[], &opts).expect("rev-list");
    let grit_oids = grit_result.commits;

    assert_eq!(
        grit_oids, git_oids,
        "grit rev-list should match git rev-list --reverse on a linear DAG"
    );
}
