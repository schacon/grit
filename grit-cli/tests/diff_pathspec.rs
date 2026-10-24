//! Path limits on commit, range, and staged diffs (`grit diff A B -- path`).

use std::process::Command;

fn grit(dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_grit"))
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test User")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test User")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .args(args)
        .output()
        .expect("run grit");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn init_repo_with_two_files(dir: &std::path::Path) {
    assert_eq!(grit(dir, &["init"]).0, 0);
    assert_eq!(grit(dir, &["config", "user.name", "Test"]).0, 0);
    assert_eq!(grit(dir, &["config", "user.email", "t@example.com"]).0, 0);
    std::fs::write(dir.join("a.txt"), "base a\n").unwrap();
    std::fs::write(dir.join("b.txt"), "base b\n").unwrap();
    assert_eq!(grit(dir, &["add", "a.txt", "b.txt"]).0, 0);
    assert_eq!(grit(dir, &["commit", "-m", "initial"]).0, 0);
    assert_eq!(grit(dir, &["switch", "-c", "feature/v1.2/x"]).0, 0);
    std::fs::write(dir.join("a.txt"), "feature a\n").unwrap();
    std::fs::write(dir.join("b.txt"), "feature b\n").unwrap();
    assert_eq!(grit(dir, &["add", "a.txt", "b.txt"]).0, 0);
    assert_eq!(grit(dir, &["commit", "-m", "feature"]).0, 0);
}

#[test]
fn diff_two_refs_with_double_dash_path_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());

    let (code, stdout, stderr) = grit(
        dir.path(),
        &["diff", "main", "feature/v1.2/x", "--", "a.txt"],
    );
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("a.txt"), "stdout={stdout}");
    assert!(!stdout.contains("b.txt"), "stdout={stdout}");
}

#[test]
fn diff_range_with_trailing_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());

    let (code, stdout, stderr) = grit(dir.path(), &["diff", "main..feature/v1.2/x", "a.txt"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("a.txt"), "stdout={stdout}");
    assert!(!stdout.contains("b.txt"), "stdout={stdout}");
}

#[test]
fn diff_range_with_pathspec_from_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());
    let list = dir.path().join("paths.txt");
    std::fs::write(&list, b"a.txt\n").unwrap();

    let (code, stdout, stderr) = grit(
        dir.path(),
        &[
            "diff",
            "--name-only",
            "main..feature/v1.2/x",
            "--pathspec-from-file",
            list.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.lines().any(|l| l == "a.txt"), "stdout={stdout}");
    assert!(!stdout.lines().any(|l| l == "b.txt"), "stdout={stdout}");
}

#[test]
fn diff_staged_with_path_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());
    assert_eq!(grit(dir.path(), &["switch", "main"]).0, 0);
    std::fs::write(dir.path().join("a.txt"), "staged a\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "staged b\n").unwrap();
    assert_eq!(grit(dir.path(), &["add", "a.txt", "b.txt"]).0, 0);

    let (code, stdout, stderr) = grit(dir.path(), &["diff", "--staged", "--", "a.txt"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("a.txt"), "stdout={stdout}");
    assert!(!stdout.contains("b.txt"), "stdout={stdout}");
}

#[test]
fn diff_two_refs_json_path_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());

    let (code, stdout, stderr) = grit(
        dir.path(),
        &["diff", "--json", "main", "feature/v1.2/x", "--", "a.txt"],
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let paths: Vec<_> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["path"].as_str())
        .collect();
    assert_eq!(paths, ["a.txt"]);
}

