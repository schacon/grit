//! `gust cat-file` — provide contents or details of repository objects.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead, Write};

use gust_lib::objects::{parse_tree, ObjectId, ObjectKind};
use gust_lib::repo::Repository;

/// Arguments for `gust cat-file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show the object type.
    #[arg(short = 't', conflicts_with_all = ["size", "pretty", "batch", "batch_check", "batch_command"])]
    pub show_type: bool,

    /// Show the object size.
    #[arg(short = 's', conflicts_with_all = ["show_type", "pretty", "batch", "batch_check", "batch_command"])]
    pub size: bool,

    /// Pretty-print object contents.
    #[arg(short = 'p', conflicts_with_all = ["show_type", "size", "batch", "batch_check", "batch_command"])]
    pub pretty: bool,

    /// Check if the object exists (exit code 0 = yes).
    #[arg(short = 'e', conflicts_with_all = ["show_type", "size", "pretty", "batch", "batch_check", "batch_command"])]
    pub exists: bool,

    /// Allow missing objects (with -e, don't error).
    #[arg(long = "allow-unknown-type")]
    pub allow_unknown_type: bool,

    /// Print info and content for each object ID on stdin.
    #[arg(long, conflicts_with_all = ["show_type", "size", "pretty", "exists"])]
    pub batch: bool,

    /// Print info (type, size) for each object ID on stdin.
    #[arg(long, conflicts_with_all = ["show_type", "size", "pretty", "exists", "batch"])]
    pub batch_check: bool,

    /// Read commands from stdin.
    #[arg(long, conflicts_with_all = ["show_type", "size", "pretty", "exists", "batch", "batch_check"])]
    pub batch_command: bool,

    /// Follow tag objects to the tagged object.
    #[arg(long = "follow-symlinks")]
    pub follow_symlinks: bool,

    /// Object to inspect (required unless --batch*).
    pub object: Option<String>,
}

/// Run `gust cat-file`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    if args.batch || args.batch_check || args.batch_command {
        return run_batch(&repo, &args);
    }

    let obj_str = args
        .object
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("object required when not in batch mode"))?;

    let oid = resolve_object(&repo, obj_str)?;
    let obj = repo.odb.read(&oid)?;

    if args.exists {
        return Ok(());
    }

    if args.show_type {
        println!("{}", obj.kind);
        return Ok(());
    }

    if args.size {
        println!("{}", obj.data.len());
        return Ok(());
    }

    if args.pretty {
        pretty_print(&obj.kind, &obj.data)?;
        return Ok(());
    }

    // Default: print raw content
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(&obj.data)?;

    Ok(())
}

fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();

        if args.batch_command {
            // <command> <args>
            let mut parts = trimmed.splitn(2, ' ');
            match parts.next() {
                Some("contents") | Some("info") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    print_batch_entry(
                        repo,
                        obj_str,
                        args.batch_command && trimmed.starts_with("contents"),
                        &mut out,
                    )?;
                }
                Some("flush") => {
                    out.flush()?;
                }
                Some(other) => bail!("unknown batch command: {other}"),
                None => {}
            }
        } else {
            print_batch_entry(repo, trimmed, args.batch, &mut out)?;
        }
    }
    Ok(())
}

fn print_batch_entry(
    repo: &Repository,
    obj_str: &str,
    include_content: bool,
    out: &mut impl Write,
) -> Result<()> {
    match resolve_object(repo, obj_str) {
        Err(_) => {
            writeln!(out, "{obj_str} missing")?;
        }
        Ok(oid) => match repo.odb.read(&oid) {
            Err(_) => {
                writeln!(out, "{obj_str} missing")?;
            }
            Ok(obj) => {
                writeln!(out, "{} {} {}", oid, obj.kind, obj.data.len())?;
                if include_content {
                    out.write_all(&obj.data)?;
                    writeln!(out)?;
                }
            }
        },
    }
    Ok(())
}

fn pretty_print(kind: &ObjectKind, data: &[u8]) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match kind {
        ObjectKind::Blob => {
            out.write_all(data)?;
        }
        ObjectKind::Tree => {
            let entries = parse_tree(data)?;
            for e in entries {
                let name = String::from_utf8_lossy(&e.name);
                let kind_str = if e.mode == 0o040000 { "tree" } else { "blob" };
                writeln!(out, "{:06o} {kind_str} {}\t{name}", e.mode, e.oid)?;
            }
        }
        ObjectKind::Commit => {
            out.write_all(data)?;
        }
        ObjectKind::Tag => {
            out.write_all(data)?;
        }
    }
    Ok(())
}

/// Resolve an object reference string to an [`ObjectId`].
///
/// Handles full hex OIDs and simple ref names.
fn resolve_object(repo: &Repository, obj_str: &str) -> Result<ObjectId> {
    // Try as a raw OID first
    if let Ok(oid) = obj_str.parse::<ObjectId>() {
        return Ok(oid);
    }

    // Try resolving as a ref
    if let Ok(oid) = gust_lib::refs::resolve_ref(&repo.git_dir, obj_str) {
        return Ok(oid);
    }

    // Try "refs/heads/<name>"
    let as_branch = format!("refs/heads/{obj_str}");
    if let Ok(oid) = gust_lib::refs::resolve_ref(&repo.git_dir, &as_branch) {
        return Ok(oid);
    }

    bail!("not a valid object name: '{obj_str}'")
}
