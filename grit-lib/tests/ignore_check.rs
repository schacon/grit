//! `Repository::ignore_check` vs `git check-ignore -v --non-matching`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "ignore_t0008_common.rs"]
mod t0008;

use grit_lib::index::Index;
use grit_lib::repo::Repository;
use grit_lib::worktree::ignore::{IgnoreCheck, IgnoreSource};
use t0008::{assert_match_eq, git_check_ignore_verbose_batch, t0008_corpus, T0008Repo};

fn grit_ignore_check(repo: &Repository, index: &Index, paths: &[String]) -> Vec<IgnoreCheck> {
    repo.ignore_check(paths, index).expect("ignore_check")
}

#[test]
fn matches_git_check_ignore_verbose() {
    let fx = T0008Repo::setup();
    let repo = fx.open_grit();
    let index = repo.index_handle().load().expect("index");
    let corpus = t0008_corpus();
    let paths: Vec<String> = corpus.iter().map(|(p, _)| (*p).to_owned()).collect();
    let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();

    let git_map = git_check_ignore_verbose_batch(&fx.home, &fx.root, &path_refs);
    let grit_results = grit_ignore_check(&repo, &index, &paths);
    assert_eq!(grit_results.len(), paths.len());

    for (path, _) in corpus {
        let git = git_map
            .get(path)
            .unwrap_or_else(|| panic!("git missing {path}"));
        let grit = grit_results
            .iter()
            .find(|c| c.path == path)
            .unwrap_or_else(|| panic!("grit missing {path}"));
        assert_match_eq(path, git, grit.ignored, &grit.matched);
    }
}

#[test]
fn tracked_paths_never_ignored() {
    let fx = T0008Repo::setup();
    let repo = fx.open_grit();
    let index = repo.index_handle().load().expect("index");
    for path in ["ignored-but-in-index", "a/ignored-but-in-index"] {
        let results = grit_ignore_check(&repo, &index, &[path.to_owned()]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].ignored, "{path} is tracked");
        assert!(results[0].matched.is_none());
    }
}

#[test]
fn ignore_source_is_typed() {
    let fx = T0008Repo::setup();
    let repo = fx.open_grit();
    let index = repo.index_handle().load().expect("index");
    let results = grit_ignore_check(
        &repo,
        &index,
        &[
            "one".into(),
            "per-repo".into(),
            "globalone".into(),
            "a/two".into(),
        ],
    );
    let one = results.iter().find(|c| c.path == "one").unwrap();
    assert!(matches!(
        one.matched.as_ref().map(|m| &m.source),
        Some(IgnoreSource::PerDirectory(p)) if p == ".gitignore"
    ));
    let per_repo = results.iter().find(|c| c.path == "per-repo").unwrap();
    assert!(matches!(
        per_repo.matched.as_ref().map(|m| &m.source),
        Some(IgnoreSource::InfoExclude)
    ));
    let global = results.iter().find(|c| c.path == "globalone").unwrap();
    assert!(matches!(
        global.matched.as_ref().map(|m| &m.source),
        Some(IgnoreSource::CoreExcludesFile(_))
    ));
    let nested = results.iter().find(|c| c.path == "a/two").unwrap();
    assert!(matches!(
        nested.matched.as_ref().map(|m| &m.source),
        Some(IgnoreSource::PerDirectory(p)) if p == "a/.gitignore"
    ));
}
