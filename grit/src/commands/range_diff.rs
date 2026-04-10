//! `grit range-diff` — compare two commit ranges (Git-compatible).
//!
//! Mirrors Git's `range-diff.c`: `grit log -p` output is normalized into patches,
//! matched with a Hungarian assignment, then printed in RHS order with optional
//! inner diffs.

use anyhow::{bail, Context, Result};
use clap::Parser;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    abbreviate_object_id, resolve_revision, split_double_dot_range, split_triple_dot_range,
};
use hungarian::minimize;
use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const COST_MAX: u64 = 1 << 16;
const DEFAULT_CREATION_FACTOR: i32 = 60;

/// Arguments for `grit range-diff`.
#[derive(Debug, Parser)]
#[command(
    name = "grit range-diff",
    about = "Compare two commit ranges",
    disable_help_subcommand = true
)]
pub struct Args {
    #[arg(long = "creation-factor", value_name = "N")]
    pub creation_factor: Option<i32>,

    #[arg(long = "no-dual-color")]
    pub no_dual_color: bool,

    #[arg(long = "left-only")]
    pub left_only: bool,

    #[arg(long = "right-only")]
    pub right_only: bool,

    #[arg(short = 's', long = "no-patch")]
    pub no_patch: bool,

    #[arg(long = "stat")]
    pub stat: bool,

    #[arg(long = "color", default_missing_value = "always", num_args = 0..=1, require_equals = true)]
    pub color: Option<String>,

    #[arg(long = "no-color")]
    pub no_color: bool,

    #[arg(long = "abbrev", value_name = "N", default_missing_value = "7", num_args = 0..=1, require_equals = true)]
    pub abbrev: Option<String>,

    #[arg(long = "no-notes")]
    pub no_notes: bool,

    #[arg(
        long = "notes",
        value_name = "REF",
        action = clap::ArgAction::Append,
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub notes: Vec<String>,

    #[arg(long = "diff-merges", value_name = "STYLE", default_missing_value = "on", num_args = 0..=1, require_equals = true)]
    pub diff_merges: Option<String>,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub rest: Vec<String>,
}

struct Patch {
    oid: ObjectId,
    full: String,
    diff_offset: usize,
    diffsize: i32,
    matching: i32,
    shown: bool,
}

const RANGE_DIFF_HELP_TEXT: &str = include_str!("../../upstream_range_diff_help.txt");

/// Parse argv after `range-diff` and run (used from `main` so `--` / pathspecs work).
pub fn run_with_rest(rest: &[String]) -> Result<()> {
    if rest.len() == 1 {
        let a = rest[0].as_str();
        if matches!(a, "-h" | "--help" | "--help-all") {
            print!("{RANGE_DIFF_HELP_TEXT}");
            if a == "--help" {
                return Ok(());
            }
            std::process::exit(129);
        }
    }
    let mut argv = vec!["grit range-diff".to_string()];
    argv.extend(rest.iter().cloned());
    let args = Args::try_parse_from(&argv).map_err(|e| anyhow::anyhow!("{e}"))?;
    run(args)
}

