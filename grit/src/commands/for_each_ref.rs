//! `grit for-each-ref` - output information on refs.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::error::Error as GustError;
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::{parse_commit, parse_tag, ObjectId, ObjectKind};
use grit_lib::refs::read_head;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Arguments for `grit for-each-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit for-each-ref`.
pub fn run(args: Args) -> Result<()> {
    if args.args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print_usage();
        std::process::exit(129);
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let opts = parse_args(args.args)?;

    let mut patterns = opts.patterns.clone();
    if opts.stdin {
        if !patterns.is_empty() {
            bail!("unknown arguments supplied with --stdin");
        }
        patterns = read_patterns_from_stdin()?;
    }

    let mut refs = collect_refs(&repo.git_dir)?;
    refs.retain(|entry| ref_matches_patterns(&entry.name, &patterns, opts.ignore_case));
    refs.retain(|entry| {
        opts.exclude.is_empty()
            || !ref_matches_patterns(&entry.name, &opts.exclude, opts.ignore_case)
    });
    apply_filters(&repo, &opts, &mut refs)?;
    refs.sort_by(|left, right| compare_refs(&repo, left, right, &opts.sort_keys, opts.ignore_case));

    let format = opts
        .format
        .unwrap_or_else(|| "%(objectname) %(objecttype)\t%(refname)".to_owned());
    let head_branch = read_head(&repo.git_dir)
        .ok()
        .flatten();
    let max = opts.count.unwrap_or(usize::MAX);
    let mut printed = 0usize;
    for entry in refs {
        if printed >= max {
            break;
        }
        match expand_format(&repo, &entry, &format, &head_branch) {
            Ok(line) => {
                println!("{line}");
                printed += 1;
            }
            Err(FormatError::MissingObject(oid, refname)) => {
                eprintln!("fatal: missing object {oid} for {refname}");
                std::process::exit(1);
            }
            Err(FormatError::Other(message)) => bail!(message),
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct RefEntry {
    name: String,
    oid: ObjectId,
}

#[derive(Debug, Clone, Copy)]
enum SortField {
    RefName,
    ObjectName,
    ObjectType,
}

#[derive(Debug, Clone, Copy)]
struct SortKey {
    field: SortField,
    descending: bool,
}

#[derive(Debug, Default)]
struct Options {
    count: Option<usize>,
    format: Option<String>,
    sort_keys: Vec<SortKey>,
    patterns: Vec<String>,
    exclude: Vec<String>,
    points_at: Option<String>,
    merged: Option<Option<String>>,
    no_merged: Option<Option<String>>,
    contains: Option<Option<String>>,
    no_contains: Option<Option<String>>,
    stdin: bool,
    ignore_case: bool,
}

#[derive(Debug)]
enum FormatError {
    MissingObject(ObjectId, String),
    Other(String),
}

fn print_usage() {
    eprintln!(
        "usage: git for-each-ref [--count=<count>] [--sort=<key>] [--format=<format>] [--points-at=<object>] [--merged[=<object>]] [--no-merged[=<object>]] [--contains[=<object>]] [--no-contains[=<object>]] [--exclude=<pattern>] [--stdin] [<pattern>...]"
    );
}

fn parse_args(args: Vec<String>) -> Result<Options> {
    let mut opts = Options::default();
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--stdin" {
            opts.stdin = true;
            i += 1;
            continue;
        }
        if arg == "--ignore-case" {
            opts.ignore_case = true;
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--count=") {
            opts.count = Some(parse_count(value)?);
            i += 1;
            continue;
        }
        if arg == "--count" {
            i += 1;
            let Some(value) = args.get(i) else {
                bail!("--count requires a value");
            };
            opts.count = Some(parse_count(value)?);
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--format=") {
            opts.format = Some(value.to_owned());
            i += 1;
            continue;
        }
        if arg == "--format" {
            i += 1;
            let Some(value) = args.get(i) else {
                bail!("--format requires a value");
            };
            opts.format = Some(value.clone());
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--sort=") {
            opts.sort_keys.push(parse_sort_key(value)?);
            i += 1;
            continue;
        }
        if arg == "--sort" {
            i += 1;
            let Some(value) = args.get(i) else {
                bail!("--sort requires a value");
            };
            opts.sort_keys.push(parse_sort_key(value)?);
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--exclude=") {
            opts.exclude.push(value.to_owned());
            i += 1;
            continue;
        }
        if arg == "--exclude" {
            i += 1;
            let Some(value) = args.get(i) else {
                bail!("--exclude requires a value");
            };
            opts.exclude.push(value.clone());
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--points-at=") {
            opts.points_at = Some(value.to_owned());
            i += 1;
            continue;
        }
        if arg == "--points-at" {
            i += 1;
            let Some(value) = args.get(i) else {
                bail!("--points-at requires a value");
            };
            opts.points_at = Some(value.clone());
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--merged=") {
            opts.merged = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--merged" {
            i += 1;
            if let Some(value) = args.get(i) {
                if !value.starts_with('-') {
                    opts.merged = Some(Some(value.clone()));
                    i += 1;
                } else {
                    opts.merged = Some(None);
                }
            } else {
                opts.merged = Some(None);
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--no-merged=") {
            opts.no_merged = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--no-merged" {
            i += 1;
            if let Some(value) = args.get(i) {
                if !value.starts_with('-') {
                    opts.no_merged = Some(Some(value.clone()));
                    i += 1;
                } else {
                    opts.no_merged = Some(None);
                }
            } else {
                opts.no_merged = Some(None);
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--contains=") {
            opts.contains = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--contains" {
            i += 1;
            if let Some(value) = args.get(i) {
                if !value.starts_with('-') {
                    opts.contains = Some(Some(value.clone()));
                    i += 1;
                } else {
                    opts.contains = Some(None);
                }
            } else {
                opts.contains = Some(None);
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--no-contains=") {
            opts.no_contains = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--no-contains" {
            i += 1;
            if let Some(value) = args.get(i) {
                if !value.starts_with('-') {
                    opts.no_contains = Some(Some(value.clone()));
                    i += 1;
                } else {
                    opts.no_contains = Some(None);
                }
            } else {
                opts.no_contains = Some(None);
            }
            continue;
        }
        if arg.starts_with('-') {
            bail!("unsupported option: {arg}");
        }
        opts.patterns.push(arg.clone());
        i += 1;
    }

    if opts.sort_keys.is_empty() {
        opts.sort_keys.push(SortKey {
            field: SortField::RefName,
            descending: false,
        });
    }

    Ok(opts)
}

fn parse_count(value: &str) -> Result<usize> {
    let parsed = value
        .parse::<isize>()
        .with_context(|| format!("invalid --count argument: `{value}`"))?;
    if parsed < 0 {
        bail!("invalid --count argument: `{value}`");
    }
    Ok(parsed as usize)
}

fn parse_sort_key(raw: &str) -> Result<SortKey> {
    let (descending, key) = if let Some(stripped) = raw.strip_prefix('-') {
        (true, stripped)
    } else {
        (false, raw)
    };
    let field = match key {
        "refname" => SortField::RefName,
        "objectname" => SortField::ObjectName,
        "objecttype" => SortField::ObjectType,
        _ => bail!("unsupported sort key: {raw}"),
    };
    Ok(SortKey { field, descending })
}

fn read_patterns_from_stdin() -> Result<Vec<String>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input.lines().map(|line| line.to_owned()).collect())
}

fn collect_refs(git_dir: &Path) -> Result<Vec<RefEntry>> {
    // Dispatch to reftable backend if configured
    if grit_lib::reftable::is_reftable_repo(git_dir) {
        let refs = grit_lib::reftable::reftable_list_refs(git_dir, "refs/")
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(refs
            .into_iter()
            .map(|(name, oid)| RefEntry { name, oid })
            .collect());
    }

    let mut refs = BTreeMap::new();
    collect_loose_refs(git_dir, &git_dir.join("refs"), "refs", &mut refs)?;
    for (name, oid) in parse_packed_refs(git_dir)? {
        refs.entry(name).or_insert(oid);
    }
    Ok(refs
        .into_iter()
        .map(|(name, oid)| RefEntry { name, oid })
        .collect())
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
            match read_loose_ref_oid(git_dir, &next_relative, &entry.path()) {
                Ok(Some(oid)) => {
                    out.insert(next_relative, oid);
                }
                Ok(None) => {}
                Err(_) => {
                    eprintln!("warning: ignoring broken ref {next_relative}");
                }
            }
        }
    }
    Ok(())
}

