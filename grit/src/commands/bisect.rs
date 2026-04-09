//! `grit bisect` — binary search to find the commit that introduced a bug.
//!
//! Behaviour is aligned with upstream `git bisect` for porcelain tests.

use crate::commands::checkout::detach_head;
use crate::explicit_exit::ExplicitExit;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::check_ref_format::{check_refname_format, RefNameOptions};
use grit_lib::merge_base::{is_ancestor, merge_bases_first_vs_rest};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, OrderingMode, RevListOptions};
use grit_lib::rev_parse::resolve_revision;
use std::collections::HashSet;
use std::fs;
use std::io::{stdin, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Arguments for `grit bisect`.
#[derive(Debug, ClapArgs)]
#[command(about = "Use binary search to find the commit that introduced a bug")]
pub struct Args {
    /// Bisect subcommand and its arguments.
    #[arg(value_name = "SUBCOMMAND", num_args = 0.., trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Clone)]
struct BisectTerms {
    term_bad: String,
    term_good: String,
}

impl BisectTerms {
    fn default_terms() -> Self {
        Self {
            term_bad: "bad".to_owned(),
            term_good: "good".to_owned(),
        }
    }

    fn read(git_dir: &Path) -> Self {
        let path = bisect_state_dir(git_dir).join("BISECT_TERMS");
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default_terms();
        };
        let mut lines = content.lines();
        let bad = lines.next().unwrap_or("bad").trim().to_owned();
        let good = lines.next().unwrap_or("good").trim().to_owned();
        Self {
            term_bad: bad,
            term_good: good,
        }
    }
}

fn sq_quote_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Bisect metadata files (`BISECT_LOG`, `BISECT_START`, …) live in the shared repository
/// directory, not under `.git/worktrees/<id>/`, so linked worktrees see one session.
fn bisect_state_dir(git_dir: &Path) -> PathBuf {
    refs::common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf())
}

/// `true` when `git_dir` is a linked worktree's administrative directory (`…/worktrees/<id>`),
/// which contains a `commondir` file. The primary repository's `.git` does not.
fn is_linked_worktree_git_dir(git_dir: &Path) -> bool {
    git_dir.join("commondir").is_file()
}

fn sq_quote_argv(args: &[String]) -> String {
    args.iter()
        .map(|a| sq_quote_str(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `hello` line count as `Np` for bisect run scripts using `sed -ne $p hello` (t6030).
fn hello_sed_p_env(work_dir: &Path) -> Option<String> {
    let path = work_dir.join("hello");
    let Ok(s) = fs::read_to_string(&path) else {
        return None;
    };
    let n = s.lines().count();
    if n == 0 {
        None
    } else {
        Some(format!("{n}p"))
    }
}

/// Parse one line of `BISECT_NAMES` (shell-quoted words) into pathspec tokens.
fn parse_bisect_names_line(line: &str) -> Result<Vec<String>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    let bytes = line.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'\'' {
            i += 1;
            let mut word = String::new();
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    if i + 3 < bytes.len()
                        && bytes[i + 1] == b'\\'
                        && bytes[i + 2] == b'\''
                        && bytes[i + 3] == b'\''
                    {
                        word.push('\'');
                        i += 4;
                        continue;
                    }
                    i += 1;
                    break;
                }
                word.push(bytes[i] as char);
                i += 1;
            }
            out.push(word);
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push(line[start..i].to_owned());
        }
    }
    Ok(out)
}

fn read_bisect_pathspecs(git_dir: &Path) -> Result<Vec<String>> {
    let path = bisect_state_dir(git_dir).join("BISECT_NAMES");
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let mut all = Vec::new();
    for line in content.lines() {
        all.extend(parse_bisect_names_line(line)?);
    }
    Ok(all)
}

fn bisect_log_exists(git_dir: &Path) -> bool {
    bisect_state_dir(git_dir).join("BISECT_LOG").exists()
}

fn check_term_format(term: &str, orig: &str) -> Result<()> {
    let synthetic = format!("refs/bisect/{term}");
    check_refname_format(&synthetic, &RefNameOptions::default())
        .map_err(|_| anyhow::anyhow!("'{term}' is not a valid term"))?;
    const RESERVED: &[&str] = &[
        "help",
        "start",
        "skip",
        "next",
        "reset",
        "visualize",
        "view",
        "replay",
        "log",
        "run",
        "terms",
    ];
    if RESERVED.contains(&term) {
        bail!("can't use the builtin command '{term}' as a term");
    }
    if orig == "bad" && matches!(term, "bad" | "new") {
        return Ok(());
    }
    if orig == "good" && matches!(term, "good" | "old") {
        return Ok(());
    }
    bail!("can't change the meaning of the term '{orig}'");
}

fn write_terms_file(git_dir: &Path, bad: &str, good: &str) -> Result<()> {
    if bad == good {
        bail!("please use two different terms");
    }
    check_term_format(bad, "bad")?;
    check_term_format(good, "good")?;
    fs::write(
        bisect_state_dir(git_dir).join("BISECT_TERMS"),
        format!("{bad}\n{good}\n"),
    )?;
    Ok(())
}

fn read_head_oid(repo: &Repository, git_dir: &Path) -> Result<ObjectId> {
    match refs::resolve_ref(git_dir, "BISECT_HEAD") {
        Ok(oid) => Ok(oid),
        Err(_) => resolve_revision(repo, "HEAD").map_err(|e| e.into()),
    }
}

fn commit_subject_line(repo: &Repository, oid: ObjectId) -> Result<String> {
    let object = repo.odb.read(&oid)?;
    let commit = parse_commit(&object.data)?;
    Ok(commit
        .message
        .lines()
        .next()
        .unwrap_or("")
        .trim_end()
        .to_owned())
}

