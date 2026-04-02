//! `grit merge-one-file` — standard helper for merge-index.
//!
//! Performs a three-way file merge using `merge-file`.  Intended to be
//! invoked by `merge-index` as the merge program.
//!
//! Arguments (passed by merge-index):
//!   <base-oid> <base-mode> <ours-oid> <ours-mode> <theirs-oid> <theirs-mode> <path>

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::Write as _;

use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::repo::Repository;

/// Arguments for `grit merge-one-file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Base blob OID (all zeros if none).
    pub base_oid: String,
    /// Base file mode (octal).
    pub base_mode: String,
    /// Ours blob OID (all zeros if none).
    pub ours_oid: String,
    /// Ours file mode (octal).
    pub ours_mode: String,
    /// Theirs blob OID (all zeros if none).
    pub theirs_oid: String,
    /// Theirs file mode (octal).
    pub theirs_mode: String,
    /// Path of the file being merged.
    pub path: String,
}

const EMPTY_OID: &str = "0000000000000000000000000000000000000000";

fn read_blob_to_temp(repo: &Repository, oid_hex: &str, label: &str) -> Result<tempfile::NamedTempFile> {
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".merge_{label}_"))
        .tempfile_in(".")
        .context("creating temp file")?;

    if oid_hex != EMPTY_OID {
        let oid = ObjectId::from_hex(oid_hex)
            .with_context(|| format!("invalid OID: {oid_hex}"))?;
        let obj = repo.odb.read(&oid)
            .with_context(|| format!("reading blob {oid_hex}"))?;
        if obj.kind != ObjectKind::Blob {
            anyhow::bail!("{oid_hex} is not a blob");
        }
        tmp.write_all(&obj.data)?;
    }

    Ok(tmp)
}

/// Run `grit merge-one-file`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // If both sides added (no base) or one side deleted, handle simple cases.
    let base_empty = args.base_oid == EMPTY_OID;
    let ours_empty = args.ours_oid == EMPTY_OID;
    let theirs_empty = args.theirs_oid == EMPTY_OID;

    // If ours and theirs are the same, no merge needed.
    if args.ours_oid == args.theirs_oid {
        return Ok(());
    }

    // If one side is empty (deleted), that's a conflict we can't auto-resolve.
    if !base_empty && (ours_empty || theirs_empty) {
        eprintln!(
            "CONFLICT (modify/delete): {} deleted in one branch and modified in the other.",
            args.path
        );
        std::process::exit(1);
    }

    // Write the three versions to temp files and run a 3-way merge.
    let base_tmp = read_blob_to_temp(&repo, &args.base_oid, "base")?;
    let ours_tmp = read_blob_to_temp(&repo, &args.ours_oid, "ours")?;
    let theirs_tmp = read_blob_to_temp(&repo, &args.theirs_oid, "theirs")?;

    let status = std::process::Command::new("grit")
        .arg("merge-file")
        .arg("-p")
        .arg(ours_tmp.path())
        .arg(base_tmp.path())
        .arg(theirs_tmp.path())
        .status()
        .context("running merge-file")?;

    if !status.success() {
        eprintln!("CONFLICT (content): Merge conflict in {}", args.path);
        // Write the conflicted result to the working tree.
        let merged = std::fs::read(ours_tmp.path())?;
        std::fs::write(&args.path, merged)?;
        std::process::exit(1);
    }

    // Success: write the merged result to the working tree path.
    let merged = std::fs::read(ours_tmp.path())?;
    std::fs::write(&args.path, merged)?;

    Ok(())
}
