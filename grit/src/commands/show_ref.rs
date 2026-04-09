//! `grit show-ref` — list and verify refs in a local repository.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::refs::RawRefLookup;
use grit_lib::repo::Repository;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

/// Arguments for `grit show-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Only show tags.
    #[arg(long = "tags")]
    pub tags: bool,

    /// Only show branches.
    #[arg(long = "branches", alias = "heads")]
    pub branches: bool,

    /// Include `HEAD` in listing mode.
    #[arg(long = "head")]
    pub head: bool,

    /// Check exact references.
    #[arg(long = "verify")]
    pub verify: bool,

    /// Check whether a single reference exists.
    #[arg(long = "exists")]
    pub exists: bool,

    /// Filter refs read from stdin (or one per line, stripping trailing
    /// whitespace) against existing refs.
    #[arg(long = "exclude-existing", num_args = 0..=1, require_equals = true, value_name = "PATTERN", default_missing_value = "")]
    pub exclude_existing: Option<Option<String>>,

    /// Show peeled tags.
    #[arg(short = 'd', long = "dereference")]
    pub dereference: bool,

    /// Only show object IDs, optional abbreviation length.
    #[arg(
        short = 's',
        long = "hash",
        num_args = 0..=1,
        require_equals = true,
        value_name = "N"
    )]
    pub hash: Option<Option<usize>>,

    /// Abbreviate object IDs, optional explicit width.
    #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, value_name = "N")]
    pub abbrev: Option<Option<usize>>,

    /// Quiet mode.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Optional patterns/refs.
    #[arg(value_name = "PATTERN_OR_REF", num_args = 0..)]
    pub refs_or_patterns: Vec<String>,
}

/// Run `grit show-ref`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let has_exclude_existing = args.exclude_existing.is_some();

    if args.verify && args.exists {
        eprintln!("fatal: options '--verify' and '--exists' cannot be used together");
        std::process::exit(1);
    }
    if args.verify && has_exclude_existing {
        eprintln!("fatal: options '--verify' and '--exclude-existing' cannot be used together");
        std::process::exit(1);
    }
    if has_exclude_existing && args.exists {
        eprintln!("fatal: options '--exclude-existing' and '--exists' cannot be used together");
        std::process::exit(1);
    }

    let abbrev_len = resolve_abbrev_len(&args);
    let hash_only = args.hash.is_some();

    if args.exists {
        return run_exists_mode(&repo, &args);
    }

    if args.verify {
        return run_verify_mode(&repo, &args, hash_only, abbrev_len);
    }

    run_pattern_mode(&repo, &args, hash_only, abbrev_len)
}

fn resolve_abbrev_len(args: &Args) -> usize {
    if let Some(Some(explicit)) = args.hash {
        return explicit.max(1);
    }
    if let Some(maybe) = args.abbrev {
        return maybe.unwrap_or(7).max(1);
    }
    40
}

fn run_exists_mode(repo: &Repository, args: &Args) -> Result<()> {
    if args.refs_or_patterns.len() != 1 {
        bail!("--exists requires exactly one reference");
    }
    let reference = &args.refs_or_patterns[0];

    match grit_lib::refs::read_raw_ref(&repo.git_dir, reference) {
        Ok(RawRefLookup::Exists) => Ok(()),
        Ok(RawRefLookup::NotFound) | Ok(RawRefLookup::IsDirectory) => {
            eprintln!("error: reference does not exist");
            std::process::exit(2);
        }
        Err(err) => Err(err.into()),
    }
}

fn run_verify_mode(
    repo: &Repository,
    args: &Args,
    hash_only: bool,
    abbrev_len: usize,
) -> Result<()> {
    if args.refs_or_patterns.is_empty() {
        bail!("--verify requires a reference");
    }

    for reference in &args.refs_or_patterns {
        if !(reference.starts_with("refs/") || is_safe_refname(reference)) {
            if args.quiet {
                std::process::exit(1);
            }
            bail!("'{reference}' - not a valid ref");
        }

        let oid = match grit_lib::refs::resolve_ref(&repo.git_dir, reference) {
            Ok(oid) => oid,
            Err(_) if args.quiet => std::process::exit(1),
            Err(_) => bail!("'{reference}' - not a valid ref"),
        };

        if !repo.odb.exists(&oid) {
            if args.quiet {
                std::process::exit(1);
            }
            bail!("git show-ref: bad ref {reference} ({oid})");
        }

        if !args.quiet {
            print_one(reference, &oid, hash_only, abbrev_len);
            if args.dereference {
                print_peeled_tag(repo, reference, &oid, hash_only, abbrev_len)?;
            }
        }
    }

    Ok(())
}

