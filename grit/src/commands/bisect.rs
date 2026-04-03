//! `grit bisect` — binary search to find the commit that introduced a bug.
//!
//! Subcommands:
//! - `bisect start [<bad> [<good>...]]` — begin bisecting
//! - `bisect bad [<rev>]` — mark a commit as bad
//! - `bisect good [<rev>]` — mark a commit as good
//! - `bisect skip [<rev>...]` — skip one or more commits
//! - `bisect reset [<branch>]` — end bisect session
//! - `bisect log` — show bisect log
//!
//! State is stored in:
//! - `.git/BISECT_LOG` — log of bisect commands
//! - `.git/BISECT_START` — the branch/commit we started from
//! - `.git/BISECT_EXPECTED_REV` — the commit we expect HEAD to be on
//! - `.git/refs/bisect/bad` — the current bad commit
//! - `.git/refs/bisect/good-*` — the good commits
//! - `.git/refs/bisect/skip-*` — the skipped commits

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Arguments for `grit bisect`.
#[derive(Debug, ClapArgs)]
#[command(about = "Use binary search to find the commit that introduced a bug")]
pub struct Args {
    /// Bisect subcommand and its arguments.
    #[arg(value_name = "SUBCOMMAND", num_args = 0.., trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    let subcmd = args.args.first().map(|s| s.as_str()).unwrap_or("help");
    let rest = if args.args.len() > 1 {
        &args.args[1..]
    } else {
        &[]
    };

    match subcmd {
        "start" => cmd_start(rest),
        "bad" | "new" => cmd_bad(rest),
        "good" | "old" => cmd_good(rest),
        "skip" => cmd_skip(rest),
        "reset" => cmd_reset(rest),
        "log" => cmd_log(),
        "run" => cmd_run(rest),
        "terms" => cmd_terms(),
        "replay" => cmd_replay(rest),
        "help" => {
            println!("usage: git bisect [start|bad|good|skip|reset|log|run|terms|replay]");
            Ok(())
        }
        other => bail!("unknown bisect subcommand: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn cmd_start(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    // If already bisecting, clean up first.
    if git_dir.join("BISECT_LOG").exists() {
        clean_bisect_state(git_dir)?;
    }

    // Save the branch/commit we started from so reset can restore it.
    let start_point = match refs::read_head(git_dir) {
        Ok(Some(symref)) => symref
            .strip_prefix("refs/heads/")
            .unwrap_or(&symref)
            .to_owned(),
        _ => {
            // Detached HEAD — save the commit hash.
            let head_content = fs::read_to_string(git_dir.join("HEAD"))
                .context("cannot read HEAD")?;
            head_content.trim().to_owned()
        }
    };
    fs::write(git_dir.join("BISECT_START"), format!("{start_point}\n"))?;

    // Initialise the log file.
    let mut log_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(git_dir.join("BISECT_LOG"))?;

    // Parse optional <bad> [<good>...] from args.
    if !args.is_empty() {
        // First arg is bad.
        let bad_rev = &args[0];
        let bad_oid = resolve_revision(&repo, bad_rev)
            .with_context(|| format!("bad revision: {bad_rev}"))?;
        refs::write_ref(git_dir, "refs/bisect/bad", &bad_oid)?;
        writeln!(log_file, "# bad: [{bad_oid}] {bad_rev}")?;
        writeln!(log_file, "git bisect bad {bad_oid}")?;

        // Remaining args are good.
        for good_rev in &args[1..] {
            let good_oid = resolve_revision(&repo, good_rev)
                .with_context(|| format!("good revision: {good_rev}"))?;
            let ref_name = format!("refs/bisect/good-{good_oid}");
            refs::write_ref(git_dir, &ref_name, &good_oid)?;
            writeln!(log_file, "# good: [{good_oid}] {good_rev}")?;
            writeln!(log_file, "git bisect good {good_oid}")?;
        }

        drop(log_file);
        return bisect_next(&repo);
    }

    drop(log_file);
    println!(
        "status: waiting for both good and bad commits\n\
         usage: git bisect bad <rev>\n\
         usage: git bisect good <rev>"
    );
    Ok(())
}

fn cmd_bad(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;

    let rev = if args.is_empty() {
        "HEAD".to_owned()
    } else {
        args[0].clone()
    };

    let oid = resolve_revision(&repo, &rev)
        .with_context(|| format!("bad revision: {rev}"))?;
    refs::write_ref(git_dir, "refs/bisect/bad", &oid)?;
    append_bisect_log(git_dir, &format!("# bad: [{oid}] {rev}"))?;
    append_bisect_log(git_dir, &format!("git bisect bad {oid}"))?;

    bisect_next(&repo)
}

fn cmd_good(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;

    let revs = if args.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        args.to_vec()
    };

    for rev in &revs {
        let oid = resolve_revision(&repo, rev)
            .with_context(|| format!("good revision: {rev}"))?;
        let ref_name = format!("refs/bisect/good-{oid}");
        refs::write_ref(git_dir, &ref_name, &oid)?;
        append_bisect_log(git_dir, &format!("# good: [{oid}] {rev}"))?;
        append_bisect_log(git_dir, &format!("git bisect good {oid}"))?;
    }

    bisect_next(&repo)
}

fn cmd_skip(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;

    let revs = if args.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        args.to_vec()
    };

