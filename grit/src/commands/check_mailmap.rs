//! `grit check-mailmap` — show canonical names/emails from .mailmap.
//!
//! Reads the `.mailmap` file (if present) in the repository root and maps
//! author/committer identities to their canonical forms.
//!
//! Usage:
//!   grit check-mailmap "Name <email>"
//!   grit check-mailmap --stdin < identities.txt

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::mailmap::{
    load_mailmap_raw, map_contact, parse_contact, parse_mailmap, read_mailmap_blob, render_contact,
};
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit check-mailmap`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Show canonical name/email from .mailmap",
    override_usage = "grit check-mailmap [--stdin] <contact>..."
)]
pub struct Args {
    /// Read contacts from stdin, one per line.
    #[arg(long = "stdin")]
    pub stdin: bool,

    /// Read additional mappings from a specific mailmap file.
    #[arg(long = "mailmap-file")]
    pub mailmap_file: Option<String>,

    /// Read additional mappings from a blob object.
    #[arg(long = "mailmap-blob")]
    pub mailmap_blob: Option<String>,

    /// Contact strings to look up (format: "Name <email>" or "<email>").
    pub contacts: Vec<String>,
}

fn resolve_mailmap_path(base: &Path, value: &str) -> PathBuf {
    let candidate = Path::new(value);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    }
}

fn read_optional_mailmap_file(path: &Path) -> Result<String> {
    if path.exists() {
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
    } else {
        Ok(String::new())
    }
}

/// Run the `check-mailmap` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)?;
    let mut mailmap_content = load_mailmap_raw(&repo)?;

    let base_dir = repo
        .work_tree
        .as_deref()
        .unwrap_or(repo.git_dir.as_path())
        .to_path_buf();

    if let Some(ref file) = args.mailmap_file {
        mailmap_content.push_str(&read_optional_mailmap_file(&resolve_mailmap_path(
            &base_dir, file,
        ))?);
        if !mailmap_content.ends_with('\n') && !mailmap_content.is_empty() {
            mailmap_content.push('\n');
        }
    }
    if let Some(ref blob) = args.mailmap_blob {
        mailmap_content.push_str(&read_mailmap_blob(&repo, blob)?);
        if !mailmap_content.ends_with('\n') && !mailmap_content.is_empty() {
            mailmap_content.push('\n');
        }
    }

    let mailmap = parse_mailmap(&mailmap_content);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if !args.stdin && args.contacts.is_empty() {
        bail!("usage: grit check-mailmap [--stdin] <contact>...");
    }

    for contact in &args.contacts {
        let (name, email) = parse_contact(contact);
        let (cn, ce) = map_contact(name.as_deref(), email.as_deref(), &mailmap);
        writeln!(out, "{}", render_contact(&cn, &ce))?;
    }

    if args.stdin {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.context("reading stdin")?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (name, email) = parse_contact(line);
            let (cn, ce) = map_contact(name.as_deref(), email.as_deref(), &mailmap);
            writeln!(out, "{}", render_contact(&cn, &ce))?;
        }
    }

    Ok(())
}
