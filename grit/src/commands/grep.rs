//! `grit grep` — search tracked files for a pattern.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use regex::{Regex, RegexBuilder};
use std::io::{self, Write};

use grit_lib::config::ConfigSet;
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

    /// Suppress line numbers (overrides -n).
    #[arg(long = "no-line-number", hide = true)]
    pub no_line_number: bool,

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

    /// Use Perl-compatible regular expressions.
    #[arg(short = 'P', long = "perl-regexp")]
    pub perl_regexp: bool,

    /// Use fixed strings (literal matching, no regex).
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,

    /// Read patterns from file, one per line.
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub pattern_file: Option<String>,

    /// Combine patterns with AND (all -e patterns must match).
    #[arg(long = "and")]
    pub and: bool,

    /// Combine patterns with OR (any -e pattern must match) — this is the default.
    #[arg(long = "or")]
    pub or: bool,

    /// Negate the next pattern (can appear multiple times).
    #[arg(long = "not", action = clap::ArgAction::Count)]
    pub not: u8,

    /// Require all patterns to match (line-level AND).
    #[arg(long = "all-match")]
    pub all_match: bool,

    /// Show the whole function as context.
    #[arg(short = 'W', long = "function-context")]
    pub function_context: bool,

    /// Number of threads to use (accepted but ignored).
    #[arg(long = "threads", value_name = "N")]
    pub threads: Option<usize>,

    /// Show column number of first match.
    #[arg(long = "column")]
    pub column: bool,

    /// Show context lines after match.
    #[arg(short = 'A', long = "after-context", value_name = "NUM", allow_hyphen_values = true)]
    pub after_context: Option<usize>,

    /// Show context lines before match.
    #[arg(short = 'B', long = "before-context", value_name = "NUM", allow_hyphen_values = true)]
    pub before_context: Option<usize>,

    /// Show context lines before and after match.
    #[arg(short = 'C', long = "context", value_name = "NUM", allow_hyphen_values = true)]
    pub context: Option<usize>,

    /// Only print the matched parts of a matching line.
    #[arg(short = 'o', long = "only-matching")]
    pub only_matching: bool,

    /// Descend at most <depth> levels of directories.
    #[arg(long = "max-depth", value_name = "DEPTH")]
    pub max_depth: Option<usize>,

    /// Use color in output: always, never, auto.
    #[arg(long = "color", value_name = "WHEN", default_value = "never")]
    pub color: String,

    /// Positional arguments: [pattern] [<tree>] [-- pathspec...]
    #[arg(trailing_var_arg = true)]
    pub positional: Vec<String>,
}

impl Args {
    fn before_ctx(&self) -> usize {
        self.context.or(self.before_context).unwrap_or(0)
    }
    fn after_ctx(&self) -> usize {
        self.context.or(self.after_context).unwrap_or(0)
    }
    fn use_color(&self) -> bool {
        self.color == "always"
    }
    fn has_context(&self) -> bool {
        self.before_ctx() > 0 || self.after_ctx() > 0
    }
    fn show_line_number(&self) -> bool {
        // effective_line_number is set in run() to account for config
        self.line_number && !self.no_line_number
    }
}