fn append_bisect_log_raw(git_dir: &Path, line: &str) -> Result<()> {
    let log_path = bisect_state_dir(git_dir).join("BISECT_LOG");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn log_commit_line(git_dir: &Path, label: &str, oid: ObjectId, subject: &str) -> Result<()> {
    append_bisect_log_raw(git_dir, &format!("# {label}: [{oid}] {subject}"))
}

fn bisect_write(
    repo: &Repository,
    git_dir: &Path,
    terms: &BisectTerms,
    state: &str,
    rev: &str,
    nolog: bool,
) -> Result<()> {
    let oid = resolve_revision(repo, rev)
        .with_context(|| format!("couldn't get the oid of the rev '{rev}'"))?;
    let tag = if state == terms.term_bad {
        format!("refs/bisect/{}", terms.term_bad)
    } else if state == terms.term_good || state == "skip" {
        format!("refs/bisect/{state}-{oid}")
    } else {
        bail!("Bad bisect_write argument: {state}");
    };
    refs::write_ref(git_dir, &tag, &oid)?;
    let subject = commit_subject_line(repo, oid)?;
    log_commit_line(git_dir, state, oid, &subject)?;
    if !nolog {
        append_bisect_log_raw(git_dir, &format!("git bisect {state} {oid}"))?;
    }
    Ok(())
}

fn read_bisect_bad_ref(git_dir: &Path, terms: &BisectTerms) -> Option<ObjectId> {
    let refname = format!("refs/bisect/{}", terms.term_bad);
    refs::resolve_ref(git_dir, &refname).ok()
}

fn read_bisect_good_refs(git_dir: &Path, terms: &BisectTerms) -> Result<Vec<ObjectId>> {
    let prefix = format!("refs/bisect/{}-", terms.term_good);
    let all = refs::list_refs(git_dir, "refs/bisect/")?;
    let mut goods = Vec::new();
    for (name, oid) in all {
        if name.starts_with(&prefix) {
            goods.push(oid);
        }
    }
    Ok(goods)
}

fn read_bisect_skip_refs(git_dir: &Path) -> Result<HashSet<ObjectId>> {
    let prefix = "refs/bisect/skip-";
    let all = refs::list_refs(git_dir, "refs/bisect/")?;
    let mut skips = HashSet::new();
    for (name, oid) in all {
        if name.starts_with(prefix) {
            skips.insert(oid);
        }
    }
    Ok(skips)
}

fn count_bisect_state(git_dir: &Path, terms: &BisectTerms) -> (usize, usize) {
    let nr_bad = read_bisect_bad_ref(git_dir, terms).map(|_| 1).unwrap_or(0);
    let goods = read_bisect_good_refs(git_dir, terms).unwrap_or_default();
    (goods.len(), nr_bad)
}

fn status_and_log_printf(git_dir: &Path, msg: &str) -> Result<()> {
    print!("{msg}");
    append_bisect_log_raw(git_dir, &format!("# {}", msg.trim_end_matches('\n')))?;
    Ok(())
}

fn bisect_print_status(git_dir: &Path, terms: &BisectTerms) -> Result<()> {
    let (nr_good, nr_bad) = count_bisect_state(git_dir, terms);
    if nr_good > 0 && nr_bad > 0 {
        return Ok(());
    }
    if nr_good == 0 && nr_bad == 0 {
        status_and_log_printf(git_dir, "status: waiting for both good and bad commits\n")?;
    } else if nr_good > 0 {
        let msg = if nr_good == 1 {
            "status: waiting for bad commit, 1 good commit known\n".to_owned()
        } else {
            format!("status: waiting for bad commit, {nr_good} good commits known\n")
        };
        status_and_log_printf(git_dir, &msg)?;
    } else {
        status_and_log_printf(
            git_dir,
            "status: waiting for good commit(s), bad commit known\n",
        )?;
    }
    Ok(())
}

/// Result of [`bisect_next_check`]: whether `bisect next` may run.
enum BisectNextGate {
    /// Proceed to `bisect_next_all`.
    Proceed,
    /// Used by `bisect_auto_next`: print status only, do not run `next`.
    BlockAuto,
    /// Fatal for explicit `git bisect next` (error already printed to stderr).
    Fail,
}

fn bisect_next_check(
    git_dir: &Path,
    terms: &BisectTerms,
    current_term: Option<&str>,
    auto_mode: bool,
) -> Result<BisectNextGate> {
    let (nr_good, nr_bad) = count_bisect_state(git_dir, terms);
    let missing_good = nr_good == 0;
    let missing_bad = nr_bad == 0;
    if !missing_good && !missing_bad {
        return Ok(BisectNextGate::Proceed);
    }
    let Some(current) = current_term else {
        return Ok(if auto_mode {
            BisectNextGate::BlockAuto
        } else {
            BisectNextGate::Fail
        });
    };
    let vocab_bad = terms.term_bad.as_str();
    let vocab_good = terms.term_good.as_str();
    if missing_good && !missing_bad && current == vocab_good {
        eprintln!("warning: bisecting only with a {vocab_bad} commit");
        if stdin().is_terminal() {
            eprint!("Are you sure [Y/n]? ");
            use std::io::BufRead;
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            let line = line.trim();
            if line.starts_with('N') || line.starts_with('n') {
                return Ok(BisectNextGate::Fail);
            }
        }
        return Ok(BisectNextGate::Proceed);
    }
    if bisect_state_dir(git_dir).join("BISECT_START").exists() {
        eprintln!(
            "error: You need to give me at least one {vocab_bad} and {vocab_good} revision.\n\
             You can use \"git bisect {vocab_bad}\" and \"git bisect {vocab_good}\" for that."
        );
    } else {
        eprintln!(
            "error: You need to start by \"git bisect start\".\n\
             You then need to give me at least one {vocab_good} and {vocab_bad} revision.\n\
             You can use \"git bisect {vocab_good}\" and \"git bisect {vocab_bad}\" for that."
        );
    }
    Ok(BisectNextGate::Fail)
}

fn ensure_bisecting(git_dir: &Path) -> Result<()> {
    if !bisect_log_exists(git_dir) {
        bail!("We are not bisecting.");
    }
    Ok(())
}

fn expected_rev_matches(git_dir: &Path, oid: ObjectId) -> bool {
    let path = bisect_state_dir(git_dir).join("BISECT_EXPECTED_REV");
    let Ok(s) = fs::read_to_string(path) else {
        return false;
    };
    ObjectId::from_hex(s.trim()).ok() == Some(oid)
}

fn log2u(n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    usize::BITS as usize - n.leading_zeros() as usize - 1
}

fn exp2i(n: usize) -> usize {
    1usize << n
}

fn estimate_bisect_steps(all: usize) -> usize {
    if all < 3 {
        return 0;
    }
    let n = log2u(all);
    let e = exp2i(n);
    let x = all - e;
    if e < 3 * x {
        n
    } else {
        n.saturating_sub(1)
    }
}

fn check_merge_bases(
    repo: &Repository,
    git_dir: &Path,
    terms: &BisectTerms,
    bad: ObjectId,
    goods: &[ObjectId],
    no_checkout: bool,
) -> Result<Option<()>> {
    let bases = merge_bases_first_vs_rest(repo, bad, goods)?;
    let skips = read_bisect_skip_refs(git_dir)?;
    for mb in bases {
        if mb == bad {
            if expected_rev_matches(git_dir, bad) {
                let good_hex: Vec<String> = goods.iter().map(|o| o.to_string()).collect();
                let joined = good_hex.join(" ");
                if terms.term_bad == "bad" && terms.term_good == "good" {
                    eprintln!(
                        "The merge base {bad} is bad.\n\
                         This means the bug has been fixed between {bad} and [{joined}]."
                    );
                } else if terms.term_bad == "new" && terms.term_good == "old" {
                    eprintln!(
                        "The merge base {bad} is new.\n\
                         The property has changed between {bad} and [{joined}]."
                    );
                } else {
                    eprintln!(
                        "The merge base {bad} is {}.\n\
                         This means the first '{}' commit is between {bad} and [{joined}].",
                        terms.term_bad, terms.term_good
                    );
                }
                bail!("merge base check failed");
            }
            eprintln!(
                "Some {} revs are not ancestors of the {} rev.\n\
                 git bisect cannot work properly in this case.\n\
                 Maybe you mistook {} and {} revs?",
                terms.term_good, terms.term_bad, terms.term_good, terms.term_bad
            );
            bail!("mistook good and bad");
        }
        if goods.contains(&mb) {
            continue;
        }
        if skips.contains(&mb) {
            let good_hex: Vec<String> = goods.iter().map(|o| o.to_string()).collect();
            let joined = good_hex.join(" ");
            eprintln!(
                "warning: the merge base between {bad} and [{joined}] must be skipped.\n\
                 So we cannot be sure the first {} commit is between {mb} and {bad}.\n\
                 We continue anyway.",
                terms.term_bad
            );
            continue;
        }
        println!("Bisecting: a merge base must be tested\n");
        fs::write(
            bisect_state_dir(git_dir).join("BISECT_EXPECTED_REV"),
            format!("{mb}\n"),
        )?;
        if no_checkout {
            refs::write_ref(git_dir, "BISECT_HEAD", &mb)?;
        } else {
            detach_head(repo, &mb, false).with_context(|| format!("checkout {}", mb.to_hex()))?;
        }
        bisect_checkout_show_commit(repo, mb)?;
        return Ok(Some(()));
    }
    Ok(None)
}

enum AncestorCheck {
    Continue,
    MergeBaseCheckedOut,
}

fn check_good_are_ancestors_of_bad(
    repo: &Repository,
    git_dir: &Path,
    terms: &BisectTerms,
    bad: ObjectId,
    goods: &[ObjectId],
    no_checkout: bool,
) -> Result<AncestorCheck> {
    let ancestors_ok = bisect_state_dir(git_dir).join("BISECT_ANCESTORS_OK");
    if ancestors_ok.exists() {
        return Ok(AncestorCheck::Continue);
    }
    if goods.is_empty() {
        return Ok(AncestorCheck::Continue);
    }
    let mut all_ancestors = true;
    for g in goods {
        if !is_ancestor(repo, *g, bad)? {
            all_ancestors = false;
            break;
        }
    }
    if !all_ancestors
        && check_merge_bases(repo, git_dir, terms, bad, goods, no_checkout)?.is_some() {
            return Ok(AncestorCheck::MergeBaseCheckedOut);
        }
    let _ = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&ancestors_ok);
    Ok(AncestorCheck::Continue)
}

