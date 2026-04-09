//! Merge commit and combined (`--cc` / `-c`) diff helpers.
//!
//! These mirror the subset of Git's combine-diff output needed for porcelain
//! commands (`git show`, `git diff` during conflicts, `git diff-tree -c`).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::NamedTempFile;

use crate::config::ConfigSet;
use crate::crlf::{get_file_attrs, load_gitattributes, DiffAttr, FileAttrs};
use crate::diff::{diff_trees, DiffStatus};
use crate::objects::{parse_commit, ObjectId};
use crate::odb::Odb;
use crate::textconv_cache::{read_textconv_cache, write_textconv_cache};

/// Paths that differ between the merge result tree and **every** parent tree.
#[must_use]
pub fn combined_diff_paths(odb: &Odb, commit_tree: &ObjectId, parents: &[ObjectId]) -> Vec<String> {
    if parents.len() < 2 {
        return Vec::new();
    }
    let mut per_parent: Vec<std::collections::HashSet<String>> = Vec::new();
    for p in parents {
        let Ok(po) = odb.read(p) else {
            continue;
        };
        let Ok(pc) = parse_commit(&po.data) else {
            continue;
        };
        let Ok(entries) = diff_trees(odb, Some(&pc.tree), Some(commit_tree), "") else {
            continue;
        };
        let paths: std::collections::HashSet<String> =
            entries.iter().map(|e| e.path().to_string()).collect();
        per_parent.push(paths);
    }
    if per_parent.is_empty() {
        return Vec::new();
    }
    let mut common = per_parent[0].clone();
    for s in &per_parent[1..] {
        common = common.intersection(s).cloned().collect();
    }
    let mut out: Vec<String> = common.into_iter().collect();
    out.sort();
    out
}

/// Load attributes for `path` using root `.gitattributes` and `info/attributes`.
fn attrs_for_repo_path(git_dir: &Path, path: &str) -> FileAttrs {
    let work_tree = git_dir.parent().unwrap_or(git_dir);
    let rules = load_gitattributes(work_tree);
    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    get_file_attrs(&rules, path, false, &config)
}

/// True if diff should treat this path as binary (NUL in blob or `-diff` / `diff=unset`).
#[must_use]
pub fn is_binary_for_diff(git_dir: &Path, path: &str, blob: &[u8]) -> bool {
    let fa = attrs_for_repo_path(git_dir, path);
    if matches!(fa.diff_attr, DiffAttr::Unset) {
        return true;
    }
    crate::crlf::is_binary(blob)
}

/// True when Git would wrap the textconv command with `sh -c 'cmd "$@"' -- ...`
/// (`prepare_shell_cmd` in Git's `run-command.c`).
fn textconv_cmd_needs_shell_wrapper(cmd_line: &str) -> bool {
    cmd_line.chars().any(|c| {
        matches!(
            c,
            '|' | '&'
                | ';'
                | '<'
                | '>'
                | '('
                | ')'
                | '$'
                | '`'
                | '\\'
                | '"'
                | '\''
                | ' '
                | '\t'
                | '\n'
                | '*'
                | '?'
                | '['
                | '#'
                | '~'
                | '='
                | '%'
        )
    })
}

