//! `gust verify-pack` command.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use gust_lib::pack::verify_pack_and_collect;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Arguments for `gust verify-pack`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show object list and delta histogram.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Show only delta histogram (and object list when `--verbose` is also set).
    #[arg(short = 's', long = "stat-only")]
    pub stat_only: bool,

    /// Hash algorithm selector (accepted for compatibility; currently ignored).
    #[arg(long = "object-format")]
    pub object_format: Option<String>,

    /// Pack index or pack path arguments.
    #[arg(value_name = "PACK", num_args = 1..)]
    pub packs: Vec<String>,
}

/// Run `gust verify-pack`.
pub fn run(args: Args) -> Result<()> {
    if let Some(fmt) = &args.object_format {
        if fmt != "sha1" {
            bail!("unsupported object format: {fmt}");
        }
    }

    let mut any_error = false;
    for input in &args.packs {
        let idx_path = normalize_to_idx(input);
        match verify_pack_and_collect(&idx_path) {
            Ok(records) => {
                if args.verbose && !args.stat_only {
                    for rec in &records {
                        if let Some(base_oid) = rec.base_oid {
                            println!(
                                "{} {} {} {} {} {} {}",
                                rec.oid,
                                rec.packed_type.as_str(),
                                rec.size,
                                rec.size_in_pack,
                                rec.offset,
                                rec.depth.unwrap_or(1),
                                base_oid
                            );
                        } else if let Some(depth) = rec.depth {
                            println!(
                                "{} {} {} {} {} {}",
                                rec.oid,
                                rec.packed_type.as_str(),
                                rec.size,
                                rec.size_in_pack,
                                rec.offset,
                                depth
                            );
                        } else {
                            println!(
                                "{} {} {} {} {}",
                                rec.oid,
                                rec.packed_type.as_str(),
                                rec.size,
                                rec.size_in_pack,
                                rec.offset
                            );
                        }
                    }
                }

                if args.verbose || args.stat_only {
                    let mut hist: BTreeMap<u64, usize> = BTreeMap::new();
                    for rec in &records {
                        let depth = rec.depth.unwrap_or(0);
                        *hist.entry(depth).or_insert(0) += 1;
                    }
                    for (depth, count) in hist {
                        println!("chain length = {depth}: {count} object(s)");
                    }
                    println!("{}: ok", normalize_to_pack(input).display());
                }
            }
            Err(_) => {
                any_error = true;
                if args.verbose || args.stat_only {
                    println!("{}: bad", normalize_to_pack(input).display());
                }
            }
        }
    }

    if any_error {
        std::process::exit(1);
    }
    Ok(())
}

fn normalize_to_idx(input: &str) -> PathBuf {
    let path = Path::new(input);
    let s = path.to_string_lossy();
    if s.ends_with(".idx") {
        return path.to_path_buf();
    }
    if s.ends_with(".pack") {
        let mut p = path.to_path_buf();
        p.set_extension("idx");
        return p;
    }
    let mut p = path.to_path_buf();
    p.set_extension("idx");
    p
}

fn normalize_to_pack(input: &str) -> PathBuf {
    let path = Path::new(input);
    let s = path.to_string_lossy();
    if s.ends_with(".pack") {
        return path.to_path_buf();
    }
    if s.ends_with(".idx") {
        let mut p = path.to_path_buf();
        p.set_extension("pack");
        return p;
    }
    let mut p = path.to_path_buf();
    p.set_extension("pack");
    p
}