    for rev in &revs {
        let oid = resolve_revision(&repo, rev)
            .with_context(|| format!("skip revision: {rev}"))?;
        let ref_name = format!("refs/bisect/skip-{oid}");
        refs::write_ref(git_dir, &ref_name, &oid)?;
        append_bisect_log(git_dir, &format!("# skip: [{oid}] {rev}"))?;
        append_bisect_log(git_dir, &format!("git bisect skip {oid}"))?;
    }

    bisect_next(&repo)
}

fn cmd_reset(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("BISECT_LOG").exists() {
        println!("We are not bisecting.");
        return Ok(());
    }

    // Determine what to checkout: explicit arg or saved start point.
    let checkout_target = if !args.is_empty() {
        args[0].clone()
    } else {
        let start_path = git_dir.join("BISECT_START");
        match fs::read_to_string(&start_path) {
            Ok(s) => s.trim().to_owned(),
            Err(_) => "HEAD".to_owned(),
        }
    };

    clean_bisect_state(git_dir)?;

    // Use git checkout to restore the branch.
    let status = std::process::Command::new(
        std::env::var_os("REAL_GIT").unwrap_or_else(|| std::ffi::OsString::from("/usr/bin/git")),
    )
    .arg("checkout")
    .arg(&checkout_target)
    .arg("--")
    .status()
    .context("failed to run git checkout")?;

    if !status.success() {
        bail!("git checkout {checkout_target} failed");
    }

    println!("Previous HEAD position was bisect state.");
    println!("Switched to branch '{checkout_target}'");
    Ok(())
}

fn cmd_log() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;

    let log_path = git_dir.join("BISECT_LOG");
    let content = fs::read_to_string(&log_path).context("cannot read BISECT_LOG")?;
    print!("{content}");
    Ok(())
}

fn cmd_run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("bisect run failed: no command provided.");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    ensure_bisecting(git_dir)?;

    let cmd_str = args.join(" ");
    let work_dir = repo
        .work_tree
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("."));

    loop {
        // Read the bad ref — if missing, bisect is not ready.
        if read_bisect_ref(git_dir, "refs/bisect/bad").is_none() {
            bail!("bisect run requires a bad commit to be marked");
        }
        if read_bisect_good_refs(git_dir)?.is_empty() {
            bail!("bisect run requires at least one good commit to be marked");
        }

        // Run the user command.
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .current_dir(work_dir)
            .status()
            .with_context(|| format!("failed to execute: {cmd_str}"))?;

        let code = status.code().unwrap_or(1);

        // Exit code meaning (git convention):
        //   0       = good
        //   1-124   = bad
        //   125     = skip
        //   126-127 = run error, abort bisect
        //   128+    = signal/fatal, abort
        //   <0      = signal, abort
        if code < 0 || code == 126 || code == 127 || code >= 128 {
            eprintln!(
                "bisect run failed:\nexit code {code} from '{}' is fatal, aborting.",
                cmd_str
            );
            return Ok(());
        }

        if code == 125 {
            // skip
            cmd_skip(&[])?;
        } else if code == 0 {
            // good
            cmd_good(&[])?;
        } else {
            // bad (1-124)
            cmd_bad(&[])?;
        }

        // Check if bisect is done (BISECT_LOG removed by reset, or
        // the first-bad-commit message was printed).
        // We detect done-ness by checking if the bad commit has been found:
        // after bisect_next finds the culprit, it prints the result and returns.
        // We need to check if there's still a bisect in progress.
        if !git_dir.join("BISECT_LOG").exists() {
            break;
        }

        // Check if bisect has converged: re-read state and see if
        // there's only one candidate left.
        let new_bad = match read_bisect_ref(git_dir, "refs/bisect/bad") {
            Some(oid) => oid,
            None => break,
        };
        let new_goods = read_bisect_good_refs(git_dir)?;
        if new_goods.is_empty() {
            break;
        }
        let candidates = find_bisect_candidates(&repo, new_bad, &new_goods)?;
        if candidates.is_empty() {
            // Already converged (printed by bisect_next)
            break;
        }

        // Filter skipped
        let skip_oids = read_bisect_skip_refs(git_dir)?;
        let unskipped: Vec<ObjectId> = candidates
            .iter()
            .copied()
            .filter(|oid| !skip_oids.contains(oid))
            .collect();

        if unskipped.len() <= 1 {
            break;
        }
    }

    Ok(())
}

