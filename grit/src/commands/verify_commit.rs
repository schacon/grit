//! `grit verify-commit` — verify a commit exists and is valid.
//!
//! Checks that the given commit(s) exist, are valid commit objects,
//! and prints basic info.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, Write};

/// Arguments for `grit verify-commit`.
#[derive(Debug, ClapArgs)]
#[command(about = "Verify a commit object")]
pub struct Args {
    /// Commit references to verify.
    #[arg(required = true)]
    pub commits: Vec<String>,

    /// Print commit contents.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

/// Run the `verify-commit` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    for rev in &args.commits {
        let oid = match resolve_revision(&repo, rev) {
            Ok(oid) => oid,
            Err(e) => {
                writeln!(err, "error: {rev}: {e}")?;
                anyhow::bail!("could not verify commit '{rev}'");
            }
        };

        let obj = repo
            .odb
            .read(&oid)
            .with_context(|| format!("could not read object '{rev}'"))?;

        if obj.kind != ObjectKind::Commit {
            writeln!(
                err,
                "error: {}: object is a {}, not a commit",
                oid.to_hex(),
                obj.kind.as_str()
            )?;
            anyhow::bail!("could not verify commit '{rev}'");
        }

        let commit =
            parse_commit(&obj.data).with_context(|| format!("failed to parse commit '{rev}'"))?;

        if args.verbose {
            writeln!(out, "tree {}", commit.tree.to_hex())?;
            for parent in &commit.parents {
                writeln!(out, "parent {}", parent.to_hex())?;
            }
            writeln!(out, "author {}", commit.author)?;
            writeln!(out, "committer {}", commit.committer)?;
            writeln!(out)?;
            write!(out, "{}", commit.message)?;
        } else {
            // Just verify — no output on success (like git)
        }
    }

    Ok(())
}