fn read_loose_ref_oid(git_dir: &Path, refname: &str, path: &Path) -> Result<Option<ObjectId>> {
    let text = fs::read_to_string(path)?;
    let raw = text.trim();
    if raw.is_empty() {
        bail!("empty ref");
    }
    if raw.starts_with("ref: ") {
        return match grit_lib::refs::resolve_ref(git_dir, refname) {
            Ok(oid) => Ok(Some(oid)),
            Err(_) => Ok(None),
        };
    }
    let oid = raw
        .parse::<ObjectId>()
        .map_err(|_| anyhow::anyhow!("invalid direct ref"))?;
    if is_zero_oid(&oid) {
        bail!("zero oid");
    }
    Ok(Some(oid))
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

fn apply_filters(repo: &Repository, opts: &Options, refs: &mut Vec<RefEntry>) -> Result<()> {
    if let Some(points_spec) = &opts.points_at {
        let points_oid = resolve_revision(repo, points_spec)?;
        refs.retain(|entry| {
            entry.oid == points_oid || peel_to_non_tag(repo, entry.oid).ok() == Some(points_oid)
        });
    }

    let merged_base = resolve_optional_commitish(repo, opts.merged.as_ref())?;
    let no_merged_base = resolve_optional_commitish(repo, opts.no_merged.as_ref())?;
    if let Some(base) = merged_base {
        refs.retain(|entry| {
            peel_to_commit(repo, entry.oid)
                .ok()
                .and_then(|oid| is_ancestor(repo, oid, base).ok())
                .unwrap_or(false)
        });
    }
    if let Some(base) = no_merged_base {
        refs.retain(|entry| {
            peel_to_commit(repo, entry.oid)
                .ok()
                .and_then(|oid| is_ancestor(repo, oid, base).ok())
                .map(|merged| !merged)
                .unwrap_or(false)
        });
    }

    let contains_base = resolve_optional_commitish(repo, opts.contains.as_ref())?;
    let no_contains_base = resolve_optional_commitish(repo, opts.no_contains.as_ref())?;
    if let Some(base) = contains_base {
        refs.retain(|entry| {
            peel_to_commit(repo, entry.oid)
                .ok()
                .and_then(|oid| is_ancestor(repo, base, oid).ok())
                .unwrap_or(false)
        });
    }
    if let Some(base) = no_contains_base {
        refs.retain(|entry| {
            peel_to_commit(repo, entry.oid)
                .ok()
                .and_then(|oid| is_ancestor(repo, base, oid).ok())
                .map(|contains| !contains)
                .unwrap_or(false)
        });
    }

    Ok(())
}

fn resolve_optional_commitish(
    repo: &Repository,
    raw: Option<&Option<String>>,
) -> Result<Option<ObjectId>> {
    match raw {
        None => Ok(None),
        Some(Some(spec)) => Ok(Some(resolve_revision(repo, spec)?)),
        Some(None) => Ok(Some(resolve_revision(repo, "HEAD")?)),
    }
}

fn compare_refs(
    repo: &Repository,
    left: &RefEntry,
    right: &RefEntry,
    keys: &[SortKey],
    ignore_case: bool,
) -> Ordering {
    for key in keys {
        let mut ord = compare_on_key(repo, left, right, key.field, ignore_case);
        if key.descending {
            ord = ord.reverse();
        }
        if ord != Ordering::Equal {
            return ord;
        }
    }
    left.name.cmp(&right.name)
}

fn compare_on_key(
    repo: &Repository,
    left: &RefEntry,
    right: &RefEntry,
    field: SortField,
    ignore_case: bool,
) -> Ordering {
    let value = |entry: &RefEntry| -> String {
        match field {
            SortField::RefName => entry.name.clone(),
            SortField::ObjectName => entry.oid.to_string(),
            SortField::ObjectType => repo
                .odb
                .read(&entry.oid)
                .ok()
                .map(|obj| obj.kind.to_string())
                .unwrap_or_default(),
        }
    };
    let mut left_val = value(left);
    let mut right_val = value(right);
    if ignore_case {
        left_val.make_ascii_lowercase();
        right_val.make_ascii_lowercase();
    }
    left_val.cmp(&right_val)
}

fn expand_format(repo: &Repository, entry: &RefEntry, format: &str, head_branch: &Option<String>) -> Result<String, FormatError> {
    let mut out = String::new();
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            return Err(FormatError::Other("unterminated format atom".to_owned()));
        };
        let atom = &after[..end];
        out.push_str(&atom_value(repo, entry, atom, head_branch)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn atom_value(
    repo: &Repository,
    entry: &RefEntry,
    atom: &str,
    head_branch: &Option<String>,
) -> Result<String, FormatError> {
    // Handle atoms with modifiers (e.g. "authordate:short")
    let (base, modifier) = if let Some(pos) = atom.find(':') {
        (&atom[..pos], Some(&atom[pos + 1..]))
    } else {
        (atom, None)
    };

    match base {
        "refname" => match modifier {
            Some("short") => Ok(short_refname(&entry.name)),
            Some(m) => Err(FormatError::Other(format!("unsupported refname modifier: {m}"))),
            None => Ok(entry.name.clone()),
        },
        "objectname" => match modifier {
            Some("short") => Ok(entry.oid.to_string().get(..7).unwrap_or("").to_owned()),
            _ => Ok(entry.oid.to_string()),
        },
        "objecttype" => {
            let object = read_object(repo, entry)?;
            Ok(object.kind.to_string())
        }
        "objectsize" => {
            let object = read_object(repo, entry)?;
            Ok(object.data.len().to_string())
        }
        "HEAD" => {
            if let Some(ref hb) = head_branch {
                if entry.name == *hb {
                    return Ok("*".to_owned());
                }
            }
            Ok(" ".to_owned())
        }
        "upstream" => resolve_upstream(repo, entry, modifier),
        "subject" => subject_for_oid(repo, entry, entry.oid),
        "*subject" => {
            let peeled = peel_to_non_tag(repo, entry.oid)
                .map_err(|_| FormatError::MissingObject(entry.oid, entry.name.clone()))?;
            subject_for_oid(repo, entry, peeled)
        }
        "body" => body_for_oid(repo, entry, entry.oid),
        "authorname" => commit_field_for_oid(repo, entry, entry.oid, |c| {
            parse_identity_name(&c.author)
        }),
        "authoremail" => commit_field_for_oid(repo, entry, entry.oid, |c| {
            parse_identity_email(&c.author)
        }),
        "authordate" => commit_field_for_oid(repo, entry, entry.oid, |c| {
            format_identity_date(&c.author, modifier)
        }),
        "committername" => commit_field_for_oid(repo, entry, entry.oid, |c| {
            parse_identity_name(&c.committer)
        }),
        "committeremail" => commit_field_for_oid(repo, entry, entry.oid, |c| {
            parse_identity_email(&c.committer)
        }),
        "committerdate" => commit_field_for_oid(repo, entry, entry.oid, |c| {
            format_identity_date(&c.committer, modifier)
        }),
        "creatordate" => {
            // creatordate: for tags use tagger date, for commits use committer date
            let object = read_object(repo, entry)?;
            match object.kind {
                ObjectKind::Tag => {
                    let tag = parse_tag(&object.data).map_err(|_| {
                        FormatError::Other(format!("failed to parse tag for {}", entry.name))
                    })?;
                    Ok(tag.tagger.as_ref().map(|t| format_identity_date(t, modifier)).unwrap_or_default())
                }
                ObjectKind::Commit => {
                    let commit = parse_commit(&object.data).map_err(|_| {
                        FormatError::Other(format!("failed to parse commit for {}", entry.name))
                    })?;
                    Ok(format_identity_date(&commit.committer, modifier))
                }
                _ => Ok(String::new()),
            }
        }
        "taggername" => tag_field_for_oid(repo, entry, |t| {
            t.tagger.as_ref().map(|s| parse_identity_name(s)).unwrap_or_default()
        }),
        "taggeremail" => tag_field_for_oid(repo, entry, |t| {
            t.tagger.as_ref().map(|s| parse_identity_email(s)).unwrap_or_default()
        }),
        "taggerdate" => tag_field_for_oid(repo, entry, |t| {
            t.tagger.as_ref().map(|s| format_identity_date(s, modifier)).unwrap_or_default()
        }),
        _ => Err(FormatError::Other(format!(
            "unsupported format atom: {atom}"
        ))),
    }
}

fn subject_for_oid(
    repo: &Repository,
    entry: &RefEntry,
    oid: ObjectId,
) -> Result<String, FormatError> {
    let object = repo
        .odb
        .read(&oid)
        .map_err(|_| FormatError::MissingObject(oid, entry.name.clone()))?;
    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data).map_err(|_| {
                FormatError::Other(format!("failed to parse commit object for {}", entry.name))
            })?;
            Ok(commit.message.lines().next().unwrap_or("").to_owned())
        }
        ObjectKind::Tag => {
            let tag = parse_tag(&object.data).map_err(|_| {
                FormatError::Other(format!("failed to parse tag for {}", entry.name))
            })?;
            Ok(tag.message.lines().next().unwrap_or("").to_owned())
        }
        _ => Ok(String::new()),
    }
}

