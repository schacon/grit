//! `grit grep` — search tracked files for a pattern.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use regex::{Regex, RegexBuilder};
use std::io::{self, Write};

use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;

/// Arguments for `grit grep`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show line numbers.
    #[arg(short = 'n', long = "line-number")]
    pub line_number: bool,

    /// Show count of matching lines per file.
    #[arg(short = 'c', long = "count")]
    pub count: bool,

    /// Show only filenames with matches.
    #[arg(short = 'l', long = "files-with-matches")]
    pub files_with_matches: bool,

    /// Show only filenames without matches.
    #[arg(short = 'L', long = "files-without-match")]
    pub files_without_match: bool,

    /// Case insensitive matching.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,

    /// Match whole words only.
    #[arg(short = 'w', long = "word-regexp")]
    pub word_regexp: bool,

    /// Invert match (show non-matching lines).
    #[arg(short = 'v', long = "invert-match")]
    pub invert_match: bool,

    /// Explicit pattern (can be used multiple times).
    #[arg(short = 'e', value_name = "PATTERN")]
    pub patterns: Vec<String>,

    /// Use extended regular expressions (default).
    #[arg(short = 'E', long = "extended-regexp")]
    pub extended_regexp: bool,

    /// Use fixed strings (literal matching, no regex).
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,

    /// Positional arguments: [pattern] [<tree>] [-- pathspec...]
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub positional: Vec<String>,
}

