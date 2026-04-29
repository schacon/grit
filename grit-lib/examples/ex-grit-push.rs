//! Minimal `git push`-style command built on [`grit_lib::http_push`].
//!
//! Run from a repository you want to push:
//!
//! ```text
//! cargo run -p grit-lib --example ex-grit-push -- <branch> <https-url>
//! ```

use grit_lib::http_push::{push_branch, HttpPushOptions};
use grit_lib::repo::Repository;

fn usage(program: &str) -> String {
    format!("usage: {program} <branch> <https-url>")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(usage(args.first().map_or("ex-grit-push", String::as_str)).into());
    }

    let branch = &args[1];
    let url = &args[2];
    if !url.starts_with("https://") {
        return Err("ex-grit-push currently expects an https:// URL".into());
    }

    let repo = Repository::discover(None)?;
    let report = push_branch(
        &repo,
        &HttpPushOptions {
            branch: branch.to_owned(),
            url: url.to_owned(),
            remote_branch: None,
            force: false,
        },
    )?;

    eprintln!(
        "pushed {} to {} ({})",
        branch, report.remote_ref, report.new_oid
    );
    Ok(())
}
