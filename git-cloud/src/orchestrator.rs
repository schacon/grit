//! Initialize the task database, sync from CSV, run the cloud agent supervisor loop, and integrate finished work.

use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::ansi::{BLUE, BOLD, CYAN, DIM, GREEN, RED, RESET, YELLOW};
use crate::cursor;
use crate::db::{self, TaskRow, TaskStatus};
use crate::git_ops;
use crate::harness;

const DEFAULT_POLL_SECS: u64 = 10;
const DEFAULT_MAX_CONCURRENT: i64 = 5;
const DEFAULT_GIT_REF: &str = "main";

const DEFAULT_MERGE_CMD: &str = r#"agent chat "Resolve any git merge conflicts in this repository. Edit conflicted files, run git add on them, then ensure the merge can complete. Follow AGENTS.md for this project.""#;

fn git_dir(repo: &Path) -> std::path::PathBuf {
    repo.join(".git")
}

pub fn init_db(repo: &Path, force: bool) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if db_path.exists() && !force {
        anyhow::bail!(
            "{} already exists (use --force to recreate)",
            db_path.display()
        );
    }
    if force && db_path.exists() {
        std::fs::remove_file(&db_path).with_context(|| format!("remove {}", db_path.display()))?;
    }

    let conn = db::open_db(&gd)?;
    db::init_schema(&conn)?;
    db::migrate_schema(&conn)?;

    let rows = harness::read_test_files_csv(repo)?;
    let mut inserted = 0usize;
    for r in harness::incomplete_in_scope_rows(&rows) {
        db::insert_task(&conn, &r.file, &r.group, r.tests_total, r.passed_last)?;
        inserted += 1;
    }

    println!(
        "{}{}git-cloud{}: seeded {} task(s) into {}{}",
        BOLD,
        GREEN,
        RESET,
        inserted,
        db_path.display(),
        RESET
    );
    Ok(())
}

/// For every CSV row with `fully_passing=true`, mark the matching SQLite task as `completed`
/// and refresh `tests_total` / `tests_passing` from the CSV. Rows with no matching task are skipped.
pub fn sync_completed_from_csv(repo: &Path) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if !db_path.is_file() {
        anyhow::bail!(
            "missing {} — run `git-cloud --init` first",
            db_path.display()
        );
    }

    let conn = db::open_db(&gd)?;
    db::init_schema(&conn)?;
    db::migrate_schema(&conn)?;
    let rows = harness::read_test_files_csv(repo)?;
    let mut rows_updated = 0usize;
    for r in rows {
        if !r.fully_passing {
            continue;
        }
        let n = conn
            .execute(
                r"UPDATE tasks SET status = 'completed', cloud_id = NULL,
                    tests_total = ?1, tests_passing = ?2
                  WHERE filename = ?3",
                params![r.tests_total, r.passed_last, r.file],
            )
            .with_context(|| format!("mark completed for {}", r.file))?;
        if n > 0 {
            rows_updated += 1;
        }
    }

    println!(
        "{}{}git-cloud sync-from-csv{}: marked {} task row(s) completed from {}{}",
        BOLD,
        GREEN,
        RESET,
        rows_updated,
        repo.join("data/test-files.csv").display(),
        RESET
    );
    Ok(())
}

/// Re-run `./scripts/run-tests.sh` for each **`failed`** task (merge/pipeline failure), re-read
/// `data/test-files.csv`, and mark tasks `completed` when the CSV reports `fully_passing`.
///
/// Does not contact Cursor Cloud; safe to use while agents are idle or to refresh local harness state.
pub fn update_harness(repo: &Path) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if !db_path.is_file() {
        anyhow::bail!(
            "missing {} — run `git-cloud --init` first",
            db_path.display()
        );
    }
    let mut conn = db::open_db(&gd)?;
    let failed = db::list_failed_tasks(&conn)?;
    println!(
        "{}{}git-cloud update{} — {} failed task(s){}\n",
        BOLD,
        CYAN,
        RESET,
        failed.len(),
        RESET
    );
    reverify_tasks_for_harness(&mut conn, repo, failed)?;
    let (p, r, fin, c, integ, f, canc) = db::summary_counts(&conn)?;
    println!(
        "{}{}Done.{} pending={} running={} finished={} completed={} integrated={} failed={} cancelled={}{}",
        BOLD, GREEN, RESET, p, r, fin, c, integ, f, canc, RESET
    );
    Ok(())
}