fn run_pattern_mode(
    repo: &Repository,
    args: &Args,
    hash_only: bool,
    abbrev_len: usize,
) -> Result<()> {
    let refs = collect_all_refs(&repo.git_dir)?;

    let mut found = 0usize;

    // When --head is given, always show HEAD first (before other refs).
    if args.head {
        if let Ok(head_oid) = grit_lib::refs::resolve_ref(&repo.git_dir, "HEAD") {
            found += 1;
            if !args.quiet {
                print_one("HEAD", &head_oid, hash_only, abbrev_len);
                if args.dereference {
                    print_peeled_tag(repo, "HEAD", &head_oid, hash_only, abbrev_len)?;
                }
            }
        }
    }

    for (name, oid) in &refs {
        if args.branches && !args.tags && !name.starts_with("refs/heads/") {
            continue;
        }
        if args.tags && !args.branches && !name.starts_with("refs/tags/") {
            continue;
        }
        if args.branches
            && args.tags
            && !name.starts_with("refs/heads/")
            && !name.starts_with("refs/tags/")
        {
            continue;
        }
        if !args.refs_or_patterns.is_empty()
            && !args
                .refs_or_patterns
                .iter()
                .any(|pattern| ref_matches_pattern(name, pattern))
        {
            continue;
        }

        found += 1;
        if args.quiet {
            continue;
        }
        print_one(name, oid, hash_only, abbrev_len);
        if args.dereference {
            print_peeled_tag(repo, name, oid, hash_only, abbrev_len)?;
        }
    }

    if found == 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn print_one(name: &str, oid: &ObjectId, hash_only: bool, abbrev_len: usize) {
    let hash = abbreviate_oid(oid, abbrev_len);
    if hash_only {
        println!("{hash}");
    } else {
        println!("{hash} {name}");
    }
}

fn print_peeled_tag(
    repo: &Repository,
    name: &str,
    oid: &ObjectId,
    hash_only: bool,
    abbrev_len: usize,
) -> Result<()> {
    let obj = match repo.odb.read(oid) {
        Ok(obj) => obj,
        Err(_) => return Ok(()),
    };
    if obj.kind != ObjectKind::Tag {
        return Ok(());
    }
    let Some(peeled) = peel_tag_target(repo, *oid) else {
        return Ok(());
    };
    let hash = abbreviate_oid(&peeled, abbrev_len);
    if hash_only {
        println!("{hash}");
    } else {
        println!("{hash} {name}^{{}}");
    }
    Ok(())
}

fn parse_tag_target(data: &[u8]) -> Option<ObjectId> {
    let text = std::str::from_utf8(data).ok()?;
    for line in text.lines() {
        if let Some(target) = line.strip_prefix("object ") {
            return target.trim().parse::<ObjectId>().ok();
        }
    }
    None
}

fn peel_tag_target(repo: &Repository, mut oid: ObjectId) -> Option<ObjectId> {
    loop {
        let obj = repo.odb.read(&oid).ok()?;
        if obj.kind != ObjectKind::Tag {
            return Some(oid);
        }
        oid = parse_tag_target(&obj.data)?;
    }
}

fn abbreviate_oid(oid: &ObjectId, len: usize) -> String {
    let hex = oid.to_string();
    let n = len.clamp(1, hex.len());
    hex[..n].to_owned()
}

fn ref_matches_pattern(refname: &str, pattern: &str) -> bool {
    if refname == pattern {
        return true;
    }
    if let Some(prefix) = refname.strip_suffix(pattern) {
        return prefix.ends_with('/');
    }
    false
}

fn collect_all_refs(git_dir: &Path) -> Result<BTreeMap<String, ObjectId>> {
    // Dispatch to reftable backend if configured
    if grit_lib::reftable::is_reftable_repo(git_dir) {
        let refs = grit_lib::reftable::reftable_list_refs(git_dir, "refs/")
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(refs.into_iter().collect());
    }

    let mut refs = BTreeMap::new();

    // For worktrees, collect from the common (shared) dir first, then overlay
    // worktree-local refs on top.
    let common_dir = worktree_common_dir(git_dir);
    if let Some(ref cdir) = common_dir {
        if cdir.as_path() != git_dir {
            collect_loose_refs(git_dir, &cdir.join("refs"), "refs", &mut refs)?;
            for (name, oid) in parse_packed_refs(cdir)? {
                refs.entry(name).or_insert(oid);
            }
        }
    }

    collect_loose_refs(git_dir, &git_dir.join("refs"), "refs", &mut refs)?;
    for (name, oid) in parse_packed_refs(git_dir)? {
        refs.entry(name).or_insert(oid);
    }
    Ok(refs)
}

/// Determine the common git directory for a possibly-worktree git dir.
fn worktree_common_dir(git_dir: &Path) -> Option<std::path::PathBuf> {
    let raw = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let rel = raw.trim();
    let path = if std::path::Path::new(rel).is_absolute() {
        std::path::PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    path.canonicalize().ok()
}

fn collect_loose_refs(
    git_dir: &Path,
    path: &Path,
    relative: &str,
    out: &mut BTreeMap<String, ObjectId>,
) -> Result<()> {
    let read_dir = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    for entry in read_dir {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let next_relative = format!("{relative}/{file_name}");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_loose_refs(git_dir, &entry.path(), &next_relative, out)?;
        } else if file_type.is_file() {
            if let Ok(oid) = grit_lib::refs::resolve_ref(git_dir, &next_relative) {
                out.insert(next_relative, oid);
            }
        }
    }
    Ok(())
}

fn parse_packed_refs(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let path = git_dir.join("packed-refs");
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(oid_str) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        if let Ok(oid) = oid_str.parse::<ObjectId>() {
            entries.push((name.to_owned(), oid));
        }
    }
    Ok(entries)
}

fn is_safe_refname(reference: &str) -> bool {
    if reference.is_empty()
        || reference.starts_with('/')
        || reference.ends_with('/')
        || reference.contains("//")
        || reference.contains("..")
        || reference.contains("@{")
        || reference
            .chars()
            .any(|c| c.is_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        return false;
    }
    true
}