/// Run `grit grep`.
pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Apply grep config settings
    {
        let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
        if let Some(ref c) = config {
            // grep.linenumber: if user didn't explicitly pass -n or --no-line-number
            if !args.line_number && !args.no_line_number {
                if let Some(val) = c.get("grep.linenumber") {
                    args.line_number = val == "true" || val == "1" || val == "yes";
                }
            }
            // grep.patternType / grep.extendedRegexp: affect regex mode
            // Only apply if user didn't explicitly pass -E, -F, -P, or -G
            if !args.extended_regexp && !args.fixed_strings && !args.perl_regexp {
                if let Some(pt) = c.get("grep.patterntype").or_else(|| c.get("grep.patternType")) {
                    match pt.to_lowercase().as_str() {
                        "extended" => args.extended_regexp = true,
                        "fixed" => args.fixed_strings = true,
                        "perl" => args.perl_regexp = true,
                        "basic" | "default" => { /* default BRE behavior */ }
                        _ => {}
                    }
                }
                // grep.extendedRegexp is lower priority than grep.patternType
                if !args.extended_regexp && !args.fixed_strings && !args.perl_regexp {
                    if let Some(val) = c.get("grep.extendedregexp").or_else(|| c.get("grep.extendedRegexp")) {
                        if val == "true" || val == "1" || val == "yes" {
                            args.extended_regexp = true;
                        }
                    }
                }
            }
        }
    }

    // Parse positional arguments: [pattern] [tree-ish] [-- pathspec...]
    let (mut patterns, tree_ish, pathspecs) = parse_positional(&args, &repo)?;

    // Read patterns from file if -f is given
    if let Some(ref pattern_file) = args.pattern_file {
        let content = std::fs::read_to_string(pattern_file)
            .with_context(|| format!("cannot read pattern file: '{pattern_file}'"))?;
        for line in content.lines() {
            if !line.is_empty() {
                patterns.push(line.to_string());
            }
        }
    }

    if patterns.is_empty() {
        bail!("no pattern given");
    }

    // Determine matching mode: --all-match or --and means all patterns must match a line
    let all_match = args.all_match || args.and;

    // Build the regex matchers
    let matchers = build_matchers(&patterns, &args)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut found_any = false;
    // Tracks whether we need a "--" separator before the next context group
    let mut need_sep = false;
    let all_match = all_match; // shadow to pass into closures

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
            0,
            &matchers,
            &args,
            &pathspecs,
            Some(tree_spec),
            &mut need_sep,
            &mut out,
            all_match,
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

            // Apply max-depth filter
            if let Some(max_depth) = args.max_depth {
                let depth = path_str.matches('/').count();
                if depth > max_depth {
                    continue;
                }
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
            if grep_content(
                &path_str,
                &content_str,
                &matchers,
                &args,
                None,
                &mut need_sep,
                &mut out,
                all_match,
            )? {
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
        if before_sep.is_empty() {
            return Ok((patterns, tree_ish, pathspecs));
        }
        patterns.push(before_sep[0].clone());
        let rest = &before_sep[1..];
        if !rest.is_empty() && is_revision(repo, &rest[0]) {
            tree_ish = Some(rest[0].clone());
            let mut ps = pathspecs;
            ps.extend(rest[1..].iter().cloned());
            return Ok((patterns, tree_ish, ps));
        }
        let mut ps = pathspecs;
        ps.extend(rest.iter().cloned());
        return Ok((patterns, tree_ish, ps));
    }

    if !before_sep.is_empty() && is_revision(repo, &before_sep[0]) {
        tree_ish = Some(before_sep[0].clone());
        let mut ps = pathspecs;
        ps.extend(before_sep[1..].iter().cloned());
        return Ok((patterns, tree_ish, ps));
    }

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

// Color constants
const COLOR_FILENAME: &str = "\x1b[35m";
const COLOR_LINENO: &str = "\x1b[32m";
const COLOR_COLUMNNO: &str = "\x1b[32m";
const COLOR_MATCH: &str = "\x1b[1;31m";
const COLOR_SEP: &str = "\x1b[36m";
const COLOR_RESET: &str = "\x1b[m";

/// Colorize all matches in a line.
fn colorize_matches(line: &str, matchers: &[Regex]) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for re in matchers {
        for m in re.find_iter(line) {
            ranges.push((m.start(), m.end()));
        }
    }
    if ranges.is_empty() {
        return line.to_string();
    }
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    let mut result = String::new();
    let mut pos = 0;
    for (s, e) in merged {
        result.push_str(&line[pos..s]);
        result.push_str(COLOR_MATCH);
        result.push_str(&line[s..e]);
        result.push_str(COLOR_RESET);
        pos = e;
    }
    result.push_str(&line[pos..]);
    result
}

fn sep_char(ch: char, color: bool) -> String {
    if color {
        format!("{COLOR_SEP}{ch}{COLOR_RESET}")
    } else {
        ch.to_string()
    }
}

fn fmt_name(name: &str, color: bool) -> String {
    if color {
        format!("{COLOR_FILENAME}{name}{COLOR_RESET}")
    } else {
        name.to_string()
    }
}

fn fmt_num(n: usize, color: bool) -> String {
    if color {
        format!("{COLOR_LINENO}{n}{COLOR_RESET}")
    } else {
        n.to_string()
    }
}

