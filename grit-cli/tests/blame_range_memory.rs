//! A three-line blame range must not retain a growing historical object pack.

#![cfg(target_os = "linux")]
#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use grit_test_support::configure_git_env;

fn peak_kib(root: &Path, program: &str, args: &[&str]) -> u64 {
    let report = tempfile::NamedTempFile::new().expect("rss report");
    let mut cmd = Command::new("/usr/bin/time");
    configure_git_env(&mut cmd);
    let out = cmd
        .current_dir(root)
        .args(["-f", "%M", "-o"])
        .arg(report.path())
        .arg(program)
        .args(args)
        .stdout(Stdio::null())
        .output()
        .expect("time");
    assert!(
        out.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    fs::read_to_string(report.path())
        .expect("rss")
        .trim()
        .parse()
        .expect("KiB")
}

#[test]
fn ranged_blame_pack_residency_is_bounded() {
    let grit = env!("CARGO_BIN_EXE_grit");
    let mut peaks = Vec::new();
    for padding in [8 * 1024, 64 * 1024] {
        let root = tempfile::tempdir().expect("repo");
        grit_test_support::blame::packed_history(root.path(), 1800, padding).expect("history");
        let args = [
            "blame",
            "--rev",
            "HEAD",
            "-L",
            "1,3",
            "--json",
            "target.txt",
        ];
        let git_args = [
            "blame",
            "--line-porcelain",
            "-L",
            "1,3",
            "HEAD",
            "--",
            "target.txt",
        ];
        peak_kib(root.path(), grit, &args);
        peak_kib(root.path(), "git", &git_args);
        let mut ours = Vec::new();
        let mut theirs = Vec::new();
        for _ in 0..3 {
            ours.push(peak_kib(root.path(), grit, &args));
            theirs.push(peak_kib(root.path(), "git", &git_args));
        }
        ours.sort_unstable();
        theirs.sort_unstable();
        let ours = ours[1];
        let theirs = theirs[1];
        eprintln!("padding={padding}: grit={ours} KiB git={theirs} KiB");
        assert!(ours <= 2 * theirs + 16 * 1024, "grit={ours}, git={theirs}");
        peaks.push(ours);
    }
    assert!(
        peaks[1] <= peaks[0] + 32 * 1024,
        "pack growth raised RSS: {peaks:?}"
    );
}

#[test]
fn ranged_blame_bounds_open_pack_files() {
    let root = tempfile::tempdir().expect("repo");
    grit_test_support::blame::many_pack_history(root.path(), 300).expect("history");
    let packs: Vec<_> = fs::read_dir(root.path().join(".git/objects/pack"))
        .expect("packs")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pack"))
        .collect();
    assert_eq!(packs.len(), 300);
    assert!(packs
        .iter()
        .all(|path| fs::metadata(path).expect("pack").len() > 8192));

    let limited = |program: &str, args: &[&str]| {
        let mut cmd = Command::new("sh");
        configure_git_env(&mut cmd);
        let output = cmd
            .current_dir(root.path())
            .args(["-c", "ulimit -n 256 && exec \"$@\"", "blame-test", program])
            .args(args)
            .output()
            .expect("limited blame");
        assert!(
            output.status.success(),
            "{program}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    let git = limited(
        "git",
        &[
            "blame",
            "--line-porcelain",
            "-L",
            "1,3",
            "HEAD",
            "--",
            "target.txt",
        ],
    );
    let expected: Vec<_> = std::str::from_utf8(&git)
        .expect("git output")
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            (fields.len() >= 3
                && fields[0].len() == 40
                && fields[0].bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| {
                (
                    fields[0],
                    fields[1].parse::<u64>().expect("original line"),
                    fields[2].parse::<u64>().expect("final line"),
                )
            })
        })
        .collect();
    assert_eq!(expected.len(), 3);
    let grit = env!("CARGO_BIN_EXE_grit");
    let full: serde_json::Value = serde_json::from_slice(&limited(
        grit,
        &["blame", "--rev", "HEAD", "--json", "target.txt"],
    ))
    .expect("full blame");
    let ranged: serde_json::Value = serde_json::from_slice(&limited(
        grit,
        &[
            "blame",
            "--rev",
            "HEAD",
            "-L",
            "1,3",
            "--json",
            "target.txt",
        ],
    ))
    .expect("ranged blame");
    let lines = ranged["lines"].as_array().expect("lines");
    assert_eq!(lines.len(), 3);
    for (i, (line, (commit, original, final_line))) in lines.iter().zip(expected).enumerate() {
        assert_eq!(line["commit"], commit);
        assert_eq!(line["original_line"], original);
        assert_eq!(line["final_line"], final_line);
        assert_eq!(line["original_path"], "target.txt");
        assert_eq!(line["content"], ["first", "second", "third"][i]);
        assert_eq!(line, &full["lines"][i]);
    }
}
