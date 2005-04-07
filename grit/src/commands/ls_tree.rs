//! `grit ls-tree` — list the contents of a tree object.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Write};

use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
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
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Show only names (same as --name-only).
    #[arg(long = "name-status")]
    pub name_status: bool,

    /// Show only object names (hashes).
    #[arg(long = "object-only")]
    pub object_only: bool,

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
pub fn run(mut args: Args) -> Result<()> {
    // Validate incompatible display-mode options
    {
        let mut display_modes: Vec<&str> = Vec::new();
        if args.long {
            display_modes.push("--long");
        }
        if args.name_only {
            display_modes.push("--name-only");
        }
        if args.name_status {
            display_modes.push("--name-status");
        }
        if args.object_only {
            display_modes.push("--object-only");
        }
        if display_modes.len() > 1 {
            eprintln!(
                "error: {} and {} cannot be used together",
                display_modes[0], display_modes[1]
            );
            std::process::exit(129);
        }
        if args.format.is_some() && !display_modes.is_empty() {
            eprintln!(
                "error: {} and --format cannot be used together",
                display_modes[0]
            );
            std::process::exit(129);
        }
    }

    // --name-status is an alias for --name-only
    if args.name_status {
        args.name_only = true;
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    // Resolve pathspecs relative to cwd within the work tree, then express
    // them as repo-root-relative paths so the tree walk can match correctly.
    if !args.paths.is_empty() {
        if let Some(ref wt) = repo.work_tree {
            let cwd = std::env::current_dir().context("resolving cwd")?;
            let prefix = cwd.strip_prefix(wt).unwrap_or(std::path::Path::new(""));
            if !prefix.as_os_str().is_empty() {
                let mut resolved = Vec::with_capacity(args.paths.len());
                for p in &args.paths {
                    let combined = prefix.join(p);
                    let mut norm = Vec::new();
                    for comp in combined.components() {
                        match comp {
                            std::path::Component::ParentDir => {
                                norm.pop();
                            }
                            std::path::Component::CurDir => {}
                            other => norm.push(other.as_os_str().to_string_lossy().into_owned()),
                        }
                    }
                    resolved.push(norm.join("/"));
                }
                args.paths = resolved;
            }
        }
        for p in &mut args.paths {
            if p == "." || p == "./" {
                p.clear();
            } else if let Some(stripped) = p.strip_prefix("./") {
                *p = stripped.to_string();
            }
        }
    }

    let oid = resolve_tree_ish(&repo, &args.tree_ish)?;
    let obj = repo.odb.read(&oid)?;

    // Peel tags to their target, then commits to their tree.
    let mut current_oid = oid;
    let mut obj = obj;
    loop {
        match obj.kind {
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data).context("parsing tag")?;
                current_oid = tag.object;
                obj = repo.odb.read(&current_oid).context("reading tag target")?;
            }
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data).context("parsing commit")?;
                current_oid = commit.tree;
                obj = repo.odb.read(&current_oid).context("reading tree")?;
            }
            ObjectKind::Tree => break,
            _ => bail!("'{}' is not a tree object", args.tree_ish),
        }
    }
    let _ = current_oid;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let term = if args.null_terminated { b'\0' } else { b'\n' };

    // Compute cwd prefix (repo-root-relative current directory)
    let cwd_prefix = if let Some(ref wt) = repo.work_tree {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.strip_prefix(wt)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };

    // Match upstream ls-tree behaviour from subdirectories:
    // when no explicit pathspec is provided, list entries relative to cwd.
    // We emulate this by implicitly filtering to "<cwd>/", then rendering
    // output relative to that cwd prefix.
    let display_base = cwd_prefix.clone();
    if args.paths.is_empty() {
        if let Some(prefix) = cwd_prefix.as_deref() {
            args.paths.push(format!("{prefix}/"));
        }
    }

    list_tree(
        &repo,
        &obj.data,
        "",
        &args,
        &mut out,
        term,
        display_base.as_deref(),
    )?;

    Ok(())
}

/// Make a repo-root-relative path display-relative to the cwd prefix.
fn make_cwd_relative(path: &str, cwd_prefix: Option<&str>) -> String {
    let cwd = match cwd_prefix {
        Some(p) if !p.is_empty() => p,
        _ => return path.to_string(),
    };

    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let cwd_parts: Vec<&str> = cwd.split('/').filter(|s| !s.is_empty()).collect();

    let mut common = 0usize;
    while common < path_parts.len()
        && common < cwd_parts.len()
        && path_parts[common] == cwd_parts[common]
    {
        common += 1;
    }

    let mut out_parts: Vec<String> = Vec::new();
    for _ in common..cwd_parts.len() {
        out_parts.push("..".to_string());
    }
    for part in path_parts.iter().skip(common) {
        out_parts.push((*part).to_string());
    }

    if out_parts.is_empty() {
        ".".to_string()
    } else {
        out_parts.join("/")
    }
}