/// Run `grit grep`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Parse positional arguments: [pattern] [tree-ish] [-- pathspec...]
    let (patterns, tree_ish, pathspecs) = parse_positional(&args, &repo)?;

    if patterns.is_empty() {
        bail!("no pattern given");
    }

    // Build the regex matchers
    let matchers = build_matchers(&patterns, &args)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut found_any = false;

    if let Some(tree_spec) = &tree_ish {
        // Search a tree object
        let oid = resolve_revision(&repo, tree_spec)
            .or_else(|_| resolve_ref(&repo.git_dir, tree_spec))
            .or_else(|_| resolve_ref(&repo.git_dir, &format!("refs/heads/{tree_spec}")))
            .with_context(|| format!("not a valid revision: '{tree_spec}'"))?;

        let obj = repo.odb.read(&oid)?;
        let tree_oid = if obj.kind == ObjectKind::Commit {
            let commit = parse_commit(&obj.data)?;
            commit.tree
        } else if obj.kind == ObjectKind::Tree {
            oid
        } else {
            bail!("'{}' is not a tree-ish", tree_spec);
        };

        let tree_obj = repo.odb.read(&tree_oid)?;
        found_any = grep_tree(
            &repo,
            &tree_obj.data,
            "",
            &matchers,
            &args,
            &pathspecs,
            Some(tree_spec),
            &mut out,
        )?;
    } else {
        // Search working tree (tracked files from index)
        let work_tree = repo
            .work_tree
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("cannot grep in bare repository"))?;

        let index = Index::load(&repo.index_path()).context("loading index")?;

        // Deduplicate paths (index may have multiple stages for conflicts)
        let mut seen = std::collections::HashSet::new();
        for entry in &index.entries {
            let path_str = String::from_utf8_lossy(&entry.path).to_string();

            if !seen.insert(path_str.clone()) {
                continue;
            }

            // Apply pathspec filter
            if !pathspecs.is_empty()
                && !pathspecs
                    .iter()
                    .any(|p| path_str == *p || path_str.starts_with(&format!("{p}/")))
            {
                continue;
            }

            let full_path = work_tree.join(&path_str);
            let content = match std::fs::read(&full_path) {
                Ok(c) => c,
                Err(_) => continue, // deleted but still in index
            };

            // Skip binary files (contains null bytes in first 8000 bytes)
            if content.iter().take(8000).any(|&b| b == 0) {
                continue;
            }

            let content_str = String::from_utf8_lossy(&content);
            if grep_content(&path_str, &content_str, &matchers, &args, None, &mut out)? {
                found_any = true;
            }
        }
    }

    if found_any {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

/// Try to resolve a string as a revision (commit/tree).
fn is_revision(repo: &Repository, spec: &str) -> bool {
    resolve_revision(repo, spec).is_ok()
        || resolve_ref(&repo.git_dir, spec).is_ok()
        || resolve_ref(&repo.git_dir, &format!("refs/heads/{spec}")).is_ok()
}

/// Parse positional arguments into (patterns, tree_ish, pathspecs).
/// Uses the repo to disambiguate tree-ish vs pathspec.
fn parse_positional(
    args: &Args,
    repo: &Repository,
) -> Result<(Vec<String>, Option<String>, Vec<String>)> {
    let mut patterns = args.patterns.clone();
    let positional = &args.positional;

    // Find `--` separator
    let sep_pos = positional.iter().position(|a| a == "--");

    let (before_sep, pathspecs) = match sep_pos {
        Some(pos) => (&positional[..pos], positional[pos + 1..].to_vec()),
        None => (positional.as_slice(), Vec::new()),
    };

    let mut tree_ish = None;

    if patterns.is_empty() {
        // First positional is the pattern (required)
        if before_sep.is_empty() {
            return Ok((patterns, tree_ish, pathspecs));
        }
        patterns.push(before_sep[0].clone());
        // Remaining: check if first remaining resolves as revision
        let rest = &before_sep[1..];
        if !rest.is_empty() && is_revision(repo, &rest[0]) {
            tree_ish = Some(rest[0].clone());
            let mut ps = pathspecs;
            ps.extend(rest[1..].iter().cloned());
            return Ok((patterns, tree_ish, ps));
        }
        // Otherwise all remaining are pathspecs
        let mut ps = pathspecs;
        ps.extend(rest.iter().cloned());
        return Ok((patterns, tree_ish, ps));
    }

    // Patterns given via -e; positional args are [tree-ish] [pathspecs...]
    if !before_sep.is_empty() && is_revision(repo, &before_sep[0]) {
        tree_ish = Some(before_sep[0].clone());
        let mut ps = pathspecs;
        ps.extend(before_sep[1..].iter().cloned());
        return Ok((patterns, tree_ish, ps));
    }

    // All positional are pathspecs
    let mut ps = pathspecs;
    ps.extend(before_sep.iter().cloned());
    Ok((patterns, tree_ish, ps))
}

/// Build regex matchers from patterns.
fn build_matchers(patterns: &[String], args: &Args) -> Result<Vec<Regex>> {
    let mut matchers = Vec::new();
    for pat in patterns {
        let effective = if args.fixed_strings {
            regex::escape(pat)
        } else {
            pat.clone()
        };
        let effective = if args.word_regexp {
            format!(r"\b{effective}\b")
        } else {
            effective
        };
        let re = RegexBuilder::new(&effective)
            .case_insensitive(args.ignore_case)
            .build()
            .with_context(|| format!("invalid pattern: '{pat}'"))?;
        matchers.push(re);
    }
    Ok(matchers)
}

/// Search content of a single file. Returns true if any match found.
fn grep_content(
    filename: &str,
    content: &str,
    matchers: &[Regex],
    args: &Args,
    tree_prefix: Option<&str>,
    out: &mut impl Write,
) -> Result<bool> {
    let mut match_count = 0u64;
    let mut has_match = false;

    for (lineno, line) in content.lines().enumerate() {
        // A line matches if ANY pattern matches (implicit OR for multiple -e)
        let line_matches = matchers.iter().any(|re| re.is_match(line));
        let effective_match = if args.invert_match {
            !line_matches
        } else {
            line_matches
        };

        if effective_match {
            has_match = true;
            match_count += 1;

            if args.files_with_matches || args.files_without_match {
                // Will handle after the loop
                continue;
            }
            if args.count {
                continue;
            }

            // Print matching line
            let display_name = match tree_prefix {
                Some(p) => format!("{p}:{filename}"),
                None => filename.to_string(),
            };
            if args.line_number {
                writeln!(out, "{display_name}:{}:{line}", lineno + 1)?;
            } else {
                writeln!(out, "{display_name}:{line}")?;
            }
        }
    }

    let display_name = match tree_prefix {
        Some(p) => format!("{p}:{filename}"),
        None => filename.to_string(),
    };

    if args.files_with_matches {
        if has_match {
            writeln!(out, "{display_name}")?;
        }
        return Ok(has_match);
    }

    if args.files_without_match {
        if !has_match {
            writeln!(out, "{display_name}")?;
            return Ok(true); // counts as "found" for exit code
        }
        return Ok(false);
    }

    if args.count {
        if match_count > 0 {
            writeln!(out, "{display_name}:{match_count}")?;
        }
        return Ok(has_match);
    }

    Ok(has_match)
}

/// Recursively search a tree object.
fn grep_tree(
    repo: &Repository,
    tree_data: &[u8],
    prefix: &str,
    matchers: &[Regex],
    args: &Args,
    pathspecs: &[String],
    tree_name: Option<&str>,
    out: &mut impl Write,
) -> Result<bool> {
    let entries = parse_tree(tree_data)?;
    let mut found = false;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let full_name = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let is_tree = entry.mode == 0o040000;

        // Apply pathspec filter
        if !pathspecs.is_empty() {
            let matches_pathspec = pathspecs.iter().any(|p| {
                full_name == *p
                    || full_name.starts_with(&format!("{p}/"))
                    || p.starts_with(&format!("{full_name}/"))
            });
            if !matches_pathspec {
                continue;
            }
        }

        if is_tree {
            let sub_obj = repo.odb.read(&entry.oid)?;
            if grep_tree(repo, &sub_obj.data, &full_name, matchers, args, pathspecs, tree_name, out)? {
                found = true;
            }
        } else {
            // Read blob
            let obj = match repo.odb.read(&entry.oid) {
                Ok(o) => o,
                Err(_) => continue,
            };

            // Skip binary
            if obj.data.iter().take(8000).any(|&b| b == 0) {
                continue;
            }

            let content = String::from_utf8_lossy(&obj.data);
            if grep_content(&full_name, &content, matchers, args, tree_name, out)? {
                found = true;
            }
        }
    }

    Ok(found)
}