fn git(dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(args)
        .output()
        .expect("run git");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn init_repo_with_deletion_on_feature(dir: &std::path::Path) {
    assert_eq!(grit(dir, &["init"]).0, 0);
    assert_eq!(grit(dir, &["config", "user.name", "Test"]).0, 0);
    assert_eq!(grit(dir, &["config", "user.email", "t@example.com"]).0, 0);
    std::fs::write(dir.join("old.txt"), "gone soon\n").unwrap();
    std::fs::write(dir.join("keep.txt"), "stay\n").unwrap();
    assert_eq!(grit(dir, &["add", "old.txt", "keep.txt"]).0, 0);
    assert_eq!(grit(dir, &["commit", "-m", "main"]).0, 0);
    assert_eq!(grit(dir, &["switch", "-c", "feature"]).0, 0);
    std::fs::remove_file(dir.join("old.txt")).unwrap();
    std::fs::write(dir.join("keep.txt"), "changed\n").unwrap();
    assert_eq!(git(dir, &["add", "-A"]).0, 0);
    assert_eq!(grit(dir, &["commit", "-m", "delete old"]).0, 0);
}

#[test]
fn diff_two_refs_deleted_path_not_in_current_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_deletion_on_feature(dir.path());

    let (g_code, g_out, g_err) = git(dir.path(), &["diff", "main", "feature", "--", "old.txt"]);
    assert_eq!(g_code, 0, "git stderr={g_err}");
    assert!(g_out.contains("old.txt"), "git stdout={g_out}");

    let (code, stdout, stderr) = grit(dir.path(), &["diff", "main", "feature", "--", "old.txt"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("old.txt"), "stdout={stdout}");
}

#[test]
fn diff_two_refs_pathspec_exclude_magic() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());

    let (g_code, _, g_err) = git(
        dir.path(),
        &[
            "diff",
            "main",
            "feature/v1.2/x",
            "--",
            ".",
            ":(exclude)a.txt",
        ],
    );
    assert_eq!(g_code, 0, "git stderr={g_err}");

    let (code, stdout, stderr) = grit(
        dir.path(),
        &[
            "diff",
            "main",
            "feature/v1.2/x",
            "--",
            ".",
            ":(exclude)a.txt",
        ],
    );
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("b.txt"), "stdout={stdout}");
    assert!(!stdout.contains("a.txt"), "stdout={stdout}");
}