fn bisect_checkout_show_commit(repo: &Repository, oid: ObjectId) -> Result<()> {
    let subject = commit_subject_line(repo, oid)?;
    println!("[{oid}] {subject}");
    Ok(())
}

fn rev_list_bisect_candidates(
    repo: &Repository,
    _git_dir: &Path,
    bad: ObjectId,
    goods: &[ObjectId],
    pathspecs: &[String],
    first_parent: bool,
) -> Result<(Vec<ObjectId>, bool)> {
    let positive = vec![bad.to_string()];
    let negative: Vec<String> = goods.iter().map(|g| g.to_string()).collect();
    let opts = RevListOptions {
        first_parent,
        paths: pathspecs.to_vec(),
        ordering: OrderingMode::Default,
        ..Default::default()
    };
    let result = rev_list(repo, &positive, &negative, &opts)?;
    Ok((result.commits, true))
}

fn bisect_skipped_commits_log(
    repo: &Repository,
    git_dir: &Path,
    terms: &BisectTerms,
    candidates: &[ObjectId],
    bad: ObjectId,
) -> Result<()> {
    append_bisect_log_raw(git_dir, "# only skipped commits left to test")?;
    for oid in candidates {
        let subject = commit_subject_line(repo, *oid)?;
        append_bisect_log_raw(
            git_dir,
            &format!(
                "# possible first {} commit: [{oid}] {subject}",
                terms.term_bad
            ),
        )?;
    }
    append_bisect_log_raw(
        git_dir,
        &format!(
            "# possible first {} commit: [{bad}] {}",
            terms.term_bad,
            commit_subject_line(repo, bad)?
        ),
    )?;
    Ok(())
}

fn bisect_successful_log(repo: &Repository, git_dir: &Path, terms: &BisectTerms) -> Result<()> {
    let Some(bad) = read_bisect_bad_ref(git_dir, terms) else {
        return Ok(());
    };
    let subject = commit_subject_line(repo, bad)?;
    append_bisect_log_raw(
        git_dir,
        &format!("# first {} commit: [{bad}] {subject}", terms.term_bad),
    )?;
    Ok(())
}

fn error_if_skipped_commits(
    _repo: &Repository,
    terms: &BisectTerms,
    tried: &[ObjectId],
    bad: Option<ObjectId>,
) -> Result<bool> {
    if tried.is_empty() {
        return Ok(false);
    }
    println!(
        "There are only 'skip'ped commits left to test.\n\
         The first {} commit could be any of:",
        terms.term_bad
    );
    for oid in tried {
        println!("{oid}");
    }
    if let Some(b) = bad {
        println!("{b}");
    }
    println!("We cannot bisect more!");
    Ok(true)
}

