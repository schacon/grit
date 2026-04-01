//! `grit unpack-file` — write a blob object to a temporary file and print its path.
//!
//! Takes a single blob OID (or any object name that resolves to a blob), writes
//! the blob's content to a new temporary file named `.merge_file_XXXXXX` in the
//! current directory, and prints the resulting path to stdout.
//!
//! This is a plumbing helper used by merge drivers.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::Write as _;

use grit_lib::objects::ObjectKind;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;

/// Arguments for `grit unpack-file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Blob object name to unpack (OID, ref, or revision like `HEAD:path`).
    pub blob: String,
}

/// Run `grit unpack-file`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let oid = resolve_revision(&repo, &args.blob)
        .with_context(|| format!("Not a valid object name {}", args.blob))?;

    let obj = repo
        .odb
        .read(&oid)
        .with_context(|| format!("unable to read blob object {oid}"))?;

    if obj.kind != ObjectKind::Blob {
        bail!(
            "unable to read blob object {}: object is of type {}",
            oid,
            obj.kind
        );
    }

    let mut tmp = tempfile::Builder::new()
        .prefix(".merge_file_")
        .tempfile_in(".")
        .context("unable to create temp file")?;

    tmp.write_all(&obj.data)
        .context("unable to write temp-file")?;

    let (_, path) = tmp.keep().context("unable to persist temp file")?;

    println!("{}", path.display());
    Ok(())
}
