//! `gust for-each-ref` - output information on refs.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use gust_lib::error::Error as GustError;
use gust_lib::merge_base::is_ancestor;
use gust_lib::objects::{parse_commit, ObjectId, ObjectKind};
use gust_lib::repo::Repository;
use gust_lib::rev_parse::resolve_revision;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Arguments for `gust for-each-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `gust for-each-ref`.
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
    let max = opts.count.unwrap_or(usize::MAX);
    let mut printed = 0usize;
    for entry in refs {
        if printed >= max {
            break;
        }
        match expand_format(&repo, &entry, &format) {
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
            opts.merged = Some(None);
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--no-merged=") {
            opts.no_merged = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--no-merged" {
            opts.no_merged = Some(None);
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--contains=") {
            opts.contains = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--contains" {
            opts.contains = Some(None);
            i += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--no-contains=") {
            opts.no_contains = Some(Some(value.to_owned()));
            i += 1;
            continue;
        }
        if arg == "--no-contains" {
            opts.no_contains = Some(None);
            i += 1;
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
        return match gust_lib::refs::resolve_ref(git_dir, refname) {
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

fn expand_format(repo: &Repository, entry: &RefEntry, format: &str) -> Result<String, FormatError> {
    let mut out = String::new();
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            return Err(FormatError::Other("unterminated format atom".to_owned()));
        };
        let atom = &after[..end];
        out.push_str(&atom_value(repo, entry, atom)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn atom_value<'a>(
    repo: &Repository,
    entry: &'a RefEntry,
    atom: &'a str,
) -> Result<String, FormatError> {
    match atom {
        "refname" => Ok(entry.name.clone()),
        "refname:short" => Ok(short_refname(&entry.name)),
        "objectname" => Ok(entry.oid.to_string()),
        "objecttype" => {
            let object = read_object(repo, entry)?;
            Ok(object.kind.to_string())
        }
        "subject" => subject_for_oid(repo, entry, entry.oid),
        "*subject" => {
            let peeled = peel_to_non_tag(repo, entry.oid)
                .map_err(|_| FormatError::MissingObject(entry.oid, entry.name.clone()))?;
            subject_for_oid(repo, entry, peeled)
        }
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
        ObjectKind::Tag => Ok(parse_tag_subject(&object.data)),
        _ => Ok(String::new()),
    }
}

fn read_object(
    repo: &Repository,
    entry: &RefEntry,
) -> Result<gust_lib::objects::Object, FormatError> {
    repo.odb
        .read(&entry.oid)
        .map_err(|_| FormatError::MissingObject(entry.oid, entry.name.clone()))
}

fn parse_tag_subject(data: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(data) else {
        return String::new();
    };
    let mut in_message = false;
    for line in text.lines() {
        if in_message {
            return line.to_owned();
        }
        if line.is_empty() {
            in_message = true;
        }
    }
    String::new()
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
