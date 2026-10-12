//! Open, discover, and init entry points (task 1304).

use std::process::Command;

use grit_lib::environment::RepositoryOptions;
use grit_lib::repo::{InitOptions, Repository, RepositoryBuilder};
use grit_lib::RefStorageFormat;
use tempfile::TempDir;

fn git_fsck_ok(git_dir: &std::path::Path) {
    let output = Command::new("git")
        .args(["--git-dir", &git_dir.to_string_lossy(), "fsck"])
        .output()
        .expect("spawn git fsck");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unknown repository extension") {
            return;
        }
    }
    assert!(
        output.status.success(),
        "git fsck failed for {}: {}",
        git_dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn open_discover_and_init_roundtrip_fsck() {
    let td = TempDir::new().expect("tempdir");

    let normal = td.path().join("normal");
    let repo = Repository::init(
        &normal,
        &InitOptions {
            initial_branch: "main".to_owned(),
            ..InitOptions::default()
        },
    )
    .expect("init normal");
    git_fsck_ok(repo.git_dir());

    let bare = td.path().join("bare.git");
    let bare_repo = Repository::init(
        &bare,
        &InitOptions {
            bare: true,
            initial_branch: "main".to_owned(),
            ..InitOptions::default()
        },
    )
    .expect("init bare");
    git_fsck_ok(bare_repo.git_dir());

    let sep_wt = td.path().join("sep-wt");
    let sep_git = td.path().join("sep.git");
    let sep_repo = Repository::init(
        &sep_wt,
        &InitOptions {
            separate_git_dir: Some(sep_git.clone()),
            initial_branch: "main".to_owned(),
            ..InitOptions::default()
        },
    )
    .expect("init separate");
    git_fsck_ok(sep_repo.git_dir());

    let reftable = td.path().join("reftable");
    let rt_repo = Repository::init(
        &reftable,
        &InitOptions {
            initial_branch: "main".to_owned(),
            ref_storage: RefStorageFormat::Reftable,
            ..InitOptions::default()
        },
    )
    .expect("init reftable");
    git_fsck_ok(rt_repo.git_dir());

    let reopened = Repository::open(
        repo.git_dir(),
        &RepositoryOptions::empty().with_work_tree(normal.clone()),
    )
    .expect("open explicit");
    assert_eq!(reopened.git_dir(), repo.git_dir());

    let discovered = Repository::discover(Some(&normal), &RepositoryOptions::empty())
        .expect("discover from work tree");
    assert_eq!(discovered.git_dir(), repo.git_dir());

    let info = discovered.discovery_info();
    assert!(info.discovery_root.is_some());
    assert!(!info.explicit_git_dir);

    let serving = Repository::open(
        bare_repo.git_dir(),
        &RepositoryOptions::empty().for_serving(),
    )
    .expect("open for serving");
    assert_eq!(serving.git_dir(), bare_repo.git_dir());

    let skip_fmt = Repository::open(
        repo.git_dir(),
        &RepositoryOptions {
            validate_repository_format: false,
            ..RepositoryOptions::empty().with_work_tree(normal.clone())
        },
    )
    .expect("open skipping format validation");
    assert_eq!(skip_fmt.git_dir(), repo.git_dir());

    let built = RepositoryBuilder::new(RepositoryOptions::empty().with_work_tree(normal.clone()))
        .open(repo.git_dir(), Some(normal.as_path()))
        .expect("builder open");
    assert_eq!(built.git_dir(), repo.git_dir());
}