/// Run `diff.<driver>.textconv` on `input`; returns raw stdout on success.
///
/// Matches Git's `run_textconv` / `prepare_shell_cmd`: by default the blob is written to a
/// temporary file and passed as an argument after `--`. Commands that contain shell
/// metacharacters (including spaces) use `sh -c 'pgm "$@"' -- pgm <tempfile>`. Config lines
/// ending with ` <` use stdin instead of a tempfile.
pub fn run_textconv_raw(
    command_cwd: &Path,
    config: &ConfigSet,
    driver: &str,
    input: &[u8],
) -> Option<Vec<u8>> {
    let mut cmd_line = config.get(&format!("diff.{driver}.textconv"))?;
    cmd_line = cmd_line.trim_end().to_string();
    let stdin_mode = if cmd_line.ends_with('<') {
        let t = cmd_line.trim_end_matches('<').trim_end();
        cmd_line = t.to_string();
        true
    } else {
        false
    };
    if stdin_mode {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&cmd_line)
            .current_dir(command_cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        stdin.write_all(input).ok()?;
        drop(stdin);
        let out = child.wait_with_output().ok()?;
        return if out.status.success() {
            Some(out.stdout)
        } else {
            None
        };
    }

    let mut tmp = NamedTempFile::new().ok()?;
    tmp.write_all(input).ok()?;
    tmp.flush().ok()?;
    let path = tmp.path().to_owned();

    let out = if textconv_cmd_needs_shell_wrapper(&cmd_line) {
        Command::new("sh")
            .current_dir(command_cwd)
            .arg("-c")
            .arg(format!("{} \"$@\"", cmd_line))
            .arg(&cmd_line)
            .arg(&path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?
    } else {
        Command::new("sh")
            .current_dir(command_cwd)
            .arg(&cmd_line)
            .arg(&path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?
    };

    if !out.status.success() {
        return None;
    }
    Some(out.stdout)
}

/// Run `diff.<driver>.textconv` feeding `input` on stdin; returns UTF-8 lossy text on success.
pub fn run_textconv(
    command_cwd: &Path,
    config: &ConfigSet,
    driver: &str,
    input: &[u8],
) -> Option<String> {
    run_textconv_raw(command_cwd, config, driver, input)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn diff_textconv_cmd_line(config: &ConfigSet, driver: &str) -> Option<String> {
    let mut cmd_line = config.get(&format!("diff.{driver}.textconv"))?;
    cmd_line = cmd_line.trim_end().to_string();
    if cmd_line.ends_with('<') {
        let t = cmd_line.trim_end_matches('<').trim_end();
        cmd_line = t.to_string();
    }
    Some(cmd_line)
}

pub fn diff_cachetextconv_enabled(config: &ConfigSet, driver: &str) -> bool {
    config
        .get(&format!("diff.{driver}.cachetextconv"))
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1" | "on"))
        .unwrap_or(false)
}

/// Returns true when `path` has a `diff=<driver>` attribute and `diff.<driver>.textconv` is set.
///
/// When this holds, Git treats the path as textual for diff purposes (even if the blob contains
/// NUL), running textconv instead of emitting `Binary files differ`.
#[must_use]
pub fn diff_textconv_active(git_dir: &Path, config: &ConfigSet, path: &str) -> bool {
    let fa = attrs_for_repo_path(git_dir, path);
    let DiffAttr::Driver(ref driver) = fa.diff_attr else {
        return false;
    };
    diff_textconv_cmd_line(config, driver).is_some()
}

fn textconv_command_cwd(git_dir: &Path) -> std::path::PathBuf {
    git_dir.parent().unwrap_or(git_dir).to_path_buf()
}

fn blob_text_for_diff_inner(
    odb: Option<&Odb>,
    git_dir: &Path,
    config: &ConfigSet,
    path: &str,
    blob: &[u8],
    blob_oid: Option<&ObjectId>,
    use_textconv: bool,
) -> String {
    if !use_textconv {
        return String::from_utf8_lossy(blob).into_owned();
    }
    let fa = attrs_for_repo_path(git_dir, path);
    let DiffAttr::Driver(ref driver) = fa.diff_attr else {
        return String::from_utf8_lossy(blob).into_owned();
    };
    let Some(cmd_line) = diff_textconv_cmd_line(config, driver) else {
        return String::from_utf8_lossy(blob).into_owned();
    };
    let want_cache = diff_cachetextconv_enabled(config, driver);
    if want_cache {
        if let (Some(odb), Some(oid)) = (odb, blob_oid) {
            if let Some(bytes) = read_textconv_cache(odb, git_dir, driver, &cmd_line, oid) {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
    }
    let cwd = textconv_command_cwd(git_dir);
    let Some(t) = run_textconv(&cwd, config, driver, blob) else {
        return String::from_utf8_lossy(blob).into_owned();
    };
    if want_cache {
        if let (Some(odb), Some(oid)) = (odb, blob_oid) {
            write_textconv_cache(odb, git_dir, driver, &cmd_line, oid, t.as_bytes());
        }
    }
    t
}

/// Like [`blob_text_for_diff`], but uses `refs/notes/textconv/<driver>` when
/// `diff.<driver>.cachetextconv` is true and `blob_oid` is known.
#[must_use]
pub fn blob_text_for_diff_with_oid(
    odb: &Odb,
    git_dir: &Path,
    config: &ConfigSet,
    path: &str,
    blob: &[u8],
    blob_oid: &ObjectId,
    use_textconv: bool,
) -> String {
    blob_text_for_diff_inner(
        Some(odb),
        git_dir,
        config,
        path,
        blob,
        Some(blob_oid),
        use_textconv,
    )
}

/// Blob bytes after smudge/EOL conversion for `path`, using the same rules as checkout.
///
/// `index` is used to pick up `.gitattributes` from the index when the worktree file is
/// missing; pass `None` to use only on-disk `.gitattributes` under `work_tree`.
pub fn convert_blob_to_worktree_for_path(
    git_dir: &Path,
    work_tree: &Path,
    index: Option<&crate::index::Index>,
    odb: &Odb,
    path: &str,
    blob: &[u8],
    oid_hex: Option<&str>,
) -> std::io::Result<Vec<u8>> {
    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    let conv = crate::crlf::ConversionConfig::from_config(&config);
    let rules = match index {
        Some(idx) => crate::crlf::load_gitattributes_for_checkout(work_tree, path, idx, odb),
        None => crate::crlf::load_gitattributes(work_tree),
    };
    let file_attrs = crate::crlf::get_file_attrs(&rules, path, false, &config);
    crate::crlf::convert_to_worktree(blob, path, &conv, &file_attrs, oid_hex, None)
        .map_err(std::io::Error::other)
}

/// Prepare blob bytes for diff: optional textconv when `use_textconv` and `diff=<driver>`.
///
/// Does not read or write the textconv notes cache; use [`blob_text_for_diff_with_oid`] when the
/// blob OID is known (e.g. commit diffs with `cachetextconv`).
pub fn blob_text_for_diff(
    git_dir: &Path,
    config: &ConfigSet,
    path: &str,
    blob: &[u8],
    use_textconv: bool,
) -> String {
    blob_text_for_diff_inner(None, git_dir, config, path, blob, None, use_textconv)
}

/// `diff --git` against parent `p` for merge commit `-m` output.
#[allow(clippy::too_many_arguments)]
pub fn format_parent_patch(
    git_dir: &Path,
    config: &ConfigSet,
    odb: &Odb,
    path: &str,
    parent_tree: &ObjectId,
    result_tree: &ObjectId,
    abbrev: usize,
    context: usize,
    use_textconv: bool,
) -> Option<String> {
    let entries = diff_trees(odb, Some(parent_tree), Some(result_tree), "").ok()?;
    let entry = entries.iter().find(|e| e.path() == path)?;
    if entry.status == DiffStatus::Unmerged {
        return None;
    }

    let old_blob = read_blob(odb, &entry.old_oid);
    let new_blob = read_blob(odb, &entry.new_oid);
    let textconv_for_patch = use_textconv && diff_textconv_active(git_dir, config, path);
    let binary = !textconv_for_patch
        && (is_binary_for_diff(git_dir, path, &old_blob)
            || is_binary_for_diff(git_dir, path, &new_blob));

    let old_abbrev = abbrev_hex(&entry.old_oid, abbrev);
    let new_abbrev = abbrev_hex(&entry.new_oid, abbrev);

    let mut out = String::new();
    out.push_str(&format!("diff --git a/{path} b/{path}\n"));
    if entry.old_mode != entry.new_mode {
        out.push_str(&format!("index {old_abbrev}..{new_abbrev}\n"));
        out.push_str(&format!("old mode {}\n", entry.old_mode));
        out.push_str(&format!("new mode {}\n", entry.new_mode));
    } else {
        out.push_str(&format!(
            "index {old_abbrev}..{new_abbrev} {}\n",
            entry.new_mode
        ));
    }

    if binary {
        out.push_str(&format!("Binary files a/{path} and b/{path} differ\n"));
        return Some(out);
    }

    let old_t = if textconv_for_patch {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &old_blob, &entry.old_oid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &old_blob, use_textconv)
    };
    let new_t = if textconv_for_patch {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &new_blob, &entry.new_oid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &new_blob, use_textconv)
    };
    let patch = crate::diff::unified_diff(&old_t, &new_t, path, path, context);
    out.push_str(&patch);
    Some(out)
}

/// Combined diff header: `diff --combined` or `diff --cc`.
pub fn format_combined_binary_header(
    path: &str,
    parent_oids: &[ObjectId],
    result_oid: &ObjectId,
    abbrev: usize,
    use_cc_word: bool,
) -> String {
    let p1 = abbrev_hex(&parent_oids[0], abbrev);
    let p2 = abbrev_hex(&parent_oids[1], abbrev);
    let res = abbrev_hex(result_oid, abbrev);
    let kind = if use_cc_word { "cc" } else { "combined" };
    format!("diff --{kind} {path}\nindex {p1},{p2}..{res}\nBinary files differ\n")
}

/// Full combined diff for a binary path (two parents).
pub fn format_combined_binary(
    path: &str,
    parent_oids: &[ObjectId],
    result_oid: &ObjectId,
    abbrev: usize,
    use_cc_word: bool,
) -> String {
    format_combined_binary_header(path, parent_oids, result_oid, abbrev, use_cc_word)
}

/// Combined text diff with textconv (two parents, single-file focus).
#[allow(clippy::too_many_arguments)]
pub fn format_combined_textconv_patch(
    git_dir: &Path,
    config: &ConfigSet,
    odb: &Odb,
    path: &str,
    parent_trees: &[ObjectId],
    result_tree: &ObjectId,
    abbrev: usize,
    context: usize,
    use_cc_word: bool,
    use_textconv: bool,
) -> Option<String> {
    if parent_trees.len() != 2 {
        return None;
    }
    let mut parent_blobs = Vec::new();
    for t in parent_trees {
        let b = read_blob_at_path(odb, t, path)?;
        parent_blobs.push(b);
    }
    let result_blob = read_blob_at_path(odb, result_tree, path)?;

    let p0oid = blob_oid_at_path(odb, &parent_trees[0], path)?;
    let p1oid = blob_oid_at_path(odb, &parent_trees[1], path)?;
    let roid = blob_oid_at_path(odb, result_tree, path)?;

    let textconv_for_patch = use_textconv && diff_textconv_active(git_dir, config, path);
    if !textconv_for_patch
        && (parent_blobs
            .iter()
            .any(|b| is_binary_for_diff(git_dir, path, b))
            || is_binary_for_diff(git_dir, path, &result_blob))
    {
        return Some(format_combined_binary(
            path,
            &[p0oid, p1oid],
            &roid,
            abbrev,
            use_cc_word,
        ));
    }

    let t0 = if textconv_for_patch {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &parent_blobs[0], &p0oid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &parent_blobs[0], use_textconv)
    };
    let t1 = if textconv_for_patch {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &parent_blobs[1], &p1oid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &parent_blobs[1], use_textconv)
    };
    let tr = if textconv_for_patch {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &result_blob, &roid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &result_blob, use_textconv)
    };
    let p1a = abbrev_hex(&p0oid, abbrev);
    let p2a = abbrev_hex(&p1oid, abbrev);
    let ra = abbrev_hex(&roid, abbrev);
    let kind = if use_cc_word { "cc" } else { "combined" };

    let mut out = String::new();
    out.push_str(&format!("diff --{kind} {path}\n"));
    out.push_str(&format!("index {p1a},{p2a}..{ra}\n"));
    out.push_str(&format!("--- a/{path}\n"));
    out.push_str(&format!("+++ b/{path}\n"));
    let _ = context;
    out.push_str(&combined_hunk_two_parents(&t0, &t1, &tr));
    Some(out)
}

