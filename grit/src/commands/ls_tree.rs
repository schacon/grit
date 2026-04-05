//! `grit ls-tree` — list the contents of a tree object.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;

use crate::pathspec::parse_magic_pathspec;

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
    let pathspecs = PathspecMatcher::compile(&repo, &args.paths)?;

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

    // Compute cwd prefix for display path adjustment
    let cwd_prefix = if let Some(ref wt) = repo.work_tree {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.strip_prefix(wt)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };

    list_tree(
        &repo,
        &obj.data,
        "",
        &args,
        &pathspecs,
        &mut out,
        term,
        cwd_prefix.as_deref(),
    )?;

    Ok(())
}

/// Make a repo-root-relative path display-relative to the cwd prefix.
/// E.g., if cwd_prefix is "aa" and path is "a[a]/three", return "../a[a]/three".
fn make_cwd_relative(path: &str, cwd_prefix: Option<&str>) -> String {
    let prefix = match cwd_prefix {
        Some(p) if !p.is_empty() => p,
        _ => return path.to_string(),
    };
    // Count depth of cwd_prefix
    let depth = prefix.split('/').count();
    let mut result = String::new();
    for _ in 0..depth {
        result.push_str("../");
    }
    result.push_str(path);
    result
}

fn list_tree(
    repo: &Repository,
    data: &[u8],
    prefix: &str,
    args: &Args,
    pathspecs: &PathspecMatcher,
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

        if !args.recursive && is_tree && pathspecs.should_descend(&full_name) {
            let sub_obj = repo.odb.read(&entry.oid)?;
            list_tree(
                repo,
                &sub_obj.data,
                &full_name,
                args,
                pathspecs,
                out,
                term,
                cwd_prefix,
            )?;
            continue;
        }

        if !pathspecs.should_output(&full_name, is_tree) {
            if args.recursive && is_tree {
                let sub_obj = repo.odb.read(&entry.oid)?;
                list_tree(
                    repo,
                    &sub_obj.data,
                    &full_name,
                    args,
                    pathspecs,
                    out,
                    term,
                    cwd_prefix,
                )?;
            }
            continue;
        }

        if args.recursive && is_tree {
            if args.show_trees {
                let display_name = make_cwd_relative(&full_name, cwd_prefix);
                print_entry(repo, entry, &display_name, args, out, term)?;
            }
            // Recurse
            let sub_obj = repo.odb.read(&entry.oid)?;
            list_tree(
                repo,
                &sub_obj.data,
                &full_name,
                args,
                pathspecs,
                out,
                term,
                cwd_prefix,
            )?;
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

#[derive(Debug, Default)]
struct PathspecMatcher {
    includes: Vec<CompiledPathspec>,
    excludes: Vec<CompiledPathspec>,
}

impl PathspecMatcher {
    fn compile(repo: &Repository, raw_specs: &[String]) -> Result<Self> {
        if raw_specs.is_empty() {
            return Ok(Self::default());
        }

        let cwd = std::env::current_dir().context("resolving cwd")?;
        let mut matcher = Self::default();
        for raw in raw_specs {
            let (exclude, spec) = CompiledPathspec::compile(repo, &cwd, raw)?;
            if exclude {
                matcher.excludes.push(spec);
            } else {
                matcher.includes.push(spec);
            }
        }
        Ok(matcher)
    }

    fn should_output(&self, path: &str, is_tree: bool) -> bool {
        let included = self.includes.is_empty()
            || self
                .includes
                .iter()
                .any(|spec| spec.matches_output(path, is_tree));
        included
            && !self
                .excludes
                .iter()
                .any(|spec| spec.matches_subtree_or_path(path))
    }

    fn should_descend(&self, tree_path: &str) -> bool {
        if self
            .excludes
            .iter()
            .any(|spec| spec.matches_subtree_or_path(tree_path))
        {
            return false;
        }

        self.includes
            .iter()
            .any(|spec| spec.requires_descend(tree_path))
    }
}

#[derive(Debug)]
struct CompiledPathspec {
    pattern: String,
    is_glob: bool,
    trailing_slash: bool,
}

impl CompiledPathspec {
    fn compile(repo: &Repository, cwd: &Path, raw: &str) -> Result<(bool, Self)> {
        let parsed = parse_magic_pathspec(raw);
        let exclude = parsed.exclude;
        let rooted = parsed.top;
        let spec = parsed.pattern;
        let trailing_slash = spec.ends_with('/') && !spec.is_empty();
        let spec = spec.strip_suffix('/').unwrap_or(spec);
        let is_glob = crate::pathspec::has_glob_chars(spec);

        let pattern = if let Some(work_tree) = repo.work_tree.as_ref() {
            resolve_pathspec_pattern(work_tree, cwd, spec, rooted)?
        } else {
            spec.to_owned()
        };

        Ok((
            exclude,
            Self {
                pattern,
                is_glob,
                trailing_slash,
            },
        ))
    }

    fn matches_output(&self, path: &str, is_tree: bool) -> bool {
        if self.trailing_slash && !is_tree && path == self.pattern {
            return false;
        }

        if self.is_glob {
            path == self.pattern
                || path.starts_with(&format!("{}/", self.pattern))
                || crate::pathspec::glob_match(&self.pattern, path)
        } else if self.pattern.is_empty() {
            true
        } else {
            path == self.pattern || path.starts_with(&format!("{}/", self.pattern))
        }
    }

    fn matches_subtree_or_path(&self, path: &str) -> bool {
        self.matches_output(path, true)
    }

    fn requires_descend(&self, tree_path: &str) -> bool {
        if self.pattern.is_empty() {
            return false;
        }
        if self.trailing_slash && tree_path == self.pattern {
            return true;
        }
        if !self.is_glob {
            return self.pattern.starts_with(&format!("{tree_path}/"));
        }

        let literal_prefix = self
            .pattern
            .find(['*', '?', '['])
            .map(|pos| &self.pattern[..pos])
            .unwrap_or(&self.pattern);
        if literal_prefix.starts_with(&format!("{tree_path}/")) {
            return true;
        }

        let trimmed_prefix = literal_prefix.trim_end_matches('/');
        !trimmed_prefix.is_empty() && tree_path.starts_with(trimmed_prefix)
    }
}

fn resolve_pathspec_pattern(
    work_tree: &Path,
    cwd: &Path,
    spec: &str,
    rooted: bool,
) -> Result<String> {
    if spec.is_empty() {
        return Ok(String::new());
    }

    let base = if rooted {
        work_tree.to_path_buf()
    } else {
        cwd.to_path_buf()
    };
    let combined = if Path::new(spec).is_absolute() {
        PathBuf::from(spec)
    } else {
        base.join(spec)
    };
    let normalized = normalize_path(&combined);
    let relative = normalized.strip_prefix(work_tree).with_context(|| {
        format!(
            "pathspec '{}' is outside repository work tree '{}'",
            spec,
            work_tree.display()
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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
