//! `grit ls-files` — list information about files in the index and working tree.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::Component;
use std::path::PathBuf;

use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{Index, IndexEntry};
use grit_lib::repo::Repository;

/// Arguments for `grit ls-files`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show cached (staged) files (default).
    #[arg(short = 'c', long)]
    pub cached: bool,

    /// Show deleted files.
    #[arg(short = 'd', long)]
    pub deleted: bool,

    /// Show modified files.
    #[arg(short = 'm', long)]
    pub modified: bool,

    /// Show other (untracked) files.
    #[arg(short = 'o', long)]
    pub others: bool,

    /// Show ignored files.
    #[arg(short = 'i', long)]
    pub ignored: bool,

    /// Show unmerged files.
    #[arg(short = 'u', long)]
    pub unmerged: bool,

    /// Show killed files.
    #[arg(short = 'k', long)]
    pub killed: bool,

    /// Show object name in each line.
    #[arg(short = 's', long)]
    pub stage: bool,

    /// \0 line termination on output.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Show only unmerged files and their stage numbers.
    #[arg(long = "error-unmatch")]
    pub error_unmatch: bool,

    /// Deduplicate entries (for untracked files).
    #[arg(long)]
    pub deduplicate: bool,

    /// Suppress any error message (for -t).
    #[arg(short = 't')]
    pub show_tag: bool,

    /// Show verbose long format.
    #[arg(long)]
    pub long: bool,

    /// Exclude pattern (e.g. --exclude='*.o').
    #[arg(short = 'x', long = "exclude", value_name = "PATTERN")]
    pub exclude: Vec<String>,

    /// Exclude patterns from file.
    #[arg(short = 'X', long = "exclude-from", value_name = "FILE")]
    pub exclude_from: Vec<PathBuf>,

    /// Read exclude patterns from file in each directory.
    #[arg(long = "exclude-per-directory", value_name = "FILE")]
    pub exclude_per_directory: Option<String>,

    /// Use standard exclude sources (.gitignore, .git/info/exclude, core.excludesFile).
    #[arg(long = "exclude-standard")]
    pub exclude_standard: bool,

    /// If showing untracked files, show only directories.
    #[arg(long = "directory")]
    pub directory: bool,

    /// Show line-ending information for files.
    #[arg(long)]
    pub eol: bool,

    /// Pathspecs to restrict output.
    pub pathspecs: Vec<PathBuf>,
}