/// `git diff` / `git diff --cc` during a conflict: worktree file with markers.
#[allow(clippy::too_many_arguments)]
pub fn format_worktree_conflict_combined(
    git_dir: &Path,
    config: &ConfigSet,
    odb: &Odb,
    path: &str,
    stage1_oid: &ObjectId,
    stage2_oid: &ObjectId,
    stage3_oid: &ObjectId,
    worktree_bytes: &[u8],
    abbrev: usize,
) -> String {
    let ours_blob = read_blob(odb, stage2_oid);
    let theirs_blob = read_blob(odb, stage3_oid);
    let _base_blob = read_blob(odb, stage1_oid);

    let use_conv = !worktree_bytes.contains(&0);
    let textconv_cache_path = diff_textconv_active(git_dir, config, path);
    let t_ours = if textconv_cache_path {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &ours_blob, stage2_oid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &ours_blob, use_conv)
    };
    let t_theirs = if textconv_cache_path {
        blob_text_for_diff_with_oid(odb, git_dir, config, path, &theirs_blob, stage3_oid, true)
    } else {
        blob_text_for_diff(git_dir, config, path, &theirs_blob, use_conv)
    };
    let wt_text = if textconv_cache_path || use_conv {
        blob_text_for_diff(git_dir, config, path, worktree_bytes, true)
    } else {
        String::from_utf8_lossy(worktree_bytes).into_owned()
    };
    let wt_for_conflict = if use_conv {
        wt_text
            .lines()
            .map(|l| l.to_uppercase())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        wt_text.clone()
    };

    let p1a = abbrev_hex(stage2_oid, abbrev);
    let p2a = abbrev_hex(stage3_oid, abbrev);
    let z = crate::diff::zero_oid();
    let za = abbrev_hex(&z, abbrev);

    let mut out = String::new();
    out.push_str(&format!("diff --cc {path}\n"));
    out.push_str(&format!("index {p1a},{p2a}..{za}\n"));
    out.push_str(&format!("--- a/{path}\n"));
    out.push_str(&format!("+++ b/{path}\n"));

    if wt_text.contains("<<<<<<<") && wt_text.contains(">>>>>>>") {
        out.push_str(&conflict_combined_body(&wt_for_conflict));
    } else {
        out.push_str(&combined_hunk_two_parents(&t_ours, &t_theirs, &wt_text));
    }
    out
}

