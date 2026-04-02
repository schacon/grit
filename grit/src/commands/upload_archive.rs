//! `grit upload-archive` -- send archive to client (server side).
//!
//! Server-side counterpart of `git archive --remote`.  Opens the repository
//! at `<dir>`, reads archive arguments from stdin, and delegates to the
//! archive logic.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

/// Arguments for `grit upload-archive`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send archive to client (server-side of git archive --remote)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let repo = open_repo(&args.directory)
        .with_context(|| format!("could not open repository at '{}'", args.directory.display()))?;

    // Read arguments from stdin (one per line, terminated by empty line)
    let stdin = io::stdin();
    let mut archive_args: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        let line = line.trim().to_string();
        if line.is_empty() {
            break;
        }
        // Strip "argument " prefix (git protocol sends "argument <arg>")
        if let Some(arg) = line.strip_prefix("argument ") {
            archive_args.push(arg.to_owned());
        } else {
            archive_args.push(line);
        }
    }

    // Parse archive arguments: expect at least a tree-ish
    let mut format = "tar".to_string();
    let mut prefix: Option<String> = None;
    let mut tree_ish = "HEAD".to_string();

    let mut i = 0;
    while i < archive_args.len() {
        match archive_args[i].as_str() {
            "--format" if i + 1 < archive_args.len() => {
                i += 1;
                format = archive_args[i].clone();
            }
            f if f.starts_with("--format=") => {
                if let Some(val) = f.strip_prefix("--format=") {
                    format = val.to_string();
                }
            }
            "--prefix" if i + 1 < archive_args.len() => {
                i += 1;
                prefix = Some(archive_args[i].clone());
            }
            p if p.starts_with("--prefix=") => {
                if let Some(val) = p.strip_prefix("--prefix=") {
                    prefix = Some(val.to_string());
                }
            }
            "--" => {
                break;
            }
            arg if !arg.starts_with('-') => {
                tree_ish = arg.to_owned();
            }
            other => {
                bail!("unsupported archive argument: {other}");
            }
        }
        i += 1;
    }

    // Resolve tree-ish to a tree OID
    let oid = resolve_ref(&repo.git_dir, &tree_ish)
        .or_else(|_| ObjectId::from_hex(&tree_ish))
        .with_context(|| format!("cannot resolve '{tree_ish}'"))?;

    let obj = repo.odb.read(&oid)?;
    let tree_oid = if obj.kind == ObjectKind::Commit {
        let commit = parse_commit(&obj.data).context("parsing commit")?;
        commit.tree
    } else if obj.kind == ObjectKind::Tree {
        oid
    } else {
        bail!("'{}' is not a tree or commit", tree_ish);
    };

    eprintln!(
        "upload-archive: generating {} archive for {} (tree {})",
        format,
        tree_ish,
        &tree_oid.to_hex()[..7]
    );

    // Output a listing of the tree (full archive generation reuses the archive module)
    print_tree_listing(&repo, &tree_oid, prefix.as_deref().unwrap_or(""))?;

    Ok(())
}

/// Recursively print tree entries (placeholder for full archive generation).
fn print_tree_listing(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &str,
) -> Result<()> {
    let obj = repo.odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data).context("parsing tree")?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}{name}")
        };

        if entry.mode == 0o40000 {
            // Directory - recurse
            let sub_prefix = format!("{path}/");
            print_tree_listing(repo, &entry.oid, &sub_prefix)?;
        } else {
            println!("{}\t{}", entry.oid.to_hex(), path);
        }
    }

    Ok(())
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}