/// Run `./scripts/run-tests.sh` once with all test stems from SQLite whose status is not `pending`
/// or `running`. Rows with `in_scope=skip` in `data/test-files.csv` are omitted.
pub fn rerun_harness(repo: &Path) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if !db_path.is_file() {
        anyhow::bail!(
            "missing {} — run `git-cloud --init` first",
            db_path.display()
        );
    }
    let conn = db::open_db(&gd)?;
    db::init_schema(&conn)?;
    db::migrate_schema(&conn)?;
    let stems = db::list_filenames_not_pending_or_running(&conn)?;
    let csv_rows = harness::read_test_files_csv(repo)?;
    let mut args: Vec<String> = Vec::new();
    for stem in stems {
        let Some(row) = harness::row_for_file(&csv_rows, &stem) else {
            continue;
        };
        if row.in_scope == "skip" {
            continue;
        }
        args.push(format!("{}.sh", stem));
    }
    if args.is_empty() {
        println!(
            "{}{}git-cloud rerun{} — no harness files to run (all tasks pending/running, or none in CSV scope){}",
            BOLD, CYAN, RESET, RESET
        );
        return Ok(());
    }
    let script = repo.join("scripts/run-tests.sh");
    if !script.is_file() {
        anyhow::bail!("missing {}", script.display());
    }
    println!(
        "{}{}git-cloud rerun{} — {} harness file(s) via {}{}",
        BOLD,
        CYAN,
        RESET,
        args.len(),
        script.display(),
        RESET
    );
    let status = std::process::Command::new("bash")
        .current_dir(repo)
        .arg(&script)
        .args(&args)
        .status()
        .with_context(|| format!("run {}", script.display()))?;
    if !status.success() {
        anyhow::bail!("{} exited with {}", script.display(), status);
    }
    Ok(())
}

/// Print task counts by status from `.git/cloud.sqlite`.
pub fn summary(repo: &Path) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if !db_path.is_file() {
        anyhow::bail!(
            "missing {} — run `git-cloud --init` first",
            db_path.display()
        );
    }
    let conn = db::open_db(&gd)?;
    let (pending, running, finished, completed, integrated, failed, cancelled) =
        db::summary_counts(&conn)?;
    println!(
        "{}{}git-cloud summary{} {}{}",
        BOLD,
        CYAN,
        RESET,
        db_path.display(),
        RESET
    );
    println!("  pending:    {}", pending);
    println!("  running:    {}", running);
    println!("  finished:   {} (awaiting integrate)", finished);
    println!("  completed:  {}", completed);
    println!("  integrated: {}", integrated);
    println!("  failed:     {}", failed);
    println!("  cancelled:  {}", cancelled);
    Ok(())
}

/// For each task in `finished` status, fetch the cloud agent branch, merge into `main` (retrying the
/// merge agent until the merge completes), run the harness file, commit/push, then mark `integrated`.
pub fn integrate_loop(repo: &Path) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if !db_path.is_file() {
        anyhow::bail!(
            "missing {} — run `git-cloud --init` first",
            db_path.display()
        );
    }

    let mut conn = db::open_db(&gd)?;
    db::init_schema(&conn)?;
    db::migrate_schema(&conn)?;
    cursor::verify_auth()?;

    println!(
        "{}{}git-cloud integrate{} — merge finished tasks, run harness, push{}\n",
        BOLD, CYAN, RESET, RESET
    );

    loop {
        let tasks = db::list_finished_tasks(&conn)?;
        if tasks.is_empty() {
            println!(
                "{}{}No tasks in `finished` status — nothing to integrate.{}",
                DIM, RESET, RESET
            );
            return Ok(());
        }
        let t = &tasks[0];
        println!(
            "{}Integrating {} (cloud id {}){}",
            GREEN,
            t.filename,
            t.cloud_id.as_deref().unwrap_or("?"),
            RESET
        );
        let info = match cursor::get_agent(t.cloud_id.as_deref().unwrap_or("")) {
            Ok(i) => i,
            Err(e) => {
                println!(
                    "{}Could not load agent metadata for {}: {}{}",
                    RED, t.filename, e, RESET
                );
                db::set_failed(&conn, t.id)?;
                continue;
            }
        };
        match integrate_task(&mut conn, repo, t, &info) {
            Ok(()) => {
                println!("{}Integrated {}{}", GREEN, t.filename, RESET);
            }
            Err(e) => {
                println!(
                    "{}integrate pipeline failed for {}: {}{}",
                    RED, t.filename, e, RESET
                );
                db::set_failed(&conn, t.id)?;
            }
        }
    }
}

