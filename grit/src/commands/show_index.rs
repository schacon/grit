//! `grit show-index` command.
//!
//! Reads a pack index file from stdin and prints each entry: offset, OID, and
//! (for version-2 indexes) a CRC32 field.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use grit_lib::pack::{oid_bytes_to_hex, show_index_entries};

/// Arguments for `grit show-index`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Hash algorithm for OID width (`sha1` = 20 bytes, `sha256` = 32).
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
    let hash_size = match args.object_format.as_deref() {
        None | Some("sha1") => 20usize,
        Some("sha256") => 32usize,
        Some(fmt) => bail!("unsupported object format: {fmt}"),
    };

    let mut stdin = std::io::stdin();
    let entries = show_index_entries(&mut stdin, hash_size)?;

    for entry in entries {
        let oid_hex = oid_bytes_to_hex(&entry.oid);
        if let Some(crc) = entry.crc32 {
            println!("{} {} ({:08x})", entry.offset, oid_hex, crc);
        } else {
            println!("{} {}", entry.offset, oid_hex);
        }
    }

    Ok(())
}
