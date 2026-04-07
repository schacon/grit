//! `grit upload-pack` — send objects for fetch (server side).
//!
//! Invoked on the remote side of a fetch. Advertises refs in pkt-line format,
//! negotiates want/have, then streams a packfile (side-band-64k) to the client.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::grit_exe::grit_executable;
use crate::pkt_line;

/// Arguments for `grit upload-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send objects for fetch (server side)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,

    /// Only advertise refs and capabilities, then exit.
    #[arg(long)]
    pub advertise_refs: bool,
}

pub fn run(args: Args) -> Result<()> {
    let repo = open_repo(&args.directory).with_context(|| {
        format!(
            "could not open repository at '{}'",
            args.directory.display()
        )
    })?;

    if args.advertise_refs {
        return advertise_refs_with_caps(&repo);
    }

    let mut out = io::stdout();
    write_ref_advertisement(&mut out, &repo.git_dir)?;
    pkt_line::write_flush(&mut out)?;
    out.flush()?;

    let mut stdin = io::stdin();
    let mut wants: HashSet<ObjectId> = HashSet::new();
    let mut haves: HashSet<ObjectId> = HashSet::new();
    let mut seen_done = false;

    loop {
        match pkt_line::read_packet(&mut stdin)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                if line == "done" {
                    seen_done = true;
                    break;
                }
                if let Some(rest) = line.strip_prefix("want ") {
                    let hex = rest.split_whitespace().next().unwrap_or(rest);
                    if let Ok(oid) = ObjectId::from_hex(hex) {
                        wants.insert(oid);
                    }
                } else if let Some(rest) = line.strip_prefix("have ") {
                    let hex = rest.trim();
                    if let Ok(oid) = ObjectId::from_hex(hex) {
                        haves.insert(oid);
                    }
                }
            }
            _ => {}
        }
    }

    if !seen_done {
        while let Some(pkt) = pkt_line::read_packet(&mut stdin)? {
            if let pkt_line::Packet::Data(line) = pkt {
                if line == "done" {
                    break;
                }
            }
        }
    }

    if wants.is_empty() {
        return Ok(());
    }

    for have in &haves {
        if repo.odb.read(have).is_ok() {
            pkt_line::write_line(&mut out, &format!("ACK {}", have.to_hex()))?;
        }
    }
    pkt_line::write_line(&mut out, "NAK")?;
    out.flush()?;

    let grit = grit_executable();
    let mut child = Command::new(&grit)
        .arg("pack-objects")
        .arg("--stdout")
        .current_dir(&repo.git_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn '{} pack-objects'", grit.display()))?;

    {
        let mut pin = child.stdin.take().context("pack-objects stdin")?;
        for w in &wants {
            writeln!(pin, "{}", w.to_hex())?;
        }
        pin.flush()?;
    }

    let mut pack_out = child.stdout.take().context("pack-objects stdout")?;
    let stderr_child = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        if let Some(mut e) = stderr_child {
            let mut buf = Vec::new();
            let _ = e.read_to_end(&mut buf);
            buf
        } else {
            Vec::new()
        }
    });

    const CHUNK: usize = 32000;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = pack_out.read(&mut buf)?;
        if n == 0 {
            break;
        }
        write_sideband_64k(&mut out, &buf[..n])?;
    }

    let status = child.wait()?;
    let err_bytes = stderr_handle.join().unwrap_or_default();
    if !err_bytes.is_empty() {
        let _ = io::stderr().write_all(&err_bytes);
    }
    if !status.success() {
        bail!(
            "pack-objects failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }

    pkt_line::write_flush(&mut out)?;
    out.flush()?;
    Ok(())
}

fn write_sideband_64k(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    const MAX_PAYLOAD: usize = 65515;
    for chunk in payload.chunks(MAX_PAYLOAD) {
        let len = 4 + 1 + chunk.len();
        write!(w, "{len:04x}")?;
        w.write_all(&[1u8])?;
        w.write_all(chunk)?;
    }
    Ok(())
}

fn write_ref_advertisement(w: &mut impl Write, git_dir: &Path) -> Result<()> {
    let version = crate::version_string();
    let caps = format!(
        "multi_ack thin-pack side-band side-band-64k ofs-delta shallow deepen-since deepen-not \
         deepen-relative no-progress include-tag multi_ack_detailed allow-tip-sha1-in-want \
         allow-reachable-sha1-in-want no-done symref=HEAD:{} filter object-format=sha1 \
         agent=git/{} ref-in-want",
        refs::read_symbolic_ref(git_dir, "HEAD")
            .ok()
            .flatten()
            .unwrap_or_else(|| "refs/heads/main".to_owned()),
        version,
    );

    let mut first = true;
    if let Ok(head_oid) = refs::resolve_ref(git_dir, "HEAD") {
        let line = format!("{}\tHEAD\0{}\n", head_oid.to_hex(), caps);
        let len = 4 + line.len();
        write!(w, "{:04x}{}", len, line)?;
        first = false;
    }

    let all_refs = list_all_refs(git_dir)?;
    for (refname, oid) in &all_refs {
        if first {
            let line = format!("{}\t{}\0{}\n", oid.to_hex(), refname, caps);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
            first = false;
        } else {
            let line = format!("{}\t{}\n", oid.to_hex(), refname);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
        }
    }

    Ok(())
}

/// Advertise refs with capabilities in pkt-line format (for --advertise-refs).
fn advertise_refs_with_caps(repo: &Repository) -> Result<()> {
    let mut out = io::stdout();
    write_ref_advertisement(&mut out, &repo.git_dir)?;
    write!(out, "0000")?;
    out.flush()?;
    Ok(())
}

/// List all refs under refs/.
fn list_all_refs(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let mut result = Vec::new();
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Ok(entries) = refs::list_refs(git_dir, prefix) {
            result.extend(entries);
        }
    }
    Ok(result)
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}