pub fn run_loop(repo: &Path) -> Result<()> {
    let gd = git_dir(repo);
    if !gd.is_dir() {
        anyhow::bail!("not a git repository: {}", repo.display());
    }
    let db_path = gd.join("cloud.sqlite");
    if !db_path.is_file() {
        anyhow::bail!(
            "missing {} — run `git-cloud --init` first",
            db_path.display()
        );
    }

    let mut conn = db::open_db(&gd)?;
    db::init_schema(&conn)?;
    db::migrate_schema(&conn)?;
    cursor::verify_auth()?;

    let poll_secs = std::env::var("GIT_CLOUD_POLL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_POLL_SECS);
    let max_concurrent = std::env::var("GIT_CLOUD_MAX_CONCURRENT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_CONCURRENT);
    let git_ref = std::env::var("GIT_CLOUD_REF").unwrap_or_else(|_| DEFAULT_GIT_REF.to_string());

    println!(
        "{}{}git-cloud run{} — poll {}s, max concurrent {} (ref={}){}\n",
        BOLD, CYAN, RESET, poll_secs, max_concurrent, git_ref, RESET
    );

    loop {
        let (files_left, tests_left) = db::remaining_files_and_tests(&conn)?;
        println!(
            "{}Left to run:{} {} test file(s), ~{} test case(s) remaining (pending + running){}",
            BOLD, CYAN, files_left, tests_left, RESET
        );
        process_running(&mut conn, repo)?;
        let running = db::count_by_status(&conn, TaskStatus::Running)?;
        if running < max_concurrent {
            spawn_pending(&mut conn, repo, &git_ref, max_concurrent - running)?;
        }
        print_summary(&conn)?;
        thread::sleep(Duration::from_secs(poll_secs));
    }
}

fn process_running(conn: &mut Connection, _repo: &Path) -> Result<()> {
    let tasks = db::list_running_with_cloud_id(conn)?;
    for t in tasks {
        let info = match cursor::get_agent(t.cloud_id.as_deref().unwrap_or("")) {
            Ok(i) => i,
            Err(e) => {
                println!("{}poll {} failed: {}{}", YELLOW, t.filename, e, RESET);
                continue;
            }
        };

        if !cursor::is_terminal(&info.status) {
            println!(
                "{}… {} {} @ {} {}{} ({}){}",
                DIM,
                t.filename,
                info.name,
                info.source.repository,
                BLUE,
                info.status,
                info.id,
                RESET
            );
            continue;
        }

        if cursor::is_finished_success(&info.status) {
            println!(
                "{}Agent finished {} — marking sqlite `finished` (use `git-cloud integrate` to merge){}",
                GREEN, t.filename, RESET
            );
            match db::set_finished(conn, t.id) {
                Ok(()) => {}
                Err(e) => {
                    println!(
                        "{}failed to mark finished for {}: {}{}",
                        RED, t.filename, e, RESET
                    );
                    db::set_failed(conn, t.id)?;
                }
            }
        } else if info.status.eq_ignore_ascii_case("CANCELLED") {
            println!(
                "{}Agent cancelled {} — marking sqlite cancelled (not retried){}",
                YELLOW, t.filename, RESET
            );
            db::set_cancelled(conn, t.id)?;
        } else {
            println!(
                "{}Agent failed ({}) for {} — marking sqlite failed (not retried){}",
                YELLOW, info.status, t.filename, RESET
            );
            db::set_failed(conn, t.id)?;
        }
    }
    Ok(())
}

fn branch_name_for_merge(info: &cursor::AgentInfo) -> Result<String> {
    info.target
        .branch_name
        .clone()
        .filter(|b| !b.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "cloud agent did not report target.branchName; merge manually or re-run the task"
            )
        })
}