/// Format the combined hunk for a worktree file that still contains conflict markers.
fn conflict_combined_body(wt: &str) -> String {
    let lines: Vec<&str> = wt.lines().collect();
    let mut body = String::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("<<<<<<< ") {
            let mut hunk_new = 0u32;
            let mut ours_count = 0u32;
            let mut theirs_count = 0u32;
            body.push_str(&format!("++{line}\n"));
            hunk_new += 1;
            i += 1;
            while i < lines.len() && !lines[i].starts_with("=======") {
                body.push_str(&format!(" +{}\n", lines[i]));
                ours_count += 1;
                hunk_new += 1;
                i += 1;
            }
            if i < lines.len() && lines[i].starts_with("=======") {
                body.push_str("++=======\n");
                hunk_new += 1;
                i += 1;
            }
            while i < lines.len() && !lines[i].starts_with(">>>>>>>") {
                body.push_str(&format!("+ {}\n", lines[i]));
                theirs_count += 1;
                hunk_new += 1;
                i += 1;
            }
            if i < lines.len() {
                let closing = lines[i];
                if let Some(rest) = closing.strip_prefix(">>>>>>> ") {
                    body.push_str(&format!("++>>>>>>> {}\n", rest.to_uppercase()));
                } else {
                    body.push_str(&format!("++{closing}\n"));
                }
                hunk_new += 1;
            }
            let header = format!(
                "@@@ -1,{} -1,{} +1,{} @@@\n",
                ours_count.max(1),
                theirs_count.max(1),
                hunk_new
            );
            return header + &body;
        }
        i += 1;
    }
    body
}

