//! `grit ls-tree` — list the contents of a tree object.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Write};

use grit_lib::objects::{parse_tree, ObjectId, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;

/// Arguments for `grit ls-tree`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show only trees (not blobs).
    #[arg(short = 'd')]
    pub only_trees: bool,

    /// Recurse into sub-trees.
    #[arg(short = 'r')]
    pub recursive: bool,

    /// Show trees even when recursing.
    #[arg(short = 't')]
    pub show_trees: bool,

    /// Show object size (long format).
    #[arg(short = 'l', long)]
    pub long: bool,

    /// Show only names.
    #[arg(long = "name-only", alias = "name-status")]
    pub name_only: bool,

    /// \0 line termination on output.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Format string for output.
    #[arg(long)]
    pub format: Option<String>,

    /// The tree-ish to list.
    pub tree_ish: String,

    /// Paths to restrict listing.
    pub paths: Vec<String>,
}

/// Run `grit ls-tree`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let oid = resolve_tree_ish(&repo, &args.tree_ish)?;
    let obj = repo.odb.read(&oid)?;

    if obj.kind != ObjectKind::Tree {
        bail!("'{}' is not a tree object", args.tree_ish);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let term = if args.null_terminated { b'\0' } else { b'\n' };

    list_tree(&repo, &obj.data, "", &args, &mut out, term)?;

    Ok(())
}

fn list_tree(
    repo: &Repository,
    data: &[u8],
    prefix: &str,
    args: &Args,
    out: &mut impl Write,
    term: u8,
) -> Result<()> {
    let entries = parse_tree(data)?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let full_name = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let is_tree = entry.mode == 0o040000;

        // Apply path filter
        if !args.paths.is_empty() {
            let matches = args
                .paths
                .iter()
                .any(|p| full_name == *p || full_name.starts_with(&format!("{p}/")));
            if !matches {
                continue;
            }
        }

        if args.recursive && is_tree {
            if args.show_trees {
                print_entry(entry, &full_name, args, out, term)?;
            }
            // Recurse
            let sub_obj = repo.odb.read(&entry.oid)?;
            list_tree(repo, &sub_obj.data, &full_name, args, out, term)?;
            continue;
        }

        if args.only_trees && !is_tree {
            continue;
        }

        print_entry(entry, &full_name, args, out, term)?;
    }
    Ok(())
}

fn print_entry(
    entry: &grit_lib::objects::TreeEntry,
    name: &str,
    args: &Args,
    out: &mut impl Write,
    term: u8,
) -> Result<()> {
    let is_tree = entry.mode == 0o040000;
    let kind_str = if is_tree { "tree" } else { "blob" };

    if let Some(fmt) = &args.format {
        let line = fmt
            .replace("%(objectmode)", &format!("{:06o}", entry.mode))
            .replace("%(objecttype)", kind_str)
            .replace("%(objectname)", &entry.oid.to_hex())
            .replace("%(path)", name);
        write!(out, "{line}")?;
    } else if args.name_only {
        if args.null_terminated {
            write!(out, "{name}")?;
        } else {
            write!(out, "{}", quote_path_name(name))?;
        }
    } else if args.long {
        let size_str = "-";
        write!(
            out,
            "{:06o} {kind_str} {}\t{size_str}\t{name}",
            entry.mode, entry.oid
        )?;
    } else {
        write!(out, "{:06o} {kind_str} {}\t{name}", entry.mode, entry.oid)?;
    }
    out.write_all(&[term])?;
    Ok(())
}

fn quote_path_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    let mut needs_quotes = false;

    for ch in name.chars() {
        match ch {
            '"' => {
                out.push_str("\\\"");
                needs_quotes = true;
            }
            '\\' => {
                out.push_str("\\\\");
                needs_quotes = true;
            }
            '\t' => {
                out.push_str("\\t");
                needs_quotes = true;
            }
            '\n' => {
                out.push_str("\\n");
                needs_quotes = true;
            }
            '\r' => {
                out.push_str("\\r");
                needs_quotes = true;
            }
            c if c.is_control() => {
                out.push_str(&format!("\\{:03o}", u32::from(c)));
                needs_quotes = true;
            }
            c => out.push(c),
        }
    }

    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

fn resolve_tree_ish(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return Ok(oid);
    }
    let as_branch = format!("refs/heads/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_branch) {
        return Ok(oid);
    }
    bail!("not a valid tree-ish: '{s}'")
}
