//! `grit check-attr` — display gitattributes information.
//!
//! Reads `.gitattributes` files and displays the value of the requested
//! attribute(s) for the given path(s).
//!
//! Usage: `grit check-attr <attr> -- <pathname>...`

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit check-attr`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display gitattributes information")]
pub struct Args {
    /// Attribute name(s) and pathnames.
    ///
    /// Usage: `grit check-attr <attr> [<attr>...] -- <pathname>...`
    /// or with `-a`: `grit check-attr -a -- <pathname>...`
    #[arg(required = true, allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,

    /// Report all attributes set for each file.
    #[arg(short = 'a', long = "all")]
    pub all: bool,
}

/// A parsed gitattributes rule.
struct AttrRule {
    pattern: String,
    attrs: Vec<(String, AttrValue)>,
}

/// Possible attribute values.
#[derive(Clone)]
enum AttrValue {
    Set,           // attr
    Unset,         // -attr
    Value(String), // attr=value
}

impl std::fmt::Display for AttrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttrValue::Set => write!(f, "set"),
            AttrValue::Unset => write!(f, "unset"),
            AttrValue::Value(v) => write!(f, "{v}"),
        }
    }
}

/// Run the `check-attr` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("bare repository — no work tree"))?;

    // Parse args: split on "--" into attrs and paths
    let (attrs, paths) = if args.all {
        // -a mode: everything after "--" is paths
        let mut paths = Vec::new();
        let mut after_sep = false;
        for arg in &args.args {
            if arg == "--" {
                after_sep = true;
                continue;
            }
            if after_sep {
                paths.push(arg.clone());
            } else {
                paths.push(arg.clone());
            }
        }
        (Vec::<String>::new(), paths)
    } else {
        let mut attrs = Vec::new();
        let mut paths = Vec::new();
        let mut after_sep = false;
        for arg in &args.args {
            if arg == "--" {
                after_sep = true;
                continue;
            }
            if after_sep {
                paths.push(arg.clone());
            } else {
                attrs.push(arg.clone());
            }
        }
        // If no separator was found, last arg is the path, rest are attrs
        if !after_sep && attrs.len() > 1 {
            let path = attrs.pop().unwrap_or_default();
            paths.push(path);
        } else if !after_sep && attrs.len() == 1 {
            // Single arg with no separator — treat as path with no attrs
            paths.push(attrs.remove(0));
        }
        (attrs, paths)
    };

    // Load gitattributes
    let rules = load_gitattributes(work_tree)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for path in &paths {
        if args.all {
            // Report all attributes
            let matched = find_all_attrs(&rules, path);
            if matched.is_empty() {
                // Nothing to report
            } else {
                for (attr_name, value) in &matched {
                    writeln!(out, "{path}: {attr_name}: {value}")?;
                }
            }
        } else {
            for attr in &attrs {
                let value = find_attr(&rules, path, attr);
                let display = match value {
                    Some(v) => v.to_string(),
                    None => "unspecified".to_owned(),
                };
                writeln!(out, "{path}: {attr}: {display}")?;
            }
        }
    }

    Ok(())
}

/// Load `.gitattributes` from the work tree root and any nested directories.
fn load_gitattributes(work_tree: &Path) -> Result<Vec<AttrRule>> {
    let mut rules = Vec::new();

    // Load root .gitattributes
    let root_attrs = work_tree.join(".gitattributes");
    if root_attrs.exists() {
        parse_gitattributes_file(&root_attrs, "", &mut rules)?;
    }

    // Load info/attributes from .git
    let info_attrs = work_tree.join(".git/info/attributes");
    if info_attrs.exists() {
        parse_gitattributes_file(&info_attrs, "", &mut rules)?;
    }

    Ok(rules)
}

/// Parse a single .gitattributes file.
fn parse_gitattributes_file(path: &Path, prefix: &str, rules: &mut Vec<AttrRule>) -> Result<()> {
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let pattern = match parts.next() {
            Some(p) => {
                if prefix.is_empty() {
                    p.to_owned()
                } else {
                    format!("{prefix}/{p}")
                }
            }
            None => continue,
        };

        let mut attrs = Vec::new();
        for part in parts {
            if let Some(rest) = part.strip_prefix('-') {
                attrs.push((rest.to_owned(), AttrValue::Unset));
            } else if let Some((key, val)) = part.split_once('=') {
                attrs.push((key.to_owned(), AttrValue::Value(val.to_owned())));
            } else {
                attrs.push((part.to_owned(), AttrValue::Set));
            }
        }

        if !attrs.is_empty() {
            rules.push(AttrRule { pattern, attrs });
        }
    }

    Ok(())
}

/// Find the value of a specific attribute for a path.
fn find_attr(rules: &[AttrRule], path: &str, attr: &str) -> Option<AttrValue> {
    // Last matching rule wins
    let mut result = None;
    for rule in rules {
        if pattern_matches(&rule.pattern, path) {
            for (name, value) in &rule.attrs {
                if name == attr {
                    result = Some(value.clone());
                }
            }
        }
    }
    result
}

/// Find all attributes set for a path.
fn find_all_attrs(rules: &[AttrRule], path: &str) -> Vec<(String, AttrValue)> {
    let mut map: std::collections::BTreeMap<String, AttrValue> = std::collections::BTreeMap::new();
    for rule in rules {
        if pattern_matches(&rule.pattern, path) {
            for (name, value) in &rule.attrs {
                map.insert(name.clone(), value.clone());
            }
        }
    }
    map.into_iter().collect()
}

/// Match a gitattributes pattern against a path.
///
/// Supports basic glob: `*` matches within a component, `**` not yet.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    // If pattern has no slash, match against basename only
    if !pattern.contains('/') {
        let basename = path.rsplit('/').next().unwrap_or(path);
        return glob_matches(pattern, basename);
    }
    glob_matches(pattern, path)
}

/// Simple glob matcher supporting `*` and `?`.
fn glob_matches(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_bytes(pat: &[u8], text: &[u8]) -> bool {
    match (pat.first(), text.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            let pat_rest = pat
                .iter()
                .position(|&b| b != b'*')
                .map_or(&pat[pat.len()..], |i| &pat[i..]);
            if pat_rest.is_empty() {
                return true;
            }
            for i in 0..=text.len() {
                if glob_match_bytes(pat_rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        (Some(&b'?'), Some(_)) => glob_match_bytes(&pat[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob_match_bytes(&pat[1..], &text[1..]),
        _ => false,
    }
}
