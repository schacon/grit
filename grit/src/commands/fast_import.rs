//! `grit fast-import` — import from a fast-export stream.
//!
//! Supports a minimal subset: `blob` objects with `data <n>` payloads, and `done`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectKind;
use grit_lib::repo::Repository;
use std::io::{self, BufRead, Read};

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments (reserved for future import options).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-import`.
pub fn run(_args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "done" {
            break;
        }
        if trimmed == "blob" {
            line.clear();
            reader
                .read_line(&mut line)
                .context("reading data line after blob")?;
            let data_line = line.trim_end();
            let rest = data_line
                .strip_prefix("data ")
                .ok_or_else(|| anyhow::anyhow!("expected 'data <n>' after blob"))?;
            let size: usize = rest
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid data size: {e}"))?;
            let mut payload = vec![0u8; size];
            reader
                .read_exact(&mut payload)
                .context("reading blob payload")?;
            let oid = repo.odb.write(ObjectKind::Blob, &payload)?;
            eprintln!("fast-import: wrote blob {oid}");
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        return Err(anyhow::anyhow!(
            "unsupported fast-import command: {trimmed}"
        ));
    }
    Ok(())
}