/// Run `grit ls-files`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot ls-files in bare repository"))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let cwd_prefix = cwd_prefix_bytes(work_tree, &cwd)?;
    let index_path = repo.index_path();
    let index = Index::load(&index_path).context("loading index")?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let term = if args.null_terminated { b'\0' } else { b'\n' };

    // Determine which mode to use
    let show_cached = args.cached
        || args.stage
        || (!args.deleted && !args.modified && !args.others && !args.ignored && !args.unmerged && !args.killed);
    let show_stage = args.stage || args.unmerged;

    let mut pathspec_filter: Vec<Pathspec> = args
        .pathspecs
        .iter()
        .map(|p| resolve_pathspec(work_tree, &cwd, p))
        .collect::<Result<Vec<_>>>()?;
    if pathspec_filter.is_empty() && !cwd_prefix.is_empty() {
        pathspec_filter.push(Pathspec::Literal(cwd_prefix.clone()));
    }

    // Track which pathspecs matched at least one entry (for --error-unmatch).
    let mut matched: Vec<bool> = vec![false; pathspec_filter.len()];

    for entry in &index.entries {
        // Filter by pathspec
        if !pathspec_filter.is_empty() {
            let idx = pathspec_filter
                .iter()
                .position(|spec| spec.matches(&entry.path));
            match idx {
                Some(i) => matched[i] = true,
                None => continue,
            }
        }

        // Unmerged: stage != 0
        if args.unmerged && entry.stage() == 0 {
            continue;
        }
        if show_cached && !args.unmerged && entry.stage() != 0 {
            continue;
        }

        // --deleted: only show entries whose file is missing from worktree
        if args.deleted && !show_cached {
            let full = work_tree.join(std::str::from_utf8(&entry.path).unwrap_or(""));
            if full.exists() {
                continue;
            }
        }

        // --modified: only show entries that differ from worktree
        if args.modified && !show_cached {
            let full = work_tree.join(std::str::from_utf8(&entry.path).unwrap_or(""));
            if !is_modified(entry, &full) {
                continue;
            }
        }

        let tag = if args.show_tag {
            Some(status_tag(entry))
        } else {
            None
        };

        if args.eol {
            let display = display_path_from_cwd(&entry.path, &cwd_prefix);
            let name = String::from_utf8_lossy(display);
            let path_str = std::str::from_utf8(&entry.path).unwrap_or("");

            // Determine index line endings
            let index_eol = if entry.oid != grit_lib::diff::zero_oid() {
                if let Ok(obj) = repo.odb.read(&entry.oid) {
                    describe_eol(&obj.data)
                } else {
                    "binary".to_string()
                }
            } else {
                "".to_string()
            };

            // Determine worktree line endings
            let wt_path = work_tree.join(path_str);
            let wt_eol = if let Ok(data) = std::fs::read(&wt_path) {
                describe_eol(&data)
            } else {
                "".to_string()
            };

            // Determine attribute setting
            let attr_str = {
                use grit_lib::crlf;
                let attrs = crlf::load_gitattributes(work_tree);
                let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
                let file_attrs = crlf::get_file_attrs(&attrs, path_str, &config);
                match file_attrs.text {
                    crlf::TextAttr::Set => match file_attrs.eol {
                        crlf::EolAttr::Lf => "text=auto eol=lf".to_string(),
                        crlf::EolAttr::Crlf => "text=auto eol=crlf".to_string(),
                        crlf::EolAttr::Unspecified => "text".to_string(),
                    },
                    crlf::TextAttr::Auto => match file_attrs.eol {
                        crlf::EolAttr::Lf => "text=auto eol=lf".to_string(),
                        crlf::EolAttr::Crlf => "text=auto eol=crlf".to_string(),
                        crlf::EolAttr::Unspecified => "text=auto".to_string(),
                    },
                    crlf::TextAttr::Unset => "binary".to_string(),
                    crlf::TextAttr::Unspecified => "".to_string(),
                }
            };

            write!(out, "i/{index_eol} w/{wt_eol} attr/{attr_str}\t{name}")?;
            out.write_all(&[term])?;
        } else if show_stage {
            let display = display_path_from_cwd(&entry.path, &cwd_prefix);
            let name = String::from_utf8_lossy(display);
            if let Some(t) = tag {
                write!(out, "{} ", t)?;
            }
            write!(
                out,
                "{:06o} {} {}\t{}",
                entry.mode,
                entry.oid,
                entry.stage(),
                name
            )?;
            out.write_all(&[term])?;
        } else if show_cached || args.deleted || args.modified {
            let display = display_path_from_cwd(&entry.path, &cwd_prefix);
            let name = String::from_utf8_lossy(display);
            if let Some(t) = tag {
                write!(out, "{} ", t)?;
            }
            write!(out, "{name}")?;
            out.write_all(&[term])?;
        }
    }

    // --error-unmatch: fail if any pathspec matched nothing.
    if args.error_unmatch {
        for (i, spec) in pathspec_filter.iter().enumerate() {
            if !matched[i] {
                let spec_str = match spec {
                    Pathspec::Literal(v) => String::from_utf8_lossy(v).into_owned(),
                    Pathspec::Glob(s) => s.clone(),
                };
                anyhow::bail!(
                    "error: pathspec '{}' did not match any file(s) known to git",
                    spec_str
                );
            }
        }
    }

    // Build exclude/ignore matcher if needed
    let has_excludes = args.exclude_standard
        || !args.exclude.is_empty()
        || !args.exclude_from.is_empty()
        || args.exclude_per_directory.is_some();
    // Load standard ignores for --exclude-standard, --ignored, or --others
    // (--others implicitly applies gitignore to hide ignored files)
    let use_standard_ignores = args.exclude_standard || args.ignored || args.others;
    let mut matcher = if use_standard_ignores {
        Some(IgnoreMatcher::from_repository(&repo).unwrap_or_default())
    } else {
        None
    };

    // --others: list untracked files
    // --ignored: show only ignored untracked files (implies --others)
    let show_others = args.others || args.ignored;
    if show_others {
        let indexed_paths: BTreeSet<Vec<u8>> = index
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect();
        let mut untracked = Vec::new();
        walk_worktree(work_tree, work_tree, &indexed_paths, &mut untracked)?;
        untracked.sort();

        // Collapse to directories if --directory
        let untracked = if args.directory {
            collapse_to_directories(&untracked)
        } else {
            untracked
        };

        for path_bytes in &untracked {
            if !pathspec_filter.is_empty() {
                let matches = pathspec_filter.iter().any(|spec| spec.matches(path_bytes));
                if !matches {
                    continue;
                }
            }

            // Apply exclude filtering (always when matcher is loaded)
            if has_excludes || args.ignored || matcher.is_some() {
                let path_str = String::from_utf8_lossy(path_bytes);
                let is_dir = path_str.ends_with('/');
                let std_ignored = if let Some(ref mut m) = matcher {
                    let (ignored, _) = m
                        .check_path(&repo, Some(&index), &path_str, is_dir)
                        .unwrap_or((false, None));
                    ignored
                } else {
                    false
                };
                let cli_excluded = args.exclude.iter().any(|pat| {
                    match_simple_pattern(pat, &path_str)
                });
                let is_excluded = std_ignored || cli_excluded;

                if args.ignored && !is_excluded {
                    continue; // --ignored: only show excluded files
                }
                if !args.ignored && is_excluded {
                    continue; // --others with excludes: hide excluded files
                }
            }

            let display = display_path_from_cwd(path_bytes, &cwd_prefix);
            let name = String::from_utf8_lossy(display);
            if args.show_tag {
                write!(out, "? {name}")?;
            } else {
                write!(out, "{name}")?;
            }
            out.write_all(&[term])?;
        }
    }

    Ok(())
}