fn merge_via_origin_branch(repo: &Path, branch: &str) -> Result<bool> {
    if !git_ops::origin_branch_exists(repo, branch) {
        git_ops::fetch_origin(repo)?;
    }
    if !git_ops::origin_branch_exists(repo, branch) {
        anyhow::bail!(
            "remote branch origin/{branch} not found after fetch — check the cloud agent push"
        );
    }
    git_ops::merge_origin_branch(repo, branch)
}

/// Merges cloud work into `main`, calling the merge agent in a loop until the merge completes or aborts cleanly.
fn merge_cloud_work_into_main(repo: &Path, info: &cursor::AgentInfo) -> Result<()> {
    let pr_num = info
        .target
        .pr_url
        .as_deref()
        .and_then(git_ops::parse_github_pr_number);

    loop {
        git_ops::fetch_origin(repo)?;
        git_ops::checkout_and_pull_main(repo)?;

        let merged = if let Some(pr) = pr_num {
            println!("{}Integrating GitHub PR #{} (auto-PR){}", BLUE, pr, RESET);
            match git_ops::merge_github_pr_head(repo, pr) {
                Ok(m) => m,
                Err(e) => {
                    git_ops::try_delete_pr_branch(repo, pr);
                    println!(
                        "{}Could not fetch/merge PR #{}: {}{}\n{}Falling back to origin branch…{}",
                        YELLOW, pr, e, RESET, DIM, RESET
                    );
                    let branch = branch_name_for_merge(info)?;
                    merge_via_origin_branch(repo, &branch)?
                }
            }
        } else {
            let branch = branch_name_for_merge(info)?;
            merge_via_origin_branch(repo, &branch)?
        };

        if merged && !git_ops::has_unmerged_paths(repo)? {
            if let Some(pr) = pr_num {
                git_ops::try_delete_pr_branch(repo, pr);
            }
            return Ok(());
        }

        println!(
            "{}merge conflict or incomplete — running merge agent ({}){}\n",
            YELLOW,
            std::env::var("GIT_CLOUD_MERGE_CMD").unwrap_or_else(|_| "default".into()),
            RESET
        );
        run_merge_agent(repo)?;
        if git_ops::has_unmerged_paths(repo)? {
            git_ops::abort_merge(repo);
            continue;
        }
        git_ops::conclude_merge_if_needed(repo)?;
        if let Some(pr) = pr_num {
            git_ops::try_delete_pr_branch(repo, pr);
        }
        return Ok(());
    }
}

fn integrate_task(
    conn: &mut Connection,
    repo: &Path,
    task: &TaskRow,
    info: &cursor::AgentInfo,
) -> Result<()> {
    merge_cloud_work_into_main(repo, info)?;

    let test_sh = format!("{}.sh", task.filename);
    let script = repo.join("scripts/run-tests.sh");
    println!("{}Running harness {}{}", BLUE, test_sh, RESET);
    let status = std::process::Command::new("bash")
        .current_dir(repo)
        .arg(&script)
        .arg(&test_sh)
        .status()
        .with_context(|| format!("run {}", script.display()))?;
    if !status.success() {
        println!(
            "{}Harness exited with {} for {}{}",
            YELLOW, status, task.filename, RESET
        );
    }

    let (passed, total) = harness::stats_for_file(repo, &task.filename)?;
    db::update_tests_for_file(conn, &task.filename, passed, total)?;

    commit_and_push(repo, task)?;

    db::set_integrated(conn, task.id)?;
    println!(
        "{}Integrated task {}{} (passed={}/{})",
        GREEN, task.filename, RESET, passed, total
    );
    Ok(())
}

