//! `resolve` / `read_object_for_display` compatibility with system `git`.

mod support;

use grit_lib::object_inspect::{ObjectContents, ResolveError};
use grit_lib::objects::ObjectKind;

use support::{
    bare_repo_with_ambiguous_prefix, commit_file, git, git_rev_parse_verify, open_grit_repo,
    HistoryRepo,
};

fn git_cat_file_type(wt: &std::path::Path, spec: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .current_dir(wt)
        .args(["cat-file", "-t", spec])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn git_cat_file_size(wt: &std::path::Path, spec: &str) -> Option<u64> {
    let out = std::process::Command::new("git")
        .current_dir(wt)
        .args(["cat-file", "-s", spec])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    std::str::from_utf8(&out.stdout).ok()?.trim().parse().ok()
}

fn git_cat_file_p(wt: &std::path::Path, spec: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("git")
        .current_dir(wt)
        .args(["cat-file", "-p", spec])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(out.stdout)
}

fn setup_rich_fixture() -> HistoryRepo {
    let hist = HistoryRepo::init();
    let wt = hist.worktree();
    commit_file(wt, "README.md", "# hello\n", "initial");
    git(wt, &["tag", "v0.1"]);
    git(wt, &["tag", "-a", "v1.0", "-m", "release one"]);
    git(wt, &["checkout", "-b", "feature"]);
    commit_file(wt, "feature.txt", "feat\n", "on feature");
    git(wt, &["checkout", "main"]);
    commit_file(wt, "main.txt", "main\n", "on main");
    git(
        wt,
        &["merge", "--no-edit", "feature", "-m", "merge feature"],
    );
    commit_file(wt, "after-merge.txt", "done\n", "after merge");
    commit_file(wt, "pack-me.txt", "pack\n", "before repack");
    git(wt, &["repack", "-a", "-d"]);
    hist
}

#[test]
fn resolve_matches_git_rev_parse() {
    let hist = setup_rich_fixture();
    let wt = hist.worktree();
    let repo = open_grit_repo(wt);

    let specs = [
        "HEAD",
        "HEAD~1",
        "HEAD~2",
        "HEAD~3",
        "HEAD~3",
        "HEAD^",
        "HEAD^1",
        "HEAD~2^2",
        "main",
        "feature",
        "v0.1",
        "v1.0",
        "v1.0^{commit}",
        "v1.0^{tag}",
        "v1.0^{}",
        "HEAD^{tree}",
        "HEAD^{commit}",
        "HEAD:README.md",
        "HEAD:feature.txt",
        "main:after-merge.txt",
        "HEAD~1^{tree}",
        "refs/heads/main",
        "refs/tags/v1.0",
        "main~1",
        "main~2",
        "feature~0",
        "@{0}",
        "HEAD~0",
        "main^{commit}",
        "HEAD^{object}",
        "HEAD~4",
        "HEAD~1^{commit}",
        "feature^{}",
        "v0.1^{commit}",
        "HEAD:main.txt",
        "HEAD~2:README.md",
    ];

    for spec in specs {
        let git_oid =
            git_rev_parse_verify(wt, spec).unwrap_or_else(|| panic!("git rev-parse {spec}"));
        let resolved = repo
            .resolve(spec)
            .unwrap_or_else(|e| panic!("grit resolve {spec}: {e}"));
        assert_eq!(resolved.oid, git_oid, "oid mismatch for `{spec}`");
        let git_kind = git_cat_file_type(wt, &git_oid.to_hex())
            .unwrap_or_else(|| panic!("git cat-file -t {}", git_oid.to_hex()));
        assert_eq!(
            kind_str(resolved.kind),
            git_kind.as_str(),
            "kind mismatch for `{spec}`"
        );
    }
}

#[test]
fn cat_matches_git_cat_file() {
    let hist = setup_rich_fixture();
    let wt = hist.worktree();
    let repo = open_grit_repo(wt);

    let specs = [
        "HEAD",
        "HEAD^{tree}",
        "HEAD:README.md",
        "v1.0^{tag}",
        "v1.0^{commit}",
        "HEAD~2",
        "feature^{commit}",
    ];

    for spec in specs {
        let resolved = repo.resolve(spec).expect("resolve");
        let git_type = git_cat_file_type(wt, spec).expect("git -t");
        assert_eq!(kind_str(resolved.kind), git_type.as_str());
        let git_size = git_cat_file_size(wt, spec).expect("git -s");
        assert_eq!(resolved.size, git_size, "size for {spec}");

        let git_body = git_cat_file_p(wt, spec).expect("git -p");
        let obj = repo.odb().read(&resolved.oid).expect("read object");
        let _parsed = repo
            .read_object_for_display(&resolved.oid)
            .expect("read_object_for_display");

        match git_type.as_str() {
            "blob" => {
                assert!(matches!(_parsed, ObjectContents::Blob(_)));
                assert_eq!(obj.data, git_body, "blob body for {spec}");
            }
            "tree" => {
                assert!(matches!(_parsed, ObjectContents::Tree(_)));
                assert_eq!(resolved.kind, ObjectKind::Tree);
            }
            "commit" => {
                assert!(matches!(_parsed, ObjectContents::Commit(_)));
                assert_eq!(obj.data, git_body, "commit body for {spec}");
            }
            "tag" => {
                assert!(matches!(_parsed, ObjectContents::Tag(_)));
                assert_eq!(obj.data, git_body, "tag body for {spec}");
            }
            other => panic!("unexpected kind {other} for {spec}"),
        }
    }
}

#[test]
fn ambiguous_short_oid_is_typed_error() {
    let (_dir, bare_repo, prefix) = bare_repo_with_ambiguous_prefix(8_000);
    let err = bare_repo.resolve(&prefix).expect_err("ambiguous");
    match err {
        ResolveError::AmbiguousShortObject { spec, candidates } => {
            assert_eq!(spec, prefix);
            assert!(candidates.len() > 1, "expected multiple candidates");
        }
        other => panic!("expected AmbiguousShortObject, got {other:?}"),
    }
}

fn kind_str(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Blob => "blob",
        ObjectKind::Tree => "tree",
        ObjectKind::Commit => "commit",
        ObjectKind::Tag => "tag",
    }
}