fn body_for_oid(
    repo: &Repository,
    entry: &RefEntry,
    oid: ObjectId,
) -> Result<String, FormatError> {
    let object = repo
        .odb
        .read(&oid)
        .map_err(|_| FormatError::MissingObject(oid, entry.name.clone()))?;
    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data).map_err(|_| {
                FormatError::Other(format!("failed to parse commit for {}", entry.name))
            })?;
            // body is everything after the first line
            let mut lines = commit.message.splitn(2, '\n');
            lines.next(); // skip subject
            Ok(lines.next().unwrap_or("").trim_start_matches('\n').to_owned())
        }
        ObjectKind::Tag => {
            let tag = parse_tag(&object.data).map_err(|_| {
                FormatError::Other(format!("failed to parse tag for {}", entry.name))
            })?;
            let mut lines = tag.message.splitn(2, '\n');
            lines.next();
            Ok(lines.next().unwrap_or("").trim_start_matches('\n').to_owned())
        }
        _ => Ok(String::new()),
    }
}

fn commit_field_for_oid<F: Fn(&grit_lib::objects::CommitData) -> String>(
    repo: &Repository,
    entry: &RefEntry,
    oid: ObjectId,
    extractor: F,
) -> Result<String, FormatError> {
    let object = repo
        .odb
        .read(&oid)
        .map_err(|_| FormatError::MissingObject(oid, entry.name.clone()))?;
    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data).map_err(|_| {
                FormatError::Other(format!("failed to parse commit for {}", entry.name))
            })?;
            Ok(extractor(&commit))
        }
        ObjectKind::Tag => {
            // Peel to commit
            let tag = parse_tag(&object.data).map_err(|_| {
                FormatError::Other(format!("failed to parse tag for {}", entry.name))
            })?;
            let peeled = peel_to_non_tag(repo, tag.object)
                .map_err(|_| FormatError::MissingObject(oid, entry.name.clone()))?;
            let inner = repo.odb.read(&peeled)
                .map_err(|_| FormatError::MissingObject(peeled, entry.name.clone()))?;
            if inner.kind == ObjectKind::Commit {
                let commit = parse_commit(&inner.data).map_err(|_| {
                    FormatError::Other(format!("failed to parse commit for {}", entry.name))
                })?;
                Ok(extractor(&commit))
            } else {
                Ok(String::new())
            }
        }
        _ => Ok(String::new()),
    }
}