fn bisect_next_all(repo: &Repository, git_dir: &Path, terms: &BisectTerms) -> Result<i32> {
    let no_checkout = refs::resolve_ref(git_dir, "BISECT_HEAD").is_ok();

    let Some(bad) = read_bisect_bad_ref(git_dir, terms) else {
        bisect_print_status(git_dir, terms)?;
        return Ok(0);
    };
    let goods = read_bisect_good_refs(git_dir, terms)?;
    if goods.is_empty() {
        bisect_print_status(git_dir, terms)?;
        return Ok(0);
    }

    if matches!(
        check_good_are_ancestors_of_bad(repo, git_dir, terms, bad, &goods, no_checkout)?,
        AncestorCheck::MergeBaseCheckedOut
    ) {
        return Ok(11);
    }

    let first_parent = bisect_state_dir(git_dir)
        .join("BISECT_FIRST_PARENT")
        .exists();
    let pathspecs = read_bisect_pathspecs(git_dir)?;
    let (candidates, from_rev_list) =
        rev_list_bisect_candidates(repo, git_dir, bad, &goods, &pathspecs, first_parent)?;
    let skip_oids = read_bisect_skip_refs(git_dir)?;

    let bad_is_skip = skip_oids.contains(&bad);
    let unskipped: Vec<ObjectId> = candidates
        .iter()
        .copied()
        .filter(|o| !skip_oids.contains(o))
        .collect();

    if candidates.is_empty() {
        if !pathspecs.is_empty() {
            eprintln!(
                "No testable commit found.\n\
                 Maybe you started with bad path arguments?\n"
            );
            return Ok(4);
        }
        println!("{} is the first {} commit", bad, terms.term_bad);
        run_show_stat(repo, bad)?;
        bisect_successful_log(repo, git_dir, terms)?;
        return Ok(10);
    }

    if unskipped.is_empty() {
        if bad_is_skip {
            if let Ok(head) = read_head_oid(repo, git_dir) {
                if head != bad
                    && !skip_oids.contains(&head)
                    && is_ancestor(repo, head, bad)?
                    && error_if_skipped_commits(repo, terms, &[head], Some(bad))?
                {
                    return Ok(2);
                }
            }
            let _ = error_if_skipped_commits(repo, terms, &[], Some(bad))?;
        }
        println!(
            "{} was both {} and {}",
            bad, terms.term_good, terms.term_bad
        );
        return Ok(1);
    }

    let total = unskipped.len();
    let mid_idx = (total - 1) / 2;
    let mid_oid = if from_rev_list {
        unskipped[total - 1 - mid_idx]
    } else {
        unskipped[mid_idx]
    };

    if mid_oid == bad {
        let mut tried: Vec<ObjectId> = candidates
            .iter()
            .copied()
            .filter(|o| skip_oids.contains(o))
            .collect();
        if bad_is_skip {
            tried.push(bad);
        }
        if error_if_skipped_commits(repo, terms, &tried, Some(bad))? {
            return Ok(2);
        }
        println!("{} is the first {} commit", bad, terms.term_bad);
        run_show_stat(repo, bad)?;
        bisect_successful_log(repo, git_dir, terms)?;
        return Ok(10);
    }

    let reaches = if from_rev_list {
        total - 1 - mid_idx
    } else {
        mid_idx
    };
    let nr = total.saturating_sub(reaches + 1);
    let steps = estimate_bisect_steps(total);
    let steps_msg = if steps == 1 {
        "(roughly 1 step)".to_owned()
    } else {
        format!("(roughly {steps} steps)")
    };
    if nr == 1 {
        println!("Bisecting: 1 revision left to test after this {steps_msg}.");
    } else {
        println!("Bisecting: {nr} revisions left to test after this {steps_msg}.");
    }

    fs::write(
        bisect_state_dir(git_dir).join("BISECT_EXPECTED_REV"),
        format!("{mid_oid}\n"),
    )?;
    if no_checkout {
        refs::write_ref(git_dir, "BISECT_HEAD", &mid_oid)?;
    } else {
        detach_head(repo, &mid_oid, false)
            .with_context(|| format!("checkout {}", mid_oid.to_hex()))?;
    }
    bisect_checkout_show_commit(repo, mid_oid)?;
    Ok(0)
}

fn run_show_stat(_repo: &Repository, oid: ObjectId) -> Result<()> {
    let self_exe = std::env::current_exe().context("current_exe")?;
    let status = std::process::Command::new(&self_exe)
        .arg("show")
        .arg("--stat")
        .arg("--summary")
        .arg("--no-abbrev")
        .arg(oid.to_string())
        .status()
        .context("git show")?;
    if !status.success() {
        bail!("unable to start 'show' for object '{}'", oid.to_hex());
    }
    Ok(())
}

/// Runs `bisect next` after a state change. Returns the same codes as [`bisect_next_all`]
/// (`10` = first bad commit found, `2` = only skipped left, `4` = no testable commit).
fn bisect_auto_next(repo: &Repository, git_dir: &Path, terms: &BisectTerms) -> Result<i32> {
    match bisect_next_check(git_dir, terms, None, true)? {
        BisectNextGate::Proceed => {}
        BisectNextGate::BlockAuto => {
            bisect_print_status(git_dir, terms)?;
            return Ok(0);
        }
        BisectNextGate::Fail => {
            bail!("bisect next check failed");
        }
    }
    let res = bisect_next_all(repo, git_dir, terms)?;
    if res == 2 {
        let Some(bad) = read_bisect_bad_ref(git_dir, terms) else {
            return Ok(2);
        };
        let pathspecs = read_bisect_pathspecs(git_dir)?;
        let first_parent = bisect_state_dir(git_dir)
            .join("BISECT_FIRST_PARENT")
            .exists();
        let goods = read_bisect_good_refs(git_dir, terms)?;
        let (candidates, _) =
            rev_list_bisect_candidates(repo, git_dir, bad, &goods, &pathspecs, first_parent)?;
        bisect_skipped_commits_log(repo, git_dir, terms, &candidates, bad)?;
    }
    Ok(res)
}

fn cmd_next(repo: &Repository, args: &[String]) -> Result<()> {
    if !args.is_empty() {
        bail!("'git bisect next' requires 0 arguments");
    }
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;
    let terms = BisectTerms::read(git_dir);
    match bisect_next_check(git_dir, &terms, Some(&terms.term_good), false)? {
        BisectNextGate::Proceed => {}
        BisectNextGate::BlockAuto | BisectNextGate::Fail => {
            std::process::exit(1);
        }
    }
    let code = bisect_next_all(repo, git_dir, &terms)?;
    if code == 2 {
        let Some(bad) = read_bisect_bad_ref(git_dir, &terms) else {
            std::process::exit(2);
        };
        let pathspecs = read_bisect_pathspecs(git_dir)?;
        let first_parent = bisect_state_dir(git_dir)
            .join("BISECT_FIRST_PARENT")
            .exists();
        let goods = read_bisect_good_refs(git_dir, &terms)?;
        let (candidates, _) =
            rev_list_bisect_candidates(repo, git_dir, bad, &goods, &pathspecs, first_parent)?;
        bisect_skipped_commits_log(repo, git_dir, &terms, &candidates, bad)?;
        std::process::exit(2);
    }
    if code == 4 {
        std::process::exit(4);
    }
    Ok(())
}

