//! `grit pack-redundant` — find redundant pack files.
//!
//! Lists pack files where every object also exists in at least one other
//! pack file.  Output: one `.pack` path per line (suitable for piping
//! to `xargs rm`).

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::collections::HashSet;
use std::path::PathBuf;

use grit_lib::pack::read_local_pack_indexes;
use grit_lib::repo::Repository;

/// Arguments for `grit pack-redundant`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Also consider alternate object databases.
    #[arg(long)]
    pub alt_odb: bool,

    /// Print all packs (for debugging).
    #[arg(long)]
    pub all: bool,

    /// Verbose output.
    #[arg(short, long)]
    pub verbose: bool,
}

/// Run `grit pack-redundant`.
pub fn run(args: Args) -> Result<()> {
    // NOTE: git itself has deprecated pack-redundant and recommends `repack -d` instead.
    eprintln!("warning: git pack-redundant is deprecated; use 'git repack -d' instead");

    let repo = Repository::discover(None).context("not a git repository")?;
    let objects_dir = repo.odb.objects_dir();

    let indexes = read_local_pack_indexes(objects_dir)
        .context("reading pack indexes")?;

    if indexes.len() < 2 {
        // With 0 or 1 packs, nothing can be redundant.
        return Ok(());
    }

    // Collect OIDs per pack.
    let packs: Vec<(PathBuf, HashSet<[u8; 20]>)> = indexes
        .iter()
        .map(|idx| {
            let oids: HashSet<[u8; 20]> = idx.entries.iter().map(|e| *e.oid.as_bytes()).collect();
            (idx.pack_path.clone(), oids)
        })
        .collect();

    // For each pack, check if all its objects exist in the union of
    // all *other* packs.
    let redundant = find_redundant(&packs);

    for path in &redundant {
        println!("{}", path.display());
    }

    Ok(())
}

fn find_redundant(packs: &[(PathBuf, HashSet<[u8; 20]>)]) -> Vec<PathBuf> {
    let mut result = Vec::new();

    for (i, (path, oids)) in packs.iter().enumerate() {
        // Build the union of all objects in *other* packs.
        let mut others: HashSet<[u8; 20]> = HashSet::new();
        for (j, (_, other_oids)) in packs.iter().enumerate() {
            if i != j {
                others.extend(other_oids);
            }
        }

        // Check if every OID in this pack exists elsewhere.
        if oids.iter().all(|oid| others.contains(oid)) {
            result.push(path.clone());
        }
    }

    result
}
