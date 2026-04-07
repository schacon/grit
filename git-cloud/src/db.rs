//! SQLite persistence for harness file tasks.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Row in `tasks` representing one test file stem (e.g. `t0000-basic`).
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: i64,
    pub filename: String,
    pub family: String,
    pub tests_total: i64,
    pub tests_passing: i64,
    pub cloud_id: Option<String>,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "running" => Some(TaskStatus::Running),
            "completed" => Some(TaskStatus::Completed),
            _ => None,
        }
    }
}

pub fn open_db(git_dir: &Path) -> Result<Connection> {
    let path = git_dir.join("cloud.sqlite");
    Connection::open(&path).with_context(|| format!("open sqlite {}", path.display()))
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            filename TEXT NOT NULL UNIQUE,
            family TEXT NOT NULL,
            tests_total INTEGER NOT NULL,
            tests_passing INTEGER NOT NULL,
            cloud_id TEXT,
            status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'completed'))
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
        CREATE INDEX IF NOT EXISTS idx_tasks_cloud ON tasks(cloud_id);
        ",
    )
    .context("create tasks schema")?;
    Ok(())
}

pub fn insert_task(
    conn: &Connection,
    filename: &str,
    family: &str,
    tests_total: i64,
    tests_passing: i64,
) -> Result<()> {
    conn.execute(
        r"
        INSERT OR IGNORE INTO tasks (filename, family, tests_total, tests_passing, cloud_id, status)
        VALUES (?1, ?2, ?3, ?4, NULL, 'pending')
        ",
        params![filename, family, tests_total, tests_passing],
    )
    .context("insert task")?;
    Ok(())
}

pub fn count_by_status(conn: &Connection, status: TaskStatus) -> Result<i64> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = ?1",
            params![status.as_str()],
            |row| row.get(0),
        )
        .context("count tasks by status")?;
    Ok(n)
}

pub fn list_running_with_cloud_id(conn: &Connection) -> Result<Vec<TaskRow>> {
    let mut stmt = conn
        .prepare(
            r"SELECT id, filename, family, tests_total, tests_passing, cloud_id, status
              FROM tasks WHERE status = 'running' AND cloud_id IS NOT NULL",
        )
        .context("prepare list running")?;
    let rows = stmt
        .query_map([], |row| {
            let status_s: String = row.get(6)?;
            let status = TaskStatus::parse(&status_s).unwrap_or(TaskStatus::Pending);
            Ok(TaskRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                family: row.get(2)?,
                tests_total: row.get(3)?,
                tests_passing: row.get(4)?,
                cloud_id: row.get(5)?,
                status,
            })
        })
        .context("query running tasks")?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn take_next_pending(conn: &Connection) -> Result<Option<TaskRow>> {
    let mut stmt = conn
        .prepare(
            r"SELECT id, filename, family, tests_total, tests_passing, cloud_id, status
              FROM tasks WHERE status = 'pending' ORDER BY filename LIMIT 1",
        )
        .context("prepare next pending")?;
    let row = stmt
        .query_row([], |row| {
            let status_s: String = row.get(6)?;
            let status = TaskStatus::parse(&status_s).unwrap_or(TaskStatus::Pending);
            Ok(TaskRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                family: row.get(2)?,
                tests_total: row.get(3)?,
                tests_passing: row.get(4)?,
                cloud_id: row.get(5)?,
                status,
            })
        })
        .optional()
        .context("query next pending")?;
    Ok(row)
}

pub fn set_running(conn: &Connection, id: i64, cloud_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET status = 'running', cloud_id = ?1 WHERE id = ?2",
        params![cloud_id, id],
    )
    .context("set running")?;
    Ok(())
}

pub fn set_pending_reset_cloud(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET status = 'pending', cloud_id = NULL WHERE id = ?1",
        params![id],
    )
    .context("reset pending")?;
    Ok(())
}

pub fn set_completed(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET status = 'completed', cloud_id = NULL WHERE id = ?1",
        params![id],
    )
    .context("set completed")?;
    Ok(())
}

pub fn update_tests_for_file(
    conn: &Connection,
    filename: &str,
    tests_passing: i64,
    tests_total: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET tests_passing = ?1, tests_total = ?2 WHERE filename = ?3",
        params![tests_passing, tests_total, filename],
    )
    .context("update tests_passing/tests_total")?;
    Ok(())
}

pub fn summary_counts(conn: &Connection) -> Result<(i64, i64, i64)> {
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let running: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = 'running'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = 'completed'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok((pending, running, completed))
}

/// Non-completed task count and sum of per-file `(tests_total - tests_passing)` (at least 0).
pub fn remaining_files_and_tests(conn: &Connection) -> Result<(i64, i64)> {
    let files: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status != 'completed'",
            [],
            |row| row.get(0),
        )
        .context("count remaining files")?;
    let tests: i64 = conn
        .query_row(
            r"SELECT COALESCE(SUM(CASE WHEN tests_total > tests_passing
                THEN tests_total - tests_passing ELSE 0 END), 0)
              FROM tasks WHERE status != 'completed'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok((files, tests))
}
