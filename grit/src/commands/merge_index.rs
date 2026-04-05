//! `grit merge-index` — run a merge program on unmerged index entries.
//!
//! For each unmerged file in the index (entries at stages 1, 2, 3),
//! invoke the specified merge program with the stage blobs and path.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::BTreeMap;
use std::process::Command;

use grit_lib::index::Index;
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;

/// Arguments for `grit merge-index`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Run a merge for files needing merge",
    override_usage = "grit merge-index <merge-program> (-a | <file>...)"
)]
pub struct Args {
    /// The merge program to invoke.
    pub merge_program: String,

    /// Merge all unmerged entries.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Specific files to merge (ignored if -a is given).
    pub files: Vec<String>,
}

/// Per-path unmerged entry: up to 3 stages.
struct UnmergedEntry {
    stages: [Option<(ObjectId, u32)>; 3], // stage 1, 2, 3 → (oid, mode)
}

/// Run `grit merge-index`.
pub fn run(args: Args) -> Result<()> {
    if !args.all && args.files.is_empty() {
        bail!("usage: grit merge-index <merge-program> (-a | <file>...)");
    }

    let repo = Repository::discover(None)?;
    let index = Index::load(&repo.index_path()).context("loading index")?;

    // Collect unmerged entries by path
    let mut unmerged: BTreeMap<Vec<u8>, UnmergedEntry> = BTreeMap::new();
    for entry in &index.entries {
        let stage = entry.stage();
        if stage == 0 {
            continue; // merged
        }
        let ue = unmerged.entry(entry.path.clone()).or_insert(UnmergedEntry {
            stages: [None, None, None],
        });
        if (1..=3).contains(&stage) {
            ue.stages[(stage - 1) as usize] = Some((entry.oid, entry.mode));
        }
    }

    // Filter to requested files if not -a
    let paths: Vec<Vec<u8>> = if args.all {
        unmerged.keys().cloned().collect()
    } else {
        let mut result = Vec::new();
        for f in &args.files {
            let path_bytes = f.as_bytes().to_vec();
            if unmerged.contains_key(&path_bytes) {
                result.push(path_bytes);
            } else {
                eprintln!("merge-index: {} is not unmerged", f);
            }
        }
        result
    };

    let mut had_error = false;

    for path in &paths {
        let ue = &unmerged[path];
        let path_str = String::from_utf8_lossy(path);

        // Build arguments for the merge program:
        // <merge-program> <base-oid> <stage1-mode> <ours-oid> <stage2-mode> <theirs-oid> <stage3-mode> <path>
        // If a stage is missing, use empty SHA-1 (all zeros) and mode 0
        let empty_oid = ObjectId::from_hex("0000000000000000000000000000000000000000")?;

        let (oid1, mode1) = ue.stages[0].unwrap_or((empty_oid, 0));
        let (oid2, mode2) = ue.stages[1].unwrap_or((empty_oid, 0));
        let (oid3, mode3) = ue.stages[2].unwrap_or((empty_oid, 0));

        let status = Command::new(&args.merge_program)
            .arg(oid1.to_hex())
            .arg(format!("{:o}", mode1))
            .arg(oid2.to_hex())
            .arg(format!("{:o}", mode2))
            .arg(oid3.to_hex())
            .arg(format!("{:o}", mode3))
            .arg(path_str.as_ref())
            .status()
            .with_context(|| format!("running merge program {:?}", args.merge_program))?;

        if !status.success() {
            had_error = true;
        }
    }

    if had_error {
        std::process::exit(1);
    }

    Ok(())
}