/// Walk the worktree and collect paths of untracked files.
fn walk_worktree(
    root: &std::path::Path,
    dir: &std::path::Path,
    indexed: &BTreeSet<Vec<u8>>,
    out: &mut Vec<Vec<u8>>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_bytes = path_to_bytes(rel);
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip .git directory
        if name_str == ".git" {
            continue;
        }

        let ft = entry.file_type()?;
        if ft.is_file() || ft.is_symlink() {
            if !indexed.contains(&rel_bytes) {
                out.push(rel_bytes);
            }
        } else if ft.is_dir() {
            let dot_git = path.join(".git");
            if dot_git.exists() {
                continue;
            }
            walk_worktree(root, &path, indexed, out)?;
        }
    }
    Ok(())
}

/// A parsed pathspec — either a literal prefix or a glob pattern.
#[derive(Debug, Clone)]
enum Pathspec {
    Literal(Vec<u8>),
    Glob(String),
}

impl Pathspec {
    fn matches(&self, path: &[u8]) -> bool {
        match self {
            Pathspec::Literal(spec) => path == spec.as_slice() || path.starts_with(spec),
            Pathspec::Glob(pattern) => {
                // Try literal match first (for files with glob chars in names)
                if path == pattern.as_bytes() {
                    return true;
                }
                let path_str = String::from_utf8_lossy(path);
                glob_match(pattern, &path_str)
            }
        }
    }
}

/// Check if a string contains glob meta-characters.
fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Simple glob matching for git pathspecs.
/// `*` matches any sequence of characters including `/`.
/// `?` matches any single character except `/`.
/// `[abc]` matches any one character in the set.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'?' && text[ti] != b'/' {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            // Character class
            if let Some((matched, end)) = match_char_class(&pattern[pi..], text[ti]) {
                if matched {
                    pi += end;
                    ti += 1;
                } else if star_pi != usize::MAX {
                    star_ti += 1;
                    ti = star_ti;
                    pi = star_pi + 1;
                } else {
                    return false;
                }
            } else if star_pi != usize::MAX {
                star_ti += 1;
                ti = star_ti;
                pi = star_pi + 1;
            } else {
                return false;
            }
        } else if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            star_ti += 1;
            ti = star_ti;
            pi = star_pi + 1;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Match a character class like [abc] or [a-z]. Returns (matched, bytes_consumed) or None if invalid.
fn match_char_class(pattern: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pattern.is_empty() || pattern[0] != b'[' {
        return None;
    }
    let mut i = 1;
    let negate = i < pattern.len() && (pattern[i] == b'!' || pattern[i] == b'^');
    if negate {
        i += 1;
    }
    let mut matched = false;
    while i < pattern.len() && pattern[i] != b']' {
        if i + 2 < pattern.len() && pattern[i + 1] == b'-' {
            if ch >= pattern[i] && ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if ch == pattern[i] {
                matched = true;
            }
            i += 1;
        }
    }
    if i < pattern.len() && pattern[i] == b']' {
        if negate {
            matched = !matched;
        }
        Some((matched, i + 1))
    } else {
        None // unclosed bracket
    }
}

