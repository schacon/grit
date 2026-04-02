//! `grit get-tar-commit-id` — extract commit ID from a tar archive.
//!
//! Reads a tar archive (from stdin) created by `git archive` and extracts
//! the commit SHA from the pax extended header.  The commit is stored in
//! a global pax header with keyword `comment`.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Read};

/// Arguments for `grit get-tar-commit-id`.
#[derive(Debug, ClapArgs)]
pub struct Args {}

/// Run `grit get-tar-commit-id`.
pub fn run(_args: Args) -> Result<()> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;

    // A tar archive produced by `git archive` has a pax global extended
    // header (typeflag 'g') in the first entry.  The header block is 512
    // bytes, followed by one or more 512-byte data blocks containing the
    // pax key=value records.
    //
    // We look for the `comment=<40-hex-char SHA>\n` record.

    if buf.len() < 1024 {
        // Too small to contain a pax header + data block.
        return Ok(());
    }

    // Check typeflag at offset 156 in the first 512-byte header block.
    let typeflag = buf[156];
    if typeflag != b'g' {
        // Not a pax global extended header — no commit stored.
        return Ok(());
    }

    // The data blocks follow the 512-byte header.  Parse the size field
    // (bytes 124..136, octal, NUL/space-terminated) to know how much
    // pax data to read.
    let size_str = std::str::from_utf8(&buf[124..136])
        .unwrap_or("")
        .trim_matches(|c: char| c == '\0' || c == ' ');
    let size = usize::from_str_radix(size_str, 8).unwrap_or(0);

    if size == 0 || 512 + size > buf.len() {
        return Ok(());
    }

    let pax_data = &buf[512..512 + size];
    let pax_str = String::from_utf8_lossy(pax_data);

    // Pax records are: "<length> <keyword>=<value>\n"
    for line in pax_str.split('\n') {
        // Find the keyword=value part (after the first space)
        if let Some(kv) = line.split_once(' ').map(|(_, kv)| kv) {
            if let Some(value) = kv.strip_prefix("comment=") {
                let sha = value.trim();
                if !sha.is_empty() {
                    println!("{sha}");
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}