#[test]
fn diff_two_refs_bare_repository() {
    let src = tempfile::tempdir().expect("tempdir");
    init_repo_with_deletion_on_feature(src.path());

    let bare = tempfile::tempdir().expect("bare tempdir");
    assert_eq!(
        git(
            src.path(),
            &[
                "clone",
                "--bare",
                src.path().to_str().unwrap(),
                bare.path().join("repo.git").to_str().unwrap(),
            ],
        )
        .0,
        0
    );
    let bare_root = bare.path().join("repo.git");

    let (g_code, g_out, g_err) = git(&bare_root, &["diff", "main", "feature", "--", "old.txt"]);
    assert_eq!(g_code, 0, "git stderr={g_err}");
    assert!(g_out.contains("old.txt"), "git stdout={g_out}");

    let (code, stdout, stderr) = grit(&bare_root, &["diff", "main", "feature", "--", "old.txt"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("old.txt"), "stdout={stdout}");
}

fn json_paths(stdout: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(stdout).expect("json");
    v["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["path"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn diff_json_attr_pathspec_matches_git() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    assert_eq!(git(root, &["init", "-q", "-b", "main"]).0, 0);
    std::fs::create_dir_all(root.join("src/sub")).unwrap();
    std::fs::write(
        root.join(".gitattributes"),
        "src/**/*.rs diff=rust linguist-generated\n",
    )
    .unwrap();
    std::fs::write(root.join("src/sub/a.rs"), "a\n").unwrap();
    std::fs::write(root.join("README.md"), "r\n").unwrap();
    assert_eq!(git(root, &["add", "."]).0, 0);
    let (code, _, err) = git(
        root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    assert_eq!(code, 0, "git commit: {err}");
    std::fs::write(root.join("src/sub/a.rs"), "a2\n").unwrap();
    std::fs::write(root.join("README.md"), "r2\n").unwrap();

    for (spec, expected) in [
        (":(attr:diff=rust)", vec!["src/sub/a.rs"]),
        (":(exclude,attr:diff=rust)", vec!["README.md"]),
    ] {
        let (g_code, g_out, g_err) = git(root, &["diff", "--name-only", "--", spec]);
        assert_eq!(g_code, 0, "git stderr={g_err}");
        assert_eq!(g_out.lines().collect::<Vec<_>>(), expected, "git {spec}");

        let (code, stdout, stderr) = grit(root, &["diff", "--json", "--", spec]);
        assert_eq!(code, 0, "stderr={stderr}");
        assert_eq!(json_paths(&stdout), expected, "grit {spec}");
    }
}

fn init_committed_repo(dir: &std::path::Path, files: &[(&str, &str)], config: &[(&str, &str)]) {
    assert_eq!(git(dir, &["init", "-q", "-b", "main"]).0, 0);
    for (key, value) in config {
        assert_eq!(git(dir, &["config", key, value]).0, 0);
    }
    for (path, content) in files {
        std::fs::write(dir.join(path), content).unwrap();
    }
    assert_eq!(git(dir, &["add", "."]).0, 0);
    let (code, _, err) = git(
        dir,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e.com",
            "commit",
            "-qm",
            "init",
        ],
    );
    assert_eq!(code, 0, "git commit: {err}");
}

#[test]
fn diff_json_attr_pathspec_macros_and_ignorecase_match_git() {
    let cases: [(&[(&str, &str)], &[(&str, &str)], &str, &str); 2] = [
        (
            &[
                (".gitattributes", "[attr]rusty diff=rust\n*.rs rusty\n"),
                ("macro.rs", "m\n"),
                ("plain.txt", "p\n"),
            ],
            &[],
            ":(attr:diff=rust)",
            "macro.rs",
        ),
        (
            &[
                (".gitattributes", "*.UPPER diff=upper\n"),
                ("case.upper", "c\n"),
                ("plain.txt", "p\n"),
            ],
            &[("core.ignorecase", "true")],
            ":(attr:diff=upper)",
            "case.upper",
        ),
    ];
    for (files, config, spec, selected) in cases {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        init_committed_repo(root, files, config);
        for (path, _) in files.iter().filter(|(p, _)| !p.starts_with('.')) {
            std::fs::write(root.join(path), "changed\n").unwrap();
        }

        let (g_code, g_out, g_err) = git(root, &["diff", "HEAD", "--name-only", "--", spec]);
        assert_eq!(g_code, 0, "git stderr={g_err}");
        assert_eq!(g_out.lines().collect::<Vec<_>>(), [selected], "git {spec}");

        let (code, stdout, stderr) = grit(root, &["diff", "--json", "--", spec]);
        assert_eq!(code, 0, "{spec}: stdout={stdout} stderr={stderr}");
        assert_eq!(json_paths(&stdout), [selected], "grit {spec}");
    }
}

#[test]
fn diff_rejects_empty_attr_pathspec_like_git() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_committed_repo(root, &[("a.txt", "a\n")], &[]);
    std::fs::write(root.join("a.txt"), "changed\n").unwrap();

    let (g_code, _, _) = git(root, &["diff", "--name-only", "--", ":(attr:)"]);
    assert_eq!(g_code, 128, "git rejects an empty attr clause");

    let (code, stdout, stderr) = grit(root, &["diff", "--json", "--", ":(attr:)"]);
    assert_ne!(code, 0, "stdout={stdout} stderr={stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let error = v["error"].as_str().expect("error message");
    assert!(error.contains(":(attr:)"), "error={error}");
    assert!(v.get("files").is_none(), "stdout={stdout}");
}

#[test]
fn diff_revision_then_bare_path_without_double_dash() {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_with_two_files(dir.path());

    let (code, stdout, stderr) = grit(dir.path(), &["diff", "feature/v1.2/x", "a.txt"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("a.txt"), "stdout={stdout}");
}