fn resolve_pathspec(
    work_tree: &std::path::Path,
    cwd: &std::path::Path,
    pathspec: &std::path::Path,
) -> Result<Pathspec> {
    if pathspec.as_os_str().is_empty() || pathspec == std::path::Path::new(".") {
        return Ok(Pathspec::Literal(cwd_prefix_bytes(work_tree, cwd)?));
    }
    let pathspec_str = pathspec.to_string_lossy();
    if has_glob_chars(&pathspec_str) {
        // For glob pathspecs, prepend the cwd prefix (relative to work_tree)
        let prefix = cwd_prefix_bytes(work_tree, cwd)?;
        let prefix_str = String::from_utf8_lossy(&prefix).into_owned();
        let pattern = format!("{}{}", prefix_str, pathspec_str);
        return Ok(Pathspec::Glob(pattern));
    }
    let combined = if pathspec.is_absolute() {
        pathspec.to_path_buf()
    } else {
        cwd.join(pathspec)
    };
    let normalized = normalize_path(&combined);
    let rel = normalized.strip_prefix(work_tree).with_context(|| {
        format!(
            "pathspec '{}' is outside repository work tree",
            pathspec.display()
        )
    })?;
    Ok(Pathspec::Literal(path_to_bytes(rel)))
}

fn cwd_prefix_bytes(work_tree: &std::path::Path, cwd: &std::path::Path) -> Result<Vec<u8>> {
    let rel = cwd.strip_prefix(work_tree).with_context(|| {
        format!(
            "current directory '{}' is outside repository work tree '{}'",
            cwd.display(),
            work_tree.display()
        )
    })?;
    if rel.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let mut bytes = path_to_bytes(rel);
    bytes.push(b'/');
    Ok(bytes)
}

fn display_path_from_cwd<'a>(path: &'a [u8], cwd_prefix: &[u8]) -> &'a [u8] {
    if cwd_prefix.is_empty() {
        return path;
    }
    path.strip_prefix(cwd_prefix).unwrap_or(path)
}

fn normalize_path(path: &std::path::Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_to_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

/// Collapse file paths into unique top-level directory entries.
/// E.g., ["dir/a", "dir/b", "file"] → ["dir/", "file"]
fn collapse_to_directories(paths: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut dirs = BTreeSet::new();
    let mut result = Vec::new();
    for p in paths {
        if let Some(pos) = p.iter().position(|&b| b == b'/') {
            let dir = p[..=pos].to_vec();
            if dirs.insert(dir.clone()) {
                result.push(dir);
            }
        } else {
            result.push(p.clone());
        }
    }
    result
}

/// Simple glob pattern matching for --exclude patterns.
/// Supports *, ?, and literal matching.
fn match_simple_pattern(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    // If pattern has no slash, match against basename only
    let name = if !pattern.contains('/') {
        path.rsplit('/').next().unwrap_or(path)
    } else {
        path
    };
    simple_glob(pattern.as_bytes(), name.as_bytes())
}

/// Basic glob matching for exclude patterns (*, ?).
fn simple_glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut pi, mut ti) = (0, 0);
    let (mut star_p, mut star_t) = (usize::MAX, 0);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Check whether an index entry's file has been modified on disk.
fn is_modified(entry: &IndexEntry, path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return true, // file missing = modified (or deleted)
    };

    // Quick stat comparison (same heuristic as git: size and mtime)
    if entry.size != 0 && meta.len() as u32 != entry.size {
        return true;
    }

    // Compare mtime seconds (and nanoseconds if available)
    let mtime_sec = meta.mtime() as u32;
    let mtime_nsec = meta.mtime_nsec() as u32;
    if mtime_sec != entry.mtime_sec {
        return true;
    }
    if entry.mtime_nsec != 0 && mtime_nsec != entry.mtime_nsec {
        return true;
    }

    false
}

/// Return the status tag character for an index entry (used by `-t`).
/// Describe the line ending style of file data.
fn describe_eol(data: &[u8]) -> String {
    use grit_lib::crlf;
    if crlf::is_binary(data) {
        return "binary".to_string();
    }
    let has_crlf = crlf::has_crlf(data);
    let has_lf = crlf::has_lone_lf(data);
    match (has_crlf, has_lf) {
        (true, true) => "mixed".to_string(),
        (true, false) => "crlf".to_string(),
        (false, true) => "lf".to_string(),
        (false, false) => "".to_string(),
    }
}

fn status_tag(entry: &IndexEntry) -> char {
    if entry.stage() != 0 {
        'M' // unmerged
    } else if entry.skip_worktree() {
        'S'
    } else if entry.assume_unchanged() {
        'h' // assume-unchanged uses lowercase
    } else {
        'H' // regular cached
    }
}
