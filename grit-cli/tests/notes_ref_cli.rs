//! Regression: `--ref` must work after the notes subcommand (documented synopsis).

use grit_test_support::CliFixture;
use grit_test_support::git;

fn seed_commit(fixture: &CliFixture) {
    git(fixture.path(), &["commit", "--allow-empty", "-m", "seed"]);
}

#[test]
fn notes_list_ref_after_subcommand() {
    let fixture = CliFixture::empty();
    seed_commit(&fixture);
    let out = fixture.run(&["notes", "list", "--ref", "refs/notes/custom"]);
    assert!(out.ok(), "{}", out.dump("notes list --ref"));
    assert!(
        out.stdout.contains("No notes yet") || out.stdout.contains("\"action\""),
        "unexpected stdout: {}",
        out.stdout
    );
}

#[test]
fn notes_show_ref_after_subcommand() {
    let fixture = CliFixture::empty();
    seed_commit(&fixture);
    fixture.run_ok(&[
        "notes",
        "add",
        "HEAD",
        "-m",
        "hi",
        "--ref",
        "refs/notes/custom",
    ]);
    let out = fixture.run(&[
        "notes",
        "show",
        "HEAD",
        "--ref",
        "refs/notes/custom",
    ]);
    assert!(out.ok(), "{}", out.dump("notes show --ref"));
    assert!(out.stdout.contains("hi"), "stdout: {}", out.stdout);
}

#[test]
fn notes_add_ref_after_subcommand() {
    let fixture = CliFixture::empty();
    seed_commit(&fixture);
    let out = fixture.run(&[
        "notes",
        "add",
        "HEAD",
        "-m",
        "placed ref",
        "--ref",
        "refs/notes/custom",
    ]);
    assert!(out.ok(), "{}", out.dump("notes add --ref"));
}

#[test]
fn notes_remove_ref_after_subcommand() {
    let fixture = CliFixture::empty();
    seed_commit(&fixture);
    fixture.run_ok(&["notes", "add", "HEAD", "-m", "drop me"]);
    let out = fixture.run(&["notes", "remove", "HEAD", "--ref", "refs/notes/commits"]);
    assert!(out.ok(), "{}", out.dump("notes remove --ref"));
}