fn list_tree(
    repo: &Repository,
    data: &[u8],
    prefix: &str,
    args: &Args,
    out: &mut impl Write,
    term: u8,
    cwd_prefix: Option<&str>,
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
            let matches = args.paths.iter().any(|p| {
                let has_trailing_slash = p.ends_with('/');
                let ps = p.strip_suffix('/').unwrap_or(p.as_str());
                if ps.is_empty() {
                    return true;
                }
                // Trailing-slash pathspec only matches trees, not blobs
                if has_trailing_slash && !is_tree && full_name == ps {
                    return false;
                }
                full_name == ps
                    || full_name.starts_with(&format!("{ps}/"))
                    || ps.starts_with(&format!("{full_name}/"))
            });
            if !matches {
                continue;
            }
            // If pathspec points INTO this tree, descend.
            // Exact match without trailing slash shows the tree entry itself.
            // Trailing slash or deeper path means descend into the tree.
            let is_ancestor = is_tree
                && args.paths.iter().any(|p| {
                    let ps = p.strip_suffix('/').unwrap_or(p.as_str());
                    ps.starts_with(&format!("{full_name}/"))
                        || (p.ends_with('/') && ps == full_name)
                });
            if is_tree && is_ancestor && !args.recursive {
                let sub_obj = repo.odb.read(&entry.oid)?;
                list_tree(repo, &sub_obj.data, &full_name, args, out, term, cwd_prefix)?;
                continue;
            }
        }

        if args.recursive && is_tree {
            if args.show_trees {
                let display_name = make_cwd_relative(&full_name, cwd_prefix);
                print_entry(repo, entry, &display_name, args, out, term)?;
            }
            // Recurse
            let sub_obj = repo.odb.read(&entry.oid)?;
            list_tree(repo, &sub_obj.data, &full_name, args, out, term, cwd_prefix)?;
            continue;
        }

        if args.only_trees && !is_tree {
            continue;
        }

        let display_name = make_cwd_relative(&full_name, cwd_prefix);
        print_entry(repo, entry, &display_name, args, out, term)?;
    }
    Ok(())
}

fn print_entry(
    repo: &Repository,
    entry: &grit_lib::objects::TreeEntry,
    name: &str,
    args: &Args,
    out: &mut impl Write,
    term: u8,
) -> Result<()> {
    let kind_str = match entry.mode & 0o170000 {
        0o160000 => "commit",
        0o040000 => "tree",
        _ => "blob",
    };

    if let Some(fmt) = &args.format {
        let line = fmt
            .replace("%(objectmode)", &format!("{:06o}", entry.mode))
            .replace("%(objecttype)", kind_str)
            .replace("%(objectname)", &entry.oid.to_hex())
            .replace("%(path)", name);
        // Expand %xNN hex escapes (e.g. %x09 -> tab)
        let line = expand_hex_escapes(&line);
        write!(out, "{line}")?;
    } else if args.name_only {
        if args.null_terminated {
            write!(out, "{name}")?;
        } else {
            write!(out, "{}", quote_path_name(name))?;
        }
    } else if args.object_only {
        write!(out, "{}", entry.oid)?;
    } else if args.long {
        let size_str = if kind_str == "blob" {
            match repo.odb.read(&entry.oid) {
                Ok(obj) => format!("{:>7}", obj.data.len()),
                Err(_) => "      -".to_string(),
            }
        } else {
            "      -".to_string()
        };
        write!(
            out,
            "{:06o} {kind_str} {} {size_str}\t{name}",
            entry.mode, entry.oid
        )?;
    } else {
        write!(out, "{:06o} {kind_str} {}\t{name}", entry.mode, entry.oid)?;
    }
    out.write_all(&[term])?;
    Ok(())
}

fn expand_hex_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if chars.peek() == Some(&'x') || chars.peek() == Some(&'X') {
                chars.next();
                let mut hex = String::new();
                for _ in 0..2 {
                    if let Some(&d) = chars.peek() {
                        if d.is_ascii_hexdigit() {
                            hex.push(d);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                if hex.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                        continue;
                    }
                }
                result.push('%');
                result.push('x');
                result.push_str(&hex);
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
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
    // First try the full revision syntax (handles ^, ~, :path, etc.)
    if let Ok(oid) = grit_lib::rev_parse::resolve_revision(repo, s) {
        return Ok(oid);
    }
    // Fallback: try as raw OID
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
    let as_tag = format!("refs/tags/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_tag) {
        return Ok(oid);
    }
    bail!("not a valid tree-ish: '{s}'")
}
