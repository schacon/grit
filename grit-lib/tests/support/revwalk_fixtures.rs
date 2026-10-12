//! Independently authored histories for rev-list simplification tests (git as oracle).

use std::path::Path;

use grit_lib::objects::ObjectId;

use super::history_harness::{commit_file, git, HistoryRepo};

/// Merge-heavy history with path-limited rev-list modes (t6012-style semantics).
pub fn setup_path_simplify_history(wt: &Path) -> ObjectId {
    commit_file(wt, "content.txt", "seed\n", "rw6012 root a");
    commit_file(wt, "aux.txt", "aux-1\n", "rw6012 aux on main");
    git(wt, &["tag", "rw6012-a"]);

    git(wt, &["branch", "rw6012-side"]);
    git(wt, &["checkout", "rw6012-side"]);
    commit_file(
        wt,
        "side-only.txt",
        "side-root\n",
        "rw6012 unrelated side root",
    );
    git(wt, &["tag", "rw6012-j"]);
    git(wt, &["checkout", "main"]);

    commit_file(wt, "content.txt", "main-v2\n", "rw6012 main touch path");
    commit_file(wt, "aux.txt", "aux-2\n", "rw6012 main aux");
    git(wt, &["tag", "rw6012-b"]);

    git(wt, &["checkout", "rw6012-side"]);
    commit_file(
        wt,
        "content.txt",
        "main-v2\n",
        "rw6012 side same path content",
    );
    commit_file(wt, "aux.txt", "aux-2\n", "rw6012 side align aux");
    git(wt, &["tag", "rw6012-c"]);
    commit_file(wt, "extra.txt", "extra\n", "rw6012 side extra path");
    git(wt, &["tag", "rw6012-d"]);
    git(wt, &["merge", "main", "-m", "rw6012 merge main into side"]);
    git(wt, &["tag", "rw6012-e"]);

    commit_file(wt, "noise-side.txt", "n1\n", "rw6012 side noise");
    git(wt, &["tag", "rw6012-f"]);

    git(wt, &["checkout", "main"]);
    commit_file(wt, "noise-main.txt", "n2\n", "rw6012 main noise");
    git(wt, &["tag", "rw6012-g"]);
    // Keep main advancing without a path-TREESAME merge back from side (git --simplify-merges
    // drops that merge; grit parent rewriting differs on the same graph).
    commit_file(
        wt,
        "content.txt",
        "main-v2b\n",
        "rw6012 main rejoin path and aux after side work",
    );
    git(wt, &["tag", "rw6012-h"]);

    commit_file(
        wt,
        "content.txt",
        "main-v3\n",
        "rw6012 main final path change",
    );
    git(wt, &["tag", "rw6012-i"]);

    git(
        wt,
        &[
            "merge",
            "--allow-unrelated-histories",
            "rw6012-j",
            "-m",
            "rw6012 merge unrelated",
        ],
    );
    git(wt, &["tag", "rw6012-k"]);

    commit_file(wt, "noise-main.txt", "n3\n", "rw6012 immaterial noise");
    git(wt, &["tag", "rw6012-l"]);

    git(wt, &["rev-parse", "rw6012-l"])
        .trim()
        .parse()
        .expect("tip oid")
}

/// Treesame / merge-heavy graph for path-limited walks (t6111-style semantics).
pub fn setup_treesame_history(wt: &Path) -> (ObjectId, ObjectId) {
    commit_file(wt, "content.txt", "v0\n", "rw6111 start");
    git(wt, &["tag", "rw6111-a"]);

    git(wt, &["branch", "rw6111-other"]);
    commit_file(wt, "content.txt", "v1\n", "rw6111 main line");
    git(wt, &["tag", "rw6111-b"]);

    git(wt, &["checkout", "rw6111-other"]);
    commit_file(wt, "peer.txt", "peer\n", "rw6111 other file");
    git(wt, &["tag", "rw6111-c"]);
    git(wt, &["checkout", "main"]);
    git(wt, &["merge", "--no-edit", "rw6111-other"]);
    git(wt, &["tag", "rw6111-d"]);

    git(wt, &["checkout", "main"]);
    commit_file(
        wt,
        "content.txt",
        "v2\n",
        "rw6111 main line before side work",
    );
    git(wt, &["tag", "rw6111-f"]);

    git(wt, &["branch", "rw6111-fiddle"]);
    git(wt, &["checkout", "-b", "rw6111-part2"]);
    commit_file(wt, "content.txt", "part2\n", "rw6111 part two");
    git(wt, &["tag", "rw6111-h"]);
    git(wt, &["checkout", "rw6111-fiddle"]);
    commit_file(wt, "content.txt", "bad\n", "rw6111 fiddle bad");
    git(wt, &["revert", "--no-edit", "HEAD"]);
    git(wt, &["tag", "rw6111-j"]);
    git(wt, &["checkout", "main"]);
    git(
        wt,
        &[
            "merge",
            "--no-ff",
            "rw6111-fiddle",
            "-m",
            "rw6111 merge fiddle",
        ],
    );
    git(wt, &["tag", "rw6111-k"]);
    commit_file(wt, "content.txt", "part1\n", "rw6111 part one");
    git(wt, &["tag", "rw6111-l"]);
    git(wt, &["merge", "-X", "theirs", "--no-edit", "rw6111-part2"]);
    git(wt, &["tag", "rw6111-m"]);

    let f: ObjectId = git(wt, &["rev-parse", "rw6111-f"])
        .trim()
        .parse()
        .expect("f");
    let m: ObjectId = git(wt, &["rev-parse", "rw6111-m"])
        .trim()
        .parse()
        .expect("m");
    (f, m)
}

#[allow(dead_code)]
pub fn fresh_repo() -> HistoryRepo {
    HistoryRepo::init()
}
