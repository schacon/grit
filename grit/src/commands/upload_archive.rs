//! `grit upload-archive` — server side of `git archive --remote`.
//!
//! Reads pkt-line framed `argument ...` lines from stdin, replies with `ACK` + flush, then emits
//! the archive on sideband channel 1 (matching Git's upload-archive child).

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::commands::archive::{
    archive_bytes_for_repo, parse_archive_argv, tar_filters_from_config, token_format, ArchiveToken,
};
use crate::pkt_line;

/// Arguments for `grit upload-archive`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send archive to client (server-side of git archive --remote)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,
}

/// Run `grit upload-archive` (invoked as `git upload-archive <dir>`).
pub fn run(args: Args) -> Result<()> {
    let repo = open_repo(&args.directory).with_context(|| {
        format!(
            "could not open repository at '{}'",
            args.directory.display()
        )
    })?;

    let archive_args = read_argument_packets()?;
    let mut rest = archive_args;
    if rest.first().is_some_and(|s| s == "archive") {
        rest.remove(0);
    }

    let parsed = parse_archive_argv(&rest)?;

    if parsed
        .tokens
        .iter()
        .any(|t| matches!(t, ArchiveToken::List))
    {
        if parsed.tree_ish.is_some() || !parsed.pathspecs.is_empty() {
            bail!("extra parameter to git archive --list");
        }
        let mut list_out = Vec::new();
        write_list_formats(&mut list_out, true)?;
        respond_ack_and_send(&list_out)?;
        return Ok(());
    }

    let tree_ish = parsed
        .tree_ish
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("must specify tree-ish"))?;

    let format = token_format(&parsed)
        .map(str::to_string)
        .unwrap_or_else(|| "tar".to_string());

    let bytes = archive_bytes_for_repo(&repo, &parsed, tree_ish, &format, true)?;
    respond_ack_and_send(&bytes)?;
    Ok(())
}

fn read_argument_packets() -> Result<Vec<String>> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut out = Vec::new();
    loop {
        match pkt_line::read_packet(&mut input)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let arg = line.strip_prefix("argument ").unwrap_or(&line).to_string();
                out.push(arg);
            }
            Some(other) => bail!("upload-archive: unexpected packet: {other:?}"),
        }
    }
    Ok(out)
}

fn write_list_formats(w: &mut impl Write, remote: bool) -> Result<()> {
    use grit_lib::config::ConfigSet;
    let config = ConfigSet::load(None, true).unwrap_or_default();
    writeln!(w, "tar")?;
    writeln!(w, "zip")?;
    for (name, _, rem) in tar_filters_from_config(&config) {
        if !remote || rem {
            writeln!(w, "{name}")?;
        }
    }
    Ok(())
}

fn respond_ack_and_send(payload: &[u8]) -> Result<()> {
    let mut out = io::stdout().lock();
    pkt_line::write_line(&mut out, "ACK")?;
    pkt_line::write_flush(&mut out)?;
    pkt_line::write_sideband_channel1_64k(&mut out, payload)?;
    pkt_line::write_flush(&mut out)?;
    out.flush()?;
    Ok(())
}

fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}
