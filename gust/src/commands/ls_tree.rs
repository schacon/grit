//! `gust ls-tree` — list the contents of a tree object.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Write};

use gust_lib::objects::{parse_tree, ObjectId, ObjectKind};
use gust_lib::refs::resolve_ref;
use gust_lib::repo::Repository;

/// Arguments for `gust ls-tree`.
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

/// Run `gust ls-tree`.
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

    list_tree(&repo, &obj.data, b"", &args, &mut out, term)?;

    Ok(())
}

fn list_tree(
    repo: &Repository,
    data: &[u8],
    prefix: &[u8],
    args: &Args,
    out: &mut impl Write,
    term: u8,
) -> Result<()> {
    let entries = parse_tree(data)?;

    for entry in &entries {
        let full_name = if prefix.is_empty() {
            entry.name.clone()
        } else {
            let mut p = Vec::with_capacity(prefix.len() + 1 + entry.name.len());
            p.extend_from_slice(prefix);
            p.push(b'/');
            p.extend_from_slice(&entry.name);
            p
        };

        let is_tree = entry.mode == 0o040000;

        // Apply path filter
        if !args.paths.is_empty() {
            let matches = args.paths.iter().any(|p| {
                let p = p.as_bytes();
                full_name == p
                    || (full_name.len() > p.len()
                        && full_name.starts_with(p)
                        && full_name[p.len()] == b'/')
            });
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
    entry: &gust_lib::objects::TreeEntry,
    name: &[u8],
    args: &Args,
    out: &mut impl Write,
    term: u8,
) -> Result<()> {
    let is_tree = entry.mode == 0o040000;
    let kind_str = if is_tree { "tree" } else { "blob" };
    let quoted_name = quoted_path(name, args.null_terminated);

    if let Some(fmt) = &args.format {
        let path_for_format = String::from_utf8_lossy(name);
        let line = fmt
            .replace("%(objectmode)", &format!("{:06o}", entry.mode))
            .replace("%(objecttype)", kind_str)
            .replace("%(objectname)", &entry.oid.to_hex())
            .replace("%(path)", &path_for_format);
        write!(out, "{line}")?;
    } else if args.name_only {
        out.write_all(&quoted_name)?;
    } else if args.long {
        let size_str = "-";
        write!(
            out,
            "{:06o} {kind_str} {}\t{size_str}\t",
            entry.mode, entry.oid
        )?;
        out.write_all(&quoted_name)?;
    } else {
        write!(out, "{:06o} {kind_str} {}\t", entry.mode, entry.oid)?;
        out.write_all(&quoted_name)?;
    }
    out.write_all(&[term])?;
    Ok(())
}

fn quoted_path(path: &[u8], null_terminated: bool) -> Vec<u8> {
    if null_terminated || !needs_c_style_quote(path) {
        return path.to_vec();
    }

    let mut out = Vec::with_capacity(path.len() + 2);
    out.push(b'"');
    for &b in path {
        match b {
            b'\n' => out.extend_from_slice(br"\n"),
            b'\t' => out.extend_from_slice(br"\t"),
            b'\r' => out.extend_from_slice(br"\r"),
            0x08 => out.extend_from_slice(br"\b"),
            0x0c => out.extend_from_slice(br"\f"),
            b'"' => out.extend_from_slice(br#"\""#),
            b'\\' => out.extend_from_slice(br"\\"),
            0x20..=0x7e => out.push(b),
            _ => out.extend_from_slice(format!(r"\{:03o}", b).as_bytes()),
        }
    }
    out.push(b'"');
    out
}

fn needs_c_style_quote(path: &[u8]) -> bool {
    path.iter()
        .any(|&b| b == b'"' || b == b'\\' || b < 0x20 || b == 0x7f)
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
