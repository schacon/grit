//! `grit_lib::diff::DiffStat` counts match system `git diff --numstat` / `--shortstat`.

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::diff::{build_file_diffs, DiffDetail, DiffOptions, DiffStat};
use grit_lib::parallel::ThreadPolicy;
use grit_lib::repo::Repository;
use grit_test_support::{git, rev_parse, write_file};

fn diffstat(repo: &Repository, detail: DiffDetail) -> DiffStat {
    let old = rev_parse(repo.work_tree().expect("work tree"), "HEAD~1^{tree}");
    let new = rev_parse(repo.work_tree().expect("work tree"), "HEAD^{tree}");
    let entries = repo
        .diff()
        .tree_to_tree(Some(&old), Some(&new))
        .expect("tree diff");
    let config = repo.config().expect("config");
    let out = build_file_diffs(
        repo.odb(),
        entries,
        detail,
        DiffOptions::default(),
        config.as_ref(),
        &ThreadPolicy::default(),
    )
    .expect("file diffs");
    DiffStat::from_files(&out.files)
}

#[test]
fn diffstat_matches_git_numstat_and_shortstat() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    let long: String = (0..200).map(|i| format!("line {i}\n")).collect();
    write_file(root, "big.txt", &long);
    write_file(root, "edit.txt", "a\nb\nc\nd\ne\n");
    write_file(root, "gone.txt", "x\ny\nz\n");
    std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).expect("binary");
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );

    let edited: String = long.replace("line 1", "LINE 1");
    write_file(root, "big.txt", &edited);
    write_file(root, "edit.txt", "a\nB\nc\ne\nf\ng\n");
    std::fs::remove_file(root.join("gone.txt")).expect("remove");
    write_file(root, "new.txt", "1\n2\n");
    std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3, 4]).expect("binary");
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "-m",
            "two",
        ],
    );

    let repo = Repository::discover(Some(root), &RepositoryOptions::empty()).expect("open");
    let numstat = git(
        root,
        &["diff", "--no-renames", "--numstat", "HEAD~1", "HEAD"],
    );
    let shortstat = git(
        root,
        &["diff", "--no-renames", "--shortstat", "HEAD~1", "HEAD"],
    );

    for detail in [DiffDetail::Stat, DiffDetail::Patch] {
        let stat = diffstat(&repo, detail);
        let ours: String = stat
            .files()
            .iter()
            .map(|f| {
                if f.binary {
                    format!("-\t-\t{}\n", f.path)
                } else {
                    format!(
                        "{}\t{}\t{}\n",
                        f.lines.insertions, f.lines.deletions, f.path
                    )
                }
            })
            .collect();
        assert_eq!(ours, numstat, "{detail:?}");

        let totals = stat.totals();
        let expected = format!(
            " {} files changed, {} insertions(+), {} deletions(-)\n",
            totals.files_changed, totals.insertions, totals.deletions
        );
        assert_eq!(expected, shortstat, "{detail:?}");
    }
}
