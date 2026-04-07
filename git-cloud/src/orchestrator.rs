//! Initialize the task database, sync from CSV, and run the cloud agent supervisor loop.

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
        "{}{}git-cloud update{}: marked {} task row(s) completed from {}{}",
        BOLD,
        GREEN,
        RESET,
        rows_updated,
        repo.join("data/test-files.csv").display(),
        RESET
    );
    Ok(())
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

fn process_running(conn: &mut Connection, repo: &Path) -> Result<()> {
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
                "{}Agent finished {} — merging {}{}",
                GREEN, t.filename, info.id, RESET
            );
            match finish_success(conn, repo, &t, &info) {
                Ok(()) => {}
                Err(e) => {
                    println!(
                        "{}merge/test pipeline failed for {}: {}{}",
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

fn finish_success(
    conn: &mut Connection,
    repo: &Path,
    task: &TaskRow,
    info: &cursor::AgentInfo,
) -> Result<()> {
    git_ops::fetch_origin(repo)?;
    git_ops::checkout_and_pull_main(repo)?;

    let pr_num = info
        .target
        .pr_url
        .as_deref()
        .and_then(git_ops::parse_github_pr_number);

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

    if !merged {
        println!(
            "{}merge conflict — running merge agent ({}){}\n",
            YELLOW,
            std::env::var("GIT_CLOUD_MERGE_CMD").unwrap_or_else(|_| "default".into()),
            RESET
        );
        run_merge_agent(repo)?;
        if git_ops::has_unmerged_paths(repo)? {
            git_ops::abort_merge(repo);
            anyhow::bail!("merge conflicts remain after merge agent; aborted merge");
        }
        git_ops::conclude_merge_if_needed(repo)?;
    }

    if let Some(pr) = pr_num {
        git_ops::try_delete_pr_branch(repo, pr);
    }

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

    db::set_completed(conn, task.id)?;
    println!(
        "{}Completed task {}{} (passed={}/{})",
        GREEN, task.filename, RESET, passed, total
    );
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
    let (pending, running, completed, failed, cancelled) = db::summary_counts(conn)?;
    println!(
        "{}Status:{} pending={} running={} completed={} failed={} cancelled={}{}",
        BOLD, CYAN, pending, running, completed, failed, cancelled, RESET
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