/// Re-run harness / sync from CSV for the given tasks (e.g. all `failed`).
fn reverify_tasks_for_harness(
    conn: &mut Connection,
    repo: &Path,
    tasks: Vec<TaskRow>,
) -> Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let script = repo.join("scripts/run-tests.sh");
    for t in tasks {
        let rows = harness::read_test_files_csv(repo)?;
        let Some(csv_row) = harness::row_for_file(&rows, &t.filename) else {
            continue;
        };
        if csv_row.in_scope == "skip" {
            continue;
        }
        if csv_row.fully_passing {
            db::update_tests_for_file(conn, &t.filename, csv_row.passed_last, csv_row.tests_total)?;
            db::set_completed(conn, t.id)?;
            println!(
                "{}Task already fully passing in CSV — marked completed {}{}",
                GREEN, t.filename, RESET
            );
            continue;
        }
        let test_sh = format!("{}.sh", t.filename);
        println!("{}Re-verifying {}{}", BLUE, test_sh, RESET);
        let status = std::process::Command::new("bash")
            .current_dir(repo)
            .arg(&script)
            .arg(&test_sh)
            .status()
            .with_context(|| format!("run {}", script.display()))?;
        if !status.success() {
            println!(
                "{}Harness exited with {} for {}{}",
                YELLOW, status, t.filename, RESET
            );
        }
        let rows = harness::read_test_files_csv(repo)?;
        let Some(csv_row) = harness::row_for_file(&rows, &t.filename) else {
            continue;
        };
        db::update_tests_for_file(conn, &t.filename, csv_row.passed_last, csv_row.tests_total)?;
        if csv_row.fully_passing {
            db::set_completed(conn, t.id)?;
            println!(
                "{}Re-verify: {} fully passing ({}/{}){}",
                GREEN, t.filename, csv_row.passed_last, csv_row.tests_total, RESET
            );
        }
    }
    Ok(())
}

fn commit_and_push(repo: &Path, task: &TaskRow) -> Result<()> {
    let msg = format!(
        "fix: merge cloud work for {} and refresh harness",
        task.filename
    );
    std::process::Command::new("git")
        .current_dir(repo)
        .args(["add", "-A"])
        .status()
        .context("git add -A")?;
    if git_ops::is_clean_worktree(repo)? {
        println!(
            "{}Nothing to commit after harness (clean tree){}",
            DIM, RESET
        );
    } else {
        let st = std::process::Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", &msg])
            .status()
            .context("git commit")?;
        if !st.success() {
            anyhow::bail!("git commit failed with status {st}");
        }
    }
    let push = std::process::Command::new("git")
        .current_dir(repo)
        .args(["push", "origin", "main"])
        .status()
        .context("git push")?;
    if !push.success() {
        anyhow::bail!("git push failed with status {push}");
    }
    println!("{}Pushed to origin/main{}", GREEN, RESET);
    Ok(())
}

fn run_merge_agent(repo: &Path) -> Result<()> {
    let cmd =
        std::env::var("GIT_CLOUD_MERGE_CMD").unwrap_or_else(|_| DEFAULT_MERGE_CMD.to_string());
    let mut child = std::process::Command::new("sh")
        .current_dir(repo)
        .arg("-c")
        .arg(&cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn merge shell: {cmd}"))?;
    let st = child.wait().context("merge agent wait")?;
    if !st.success() {
        anyhow::bail!("merge agent command exited with {st}");
    }
    Ok(())
}

fn spawn_pending(conn: &mut Connection, repo: &Path, git_ref: &str, mut slots: i64) -> Result<()> {
    if slots <= 0 {
        return Ok(());
    }
    let url = git_ops::origin_https_url(repo)?;
    while slots > 0 {
        let Some(task) = db::take_next_pending(conn)? else {
            break;
        };
        let prompt = format!("make {} pass all tests", task.filename);
        match cursor::launch_agent(&url, &prompt, git_ref) {
            Ok(info) => {
                db::set_running(conn, task.id, &info.id)?;
                println!(
                    "{}Started {} — agent {} ({}){}",
                    GREEN, task.filename, info.id, info.status, RESET
                );
            }
            Err(e) => {
                println!(
                    "{}Failed to start agent for {}: {}{}",
                    RED, task.filename, e, RESET
                );
                break;
            }
        }
        slots -= 1;
    }
    Ok(())
}

fn print_summary(conn: &Connection) -> Result<()> {
    let (pending, running, finished, completed, integrated, failed, cancelled) =
        db::summary_counts(conn)?;
    println!(
        "{}Status:{} pending={} running={} finished={} completed={} integrated={} failed={} cancelled={}{}",
        BOLD,
        CYAN,
        pending,
        running,
        finished,
        completed,
        integrated,
        failed,
        cancelled,
        RESET
    );
    let active = db::list_running_with_cloud_id(conn)?;
    for t in active {
        println!(
            "{}  · {} [{}] {} pass/{} {} id={}{}",
            DIM,
            t.filename,
            t.family,
            t.tests_passing,
            t.tests_total,
            t.status.as_str(),
            t.cloud_id.as_deref().unwrap_or("?"),
            RESET
        );
    }
    Ok(())
}