fn tag_field_for_oid<F: Fn(&grit_lib::objects::TagData) -> String>(
    repo: &Repository,
    entry: &RefEntry,
    extractor: F,
) -> Result<String, FormatError> {
    let object = read_object(repo, entry)?;
    if object.kind == ObjectKind::Tag {
        let tag = parse_tag(&object.data).map_err(|_| {
            FormatError::Other(format!("failed to parse tag for {}", entry.name))
        })?;
        Ok(extractor(&tag))
    } else {
        Ok(String::new())
    }
}

/// Parse identity name from a raw Git identity string like "Name <email> timestamp tz"
fn parse_identity_name(raw: &str) -> String {
    if let Some(pos) = raw.find('<') {
        raw[..pos].trim().to_owned()
    } else {
        raw.to_owned()
    }
}

/// Parse identity email from a raw Git identity string (includes angle brackets)
fn parse_identity_email(raw: &str) -> String {
    if let Some(start) = raw.find('<') {
        if let Some(end) = raw[start..].find('>') {
            return raw[start..start + end + 1].to_owned();
        }
    }
    String::new()
}

/// Parse the Unix timestamp and timezone from a raw Git identity string.
/// Returns (epoch_seconds, tz_offset_str like "+0200").
fn parse_identity_timestamp(raw: &str) -> Option<(i64, String)> {
    // Format: "Name <email> 1234567890 +0200"
    let after_email = if let Some(pos) = raw.find('>') {
        raw[pos + 1..].trim()
    } else {
        return None;
    };
    let mut parts = after_email.split_whitespace();
    let epoch: i64 = parts.next()?.parse().ok()?;
    let tz = parts.next().unwrap_or("+0000").to_owned();
    Some((epoch, tz))
}