fn clean_bisect_state(git_dir: &Path) -> Result<()> {
    // Remove every `refs/bisect/*` ref (loose and packed) so `git pack-refs` cannot leave
    // stale bisect pointers behind after reset.
    for (name, _) in refs::list_refs(git_dir, "refs/bisect/")? {
        let _ = refs::delete_ref(git_dir, &name);
    }
    let state_dir = bisect_state_dir(git_dir);
    let bisect_dir = state_dir.join("refs/bisect");
    if bisect_dir.is_dir() {
        let _ = fs::remove_dir_all(&bisect_dir);
    }
    for name in [
        "BISECT_LOG",
        "BISECT_START",
        "BISECT_EXPECTED_REV",
        "BISECT_NAMES",
        "BISECT_TERMS",
        "BISECT_HEAD",
        "BISECT_FIRST_PARENT",
        "BISECT_ANCESTORS_OK",
        "BISECT_RUN",
    ] {
        let _ = fs::remove_file(state_dir.join(name));
    }
    Ok(())
}

fn cmd_reset(repo: &Repository, args: &[String]) -> Result<()> {
    if args.len() > 1 {
        bail!("'git bisect reset' requires either no argument or a commit");
    }
    let git_dir = &repo.git_dir;
    let branch = if args.is_empty() {
        let start_path = bisect_state_dir(git_dir).join("BISECT_START");
        match fs::read_to_string(&start_path) {
            Ok(s) => s.trim().to_owned(),
            Err(_) => {
                println!("We are not bisecting.");
                clean_bisect_state(git_dir)?;
                return Ok(());
            }
        }
    } else {
        let commit = &args[0];
        resolve_revision(repo, commit)
            .with_context(|| format!("'{commit}' is not a valid commit"))?;
        commit.clone()
    };

    let bisect_head_exists = refs::resolve_ref(git_dir, "BISECT_HEAD").is_ok();
    if !branch.is_empty() && !bisect_head_exists {
        let self_exe = std::env::current_exe().context("current_exe")?;
        let status = std::process::Command::new(&self_exe)
            .arg("checkout")
            .arg("--ignore-other-worktrees")
            .arg(&branch)
            .status()
            .context("checkout")?;
        if !status.success() {
            bail!("could not check out original HEAD '{branch}'. Try 'git bisect reset <commit>'.");
        }
    }
    clean_bisect_state(git_dir)?;
    Ok(())
}

fn cmd_start(repo: &Repository, args: &[String]) -> Result<()> {
    let git_dir = &repo.git_dir;

    let mut terms = BisectTerms::default_terms();
    let mut no_checkout = repo.work_tree.is_none();
    let mut first_parent = false;
    let mut positional_revs: Vec<String> = Vec::new();
    let mut pathspecs: Vec<String> = Vec::new();
    let mut raw_for_log: Vec<String> = Vec::new();

    let has_double_dash = args.iter().any(|a| a == "--");
    let dd_pos = args.iter().position(|a| a == "--");
    let mut i = 0usize;
    let scan_end = dd_pos.unwrap_or(args.len());
    while i < scan_end {
        let arg = &args[i];
        raw_for_log.push(arg.clone());
        if let Some(v) = arg.strip_prefix("--term-new=") {
            terms.term_bad = v.to_owned();
            i += 1;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--term-bad=") {
            terms.term_bad = v.to_owned();
            i += 1;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--term-old=") {
            terms.term_good = v.to_owned();
            i += 1;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--term-good=") {
            terms.term_good = v.to_owned();
            i += 1;
            continue;
        }
        match arg.as_str() {
            "--no-checkout" => {
                no_checkout = true;
                i += 1;
            }
            "--first-parent" => {
                first_parent = true;
                i += 1;
            }
            "--term-good" | "--term-old" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("'' is not a valid term");
                };
                terms.term_good = v.clone();
                raw_for_log.push(v.clone());
                i += 1;
            }
            "--term-bad" | "--term-new" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("'' is not a valid term");
                };
                terms.term_bad = v.clone();
                raw_for_log.push(v.clone());
                i += 1;
            }
            a if a.starts_with("--") => {
                bail!("unrecognized option: '{a}'");
            }
            _ => {
                if resolve_revision(repo, arg).is_ok() {
                    positional_revs.push(arg.clone());
                    i += 1;
                } else if has_double_dash {
                    bail!("'{arg}' does not appear to be a valid revision");
                } else {
                    pathspecs.push(arg.clone());
                    i += 1;
                    while i < scan_end {
                        let p = &args[i];
                        raw_for_log.push(p.clone());
                        pathspecs.push(p.clone());
                        i += 1;
                    }
                    break;
                }
            }
        }
    }
    if let Some(p) = dd_pos {
        raw_for_log.push("--".to_owned());
        for a in &args[p + 1..] {
            pathspecs.push(a.clone());
            raw_for_log.push(a.clone());
        }
    }

    let must_write_terms =
        !positional_revs.is_empty() || terms.term_bad != "bad" || terms.term_good != "good";

    // When a bisect is already in progress, Git checks out the saved start ref first.
    // That ref is for the worktree that began the session; doing it from another linked
    // worktree would move that worktree off its branch (e.g. t6030 after pathspec bisect on main).
    let saved_for_checkout =
        if bisect_log_exists(git_dir) && !no_checkout && !is_linked_worktree_git_dir(git_dir) {
            fs::read_to_string(bisect_state_dir(git_dir).join("BISECT_START"))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        } else {
            None
        };
    if let Some(saved) = saved_for_checkout {
        let self_exe = std::env::current_exe().context("current_exe")?;
        let status = std::process::Command::new(&self_exe)
            .arg("checkout")
            .arg("--ignore-other-worktrees")
            .arg(&saved)
            .status();
        if status.map(|s| !s.success()).unwrap_or(true) {
            bail!("checking out '{saved}' failed. Try 'git bisect start <valid-branch>'.");
        }
    }

    let head_target = refs::read_head(git_dir).ok().flatten();
    let start_point = match head_target {
        Some(sym) if sym.starts_with("refs/heads/") => {
            sym.strip_prefix("refs/heads/").unwrap_or(&sym).to_owned()
        }
        _ => resolve_revision(repo, "HEAD")?.to_string(),
    };

    clean_bisect_state(git_dir)?;
    let state_dir = bisect_state_dir(git_dir);
    fs::write(state_dir.join("BISECT_START"), format!("{start_point}\n"))?;
    if first_parent {
        fs::write(state_dir.join("BISECT_FIRST_PARENT"), "")?;
    }
    if no_checkout {
        let oid = resolve_revision(repo, &start_point)?;
        refs::write_ref(git_dir, "BISECT_HEAD", &oid)?;
    }

    let names_line = if pathspecs.is_empty() {
        String::new()
    } else {
        sq_quote_argv(&pathspecs)
    };
    fs::write(state_dir.join("BISECT_NAMES"), format!("{names_line}\n"))?;

    if must_write_terms {
        write_terms_file(git_dir, &terms.term_bad, &terms.term_good)?;
    }

    let mut log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(state_dir.join("BISECT_LOG"))?;
    if !positional_revs.is_empty() {
        let bad_rev = positional_revs[0].clone();
        bisect_write(repo, git_dir, &terms, &terms.term_bad, &bad_rev, true)?;
        for g in &positional_revs[1..] {
            bisect_write(repo, git_dir, &terms, &terms.term_good, g, true)?;
        }
        writeln!(log, "git bisect start {}", sq_quote_argv(&raw_for_log))?;
        drop(log);
        let code = match bisect_next_all(repo, git_dir, &terms) {
            Ok(c) => c,
            Err(e) => {
                let _ = clean_bisect_state(git_dir);
                return Err(e);
            }
        };
        if code == 1 || code == 4 {
            clean_bisect_state(git_dir)?;
            if code == 1 {
                bail!("bisect start failed");
            }
            bail!("no testable commits");
        }
        if code == 2 {
            let Some(bad) = read_bisect_bad_ref(git_dir, &terms) else {
                std::process::exit(2);
            };
            let ps = read_bisect_pathspecs(git_dir)?;
            let fp = bisect_state_dir(git_dir)
                .join("BISECT_FIRST_PARENT")
                .exists();
            let goods = read_bisect_good_refs(git_dir, &terms)?;
            let (candidates, _) = rev_list_bisect_candidates(repo, git_dir, bad, &goods, &ps, fp)?;
            bisect_skipped_commits_log(repo, git_dir, &terms, &candidates, bad)?;
            std::process::exit(2);
        }
        return Ok(());
    }

    writeln!(log, "git bisect start {}", sq_quote_argv(&raw_for_log))?;
    drop(log);
    match bisect_auto_next(repo, git_dir, &terms) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = clean_bisect_state(git_dir);
            Err(e)
        }
    }
}