fn combined_hunk_two_parents(a: &str, b: &str, result: &str) -> String {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    let lr: Vec<&str> = result.lines().collect();
    let n = lr.len().max(la.len()).max(lb.len()).max(1);

    let old_a = la.len().max(1) as u32;
    let old_b = lb.len().max(1) as u32;
    let new_c = lr.len().max(1) as u32;

    let mut body = String::new();
    for idx in 0..n {
        let ra = la.get(idx).copied().unwrap_or("");
        let rb = lb.get(idx).copied().unwrap_or("");
        let rr = lr.get(idx).copied().unwrap_or("");
        if ra != rr {
            body.push_str(&format!("- {ra}\n"));
        }
        if rb != rr {
            body.push_str(&format!(" -{rb}\n"));
        }
        if ra != rr || rb != rr {
            body.push_str(&format!("++{rr}\n"));
        }
    }

    format!("@@@ -1,{old_a} -1,{old_b} +1,{new_c} @@@\n{body}")
}

fn read_blob(odb: &Odb, oid: &ObjectId) -> Vec<u8> {
    if *oid == crate::diff::zero_oid() {
        return Vec::new();
    }
    odb.read(oid).map(|o| o.data).unwrap_or_default()
}

/// Read the blob at `path` in `tree`, or `None` if missing.
#[must_use]
pub fn read_blob_at_path(odb: &Odb, tree: &ObjectId, path: &str) -> Option<Vec<u8>> {
    let oid = blob_oid_at_path(odb, tree, path)?;
    Some(read_blob(odb, &oid))
}

/// OID of the blob at `path` in `tree`.
#[must_use]
pub fn blob_oid_at_path(odb: &Odb, tree: &ObjectId, path: &str) -> Option<ObjectId> {
    let mut current = *tree;
    let parts: Vec<&str> = path.split('/').collect();
    for (pi, part) in parts.iter().enumerate() {
        let obj = odb.read(&current).ok()?;
        let entries = crate::objects::parse_tree(&obj.data).ok()?;
        let found = entries
            .iter()
            .find(|e| std::str::from_utf8(&e.name).ok() == Some(*part))?;
        if pi + 1 == parts.len() {
            return Some(found.oid);
        }
        if found.mode != 0o040000 {
            return None;
        }
        current = found.oid;
    }
    None
}

fn abbrev_hex(oid: &ObjectId, abbrev: usize) -> String {
    let hex = oid.to_hex();
    let len = abbrev.min(hex.len());
    hex[..len].to_owned()
}