/// Format a date from a raw identity string with an optional modifier.
fn format_identity_date(raw: &str, modifier: Option<&str>) -> String {
    let Some((epoch, tz)) = parse_identity_timestamp(raw) else {
        return String::new();
    };

    // Parse tz offset into seconds
    let tz_offset_secs = parse_tz_offset(&tz);
    let adjusted = epoch + tz_offset_secs as i64;

    match modifier {
        Some("short") => format_epoch_short(adjusted),
        Some("iso") | Some("iso8601") => format_epoch_iso(adjusted, &tz),
        Some("iso-strict") | Some("iso8601-strict") => format_epoch_iso_strict(adjusted, &tz),
        Some("unix") => epoch.to_string(),
        Some("relative") => format_epoch_relative(epoch),
        Some("raw") => format!("{epoch} {tz}"),
        _ => format_epoch_default(adjusted, &tz),
    }
}

fn parse_tz_offset(tz: &str) -> i32 {
    if tz.len() < 5 {
        return 0;
    }
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let hours: i32 = tz[1..3].parse().unwrap_or(0);
    let minutes: i32 = tz[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

fn days_from_epoch(epoch_adjusted: i64) -> (i32, u32, u32) {
    // Convert epoch seconds (already tz-adjusted) to Y-M-D
    let days = (epoch_adjusted / 86400) as i32;
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn format_epoch_short(epoch_adjusted: i64) -> String {
    let (y, m, d) = days_from_epoch(epoch_adjusted);
    format!("{y:04}-{m:02}-{d:02}")
}

fn format_epoch_iso(epoch_adjusted: i64, tz: &str) -> String {
    let (y, m, d) = days_from_epoch(epoch_adjusted);
    let secs_in_day = ((epoch_adjusted % 86400) + 86400) % 86400;
    let hh = secs_in_day / 3600;
    let mm = (secs_in_day % 3600) / 60;
    let ss = secs_in_day % 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} {tz}")
}

fn format_epoch_iso_strict(epoch_adjusted: i64, tz: &str) -> String {
    let (y, m, d) = days_from_epoch(epoch_adjusted);
    let secs_in_day = ((epoch_adjusted % 86400) + 86400) % 86400;
    let hh = secs_in_day / 3600;
    let mm = (secs_in_day % 3600) / 60;
    let ss = secs_in_day % 60;
    let tz_display = if tz.len() >= 5 {
        format!("{}:{}", &tz[..3], &tz[3..5])
    } else {
        tz.to_owned()
    };
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}{tz_display}")
}

fn format_epoch_default(epoch_adjusted: i64, tz: &str) -> String {
    // Git default: "Thu Jan  1 00:00:00 1970 +0000"
    let (y, m, d) = days_from_epoch(epoch_adjusted);
    let secs_in_day = ((epoch_adjusted % 86400) + 86400) % 86400;
    let hh = secs_in_day / 3600;
    let mm = (secs_in_day % 3600) / 60;
    let ss = secs_in_day % 60;

    let month_name = match m {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "???",
    };

    // Compute day of week via Zeller-like
    // epoch_adjusted / 86400 gives days since 1970-01-01 which was a Thursday
    let day_index = ((epoch_adjusted / 86400) % 7 + 4 + 7) % 7; // 0=Sun
    let day_name = match day_index {
        0 => "Sun", 1 => "Mon", 2 => "Tue", 3 => "Wed",
        4 => "Thu", 5 => "Fri", 6 => "Sat", _ => "???",
    };

    format!("{day_name} {month_name} {d} {hh:02}:{mm:02}:{ss:02} {y:04} {tz}")
}

fn format_epoch_relative(epoch: i64) -> String {
    // Very basic relative date
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = now - epoch;
    if diff < 60 {
        "just now".to_owned()
    } else if diff < 3600 {
        let m = diff / 60;
        format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
    } else if diff < 86400 {
        let h = diff / 3600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else if diff < 86400 * 30 {
        let d = diff / 86400;
        format!("{d} day{} ago", if d == 1 { "" } else { "s" })
    } else if diff < 86400 * 365 {
        let m = diff / (86400 * 30);
        format!("{m} month{} ago", if m == 1 { "" } else { "s" })
    } else {
        let y = diff / (86400 * 365);
        format!("{y} year{} ago", if y == 1 { "" } else { "s" })
    }
}

/// Resolve upstream tracking info for a branch ref.
fn resolve_upstream(
    repo: &Repository,
    entry: &RefEntry,
    modifier: Option<&str>,
) -> Result<String, FormatError> {
    // Only branches have upstreams
    let branch = match entry.name.strip_prefix("refs/heads/") {
        Some(b) => b,
        None => return Ok(String::new()),
    };

    // Read from git config: branch.<name>.remote and branch.<name>.merge
    let config_path = repo.git_dir.join("config");
    let config_text = fs::read_to_string(&config_path).unwrap_or_default();

    let remote = match parse_branch_config(&config_text, branch, "remote") {
        Some(r) => r,
        None => return Ok(String::new()),
    };
    let merge = match parse_branch_config(&config_text, branch, "merge") {
        Some(m) => m,
        None => return Ok(String::new()),
    };

    // Convert merge ref (refs/heads/X) to remote tracking ref (refs/remotes/<remote>/X)
    let remote_branch = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    let upstream_ref = format!("refs/remotes/{remote}/{remote_branch}");

    match modifier {
        Some("track") => {
            // Simple ahead/behind tracking
            let upstream_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &upstream_ref).ok();
            match upstream_oid {
                Some(up_oid) if up_oid == entry.oid => Ok(String::new()),
                Some(_up_oid) => Ok("[differs]".to_owned()),
                None => Ok("[gone]".to_owned()),
            }
        }
        Some("trackshort") => {
            let upstream_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &upstream_ref).ok();
            match upstream_oid {
                Some(up_oid) if up_oid == entry.oid => Ok("=".to_owned()),
                Some(_) => Ok("<>".to_owned()),
                None => Ok(String::new()),
            }
        }
        Some("short") => Ok(format!("{remote}/{remote_branch}")),
        _ => Ok(upstream_ref),
    }
}