fn run(args: Args) -> Result<()> {
    if args.left_only && args.right_only {
        bail!("options '--left-only' and '--right-only' cannot be used together");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let (rev1, rev2, log_extra) = parse_invocation(&repo, &args.rest)?;

    let creation = args
        .creation_factor
        .unwrap_or(DEFAULT_CREATION_FACTOR)
        .max(0) as u64;

    let mut branch1 = read_patches_from_log(&repo, &rev1, &args, &log_extra)?;
    let mut branch2 = read_patches_from_log(&repo, &rev2, &args, &log_extra)?;

    find_exact_matches(&mut branch1, &mut branch2);
    get_correspondences(&mut branch1, &mut branch2, creation);

    let use_color = !args.no_color
        && args
            .color
            .as_deref()
            .map(|c| c == "always" || c.is_empty())
            .unwrap_or_else(|| {
                std::io::stdout().is_terminal() || std::env::var_os("GIT_PAGER_IN_USE").is_some()
            })
        && !args.no_dual_color;

    let abbrev_len = args
        .abbrev
        .as_deref()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(7)
        .clamp(4, 40);

    output(
        &repo,
        &mut branch1,
        &mut branch2,
        args.left_only,
        args.right_only,
        args.no_patch,
        args.stat,
        use_color,
        abbrev_len,
    )?;

    Ok(())
}

fn parse_invocation(
    repo: &Repository,
    rest: &[String],
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let (rpart, pspecs) = if let Some(i) = rest.iter().position(|s| s == "--") {
        (&rest[..i], &rest[i + 1..])
    } else {
        (rest, &[][..])
    };

    let mut log_extra: Vec<String> = Vec::new();
    if !pspecs.is_empty() {
        log_extra.push("--".to_string());
        log_extra.extend(pspecs.iter().cloned());
    }

    let argc = rpart.len();

    if argc >= 3 && all_committish(repo, &rpart[..3]) {
        for s in &rpart[..3] {
            resolve_revision(repo, s).map_err(|_| anyhow::anyhow!("not a revision: '{s}'"))?;
        }
        let s1 = log_range_vec(&format!("{}..{}", rpart[0], rpart[1]));
        let s2 = log_range_vec(&format!("{}..{}", rpart[0], rpart[2]));
        return Ok((s1, s2, log_extra));
    }

    if argc >= 2 && is_range(repo, &rpart[0]) && is_range(repo, &rpart[1]) {
        return Ok((
            log_range_vec(&rpart[0]),
            log_range_vec(&rpart[1]),
            log_extra,
        ));
    }

    if argc == 2 {
        let a = expand_parent_only_syntax(repo, &rpart[0])?;
        let b = expand_parent_only_syntax(repo, &rpart[1])?;
        if let (Some(va), Some(vb)) = (a, b) {
            return Ok((va, vb, log_extra));
        }
        if rpart[0].contains("..") || rpart[1].contains("..") {
            bail!("not a commit range");
        }
    }

    if argc >= 1 {
        let arg0 = rpart[0].as_str();
        if let Some((a, b)) = split_triple_dot_range(arg0) {
            if !a.is_empty() || !b.is_empty() {
                let a_oid = resolve_revision(repo, if a.is_empty() { "HEAD" } else { a })?;
                let b_oid = resolve_revision(repo, if b.is_empty() { "HEAD" } else { b })?;
                // Match `git range-diff` / `builtin/range-diff.c`: `B..A` then `A..B`.
                let range1 = format!("{}..{}", b_oid.to_hex(), a_oid.to_hex());
                let range2 = format!("{}..{}", a_oid.to_hex(), b_oid.to_hex());
                return Ok((log_range_vec(&range1), log_range_vec(&range2), log_extra));
            }
        }
    }

    bail!("need two commit ranges");
}

/// `topic^!` → `^p1 ^p2 … tip`; `topic^-N` → `^pN tip` (Git parent-only revision syntax).
fn expand_parent_only_syntax(repo: &Repository, spec: &str) -> Result<Option<Vec<String>>> {
    if let Some(base) = spec.strip_suffix("^!") {
        if base.is_empty() {
            return Ok(None);
        }
        let oid = resolve_revision(repo, base)?;
        let obj = repo.odb.read(&oid)?;
        let commit = parse_commit(&obj.data)?;
        let mut out: Vec<String> = commit
            .parents
            .iter()
            .map(|p| format!("^{}", p.to_hex()))
            .collect();
        out.push(oid.to_hex());
        return Ok(Some(out));
    }
    if let Some(pos) = spec.rfind("^-") {
        let base = &spec[..pos];
        let rest = &spec[pos + 2..];
        if base.is_empty() {
            return Ok(None);
        }
        let n: usize = if rest.is_empty() {
            1
        } else {
            rest.parse()
                .map_err(|_| anyhow::anyhow!("invalid parent spec"))?
        };
        if n < 1 {
            return Ok(None);
        }
        let oid = resolve_revision(repo, base)?;
        let obj = repo.odb.read(&oid)?;
        let commit = parse_commit(&obj.data)?;
        let parent = commit
            .parents
            .get(n - 1)
            .ok_or_else(|| anyhow::anyhow!("revision '{spec}' has no parent {n}"))?;
        return Ok(Some(vec![format!("^{}", parent.to_hex()), oid.to_hex()]));
    }
    Ok(None)
}

fn log_range_vec(range: &str) -> Vec<String> {
    log_range_args(range)
}

fn all_committish(repo: &Repository, specs: &[String]) -> bool {
    specs.iter().all(|s| resolve_revision(repo, s).is_ok())
}

fn is_range(repo: &Repository, spec: &str) -> bool {
    if split_triple_dot_range(spec).is_some() {
        return true;
    }
    let Some((left, right)) = split_double_dot_range(spec) else {
        return false;
    };
    if left.is_empty() || right.is_empty() {
        return false;
    }
    resolve_revision(repo, left).is_ok() && resolve_revision(repo, right).is_ok()
}

fn read_patches_from_log(
    repo: &Repository,
    rev_args: &[String],
    args: &Args,
    log_extra: &[String],
) -> Result<Vec<Patch>> {
    let exe = std::env::current_exe().context("current_exe")?;
    let mut cmd = Command::new(&exe);
    cmd.arg("-C")
        .arg(repo_work_dir(repo))
        .arg("log")
        .arg("--no-color")
        .arg("--no-abbrev")
        .arg("--reverse")
        .arg("--date-order")
        .arg("--decorate=no")
        .arg("--no-prefix")
        .arg("--output-indicator-new")
        .arg(">")
        .arg("--output-indicator-old")
        .arg("<")
        .arg("--output-indicator-context")
        .arg("#")
        .arg("--pretty=medium")
        .arg("-p");
    for arg in rev_args {
        cmd.arg(arg);
    }
    if args.diff_merges.is_none() {
        cmd.arg("--no-merges");
    }
    if args.no_notes {
        cmd.arg("--no-notes");
    }
    for n in &args.notes {
        if n.is_empty() {
            cmd.arg("--notes");
        } else {
            cmd.arg(format!("--notes={n}"));
        }
    }
    if let Some(dm) = &args.diff_merges {
        cmd.arg(format!("--diff-merges={dm}"));
    }
    for e in log_extra {
        cmd.arg(e);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let out = cmd.output().context("spawn grit log")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("log failed: {stderr}");
    }
    parse_log_into_patches(&String::from_utf8_lossy(&out.stdout))
}

fn log_range_args(range: &str) -> Vec<String> {
    if let Some((left, right)) = split_double_dot_range(range) {
        if !left.is_empty() && !right.is_empty() && split_triple_dot_range(range).is_none() {
            return vec![format!("^{left}"), right.to_string()];
        }
    }
    vec![range.to_string()]
}

fn repo_work_dir(repo: &Repository) -> &Path {
    repo.work_tree.as_deref().unwrap_or(repo.git_dir.as_path())
}

fn parse_log_into_patches(contents: &str) -> Result<Vec<Patch>> {
    let mut list: Vec<Patch> = Vec::new();
    let mut buf = String::new();
    let mut current: Option<Patch> = None;
    let mut in_header = true;
    let mut current_filename: Option<String> = None;
    let mut skip_diff_header = false;

    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("commit ") {
            if let Some(p) = current.take() {
                list.push(finish_patch(p, &buf)?);
                buf.clear();
            }
            let oid_str = rest
                .split(" (from ")
                .next()
                .unwrap_or(rest)
                .split_whitespace()
                .next()
                .unwrap_or(rest)
                .trim();
            let oid: ObjectId = oid_str
                .parse()
                .map_err(|_| anyhow::anyhow!("could not parse commit '{oid_str}'"))?;
            current = Some(Patch {
                oid,
                full: String::new(),
                diff_offset: 0,
                diffsize: 0,
                matching: -1,
                shown: false,
            });
            in_header = true;
            current_filename = None;
            continue;
        }

        let util = current.as_mut().ok_or_else(|| {
            anyhow::anyhow!("could not parse log output (expected commit line first)")
        })?;

        if line.starts_with("diff --git ") {
            in_header = false;
            buf.push('\n');
            if util.diff_offset == 0 {
                util.diff_offset = buf.len();
            }
            let (summary, fname) = parse_diff_git_header(line);
            util.diffsize += summary.lines().count() as i32;
            buf.push_str(&summary);
            current_filename = Some(fname);
            skip_diff_header = true;
            continue;
        }

        if skip_diff_header {
            if line.starts_with("@@ ") {
                skip_diff_header = false;
            } else {
                continue;
            }
        }

        if in_header {
            if line.starts_with("Author: ") {
                buf.push_str(" ## Metadata ##\n");
                buf.push_str(line);
                buf.push_str("\n\n");
                buf.push_str(" ## Commit message ##\n");
            } else if line.starts_with("Notes") && line.ends_with(':') {
                buf.push_str("\n\n");
                let name = line.trim_end_matches(':');
                buf.push_str(&format!(" ## {name} ##\n"));
            } else if let Some(body) = line.strip_prefix("    ") {
                let trimmed = body.trim_end();
                buf.push_str(trimmed);
                buf.push('\n');
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("@@ ") {
            let mut h = String::from("@@");
            if let Some(pos) = rest.find("@@") {
                let after = &rest[pos + 2..];
                if let Some(ref fname) = current_filename {
                    if !after.is_empty() {
                        h.push_str(&format!(" {fname}:"));
                    }
                }
                h.push_str(after);
            }
            buf.push_str(&h);
            buf.push('\n');
            util.diffsize += 1;
            continue;
        }

        if line.is_empty() {
            continue;
        }

        let first = line.as_bytes().first().copied();
        if first == Some(b'>') {
            buf.push('+');
            buf.push_str(&line[1..]);
        } else if first == Some(b'<') {
            buf.push('-');
            buf.push_str(&line[1..]);
        } else if first == Some(b'#') {
            buf.push(' ');
            buf.push_str(&line[1..]);
        } else {
            buf.push(' ');
            buf.push_str(line);
        }
        buf.push('\n');
        util.diffsize += 1;
    }

    if let Some(p) = current {
        list.push(finish_patch(p, &buf)?);
    }

    Ok(list)
}

fn finish_patch(mut p: Patch, buf: &str) -> Result<Patch> {
    p.full = buf.to_string();
    if p.diff_offset > p.full.len() {
        p.diff_offset = p.full.len();
    }
    Ok(p)
}

fn parse_diff_git_header(line: &str) -> (String, String) {
    let rest = line.strip_prefix("diff --git ").unwrap_or("");
    let mut parts = rest.split_whitespace();
    let a_raw = parts.next().unwrap_or("");
    let b_raw = parts.next().unwrap_or("");
    let a = a_raw.strip_prefix("a/").unwrap_or(a_raw);
    let b = b_raw.strip_prefix("b/").unwrap_or(b_raw);
    let summary = format!(" ## {b} ##\n");
    (
        summary,
        if b.is_empty() {
            a.to_string()
        } else {
            b.to_string()
        },
    )
}

fn find_exact_matches(a: &mut [Patch], b: &mut [Patch]) {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in a.iter().enumerate() {
        let d = p.full[p.diff_offset..].to_string();
        map.entry(d).or_default().push(i);
    }
    for (j, p) in b.iter_mut().enumerate() {
        let d = &p.full[p.diff_offset..];
        if let Some(indices) = map.get(d) {
            for &i in indices {
                if a[i].matching < 0 && p.matching < 0 {
                    a[i].matching = j as i32;
                    p.matching = i as i32;
                    break;
                }
            }
        }
    }
}

fn diffsize_lines(x: &str, y: &str) -> i32 {
    if x == y {
        return 0;
    }
    line_diff_inner(x, y).lines().count() as i32
}

fn get_correspondences(a: &mut [Patch], b: &mut [Patch], creation_factor: u64) {
    let n = a.len() + b.len();
    if n == 0 {
        return;
    }
    let mut cost = vec![0u64; n * n];
    for i in 0..a.len() {
        for j in 0..b.len() {
            let ai = &a[i];
            let bj = &b[j];
            let c = if ai.matching == j as i32 {
                0
            } else if ai.matching < 0 && bj.matching < 0 {
                let da = &ai.full[ai.diff_offset..];
                let db = &bj.full[bj.diff_offset..];
                diffsize_lines(da, db) as u64
            } else {
                COST_MAX
            };
            cost[j + n * i] = c;
        }
        let ai = &a[i];
        let c_pad = if ai.matching < 0 {
            (ai.diffsize as u64).saturating_mul(creation_factor) / 100
        } else {
            COST_MAX
        };
        for j in b.len()..n {
            cost[j + n * i] = c_pad;
        }
    }
    for j in 0..b.len() {
        let bj = &b[j];
        let c_pad = if bj.matching < 0 {
            (bj.diffsize as u64).saturating_mul(creation_factor) / 100
        } else {
            COST_MAX
        };
        for i in a.len()..n {
            cost[j + n * i] = c_pad;
        }
    }
    for i in a.len()..n {
        for j in b.len()..n {
            cost[j + n * i] = 0;
        }
    }

    let assign = minimize(&cost, n, n);
    for i in 0..a.len() {
        if let Some(Some(j)) = assign.get(i) {
            if *j < b.len() {
                a[i].matching = *j as i32;
                b[*j].matching = i as i32;
            }
        }
    }
}

fn output(
    repo: &Repository,
    a: &mut [Patch],
    b: &mut [Patch],
    left_only: bool,
    right_only: bool,
    no_patch: bool,
    stat_mode: bool,
    use_color: bool,
    abbrev_len: usize,
) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let max_n = 1 + a.len().max(b.len());
    let w = decimal_width(max_n);
    let reset = if use_color { "\x1b[m" } else { "" };
    let color_old = if use_color { "\x1b[31m" } else { "" };
    let color_new = if use_color { "\x1b[32m" } else { "" };
    let color_commit = if use_color { "\x1b[33m" } else { "" };

    let dash_str: String = std::iter::repeat_n('-', abbrev_len).collect();

    let mut i = 0usize;
    let mut j = 0usize;
    while i < a.len() || j < b.len() {
        while i < a.len() && a[i].shown {
            i += 1;
        }
        if i < a.len() && a[i].matching < 0 {
            if !right_only {
                write_pair_header(
                    &mut out,
                    repo,
                    w,
                    Some((i, &a[i])),
                    None,
                    abbrev_len,
                    &dash_str,
                    reset,
                    color_old,
                    color_new,
                    color_commit,
                )?;
            }
            i += 1;
            continue;
        }
        while j < b.len() && b[j].matching < 0 {
            if !left_only {
                write_pair_header(
                    &mut out,
                    repo,
                    w,
                    None,
                    Some((j, &b[j])),
                    abbrev_len,
                    &dash_str,
                    reset,
                    color_old,
                    color_new,
                    color_commit,
                )?;
            }
            j += 1;
        }
        if j < b.len() {
            let bj = &b[j];
            let ai_idx = bj.matching as usize;
            let ai = &a[ai_idx];
            write_pair_header(
                &mut out,
                repo,
                w,
                Some((ai_idx, ai)),
                Some((j, bj)),
                abbrev_len,
                &dash_str,
                reset,
                color_old,
                color_new,
                color_commit,
            )?;
            if !no_patch && ai.full != bj.full {
                let inner = diff_patches(&ai.full, &bj.full);
                if stat_mode {
                    write_stat_summary(&mut out, &inner)?;
                } else {
                    for line in inner.lines() {
                        writeln!(out, "    {line}")?;
                    }
                }
            }
            a[ai_idx].shown = true;
            j += 1;
        }
    }

    Ok(())
}

fn write_stat_summary(out: &mut impl Write, diff: &str) -> Result<()> {
    let mut ins = 0usize;
    let mut dels = 0usize;
    let mut files = 0usize;
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            ins += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            dels += 1;
        }
    }
    if diff.contains("diff --git") {
        files = diff.matches("diff --git").count();
    } else if ins + dels > 0 {
        files = 1;
    }
    writeln!(out, "     a => b | {} +-", ins.saturating_add(dels))?;
    writeln!(
        out,
        "     {} file{} changed, {} insertion{}(+), {} deletion{}(-)",
        files,
        if files == 1 { "" } else { "s" },
        ins,
        if ins == 1 { "" } else { "s" },
        dels,
        if dels == 1 { "" } else { "s" },
    )?;
    Ok(())
}

