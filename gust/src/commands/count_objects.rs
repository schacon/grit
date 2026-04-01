//! `gust count-objects` command.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use gust_lib::pack::{collect_local_pack_info, read_alternates_recursive};
use gust_lib::repo::Repository;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

/// Arguments for `gust count-objects`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Verbose breakdown.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}

/// Run `gust count-objects`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;
    let objects_dir = repo.git_dir.join("objects");

    let (loose_count, loose_size, loose_ids) = scan_loose_objects(&objects_dir)?;
    if !args.verbose {
        println!("{} objects, {} kilobytes", loose_count, loose_size / 1024);
        return Ok(());
    }

    let pack_info = collect_local_pack_info(&objects_dir)?;
    let prune_packable = loose_ids.intersection(&pack_info.object_ids).count();
    let (garbage_count, garbage_size) = scan_pack_garbage(&objects_dir)?;
    let alternates = read_alternates_recursive(&objects_dir)?;

    println!("count: {loose_count}");
    println!("size: {}", loose_size / 1024);
    println!("in-pack: {}", pack_info.object_count);
    println!("packs: {}", pack_info.pack_count);
    println!("size-pack: {}", pack_info.size_bytes / 1024);
    println!("prune-packable: {prune_packable}");
    println!("garbage: {garbage_count}");
    println!("size-garbage: {}", garbage_size / 1024);
    for alt in alternates {
        println!("alternate: {}", alt.display());
    }
    Ok(())
}

fn scan_loose_objects(
    objects_dir: &Path,
) -> Result<(usize, u64, HashSet<gust_lib::objects::ObjectId>)> {
    let mut count = 0usize;
    let mut size = 0u64;
    let mut ids = HashSet::new();
    let rd = fs::read_dir(objects_dir).map_err(anyhow::Error::from)?;
    for entry in rd {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "pack" || name == "info" || name.len() != 2 {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let sub = fs::read_dir(&path)?;
        for file in sub {
            let file = file?;
            let file_name = file.file_name().to_string_lossy().to_string();
            if file_name.len() != 38 {
                continue;
            }
            let obj_path = file.path();
            let meta = match fs::metadata(&obj_path) {
                Ok(meta) => meta,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            };
            if !meta.is_file() {
                continue;
            }
            let hex = format!("{name}{file_name}");
            if let Ok(oid) = hex.parse() {
                ids.insert(oid);
            }
            count += 1;
            size += meta.len();
        }
    }
    Ok((count, size, ids))
}

fn scan_pack_garbage(objects_dir: &Path) -> Result<(usize, u64)> {
    let pack_dir = objects_dir.join("pack");
    let rd = match fs::read_dir(&pack_dir) {
        Ok(rd) => rd,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(err) => return Err(err.into()),
    };

    let mut pack_by_stem: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    let mut garbage_count = 0usize;
    let mut garbage_size = 0u64;

    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = fs::metadata(&path)?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();

        match ext {
            "pack" => {
                let e = pack_by_stem.entry(stem).or_insert((false, false));
                e.0 = true;
            }
            "idx" => {
                let e = pack_by_stem.entry(stem).or_insert((false, false));
                e.1 = true;
            }
            "keep" | "bitmap" | "rev" | "mtimes" | "promisor" | "midx" => {}
            _ => {
                garbage_count += 1;
                garbage_size += meta.len();
            }
        }

        if !meta.is_file() {
            garbage_count += 1;
            garbage_size += meta.len();
        }
        let _ = name;
    }

    for (_stem, (has_pack, has_idx)) in pack_by_stem {
        if has_pack ^ has_idx {
            garbage_count += 1;
        }
    }

    Ok((garbage_count, garbage_size))
}