/// Parse a simple branch config value from a git config file.
fn parse_branch_config(config: &str, branch: &str, key: &str) -> Option<String> {
    let section_header = format!("[branch \"{}\"]", branch);
    let mut in_section = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section_header
                || trimmed.replace(' ', "") == format!("[branch\"{}\"]", branch);
            continue;
        }
        if in_section {
            if let Some(rest) = trimmed.strip_prefix(key) {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    return Some(value.trim().to_owned());
                }
            }
        }
    }
    None
}

fn read_object(
    repo: &Repository,
    entry: &RefEntry,
) -> Result<grit_lib::objects::Object, FormatError> {
    repo.odb
        .read(&entry.oid)
        .map_err(|_| FormatError::MissingObject(entry.oid, entry.name.clone()))
}


fn short_refname(name: &str) -> String {
    for prefix in ["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Some(short) = name.strip_prefix(prefix) {
            return short.to_owned();
        }
    }
    name.to_owned()
}

fn peel_to_non_tag(
    repo: &Repository,
    mut oid: ObjectId,
) -> std::result::Result<ObjectId, GustError> {
    loop {
        let object = repo.odb.read(&oid)?;
        if object.kind != ObjectKind::Tag {
            return Ok(oid);
        }
        oid = parse_tag_target(&object.data)?;
    }
}

