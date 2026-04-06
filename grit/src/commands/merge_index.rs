//! `grit merge-index` — run a merge program on unmerged index entries.
//!
//! For each unmerged file in the index (entries at stages 1, 2, 3),
//! invoke the specified merge program with the stage blobs and path.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::BTreeMap;
use std::path::PathBuf;
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

    /// Continue with other files after a merge-program failure.
    #[arg(short = 'o')]
    pub one_shot: bool,

    /// Suppress merge-program failure diagnostics from this command.
    #[arg(short = 'q')]
    pub quiet: bool,

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
    let index_path = effective_index_path(&repo)?;
    let index = Index::load(&index_path).context("loading index")?;

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
        // <merge-program> <base-oid> <ours-oid> <theirs-oid> <path> <base-mode> <ours-mode> <theirs-mode>
        // If a stage is missing, pass empty argument for both oid and mode.
        let (oid1, mode1) = ue.stages[0]
            .map(|(oid, mode)| (oid.to_hex(), format!("{mode:o}")))
            .unwrap_or_else(|| (String::new(), String::new()));
        let (oid2, mode2) = ue.stages[1]
            .map(|(oid, mode)| (oid.to_hex(), format!("{mode:o}")))
            .unwrap_or_else(|| (String::new(), String::new()));
        let (oid3, mode3) = ue.stages[2]
            .map(|(oid, mode)| (oid.to_hex(), format!("{mode:o}")))
            .unwrap_or_else(|| (String::new(), String::new()));

        let mut cmd = if args.merge_program == "git-merge-one-file" {
            let exe = std::env::current_exe().context("locating current executable")?;
            let mut c = Command::new(exe);
            c.arg("merge-one-file");
            c
        } else {
            Command::new(&args.merge_program)
        };

        let status = cmd
            .arg(&oid1)
            .arg(&oid2)
            .arg(&oid3)
            .arg(path_str.as_ref())
            .arg(&mode1)
            .arg(&mode2)
            .arg(&mode3)
            .status()
            .with_context(|| format!("running merge program {:?}", args.merge_program))?;

        if !status.success() {
            had_error = true;
            if !args.one_shot {
                break;
            }
        }
    }

    if had_error {
        std::process::exit(1);
    }

    Ok(())
}

fn effective_index_path(repo: &Repository) -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Ok(path);
        }
        let cwd = std::env::current_dir().context("resolving GIT_INDEX_FILE")?;
        return Ok(cwd.join(path));
    }
    Ok(repo.index_path())
}
