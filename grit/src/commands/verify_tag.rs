//! `grit verify-tag` — verify a tag.
//!
//! Checks that a tag exists and is annotated, prints tag info,
//! and exits with code 0 (valid) or 1 (invalid/not found).

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_tag, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Write};

/// Arguments for `grit verify-tag`.
#[derive(Debug, ClapArgs)]
#[command(about = "Verify a tag object")]
pub struct Args {
    /// Tag names to verify.
    #[arg(required = true)]
    pub tags: Vec<String>,

    /// Print tag contents.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

/// Run the `verify-tag` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    for tag_name in &args.tags {
        let tag_ref = repo.git_dir.join("refs/tags").join(tag_name);
        let hex = match fs::read_to_string(&tag_ref) {
            Ok(content) => content.trim().to_owned(),
            Err(_) => {
                writeln!(err, "error: tag '{tag_name}' not found.")?;
                bail!("could not verify tag '{tag_name}'");
            }
        };

        let oid = ObjectId::from_hex(&hex)
            .map_err(|_| anyhow::anyhow!("bad tag ref for '{tag_name}'"))?;

        let obj = repo
            .odb
            .read(&oid)
            .with_context(|| format!("could not read object for tag '{tag_name}'"))?;

        if obj.kind != ObjectKind::Tag {
            // Lightweight tag — not an annotated tag object
            writeln!(
                err,
                "error: {hex}: cannot verify a non-tag object of type {}.",
                obj.kind.as_str()
            )?;
            bail!("could not verify tag '{tag_name}'");
        }

        let tag = parse_tag(&obj.data)
            .with_context(|| format!("failed to parse tag object '{tag_name}'"))?;

        // Print tag info
        writeln!(out, "object {}", tag.object.to_hex())?;
        writeln!(out, "type {}", tag.object_type)?;
        writeln!(out, "tag {}", tag.tag)?;
        if let Some(ref tagger) = tag.tagger {
            writeln!(out, "tagger {tagger}")?;
        }
        writeln!(out)?;
        write!(out, "{}", tag.message)?;
    }

    Ok(())
}