fn diff_patches(x: &str, y: &str) -> String {
    line_diff_inner(x, y)
}

/// Git-style line diff (one prefix character per output line).
fn line_diff_inner(a: &str, b: &str) -> String {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    let n = la.len();
    let m = lb.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if la[i] == lb[j] {
                1 + dp[i + 1][j + 1]
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut i = 0usize;
    let mut j = 0usize;
    let mut out = String::new();
    while i < n || j < m {
        if i < n && j < m && la[i] == lb[j] {
            out.push(' ');
            out.push_str(la[i]);
            out.push('\n');
            i += 1;
            j += 1;
        } else if i < n && (j == m || dp[i + 1][j] >= dp[i][j + 1]) {
            out.push('-');
            out.push_str(la[i]);
            out.push('\n');
            i += 1;
        } else if j < m {
            out.push('+');
            out.push_str(lb[j]);
            out.push('\n');
            j += 1;
        }
    }
    out
}

fn write_pair_header(
    out: &mut impl Write,
    repo: &Repository,
    w: usize,
    left: Option<(usize, &Patch)>,
    right: Option<(usize, &Patch)>,
    abbrev_len: usize,
    dash_str: &str,
    reset: &str,
    color_old: &str,
    color_new: &str,
    color_commit: &str,
) -> Result<()> {
    let status = match (left, right) {
        (Some(_), None) => '<',
        (None, Some(_)) => '>',
        (Some((_, l)), Some((_, r))) => {
            if l.full == r.full {
                '='
            } else {
                '!'
            }
        }
        (None, None) => unreachable!(),
    };

    let oid_for_subject = left
        .map(|(_, p)| p.oid)
        .or_else(|| right.map(|(_, p)| p.oid))
        .unwrap();

    match (left, right) {
        (None, Some((rj, rp))) => {
            write!(out, "{color_new}{:>w$}:  {} ", "-", dash_str, w = w)?;
            write!(out, "{status} ")?;
            write!(
                out,
                "{:>w$}:  {}",
                rj + 1,
                abbreviate_object_id(repo, rp.oid, abbrev_len)?,
                w = w
            )?;
        }
        (Some((li, lp)), None) => {
            let c0 = if status == '!' {
                color_old
            } else {
                color_commit
            };
            write!(
                out,
                "{c0}{:>w$}:  {} ",
                li + 1,
                abbreviate_object_id(repo, lp.oid, abbrev_len)?,
                w = w
            )?;
            if status == '!' {
                write!(out, "{reset}{color_commit}")?;
            }
            write!(out, "{status}")?;
            if status == '!' {
                write!(out, "{reset}{color_new}")?;
            }
            write!(out, " {:>w$}:  {}", "-", dash_str, w = w)?;
        }
        (Some((li, lp)), Some((rj, rp))) => {
            let c0 = if status == '!' {
                color_old
            } else {
                color_commit
            };
            write!(
                out,
                "{c0}{:>w$}:  {} ",
                li + 1,
                abbreviate_object_id(repo, lp.oid, abbrev_len)?,
                w = w
            )?;
            if status == '!' {
                write!(out, "{reset}{color_commit}")?;
            }
            write!(out, "{status}")?;
            if status == '!' {
                write!(out, "{reset}{color_new}")?;
            }
            write!(
                out,
                " {:>w$}:  {}",
                rj + 1,
                abbreviate_object_id(repo, rp.oid, abbrev_len)?,
                w = w
            )?;
        }
        _ => {}
    }

    let subj = lookup_commit_subject(repo, oid_for_subject)?;
    if !subj.is_empty() {
        if status == '!' {
            write!(out, "{reset}")?;
        }
        write!(out, " {subj}")?;
    }
    writeln!(out, "{reset}")?;
    Ok(())
}

fn lookup_commit_subject(repo: &Repository, oid: ObjectId) -> Result<String> {
    let obj = repo.odb.read(&oid)?;
    let c = parse_commit(&obj.data)?;
    Ok(c.message.lines().next().unwrap_or("").to_string())
}

fn decimal_width(mut n: usize) -> usize {
    let mut w = 1;
    while n >= 10 {
        n /= 10;
        w += 1;
    }
    w
}