fn fmt_col(n: usize, color: bool) -> String {
    if color {
        format!("{COLOR_COLUMNNO}{n}{COLOR_RESET}")
    } else {
        n.to_string()
    }
}

/// Get column (1-based) of first match in a line.
fn first_match_col(line: &str, matchers: &[Regex]) -> Option<usize> {
    let mut earliest: Option<usize> = None;
    for re in matchers {
        if let Some(m) = re.find(line) {
            let col = m.start() + 1;
            earliest = Some(earliest.map_or(col, |e: usize| e.min(col)));
        }
    }
    earliest
}

/// Search content of a single file. Returns true if any match found.
/// `need_sep` tracks whether a "--" separator should be printed before the next context group.
fn grep_content(
    filename: &str,
    content: &str,
    matchers: &[Regex],
    args: &Args,
    tree_prefix: Option<&str>,
    need_sep: &mut bool,
    out: &mut impl Write,
    all_match: bool,
) -> Result<bool> {
    let color = args.use_color();

    let display_name = match tree_prefix {
        Some(p) => format!("{p}:{filename}"),
        None => filename.to_string(),
    };

    let lines: Vec<&str> = content.lines().collect();
    let nlines = lines.len();
    let before = args.before_ctx();
    let after = args.after_ctx();
    let use_context = args.has_context();

    // Collect matching line indices
    let mut match_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line_matches = if all_match {
            matchers.iter().all(|re| re.is_match(line))
        } else {
            matchers.iter().any(|re| re.is_match(line))
        };
        let effective_match = if args.invert_match {
            !line_matches
        } else {
            line_matches
        };
        if effective_match {
            match_indices.push(i);
        }
    }

    let has_match = !match_indices.is_empty();
    let match_count = match_indices.len() as u64;

    // Special modes: files-with-matches, files-without-match, count
    if args.files_with_matches {
        if has_match {
            writeln!(out, "{}", fmt_name(&display_name, color))?;
        }
        return Ok(has_match);
    }

    if args.files_without_match {
        if !has_match {
            writeln!(out, "{}", fmt_name(&display_name, color))?;
            return Ok(true);
        }
        return Ok(false);
    }

    if args.count {
        if match_count > 0 {
            writeln!(
                out,
                "{}{}{}",
                fmt_name(&display_name, color),
                sep_char(':', color),
                match_count
            )?;
        }
        return Ok(has_match);
    }

    if !has_match {
        return Ok(false);
    }

    // --only-matching
    if args.only_matching {
        for &idx in &match_indices {
            let line = lines[idx];
            // For each matcher, iterate over matches tracking column
            // as git does: accumulate cno from initial col+1 then add rm_eo
            // after each match (git grep.c show_line logic).
            for re in matchers {
                // Find the first match to get the initial column.
                let first_m = re.find(line);
                if first_m.is_none() {
                    continue;
                }
                let first_m = first_m.unwrap();
                let mut cno = first_m.start() + 1; // 1-indexed initial column
                let mut remaining = &line[first_m.start()..];

                // Output first match
                {
                    let matched_text = first_m.as_str();
                    let mut prefix_str = fmt_name(&display_name, color);
                    prefix_str.push_str(&sep_char(':', color));
                    if args.show_line_number() {
                        prefix_str.push_str(&fmt_num(idx + 1, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if args.column {
                        prefix_str.push_str(&fmt_col(cno, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if color {
                        writeln!(out, "{prefix_str}{COLOR_MATCH}{matched_text}{COLOR_RESET}")?;
                    } else {
                        writeln!(out, "{prefix_str}{matched_text}")?;
                    }
                }

                // Advance past first match and find subsequent matches
                let match_len = first_m.end() - first_m.start();
                cno += match_len;
                remaining = &remaining[match_len..];

                loop {
                    let next_m = re.find(remaining);
                    if next_m.is_none() {
                        break;
                    }
                    let next_m = next_m.unwrap();
                    let matched_text = next_m.as_str();
                    let col = cno + next_m.start();
                    let mut prefix_str = fmt_name(&display_name, color);
                    prefix_str.push_str(&sep_char(':', color));
                    if args.show_line_number() {
                        prefix_str.push_str(&fmt_num(idx + 1, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if args.column {
                        prefix_str.push_str(&fmt_col(col, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if color {
                        writeln!(out, "{prefix_str}{COLOR_MATCH}{matched_text}{COLOR_RESET}")?;
                    } else {
                        writeln!(out, "{prefix_str}{matched_text}")?;
                    }
                    let advance = next_m.end();
                    cno += advance;
                    remaining = &remaining[advance..];
                }
            }
        }
        return Ok(true);
    }

    // Context mode
    if use_context {
        // Build groups of (start, end) ranges
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for &idx in &match_indices {
            let start = idx.saturating_sub(before);
            let end = (idx + after).min(nlines - 1);
            if let Some(last) = groups.last_mut() {
                if start <= last.1 + 1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            groups.push((start, end));
        }

        let match_set: std::collections::HashSet<usize> =
            match_indices.iter().copied().collect();

        for &(start, end) in &groups {
            // Print separator between groups (including across files)
            if *need_sep {
                writeln!(out, "--")?;
            }
            *need_sep = true;

            for i in start..=end {
                let is_match_line = match_set.contains(&i);
                let separator = if is_match_line { ':' } else { '-' };
                let mut prefix_str = fmt_name(&display_name, color);
                prefix_str.push_str(&sep_char(separator, color));
                if args.show_line_number() {
                    prefix_str.push_str(&fmt_num(i + 1, color));
                    prefix_str.push_str(&sep_char(separator, color));
                }
                if args.column && is_match_line {
                    if let Some(col) = first_match_col(lines[i], matchers) {
                        prefix_str.push_str(&fmt_col(col, color));
                        prefix_str.push_str(&sep_char(separator, color));
                    }
                }
                if color && is_match_line {
                    writeln!(
                        out,
                        "{prefix_str}{}",
                        colorize_matches(lines[i], matchers)
                    )?;
                } else {
                    writeln!(out, "{prefix_str}{}", lines[i])?;
                }
            }
        }
    } else {
        // No context — just print matching lines
        for &idx in &match_indices {
            let line = lines[idx];
            let mut prefix_str = fmt_name(&display_name, color);
            prefix_str.push_str(&sep_char(':', color));
            if args.show_line_number() {
                prefix_str.push_str(&fmt_num(idx + 1, color));
                prefix_str.push_str(&sep_char(':', color));
            }
            if args.column {
                if let Some(col) = first_match_col(line, matchers) {
                    prefix_str.push_str(&fmt_col(col, color));
                    prefix_str.push_str(&sep_char(':', color));
                }
            }
            if color {
                writeln!(
                    out,
                    "{prefix_str}{}",
                    colorize_matches(line, matchers)
                )?;
            } else {
                writeln!(out, "{prefix_str}{line}")?;
            }
        }
    }

    Ok(true)
}

/// Recursively search a tree object.
fn grep_tree(
    repo: &Repository,
    tree_data: &[u8],
    prefix: &str,
    depth: usize,
    matchers: &[Regex],
    args: &Args,
    pathspecs: &[String],
    tree_name: Option<&str>,
    need_sep: &mut bool,
    out: &mut impl Write,
    all_match: bool,
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
            if let Some(max_depth) = args.max_depth {
                if depth >= max_depth {
                    continue;
                }
            }
            let sub_obj = repo.odb.read(&entry.oid)?;
            if grep_tree(
                repo,
                &sub_obj.data,
                &full_name,
                depth + 1,
                matchers,
                args,
                pathspecs,
                tree_name,
                need_sep,
                out,
                all_match,
            )? {
                found = true;
            }
        } else {
            if let Some(max_depth) = args.max_depth {
                let file_depth = full_name.matches('/').count();
                if file_depth > max_depth {
                    continue;
                }
            }

            let obj = match repo.odb.read(&entry.oid) {
                Ok(o) => o,
                Err(_) => continue,
            };

            if obj.data.iter().take(8000).any(|&b| b == 0) {
                continue;
            }

            let content = String::from_utf8_lossy(&obj.data);
            if grep_content(&full_name, &content, matchers, args, tree_name, need_sep, out, all_match)? {
                found = true;
            }
        }
    }

    Ok(found)
}