fn cmd_terms() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    // Read custom terms if they exist, otherwise use defaults.
    let (term_bad, term_good) = read_bisect_terms(git_dir);
    println!("Your current terms are {} for the old state and {} for the new state.", term_good, term_bad);
    Ok(())
}

fn cmd_replay(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("no logfile given");
    }

    let logfile = &args[0];
    let content = fs::read_to_string(logfile)
        .with_context(|| format!("cannot open file '{}' for replaying", logfile))?;

    // Reset any current bisect state first.
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;
    if git_dir.join("BISECT_LOG").exists() {
        clean_bisect_state(git_dir)?;
    }

    // Auto-start a bisect session for replay.
    cmd_start(&[])?;

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Lines look like: git bisect <subcmd> <args...>
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 || parts[0] != "git" || parts[1] != "bisect" {
            continue;
        }

        let subcmd = parts[2];
        let rest_args: Vec<String> = parts[3..].iter().map(|s| s.to_string()).collect();

        match subcmd {
            "start" => { /* already started */ }
            "bad" | "new" => cmd_bad(&rest_args)?,
            "good" | "old" => cmd_good(&rest_args)?,
            "skip" => cmd_skip(&rest_args)?,
            _ => {
                // Ignore unknown commands in replay
            }
        }
    }

    Ok(())
}

/// Read custom bisect terms from .git/BISECT_TERMS or return defaults.
fn read_bisect_terms(git_dir: &Path) -> (String, String) {
    let terms_path = git_dir.join("BISECT_TERMS");
    if let Ok(content) = fs::read_to_string(&terms_path) {
        let mut lines = content.lines();
        let bad = lines.next().unwrap_or("bad").trim().to_owned();
        let good = lines.next().unwrap_or("good").trim().to_owned();
        (bad, good)
    } else {
        ("bad".to_owned(), "good".to_owned())
    }
}

// ---------------------------------------------------------------------------
// Core bisect logic
// ---------------------------------------------------------------------------

/// Compute the next bisect step: find the midpoint between good and bad,
/// check it out, and report progress.
fn bisect_next(repo: &Repository) -> Result<()> {
    let git_dir = &repo.git_dir;

    // Read the bad ref.
    let bad_oid = match read_bisect_ref(git_dir, "refs/bisect/bad") {
        Some(oid) => oid,
        None => {
            println!("status: waiting for bad commit, 1 good commit known");
            return Ok(());
        }
    };

    // Read all good refs.
    let good_oids = read_bisect_good_refs(git_dir)?;
    if good_oids.is_empty() {
        println!(
            "status: waiting for good commit(s), bad commit known"
        );
        return Ok(());
    }

    // Read skipped refs.
    let skip_oids = read_bisect_skip_refs(git_dir)?;

    // Walk from bad backwards, stopping at good commits, to find the
    // candidate set of commits to bisect through.
    let candidates = find_bisect_candidates(repo, bad_oid, &good_oids)?;

    if candidates.is_empty() {
        // bad is itself a good ancestor — the first bad commit is bad_oid
        print_bisect_result(repo, bad_oid)?;
        return Ok(());
    }

    // Filter out skipped commits for midpoint selection but keep them for counting.
    let unskipped: Vec<ObjectId> = candidates
        .iter()
        .copied()
        .filter(|oid| !skip_oids.contains(oid))
        .collect();

    if unskipped.is_empty() {
        println!(
            "There are only 'skip'ped commits left to test.\n\
             The first bad commit could be any of:"
        );
        for oid in &candidates {
            println!("{oid}");
        }
        println!("We cannot bisect more!");
        return Ok(());
    }

    // If only one candidate remains, we found it.
    if unskipped.len() == 1 {
        print_bisect_result(repo, bad_oid)?;
        return Ok(());
    }

    // Pick the midpoint.
    let mid_idx = unskipped.len() / 2;
    let mid_oid = unskipped[mid_idx];

    // Write BISECT_EXPECTED_REV so status knows what we expect.
    fs::write(
        git_dir.join("BISECT_EXPECTED_REV"),
        format!("{mid_oid}\n"),
    )?;

    // Checkout the midpoint using git.
    let status = std::process::Command::new(
        std::env::var_os("REAL_GIT").unwrap_or_else(|| std::ffi::OsString::from("/usr/bin/git")),
    )
    .arg("checkout")
    .arg(mid_oid.to_hex())
    .arg("--detach")
    .stderr(std::process::Stdio::null())
    .status()
    .context("failed to run git checkout")?;

    if !status.success() {
        bail!("git checkout {} failed", mid_oid);
    }

    let remaining = unskipped.len() - 1; // excluding the midpoint itself
    let steps = (remaining as f64).log2().ceil() as usize;
    println!(
        "Bisecting: {} revision{} left to test after this (roughly {} step{}).",
        remaining,
        if remaining == 1 { "" } else { "s" },
        steps,
        if steps == 1 { "" } else { "s" },
    );
    println!("[{}]", mid_oid);

    Ok(())
}