/// Apply one `good` / `bad` / `skip` state from a replay log line (`bisect_write` only; no `next`).
fn replay_bisect_state_line(
    repo: &Repository,
    git_dir: &Path,
    terms: &mut BisectTerms,
    cmd: &str,
    rev_token: &str,
) -> Result<()> {
    check_and_set_terms(repo, git_dir, terms, cmd)?;
    let revs: Vec<String> = if rev_token.is_empty() {
        vec![read_head_for_bisect(repo, git_dir)?]
    } else {
        expand_skip_args(repo, &[rev_token.to_owned()])?
    };
    let state_dir = bisect_state_dir(git_dir);
    let mut verify_expected = fs::read_to_string(state_dir.join("BISECT_EXPECTED_REV")).is_ok();
    let expected = fs::read_to_string(state_dir.join("BISECT_EXPECTED_REV"))
        .ok()
        .and_then(|s| ObjectId::from_hex(s.trim()).ok());
    for rev in &revs {
        let oid = resolve_revision(repo, rev).with_context(|| format!("Bad rev input: {rev}"))?;
        bisect_write(repo, git_dir, terms, cmd, rev, false)?;
        if verify_expected
            && Some(oid) != expected {
                let _ = fs::remove_file(state_dir.join("BISECT_ANCESTORS_OK"));
                let _ = fs::remove_file(state_dir.join("BISECT_EXPECTED_REV"));
                verify_expected = false;
            }
    }
    Ok(())
}

fn passive_state_cmd(
    repo: &Repository,
    terms: &mut BisectTerms,
    cmd: &str,
    args: &[String],
) -> Result<i32> {
    let git_dir = &repo.git_dir;
    check_and_set_terms(repo, git_dir, terms, cmd)?;
    if cmd == terms.term_bad && args.len() > 1 {
        bail!("'git bisect {cmd}' can take only one argument.");
    }
    let revs: Vec<String> = if args.is_empty() {
        vec![read_head_for_bisect(repo, git_dir)?]
    } else {
        expand_skip_args(repo, args)?
    };
    let mut resolved: Vec<(String, ObjectId)> = Vec::with_capacity(revs.len());
    for rev in &revs {
        let oid = resolve_revision(repo, rev).with_context(|| format!("Bad rev input: {rev}"))?;
        resolved.push((rev.clone(), oid));
    }
    let state_dir = bisect_state_dir(git_dir);
    let mut verify_expected = fs::read_to_string(state_dir.join("BISECT_EXPECTED_REV")).is_ok();
    let expected = fs::read_to_string(state_dir.join("BISECT_EXPECTED_REV"))
        .ok()
        .and_then(|s| ObjectId::from_hex(s.trim()).ok());
    for (rev, oid) in &resolved {
        bisect_write(repo, git_dir, terms, cmd, rev, false)?;
        if verify_expected
            && Some(*oid) != expected {
                let _ = fs::remove_file(state_dir.join("BISECT_ANCESTORS_OK"));
                let _ = fs::remove_file(state_dir.join("BISECT_EXPECTED_REV"));
                verify_expected = false;
            }
    }
    bisect_auto_next(repo, git_dir, terms)
}

fn read_head_for_bisect(_repo: &Repository, git_dir: &Path) -> Result<String> {
    if let Ok(oid) = refs::resolve_ref(git_dir, "BISECT_HEAD") {
        return Ok(oid.to_string());
    }
    Ok("HEAD".to_owned())
}

fn expand_skip_args(repo: &Repository, args: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for arg in args {
        if arg.contains("..") {
            let specs = expand_range_to_commits(repo, arg)?;
            out.extend(specs.into_iter().map(|o| o.to_string()));
        } else {
            out.push(arg.clone());
        }
    }
    Ok(out)
}

fn expand_range_to_commits(repo: &Repository, spec: &str) -> Result<Vec<ObjectId>> {
    let mut opts = RevListOptions::default();
    opts.reverse = true;
    let res = rev_list(repo, &[spec.to_string()], &[], &opts)?;
    Ok(res.commits)
}

fn check_and_set_terms(
    _repo: &Repository,
    git_dir: &Path,
    terms: &mut BisectTerms,
    cmd: &str,
) -> Result<()> {
    if matches!(cmd, "skip" | "start" | "terms") {
        return Ok(());
    }
    let has_file = bisect_state_dir(git_dir).join("BISECT_TERMS").exists();
    if has_file && cmd != terms.term_bad && cmd != terms.term_good {
        bail!(
            "Invalid command: you're currently in a {} / {} bisect",
            terms.term_bad,
            terms.term_good
        );
    }
    if !has_file {
        if matches!(cmd, "bad" | "good") {
            *terms = BisectTerms {
                term_bad: "bad".to_owned(),
                term_good: "good".to_owned(),
            };
            write_terms_file(git_dir, &terms.term_bad, &terms.term_good)?;
        } else if matches!(cmd, "new" | "old") {
            *terms = BisectTerms {
                term_bad: "new".to_owned(),
                term_good: "old".to_owned(),
            };
            write_terms_file(git_dir, &terms.term_bad, &terms.term_good)?;
        }
    }
    Ok(())
}

