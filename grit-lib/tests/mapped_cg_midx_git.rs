//! Git-written commit-graph chains and MIDX: mapped open matches full verify parsing.

use std::path::Path;
use std::process::Command;

use grit_lib::objects::commit_graph::{CommitGraphChain, CommitGraphLayer};
use grit_lib::objects::midx::{midx_lookup_pack_and_offset_opt, try_read_object_via_midx};
use grit_lib::objects::pack::clear_pack_cache;
use grit_lib::objects::ObjectId;

const MMAP_THRESHOLD: u64 = 8192;

fn git_ok() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).expect("utf8")
}

fn git(dir: &Path, args: &[&str]) {
    git_out(dir, args);
}

fn write_multi_layer_split_commit_graph(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "--allow-empty", "-q", "-m", "bootstrap"]);
    for i in 0..150 {
        git(
            dir,
            &[
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                &format!("base-fill-{i}"),
            ],
        );
    }
    git(
        dir,
        &[
            "commit-graph",
            "write",
            "--reachable",
            "--split",
            "--max-commits",
            "40",
        ],
    );
    for i in 150..190 {
        git(
            dir,
            &[
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                &format!("tip-fill-{i}"),
            ],
        );
    }
    git(
        dir,
        &[
            "commit-graph",
            "write",
            "--reachable",
            "--split",
            "--max-commits",
            "40",
        ],
    );
}

fn write_large_midx(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("seed.txt"), b"seed\n").unwrap();
    git(dir, &["add", "seed.txt"]);
    git(dir, &["commit", "-q", "-m", "seed"]);
    for i in 0..150 {
        std::fs::write(dir.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
        git(dir, &["add", &format!("f{i}.txt")]);
        git(dir, &["commit", "-q", "-m", &format!("c{i}")]);
    }
    git(dir, &["repack", "-adf"]);
    git(dir, &["multi-pack-index", "write"]);
}

#[test]
fn git_split_commit_graph_lazy_load_matches_verify_parse() {
    if !git_ok() {
        eprintln!("SKIP: git unavailable");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    write_multi_layer_split_commit_graph(dir.path());

    let objects = dir.path().join(".git/objects");
    let chain = CommitGraphChain::try_load(&objects)
        .expect("load")
        .expect("chain");
    assert!(
        chain.num_layers() >= 2,
        "expected a multi-layer split chain, got {} layer(s)",
        chain.num_layers()
    );
    let chain_file = objects.join("info/commit-graphs/commit-graph-chain");
    assert!(chain_file.is_file(), "expected split chain file");
    let chain_lines = std::fs::read_to_string(&chain_file)
        .expect("chain file")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    assert!(
        chain_lines >= 2,
        "commit-graph-chain must list at least two layers, got {chain_lines}"
    );

    let mut mapped_large_layer = false;
    for path in chain.layer_paths_oldest_first() {
        let len = std::fs::metadata(&path).expect("meta").len();
        if len > MMAP_THRESHOLD {
            mapped_large_layer = true;
        }
        let raw = std::fs::read(&path).expect("read layer");
        CommitGraphLayer::try_parse(path.clone(), raw).expect("full verify parse");
    }
    assert!(
        mapped_large_layer,
        "expected at least one chain layer larger than {MMAP_THRESHOLD} bytes for mmap"
    );

    CommitGraphChain::verify_on_disk(&objects).expect("verify_on_disk");

    let tip_oids = chain.layer_oids(0);
    let base_oids = chain.layer_oids(chain.num_layers() - 1);
    assert!(!tip_oids.is_empty() && !base_oids.is_empty());
    let base_only = base_oids
        .iter()
        .find(|o| !tip_oids.contains(o))
        .expect("base layer must contain commits not only in the tip layer");
    let tip_head = *tip_oids.last().expect("tip commits");
    for oid in [base_only, &tip_head] {
        let parents_time = chain.graph_commit(oid).expect("graph_commit");
        assert!(!parents_time.0.is_empty() || parents_time.1 > 0);
    }
    for oid in chain.all_oids_in_order() {
        assert!(
            chain.graph_commit(&oid).is_some(),
            "missing graph_commit for {}",
            oid.to_hex()
        );
    }
}

#[test]
fn git_large_midx_lazy_load_matches_verify_and_lookup() {
    if !git_ok() {
        eprintln!("SKIP: git unavailable");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    write_large_midx(dir.path());

    let objects = dir.path().join(".git/objects");
    let midx_path = objects.join("pack/multi-pack-index");
    let len = std::fs::metadata(&midx_path).expect("meta").len();
    assert!(
        len > MMAP_THRESHOLD,
        "fixture MIDX is {len} bytes; need > {MMAP_THRESHOLD} for mmap"
    );

    let head_hex = git_out(dir.path(), &["rev-parse", "HEAD"]);
    let oid = ObjectId::from_hex(head_hex.trim()).expect("head");

    let loc = midx_lookup_pack_and_offset_opt(&objects, &oid)
        .expect("lookup")
        .expect("listed in midx");
    grit_lib::objects::midx::evict_midx_read_cache_for_pack_dir(&objects.join("pack"));
    clear_pack_cache();
    let obj = try_read_object_via_midx(&objects, &oid)
        .expect("read")
        .expect("object bytes");
    assert!(!obj.data.is_empty());
    assert!(loc.1 > 0 || loc.0 == 0);
}
