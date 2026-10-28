//! `grit blame` blames the working tree file by default, `--rev` blames the commit, and
//! `--contents` blames a supplied buffer.

use grit_test_support::cli_fixture::CliFixture;
use grit_test_support::grit_cmd;

const NULL: &str = "0000000000000000000000000000000000000000";

fn committed() -> CliFixture {
    let fx = CliFixture::empty();
    fx.write_repo_file("f.txt", "one\ntwo\nthree\n");
    fx.run_ok(&["add", "f.txt"]);
    fx.run_ok(&["commit", "-m", "add f"]);
    fx
}

fn json(out: &grit_test_support::Output) -> serde_json::Value {
    serde_json::from_str(&out.stdout).expect("json blame output")
}

#[test]
fn json_marks_uncommitted_lines_and_shifts_the_rest() {
    let fx = committed();
    fx.write_repo_file("f.txt", "// edited\none\ntwo\nthree\n");

    let value = json(&fx.run_ok(&["--json", "blame", "f.txt"]));
    let lines = value["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["commit"], NULL);
    assert_eq!(lines[0]["not_committed"], true);
    assert_eq!(lines[0]["content"], "// edited");
    assert_eq!(lines[1]["content"], "one");
    assert_eq!(lines[1]["final_line"], 2);
    assert_eq!(lines[1]["original_line"], 1);
    assert!(lines[1].get("not_committed").is_none());
    assert_eq!(
        value["commits"][NULL]["author"]["name"],
        "Not Committed Yet"
    );

    let ranged = json(&fx.run_ok(&["--json", "blame", "-L", "1,1", "f.txt"]));
    assert_eq!(ranged["lines"][0]["content"], "// edited");

    let grouped = json(&fx.run_ok(&["--json", "blame", "--group", "f.txt"]));
    assert_eq!(grouped["hunks"][0]["not_committed"], true);
    assert!(grouped["hunks"][1].get("not_committed").is_none());
}

#[test]
fn human_output_labels_uncommitted_lines() {
    let fx = committed();
    fx.write_repo_file("f.txt", "one\nnew\ntwo\nthree\n");
    let out = fx.run_ok(&["blame", "f.txt"]);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(
        lines[1].starts_with("0000000 (Not Committed Yet") && lines[1].ends_with("2) new"),
        "{}",
        out.stdout
    );
    assert!(!lines[0].starts_with("0000000"), "{}", out.stdout);

    let md = fx.run_ok(&["--markdown", "blame", "f.txt"]);
    assert!(md.stdout.contains("Not Committed Yet"), "{}", md.stdout);
}

#[test]
fn explicit_rev_ignores_the_worktree() {
    let fx = committed();
    fx.write_repo_file("f.txt", "dirty\n");
    let value = json(&fx.run_ok(&["--json", "blame", "--rev", "HEAD", "f.txt"]));
    let lines = value["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line["commit"] != NULL));
    let positional = json(&fx.run_ok(&["--json", "blame", "HEAD", "--", "f.txt"]));
    assert_eq!(positional["lines"], value["lines"]);
}

#[test]
fn contents_file_and_stdin() {
    let fx = committed();
    fx.write_repo_file("f.txt", "worktree edit\n");
    fx.write_repo_file("buffer.txt", "one\nunsaved\nthree\n");

    let value = json(&fx.run_ok(&["--json", "blame", "--contents", "buffer.txt", "f.txt"]));
    let flags: Vec<bool> = value["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["not_committed"] == true)
        .collect();
    assert_eq!(flags, [false, true, false]);
    assert_eq!(
        value["commits"][NULL]["author"]["name"],
        "External file (--contents)"
    );

    let out = grit_cmd(&["--json", "blame", "--contents", "-", "f.txt"])
        .in_dir(fx.path())
        .stdin("stdin line\none\n")
        .suc();
    let value: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(value["lines"][0]["not_committed"], true);
    assert_eq!(value["lines"][1]["content"], "one");
}

#[test]
fn untracked_file_fails() {
    let fx = committed();
    fx.write_repo_file("new.txt", "x\n");
    let out = fx.run(&["blame", "new.txt"]);
    assert_ne!(out.status, Some(0), "{}", out.stdout);
}