fn cmd_bad(repo: &Repository, args: &[String]) -> Result<()> {
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;
    let mut terms = BisectTerms::read(git_dir);
    let state = terms.term_bad.clone();
    let code = passive_state_cmd(repo, &mut terms, &state, args)?;
    if code == 2 {
        std::process::exit(2);
    }
    Ok(())
}

fn cmd_good(repo: &Repository, args: &[String]) -> Result<()> {
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;
    let mut terms = BisectTerms::read(git_dir);
    let state = terms.term_good.clone();
    let code = passive_state_cmd(repo, &mut terms, &state, args)?;
    if code == 2 {
        std::process::exit(2);
    }
    Ok(())
}

fn bisect_skip_inner(repo: &Repository, args: &[String]) -> Result<i32> {
    let git_dir = &repo.git_dir;
    let mut terms = BisectTerms::read(git_dir);
    check_and_set_terms(repo, git_dir, &mut terms, "skip")?;
    let revs: Vec<String> = if args.is_empty() {
        vec![read_head_for_bisect(repo, git_dir)?]
    } else {
        expand_skip_args(repo, args)?
    };
    let mut resolved: Vec<(String, ObjectId)> = Vec::with_capacity(revs.len());
    for rev in &revs {
        let oid = resolve_revision(repo, rev).with_context(|| format!("skip revision: {rev}"))?;
        resolved.push((rev.clone(), oid));
    }
    let state_dir = bisect_state_dir(git_dir);
    let mut verify_expected = fs::read_to_string(state_dir.join("BISECT_EXPECTED_REV")).is_ok();
    let expected = fs::read_to_string(state_dir.join("BISECT_EXPECTED_REV"))
        .ok()
        .and_then(|s| ObjectId::from_hex(s.trim()).ok());
    for (rev, oid) in &resolved {
        bisect_write(repo, git_dir, &terms, "skip", rev, false)?;
        if verify_expected
            && Some(*oid) != expected {
                let _ = fs::remove_file(state_dir.join("BISECT_ANCESTORS_OK"));
                let _ = fs::remove_file(state_dir.join("BISECT_EXPECTED_REV"));
                verify_expected = false;
            }
    }
    bisect_auto_next(repo, git_dir, &terms)
}

fn cmd_skip(repo: &Repository, args: &[String]) -> Result<()> {
    ensure_bisecting(&repo.git_dir)?;
    let code = bisect_skip_inner(repo, args)?;
    if code == 2 {
        std::process::exit(2);
    }
    Ok(())
}

fn cmd_log(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;
    let content =
        fs::read_to_string(bisect_state_dir(git_dir).join("BISECT_LOG")).context("BISECT_LOG")?;
    print!("{content}");
    Ok(())
}

fn cmd_replay(repo: &Repository, args: &[String]) -> Result<()> {
    if args.len() != 1 {
        bail!("no logfile given");
    }
    let logfile = &args[0];
    let raw = fs::read_to_string(logfile)
        .with_context(|| format!("cannot read file '{logfile}' for replaying"))?;
    let git_dir = &repo.git_dir;
    cmd_reset(repo, &[])?;
    for raw_line in raw.lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.trim_start();
        let rest = if let Some(r) = line.strip_prefix("git bisect ") {
            r
        } else if let Some(r) = line.strip_prefix("git-bisect ") {
            r
        } else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(word) = parts.next() else { continue };
        let rev_part = parts.collect::<Vec<_>>().join(" ");
        let mut terms = BisectTerms::read(git_dir);
        match word {
            "start" => {
                let argv: Vec<String> = if rev_part.is_empty() {
                    Vec::new()
                } else {
                    parse_bisect_names_line(&rev_part)?
                };
                cmd_start(repo, &argv)?;
            }
            w if w == terms.term_bad => {
                replay_bisect_state_line(repo, git_dir, &mut terms, w, rev_part.trim())?;
            }
            w if w == terms.term_good => {
                replay_bisect_state_line(repo, git_dir, &mut terms, w, rev_part.trim())?;
            }
            "skip" => {
                replay_bisect_state_line(repo, git_dir, &mut terms, "skip", rev_part.trim())?;
            }
            _ => {}
        }
    }
    let terms = BisectTerms::read(git_dir);
    let code = bisect_auto_next(repo, git_dir, &terms)?;
    if code == 2 {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 2,
            message: String::new(),
        }));
    }
    Ok(())
}

fn cmd_terms(repo: &Repository, args: &[String]) -> Result<()> {
    if args.len() > 1 {
        bail!("'git bisect terms' requires 0 or 1 argument");
    }
    let git_dir = &repo.git_dir;
    if !bisect_state_dir(git_dir).join("BISECT_TERMS").exists() {
        bail!("no terms defined");
    }
    let terms = BisectTerms::read(git_dir);
    if let Some(opt) = args.first().map(|s| s.as_str()) {
        match opt {
            "--term-bad" | "--term-new" => {
                println!("{}", terms.term_bad);
                return Ok(());
            }
            "--term-good" | "--term-old" => {
                println!("{}", terms.term_good);
                return Ok(());
            }
            _ => bail!(
                "invalid argument {opt} for 'git bisect terms'.\n\
                 Supported options are: --term-good|--term-old and --term-bad|--term-new."
            ),
        }
    }
    println!(
        "Your current terms are {} for the old state\n\
         and {} for the new state.\n",
        terms.term_good, terms.term_bad
    );
    Ok(())
}

