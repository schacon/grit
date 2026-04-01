//! `grit show-index` command.
//!
//! Reads a pack index file from stdin and prints each entry: offset, OID, and
//! (for version-2 indexes) a CRC32 field.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use grit_lib::pack::show_index_entries;

/// Arguments for `grit show-index`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Hash algorithm (only `sha1` is supported; accepted for compatibility).
    #[arg(long = "object-format")]
    pub object_format: Option<String>,
}

/// Run `grit show-index`.
///
/// Reads the `.idx` file from standard input and prints one line per object:
///
/// - Version 1: `<offset> <oid>`
/// - Version 2: `<offset> <oid> (<crc32>)`
pub fn run(args: Args) -> Result<()> {
    if let Some(fmt) = &args.object_format {
        if fmt != "sha1" {
            bail!("unsupported object format: {fmt}");
        }
    }

    let mut stdin = std::io::stdin();
    let entries = show_index_entries(&mut stdin, 20)?;

    for entry in entries {
        if let Some(crc) = entry.crc32 {
            println!("{} {} ({:08x})", entry.offset, entry.oid, crc);
        } else {
            println!("{} {}", entry.offset, entry.oid);
        }
    }

    Ok(())
}
