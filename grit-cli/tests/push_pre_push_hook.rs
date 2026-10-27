//! `grit push` runs the `pre-push` hook and stops when it fails; `--no-verify` skips it
//! (issue #1799).

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use grit_test_support::{git, git_cmd, grit_cmd, CliFixture};

fn write_hook(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().expect("hook dir")).expect("mkdir");
    std::fs::write(path, body).expect("write hook");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn remote_has(bare: &Path, refname: &str) -> bool {
    git_cmd(&["rev-parse", "--verify", "-q", refname])
        .in_dir(bare)
        .exec()
        .ok()
}

#[test]
fn failing_pre_push_hook_blocks_push_in_every_output_mode() {
    let fx = CliFixture::new().with_upstream();
    let bare = fx.upstream_path();
    let client = fx.git_clone_client("client");
    git(&client, &["checkout", "-q", "-b", "gated"]);
    std::fs::write(client.join("gated.txt"), "x\n").unwrap();
    git(&client, &["add", "gated.txt"]);
    git(&client, &["commit", "-q", "-m", "gated"]);
    write_hook(
        &client.join(".git/hooks/pre-push"),
        "#!/bin/sh\necho 'hook says no'\nexit 1\n",
    );

    let human = grit_cmd(&["push", "origin", "gated"])
        .in_dir(&client)
        .exec();
    assert_eq!(human.status, Some(1), "{}", human.dump("push"));
    assert!(human.stderr.contains("hook says no"), "{}", human.stderr);
    assert!(
        human
            .stderr
            .contains("hook pre-push failed with exit status 1"),
        "{}",
        human.stderr
    );
    assert!(!human.stdout.contains("pushed"), "{}", human.stdout);

    let json = grit_cmd(&["--json", "push", "origin", "gated"])
        .in_dir(&client)
        .exec();
    assert_eq!(json.status, Some(1), "{}", json.dump("push --json"));
    let value: serde_json::Value =
        serde_json::from_str(json.stdout.trim()).expect("stdout is one JSON object");
    assert_eq!(
        value["error"].as_str(),
        Some("hook pre-push failed with exit status 1")
    );

    let md = grit_cmd(&["--markdown", "push", "--dry-run", "origin", "gated"])
        .in_dir(&client)
        .exec();
    assert_eq!(md.status, Some(1), "{}", md.dump("push --markdown"));
    assert!(
        md.stdout
            .contains("**Error:** hook pre-push failed with exit status 1"),
        "{}",
        md.stdout
    );

    assert!(
        !remote_has(bare, "refs/heads/gated"),
        "remote must be untouched"
    );

    let skipped = grit_cmd(&["push", "--no-verify", "origin", "gated"])
        .in_dir(&client)
        .exec();
    assert!(skipped.ok(), "{}", skipped.dump("push --no-verify"));
    assert!(remote_has(bare, "refs/heads/gated"));
    git(bare, &["fsck", "--strict"]);
}

#[test]
fn passing_pre_push_hook_sees_remote_and_ref_lines() {
    let fx = CliFixture::new().with_upstream();
    let client = fx.git_clone_client("client");
    let log = client.join("..").join("pre-push.log");
    write_hook(
        &client.join(".git/hooks/pre-push"),
        &format!("#!/bin/sh\n{{ echo \"$1\"; cat; }} > '{}'\n", log.display()),
    );
    let out = grit_cmd(&["--json", "push", "origin", "HEAD:refs/heads/copy"])
        .in_dir(&client)
        .exec();
    assert!(out.ok(), "{}", out.dump("push"));
    serde_json::from_str::<serde_json::Value>(out.stdout.trim()).expect("clean JSON stdout");
    let head = git(&client, &["rev-parse", "HEAD"]);
    let logged = std::fs::read_to_string(&log).expect("hook ran");
    assert_eq!(
        logged,
        format!(
            "origin\nHEAD {} refs/heads/copy {}\n",
            head.trim(),
            "0".repeat(40)
        )
    );
}

#[test]
fn hook_that_ignores_its_stdin_fails_the_push_without_killing_grit() {
    let fx = CliFixture::new().with_upstream();
    let bare = fx.upstream_path();
    let client = fx.git_clone_client("client");
    let head = git(&client, &["rev-parse", "HEAD"]);
    // More ref lines than a pipe buffer holds, so writing them must hit a closed pipe.
    let updates: String = (0..800)
        .map(|i| format!("create refs/heads/many-{i:04} {}\n", head.trim()))
        .collect();
    let out = git_cmd(&["update-ref", "--stdin"])
        .in_dir(&client)
        .stdin(updates)
        .exec();
    assert!(out.ok(), "{}", out.dump("update-ref"));
    write_hook(&client.join(".git/hooks/pre-push"), "#!/bin/sh\nexit 1\n");

    let names: Vec<String> = (0..800).map(|i| format!("many-{i:04}")).collect();
    let mut args = vec!["push", "origin"];
    args.extend(names.iter().map(String::as_str));
    let out = grit_cmd(&args).in_dir(&client).exec();
    assert_eq!(out.status, Some(1), "{}", out.dump("push"));
    assert!(
        out.stderr
            .contains("hook pre-push failed with exit status 1"),
        "{}",
        out.stderr
    );
    assert!(!remote_has(bare, "refs/heads/many-0000"));
}
