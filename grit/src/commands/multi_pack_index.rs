//! `grit multi-pack-index` — manage multi-pack index files.
//!
//! [`verify`](MpiCommand::Verify) checks active MIDX layer(s) (root file or chain in
//! `multi-pack-index.d`). [`write`](MpiCommand::Write) builds a new MIDX from pack indexes,
//! including incremental split layout when `--incremental` is set.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::midx::{write_multi_pack_index_with_options, WriteMultiPackIndexOptions};
use grit_lib::repo::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::grit_exe;

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
    /// Build a new multi-pack index from existing pack indexes.
    Write(WriteArgs),
    /// Run `grit repack -d`, then write the multi-pack index.
    Repack(RepackArgs),
    /// Rewrite the multi-pack index from all packs (no incremental chain merge).
    Compact(CompactArgs),
}

#[derive(Debug, ClapArgs)]
pub struct VerifyArgs {}

#[derive(Debug, ClapArgs)]
pub struct WriteArgs {
    /// Write an incremental MIDX layer (split layout under `multi-pack-index.d`).
    #[arg(long)]
    pub incremental: bool,
    /// Write placeholder bitmap sidecar (compat with Git `--bitmap`).
    #[arg(long)]
    pub bitmap: bool,
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
        MpiCommand::Write(w) => cmd_write(&repo, &w),
        MpiCommand::Repack(a) => cmd_repack(&repo, &a),
        MpiCommand::Compact(_) => cmd_compact(&repo),
    }
}

/// Parse argv when clap would reject unknown global flags (e.g. `--object-dir` before `write`).
pub fn run_from_argv(argv: &[String]) -> Result<()> {
    let mut object_dir: Option<PathBuf> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].as_str();
        if a == "--object-dir" {
            let Some(val) = argv.get(i + 1) else {
                bail!("--object-dir requires a path");
            };
            object_dir = Some(PathBuf::from(val));
            i += 2;
            continue;
        }
        if let Some(val) = a.strip_prefix("--object-dir=") {
            object_dir = Some(PathBuf::from(val));
            i += 1;
            continue;
        }
        rest.push(argv[i].clone());
        i += 1;
    }

    if let Some(dir) = object_dir {
        std::env::set_var("GIT_OBJECT_DIRECTORY", dir);
    }

    let sub = rest.first().map(|s| s.as_str()).unwrap_or("");
    let repo = Repository::discover(None).context("not a git repository")?;
    match sub {
        "write" => {
            let mut incremental = false;
            let mut bitmap = false;
            for a in rest.iter().skip(1) {
                match a.as_str() {
                    "--incremental" => incremental = true,
                    "--bitmap" => bitmap = true,
                    other => bail!("unsupported multi-pack-index write option: {other}"),
                }
            }
            cmd_write(
                &repo,
                &WriteArgs {
                    incremental,
                    bitmap,
                },
            )
        }
        "verify" => cmd_verify(&repo),
        "repack" => {
            let mut no_progress = false;
            for a in rest.iter().skip(1) {
                if a == "--no-progress" {
                    no_progress = true;
                } else {
                    bail!("unsupported multi-pack-index repack option: {a}");
                }
            }
            cmd_repack(&repo, &RepackArgs { no_progress })
        }
        "compact" => {
            if rest.len() > 1 {
                bail!("unsupported multi-pack-index compact arguments");
            }
            cmd_compact(&repo)
        }
        other => bail!("unsupported multi-pack-index subcommand: {other}"),
    }
}

fn objects_dir_for_repo(repo: &Repository) -> PathBuf {
    if let Ok(rel) = std::env::var("GIT_OBJECT_DIRECTORY") {
        let base = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
        base.join(rel)
    } else {
        repo.git_dir.join("objects")
    }
}

fn pack_dir(repo: &Repository) -> PathBuf {
    objects_dir_for_repo(repo).join("pack")
}

fn cmd_write(repo: &Repository, args: &WriteArgs) -> Result<()> {
    let write_rev = std::env::var("GIT_TEST_MIDX_WRITE_REV").ok().as_deref() == Some("1");
    write_multi_pack_index_with_options(
        &pack_dir(repo),
        &WriteMultiPackIndexOptions {
            preferred_pack_idx: None,
            write_bitmap_placeholders: args.bitmap,
            incremental: args.incremental,
            write_rev_placeholder: write_rev,
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn cmd_repack(repo: &Repository, args: &RepackArgs) -> Result<()> {
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let mut cmd = Command::new(grit_exe::grit_executable());
    cmd.current_dir(work_dir).args(["repack", "-d"]);
    if args.no_progress {
        cmd.arg("-q");
    }
    let status = cmd
        .status()
        .context("failed to run grit repack for multi-pack-index")?;
    if !status.success() {
        bail!("repack failed with status {status}");
    }
    write_multi_pack_index_with_options(&pack_dir(repo), &WriteMultiPackIndexOptions::default())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn cmd_compact(repo: &Repository) -> Result<()> {
    write_multi_pack_index_with_options(&pack_dir(repo), &WriteMultiPackIndexOptions::default())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn midx_chain_path(pack_dir: &Path) -> PathBuf {
    pack_dir
        .join("multi-pack-index.d")
        .join("multi-pack-index-chain")
}

fn cmd_verify(repo: &Repository) -> Result<()> {
    let pd = pack_dir(repo);
    let root = pd.join("multi-pack-index");
    let chain = midx_chain_path(&pd);
    if root.exists() {
        let data = fs::read(&root).with_context(|| format!("could not read {}", root.display()))?;
        verify_midx_header_bytes(&data).with_context(|| format!("{}", root.display()))?;
        return Ok(());
    }
    if chain.exists() {
        let contents = fs::read_to_string(&chain)
            .with_context(|| format!("could not read {}", chain.display()))?;
        let midx_d = pd.join("multi-pack-index.d");
        for line in contents.lines() {
            let h = line.trim();
            if h.is_empty() {
                continue;
            }
            let path = midx_d.join(format!("multi-pack-index-{h}.midx"));
            let data =
                fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
            verify_midx_header_bytes(&data).with_context(|| format!("{}", path.display()))?;
        }
        return Ok(());
    }
    bail!(
        "no multi-pack-index at {} or chain at {}",
        root.display(),
        chain.display()
    );
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
