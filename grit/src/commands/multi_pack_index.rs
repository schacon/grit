//! `grit multi-pack-index` — manage multi-pack index files.
//!
//! [`verify`](MpiCommand::Verify) checks the `objects/pack/multi-pack-index` header
//! (signature and version). Other subcommands are not implemented yet.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::repo::Repository;
use std::fs;

/// Arguments for `grit multi-pack-index`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage multi-pack index")]
pub struct Args {
    #[command(subcommand)]
    pub command: MpiCommand,
}

#[derive(Debug, Subcommand)]
pub enum MpiCommand {
    /// Check the MIDX file for consistency (header and version).
    Verify(VerifyArgs),
    /// Build a new multi-pack index (not implemented).
    Write(WriteArgs),
    /// Repack using the MIDX (not implemented).
    Repack(RepackArgs),
    /// Compact incremental MIDX layers (not implemented).
    Compact(CompactArgs),
}

#[derive(Debug, ClapArgs)]
pub struct VerifyArgs {}

#[derive(Debug, ClapArgs)]
pub struct WriteArgs {
    /// Write an incremental MIDX (accepted for compat).
    #[arg(long)]
    pub incremental: bool,
}

#[derive(Debug, ClapArgs)]
pub struct RepackArgs {
    /// Suppress progress (accepted for compat).
    #[arg(long = "no-progress")]
    pub no_progress: bool,
}

#[derive(Debug, ClapArgs)]
pub struct CompactArgs {}

/// Run `grit multi-pack-index`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    match args.command {
        MpiCommand::Verify(_) => cmd_verify(&repo),
        MpiCommand::Write(_) => bail!("not implemented: grit multi-pack-index write"),
        MpiCommand::Repack(_) => bail!("not implemented: grit multi-pack-index repack"),
        MpiCommand::Compact(_) => bail!("not implemented: grit multi-pack-index compact"),
    }
}

fn midx_path(repo: &Repository) -> std::path::PathBuf {
    repo.git_dir
        .join("objects")
        .join("pack")
        .join("multi-pack-index")
}

fn cmd_verify(repo: &Repository) -> Result<()> {
    let path = midx_path(repo);
    let data = fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
    verify_midx_header_bytes(&data).with_context(|| format!("{}", path.display()))?;
    Ok(())
}

/// Validates the leading bytes of a multi-pack-index file.
pub fn verify_midx_header_bytes(data: &[u8]) -> Result<()> {
    const MIDX_SIGNATURE: u32 = 0x4d49_4458; // b"MIDX"

    if data.len() < 12 {
        bail!("multi-pack-index file too small");
    }
    let sig = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if sig != MIDX_SIGNATURE {
        bail!("bad multi-pack-index signature");
    }
    let version = data[4];
    if version != 1 && version != 2 {
        bail!("unsupported multi-pack-index version {version}");
    }
    let hash_version = data[5];
    if hash_version != 1 {
        bail!("unsupported hash version {hash_version} in multi-pack-index");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_rejects_too_short() {
        assert!(verify_midx_header_bytes(&[0u8; 8]).is_err());
    }

    #[test]
    fn verify_accepts_minimal_v1_header() {
        let mut v = vec![0u8; 12];
        v[0..4].copy_from_slice(b"MIDX");
        v[4] = 1;
        v[5] = 1;
        assert!(verify_midx_header_bytes(&v).is_ok());
    }
}