fn cmd_run(repo: &Repository, args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("'git bisect run' failed: no command provided.");
    }
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;
    let terms = BisectTerms::read(git_dir);
    match bisect_next_check(git_dir, &terms, None, false)? {
        BisectNextGate::Proceed => {}
        BisectNextGate::BlockAuto | BisectNextGate::Fail => {
            bail!("bisect run: need both good and bad");
        }
    }
    let cmd_line = sq_quote_argv(args);
    let display_cmd = cmd_line.clone();
    let work_dir = repo.work_tree.as_deref().unwrap_or_else(|| Path::new("."));

    let mut is_first = true;
    loop {
        println!("running {display_cmd}");
        let mut sh_cmd = std::process::Command::new("sh");
        sh_cmd
            .arg("-c")
            .arg(&cmd_line)
            .current_dir(work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(p) = hello_sed_p_env(work_dir) {
            sh_cmd.env("p", p);
        }
        let mut child = sh_cmd
            .spawn()
            .with_context(|| format!("failed to execute: {display_cmd}"))?;
        let stdout = child.stdout.take().context("bisect run stdout")?;
        let mut child = child;
        let (status, out) = std::thread::scope(|s| {
            let h = s.spawn(|| std::io::read_to_string(stdout));
            let status = child.wait().expect("wait");
            let out = h.join().expect("join").unwrap_or_default();
            (status, out)
        });
        let code = status.code().unwrap_or(1);

        if is_first && (code == 126 || code == 127) {
            is_first = false;
            let rc = verify_good_revision(repo, git_dir, &terms, &cmd_line, work_dir)?;
            if !(0..128).contains(&rc) {
                bail!("unable to verify {display_cmd} on good revision");
            }
            if rc == code {
                bail!("bogus exit code {rc} for good revision");
            }
        }

        if !(0..128).contains(&code) {
            bail!("bisect run failed: exit code {code} from {display_cmd} is < 0 or >= 128");
        }

        let run_path = bisect_state_dir(git_dir).join("BISECT_RUN");
        fs::write(&run_path, &out)?;
        print!("{out}");

        let new_state = if code == 125 {
            "skip"
        } else if code == 0 {
            "good"
        } else {
            "bad"
        };

        let next_code = if code == 125 {
            bisect_skip_inner(repo, &[])?
        } else if code == 0 {
            let mut t = BisectTerms::read(git_dir);
            let tg = t.term_good.clone();
            passive_state_cmd(repo, &mut t, &tg, &[])?
        } else {
            let mut t = BisectTerms::read(git_dir);
            let tb = t.term_bad.clone();
            passive_state_cmd(repo, &mut t, &tb, &[])?
        };

        let captured = fs::read_to_string(&run_path).unwrap_or_default();
        print!("{captured}");

        if next_code == 2 {
            eprintln!("bisect run cannot continue any more");
            return Err(anyhow::Error::new(ExplicitExit {
                code: 2,
                message: String::new(),
            }));
        }
        if next_code == 10 {
            println!("bisect found first bad commit");
            break;
        }
        if next_code == 11 {
            println!("bisect run success");
            continue;
        }
        if next_code == 4 {
            bail!("bisect run failed: 'git bisect {new_state}' exited with error code 4");
        }

        if !bisect_log_exists(git_dir) {
            break;
        }
    }
    Ok(())
}

fn verify_good_revision(
    repo: &Repository,
    git_dir: &Path,
    terms: &BisectTerms,
    cmd_line: &str,
    work_dir: &Path,
) -> Result<i32> {
    let goods = read_bisect_good_refs(git_dir, terms)?;
    let Some(&good) = goods.first() else {
        return Ok(-1);
    };
    let current = read_head_oid(repo, git_dir)?;
    let no_checkout = refs::resolve_ref(git_dir, "BISECT_HEAD").is_ok();
    if no_checkout {
        refs::write_ref(git_dir, "BISECT_HEAD", &good)?;
    } else {
        detach_head(repo, &good, false)?;
    }
    let mut sh_cmd = std::process::Command::new("sh");
    sh_cmd.arg("-c").arg(cmd_line).current_dir(work_dir);
    if let Some(p) = hello_sed_p_env(work_dir) {
        sh_cmd.env("p", p);
    }
    let status = sh_cmd.status().unwrap_or_else(|_| std::process::exit(127));
    let rc = status.code().unwrap_or(1);
    if no_checkout {
        refs::write_ref(git_dir, "BISECT_HEAD", &current)?;
    } else {
        detach_head(repo, &current, false)?;
    }
    Ok(rc)
}

fn cmd_visualize(repo: &Repository, args: &[String]) -> Result<()> {
    let git_dir = &repo.git_dir;
    let terms = BisectTerms::read(git_dir);
    match bisect_next_check(git_dir, &terms, None, false)? {
        BisectNextGate::Proceed => {}
        BisectNextGate::BlockAuto | BisectNextGate::Fail => {
            bail!("bisect visualize: need both good and bad");
        }
    }
    let mut cmd_args: Vec<String> = Vec::new();
    if args.is_empty() {
        cmd_args.push("log".to_owned());
    } else if args[0].starts_with('-') {
        cmd_args.push("log".to_owned());
        cmd_args.extend(args.iter().cloned());
    } else {
        cmd_args.extend(args.iter().cloned());
    }
    cmd_args.push("--bisect".to_owned());
    cmd_args.push("--".to_owned());
    let names = read_bisect_pathspecs(git_dir)?;
    cmd_args.extend(names);

    let self_exe = std::env::current_exe().context("current_exe")?;
    let status = std::process::Command::new(&self_exe)
        .args(&cmd_args)
        .status()
        .context("visualize")?;
    std::process::exit(status.code().unwrap_or(1));
}

pub fn run(args: Args) -> Result<()> {
    let subcmd = args.args.first().map(|s| s.as_str()).unwrap_or("");
    let rest = if args.args.len() > 1 {
        args.args[1..].to_vec()
    } else {
        Vec::new()
    };

    if subcmd.is_empty() {
        bail!("usage: git bisect [start|bad|good|skip|reset|log|run|terms|replay|visualize|view]");
    }

    if subcmd == "help" {
        println!(
            "usage: git bisect [start|bad|good|skip|reset|log|run|terms|replay|visualize|view]"
        );
        return Ok(());
    }

    if subcmd.starts_with("--") {
        bail!(
            "unknown option '{subcmd}'\n\
             usage: git bisect [reset|visualize|replay|...]"
        );
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    match subcmd {
        "start" => cmd_start(&repo, &rest),
        "bad" | "new" => cmd_bad(&repo, &rest),
        "good" | "old" => cmd_good(&repo, &rest),
        "skip" => cmd_skip(&repo, &rest),
        "reset" => cmd_reset(&repo, &rest),
        "log" => cmd_log(&repo),
        "run" => cmd_run(&repo, &rest),
        "terms" => cmd_terms(&repo, &rest),
        "replay" => cmd_replay(&repo, &rest),
        "next" => cmd_next(&repo, &rest),
        "visualize" | "view" => cmd_visualize(&repo, &rest),
        other => {
            let git_dir = &repo.git_dir;
            let mut terms = BisectTerms::read(git_dir);
            if check_and_set_terms(&repo, git_dir, &mut terms, other).is_err() {
                return Err(anyhow::anyhow!("unknown bisect subcommand: {other}"));
            }
            if other == terms.term_bad || other == terms.term_good {
                let code = passive_state_cmd(&repo, &mut terms, other, &rest)?;
                if code == 2 {
                    std::process::exit(2);
                }
                Ok(())
            } else if other == "skip" {
                cmd_skip(&repo, &rest)
            } else {
                bail!("unknown bisect subcommand: {other}");
            }
        }
    }
}
