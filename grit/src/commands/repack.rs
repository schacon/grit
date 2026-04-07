//! `grit repack` — pack unpacked objects into a new pack.
//!
//! Runs [`pack_objects`](crate::commands::pack_objects) with `--all`. With **`-d`**, removes
//! older `pack-*.pack` / `pack-*.idx` pairs (skipping any `pack-*.keep`) and refreshes
//! [`objects/info/packs`](crate::commands::update_server_info::refresh_objects_info_packs).

use crate::commands::update_server_info;
use crate::grit_exe;
use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// Arguments for `grit repack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Pack unpacked objects in a repository")]
pub struct Args {
    /// Remove redundant packs after repacking (keeps the pack created by this run).
    #[arg(short = 'd')]
    pub delete_old: bool,

    /// Pass `--local` to pack-objects (accepted for compat).
    #[arg(short = 'l')]
    pub local: bool,

    /// Pack everything in all packs (accepted; pack-objects `--all` is always used).
    #[arg(short = 'a')]
    pub all: bool,

    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Pass `--no-reuse-delta` (accepted; forwarded to pack-objects).
    #[arg(short = 'f')]
    pub force: bool,

    /// Use deeper delta compression (same as `git gc --aggressive`).
    #[arg(long)]
    pub aggressive: bool,

    #[arg(long)]
    pub window: Option<i64>,

    #[arg(long)]
    pub depth: Option<i64>,

    /// Write cruft pack (accepted; forwarded to pack-objects).
    #[arg(long)]
    pub cruft: bool,

    #[arg(long = "no-cruft")]
    pub no_cruft: bool,

    #[arg(long = "keep-largest-pack")]
    pub keep_largest_pack: bool,

    /// Extra arguments (ignored).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub rest: Vec<String>,
}

/// Run `grit repack`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let grit_bin = grit_exe::grit_executable();

    let pack_base = if repo.work_tree.is_some() {
        ".git/objects/pack/pack"
    } else {
        "objects/pack/pack"
    };

    let pack_dir_abs = repo.git_dir.join("objects").join("pack");

    let mut cmd = Command::new(&grit_bin);
    cmd.current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .arg("pack-objects")
        .arg("--all")
        .arg(pack_base);
    if args.quiet {
        cmd.arg("-q");
    }
    if args.aggressive {
        cmd.arg("-f");
        cmd.arg("--window").arg("250");
        cmd.arg("--depth").arg("250");
    } else {
        if args.force {
            cmd.arg("-f");
        }
        if let Some(w) = args.window {
            cmd.arg("--window").arg(w.to_string());
        }
        if let Some(d) = args.depth {
            cmd.arg("--depth").arg(d.to_string());
        }
    }
    if args.cruft {
        cmd.arg("--cruft");
    }
    if args.no_cruft {
        cmd.arg("--no-cruft");
    }
    if args.keep_largest_pack {
        cmd.arg("--keep-largest-pack");
    }
    let output = cmd.output().context("failed to run grit pack-objects")?;
    if !output.status.success() {
        anyhow::bail!("pack-objects failed with status {}", output.status);
    }

    let hash = output
        .stdout
        .split(|b| *b == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .context("pack-objects did not print a pack hash on stdout")?;

    let new_pack_name = format!("pack-{hash}.pack");

    if args.delete_old {
        remove_superseded_packs(&pack_dir_abs, &new_pack_name)?;
        update_server_info::refresh_objects_info_packs(&repo)?;
    }

    Ok(())
}

/// Deletes every `pack-*.pack` in `pack_dir` except `keep_pack_name`, unless a matching
/// `pack-*.keep` file exists for that pack.
fn remove_superseded_packs(pack_dir: &Path, keep_pack_name: &str) -> Result<()> {
    let rd = match fs::read_dir(pack_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    for entry in rd {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".pack") {
            continue;
        }
        if name == keep_pack_name {
            continue;
        }
        let stem = name
            .strip_suffix(".pack")
            .unwrap_or(name.as_str())
            .to_string();
        if pack_dir.join(format!("{stem}.keep")).exists() {
            continue;
        }
        let pack_path = pack_dir.join(&name);
        let idx_path = pack_dir.join(format!("{stem}.idx"));
        let _ = fs::remove_file(&pack_path);
        let _ = fs::remove_file(&idx_path);
    }

    Ok(())
}