/// Print the final bisect result.
fn print_bisect_result(repo: &Repository, bad_oid: ObjectId) -> Result<()> {
    let object = repo.odb.read(&bad_oid)?;
    let commit = parse_commit(&object.data)?;
    let subject = commit.message.lines().next().unwrap_or("");
    println!("{bad_oid} is the first bad commit");
    println!();
    println!("commit {bad_oid}");
    println!("Author: {}", commit.author);
    println!();
    println!("    {subject}");
    Ok(())
}

/// Walk from `bad` backwards through parents, collecting all commits that
/// are reachable from `bad` but NOT reachable from any of the `good` commits.
/// Returns them in topological order (bad-first / newest-first).
fn find_bisect_candidates(
    repo: &Repository,
    bad: ObjectId,
    goods: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    // First, compute the set of commits reachable from good refs.
    let mut good_set = HashSet::new();
    {
        let mut queue = VecDeque::new();
        for &g in goods {
            queue.push_back(g);
        }
        while let Some(oid) = queue.pop_front() {
            if !good_set.insert(oid) {
                continue;
            }
            let object = repo.odb.read(&oid)?;
            if object.kind != ObjectKind::Commit {
                continue;
            }
            let commit = parse_commit(&object.data)?;
            for parent in commit.parents {
                queue.push_back(parent);
            }
        }
    }

    // Now walk from bad, stopping at good commits.
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(bad);
    while let Some(oid) = queue.pop_front() {
        if !seen.insert(oid) {
            continue;
        }
        if good_set.contains(&oid) {
            continue;
        }
        candidates.push(oid);
        let object = repo.odb.read(&oid)?;
        if object.kind != ObjectKind::Commit {
            continue;
        }
        let commit = parse_commit(&object.data)?;
        for parent in commit.parents {
            queue.push_back(parent);
        }
    }

    // candidates[0] is bad itself; the rest are in BFS order.
    // Remove bad itself — the bisect midpoint should be among the remaining.
    if !candidates.is_empty() {
        candidates.remove(0);
    }

    Ok(candidates)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_bisecting(git_dir: &Path) -> Result<()> {
    if !git_dir.join("BISECT_LOG").exists() {
        bail!(
            "You need to start by \"git bisect start\".\n\
             You then need to give me at least one good and one bad revision."
        );
    }
    Ok(())
}

fn read_bisect_ref(git_dir: &Path, refname: &str) -> Option<ObjectId> {
    let path = git_dir.join(refname);
    let content = fs::read_to_string(&path).ok()?;
    ObjectId::from_hex(content.trim()).ok()
}

fn read_bisect_good_refs(git_dir: &Path) -> Result<Vec<ObjectId>> {
    let bisect_dir = git_dir.join("refs/bisect");
    let mut goods = Vec::new();
    let entries = match fs::read_dir(&bisect_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(goods),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("good-") {
            let content = fs::read_to_string(entry.path())?;
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                goods.push(oid);
            }
        }
    }
    Ok(goods)
}

fn read_bisect_skip_refs(git_dir: &Path) -> Result<HashSet<ObjectId>> {
    let bisect_dir = git_dir.join("refs/bisect");
    let mut skips = HashSet::new();
    let entries = match fs::read_dir(&bisect_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(skips),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("skip-") {
            let content = fs::read_to_string(entry.path())?;
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                skips.insert(oid);
            }
        }
    }
    Ok(skips)
}

fn append_bisect_log(git_dir: &Path, line: &str) -> Result<()> {
    let log_path = git_dir.join("BISECT_LOG");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn clean_bisect_state(git_dir: &Path) -> Result<()> {
    // Remove bisect refs.
    let bisect_dir = git_dir.join("refs/bisect");
    if bisect_dir.is_dir() {
        fs::remove_dir_all(&bisect_dir)?;
    }

    // Remove state files.
    for name in &[
        "BISECT_LOG",
        "BISECT_START",
        "BISECT_EXPECTED_REV",
        "BISECT_NAMES",
    ] {
        let _ = fs::remove_file(git_dir.join(name));
    }

    Ok(())
}