fn peel_to_commit(repo: &Repository, oid: ObjectId) -> std::result::Result<ObjectId, GustError> {
    let peeled = peel_to_non_tag(repo, oid)?;
    let object = repo.odb.read(&peeled)?;
    if object.kind == ObjectKind::Commit {
        Ok(peeled)
    } else {
        Err(GustError::CorruptObject(
            "object is not a commit".to_owned(),
        ))
    }
}

fn parse_tag_target(data: &[u8]) -> std::result::Result<ObjectId, GustError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| GustError::CorruptObject("invalid tag object".to_owned()))?;
    let Some(line) = text.lines().find(|line| line.starts_with("object ")) else {
        return Err(GustError::CorruptObject(
            "tag missing object header".to_owned(),
        ));
    };
    line.trim_start_matches("object ")
        .trim()
        .parse::<ObjectId>()
        .map_err(|_| GustError::CorruptObject("invalid tag target".to_owned()))
}

fn ref_matches_patterns(refname: &str, patterns: &[String], ignore_case: bool) -> bool {
    if patterns.is_empty() {
        return true;
    }
    patterns
        .iter()
        .any(|pattern| ref_matches_pattern(refname, pattern, ignore_case))
}

fn ref_matches_pattern(refname: &str, pattern: &str, ignore_case: bool) -> bool {
    let (name, pat) = if ignore_case {
        (refname.to_ascii_lowercase(), pattern.to_ascii_lowercase())
    } else {
        (refname.to_owned(), pattern.to_owned())
    };
    if has_wildcard(&pat) {
        wildcard_match(&name, &pat)
    } else if name == pat {
        true
    } else if pat.ends_with('/') {
        name.starts_with(&pat)
    } else {
        name.starts_with(&pat) && name.as_bytes().get(pat.len()) == Some(&b'/')
    }
}

fn has_wildcard(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn wildcard_match(name: &str, pattern: &str) -> bool {
    wildcard_match_bytes(name.as_bytes(), pattern.as_bytes())
}

fn wildcard_match_bytes(name: &[u8], pattern: &[u8]) -> bool {
    if pattern.is_empty() {
        return name.is_empty();
    }
    match pattern[0] {
        b'*' => {
            if wildcard_match_bytes(name, &pattern[1..]) {
                return true;
            }
            if !name.is_empty() {
                return wildcard_match_bytes(&name[1..], pattern);
            }
            false
        }
        b'?' => !name.is_empty() && wildcard_match_bytes(&name[1..], &pattern[1..]),
        ch => !name.is_empty() && name[0] == ch && wildcard_match_bytes(&name[1..], &pattern[1..]),
    }
}

fn is_zero_oid(oid: &ObjectId) -> bool {
    oid.as_bytes().iter().all(|b| *b == 0)
}
