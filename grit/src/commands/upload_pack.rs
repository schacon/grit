//! `grit upload-pack` — send objects for fetch (server side).
//!
//! Invoked on the remote side of a fetch.  Advertises refs, then responds
//! to want/have negotiation and sends requested objects.
//! Only local transport is supported.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit upload-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send objects for fetch (server side)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let repo = open_repo(&args.directory)
        .with_context(|| format!("could not open repository at '{}'", args.directory.display()))?;

    // Phase 1: Advertise refs
    advertise_refs(&repo.git_dir)?;

    // Flush packet
    println!("0000");

    // Phase 2: Read want/have lines from stdin
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut wants: HashSet<ObjectId> = HashSet::new();
    let mut haves: HashSet<ObjectId> = HashSet::new();

    while let Some(Ok(line)) = lines.next() {
        let line = line.trim().to_string();
        if line.is_empty() || line == "0000" || line == "done" {
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

    if wants.is_empty() {
        // Nothing requested
        return Ok(());
    }

    // Phase 3: Send ACK for common objects
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for have in &haves {
        // Check if we have the object
        if repo.odb.read(have).is_ok() {
            writeln!(out, "ACK {}", have.to_hex())?;
        }
    }
    writeln!(out, "NAK")?;

    // Phase 4: Send pack data containing wanted objects
    // For local transport, objects can be read directly.
    // We write loose objects for each wanted OID to stdout as a simple
    // object listing (real git would send a packfile here).
    // For now, list the objects that should be sent.
    for want in &wants {
        if repo.odb.read(want).is_ok() {
            writeln!(out, "{}", want.to_hex())?;
        }
    }

    out.flush()?;
    Ok(())
}

/// Advertise all refs in the repository to stdout.
fn advertise_refs(git_dir: &Path) -> Result<()> {
    // HEAD first
    if let Ok(head_oid) = refs::resolve_ref(git_dir, "HEAD") {
        println!("{}\tHEAD", head_oid.to_hex());
    }

    // All refs
    let all_refs = list_all_refs(git_dir)?;
    for (refname, oid) in &all_refs {
        println!("{}\t{}", oid.to_hex(), refname);
    }

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
