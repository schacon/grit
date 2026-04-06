//! `grit maintenance` — run repository maintenance tasks.
//!
//! Supports: run, start, stop, register.
//! - `run` — run default maintenance tasks (gc, pack)
//! - `start` — schedule periodic maintenance via cron/launchd
//! - `stop` — stop scheduled maintenance
//! - `register` — register repo for maintenance

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::repo::Repository;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Arguments for `grit maintenance`.
#[derive(Debug, ClapArgs)]
#[command(about = "Run maintenance tasks on the repository")]
pub struct Args {
    #[command(subcommand)]
    pub command: MaintenanceCommand,
}

/// Subcommands for `grit maintenance`.
#[derive(Debug, Subcommand)]
pub enum MaintenanceCommand {
    /// Run maintenance tasks (gc, repack).
    Run(RunArgs),
    /// Schedule periodic maintenance.
    Start(StartArgs),
    /// Stop scheduled maintenance.
    Stop(StopArgs),
    /// Register this repository for scheduled maintenance.
    Register(RegisterArgs),
    /// Unregister this repository from scheduled maintenance.
    Unregister(UnregisterArgs),
}

#[derive(Debug, ClapArgs)]
pub struct RunArgs {
    /// Run specific task (gc, commit-graph, prefetch, loose-objects,
    /// incremental-repack, pack-refs).
    #[arg(long)]
    pub task: Option<String>,

    /// Run all tasks, not just the default set.
    #[arg(long)]
    pub auto: bool,

    /// Schedule (hourly, daily, weekly) — controls which tasks run.
    #[arg(long)]
    pub schedule: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct StartArgs {
    /// Scheduler to use (crontab, launchctl, schtasks).
    #[arg(long)]
    pub scheduler: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct StopArgs {
    /// Scheduler to use (crontab, launchctl, schtasks).
    #[arg(long)]
    pub scheduler: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub struct RegisterArgs {}

#[derive(Debug, ClapArgs)]
pub struct UnregisterArgs {}

/// Run the `maintenance` command.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        MaintenanceCommand::Run(a) => run_maintenance(&a),
        MaintenanceCommand::Start(a) => run_start(&a),
        MaintenanceCommand::Stop(a) => run_stop(&a),
        MaintenanceCommand::Register(_) => run_register(),
        MaintenanceCommand::Unregister(_) => run_unregister(),
    }
}

// ── maintenance run ──────────────────────────────────────────────────

fn run_maintenance(args: &RunArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_bin = git_binary();

    if let Some(task) = &args.task {
        run_task(&git_bin, task, &repo)?;
        return Ok(());
    }

    // Default tasks based on schedule.
    let tasks = match args.schedule.as_deref() {
        Some("hourly") => vec!["prefetch", "loose-objects", "incremental-repack"],
        Some("daily") => vec!["loose-objects", "incremental-repack", "pack-refs"],
        Some("weekly") => vec![
            "loose-objects",
            "incremental-repack",
            "pack-refs",
            "commit-graph",
        ],
        _ => vec!["gc"],
    };

    for task in &tasks {
        run_task(&git_bin, task, &repo)?;
    }

    Ok(())
}

fn run_task(git_bin: &str, task: &str, repo: &Repository) -> Result<()> {
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);

    match task {
        "gc" => {
            let status = Command::new(git_bin)
                .arg("gc")
                .arg("--auto")
                .current_dir(work_dir)
                .status()
                .context("failed to run git gc")?;
            if !status.success() {
                eprintln!("warning: gc returned non-zero status");
            }
        }
        "commit-graph" => {
            let status = Command::new(git_bin)
                .args(["commit-graph", "write", "--reachable", "--changed-paths"])
                .current_dir(work_dir)
                .status()
                .context("failed to write commit-graph")?;
            if !status.success() {
                eprintln!("warning: commit-graph write returned non-zero status");
            }
        }
        "prefetch" => {
            let status = Command::new(git_bin)
                .args(["fetch", "--all", "--quiet"])
                .current_dir(work_dir)
                .status()
                .context("failed to prefetch")?;
            if !status.success() {
                eprintln!("warning: prefetch returned non-zero status");
            }
        }
        "loose-objects" => {
            let status = Command::new(git_bin)
                .args(["repack", "-d", "-l"])
                .current_dir(work_dir)
                .status()
                .context("failed to repack loose objects")?;
            if !status.success() {
                eprintln!("warning: loose-objects repack returned non-zero status");
            }
        }
        "incremental-repack" => {
            let status = Command::new(git_bin)
                .args(["multi-pack-index", "repack", "--no-progress"])
                .current_dir(work_dir)
                .status();
            // multi-pack-index may not be available; silently ignore failure.
            if status.is_err() {
                // Fallback: do a regular repack.
                let _ = Command::new(git_bin)
                    .args(["repack", "-d"])
                    .current_dir(work_dir)
                    .status();
            }
        }
        "pack-refs" => {
            let status = Command::new(git_bin)
                .args(["pack-refs", "--all"])
                .current_dir(work_dir)
                .status()
                .context("failed to pack refs")?;
            if !status.success() {
                eprintln!("warning: pack-refs returned non-zero status");
            }
        }
        other => {
            eprintln!("warning: unknown maintenance task '{}'", other);
        }
    }

    Ok(())
}

// ── maintenance start ────────────────────────────────────────────────

fn run_start(args: &StartArgs) -> Result<()> {
    // Ensure the repo is registered first.
    let _ = run_register();

    let scheduler = args.scheduler.as_deref().unwrap_or(detect_scheduler());
    let grit_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit"));

    match scheduler {
        "crontab" => install_crontab(&grit_bin)?,
        "launchctl" => {
            eprintln!("launchctl scheduler not yet implemented; use crontab");
        }
        "schtasks" => {
            eprintln!("schtasks scheduler not yet implemented; use crontab");
        }
        other => {
            eprintln!("unknown scheduler: {other}; using crontab");
            install_crontab(&grit_bin)?;
        }
    }

    Ok(())
}

fn detect_scheduler() -> &'static str {
    if cfg!(target_os = "macos") {
        "launchctl"
    } else if cfg!(target_os = "windows") {
        "schtasks"
    } else {
        "crontab"
    }
}

#[derive(Debug)]
enum SchedulerCmd {
    Command(String),
    Unavailable,
}

fn scheduler_command(name: &str) -> SchedulerCmd {
    let testing = match std::env::var("GIT_TEST_MAINT_SCHEDULER") {
        Ok(v) => v,
        Err(_) => return SchedulerCmd::Command(name.to_string()),
    };

    for entry in testing.split(',') {
        let mut parts = entry.splitn(2, ':');
        let Some(key) = parts.next() else { continue };
        let Some(value) = parts.next() else { continue };
        if key != name {
            continue;
        }

        return match value {
            "false" => SchedulerCmd::Unavailable,
            "true" => SchedulerCmd::Command(name.to_string()),
            other => SchedulerCmd::Command(other.to_string()),
        };
    }

    // If test override exists but no matching scheduler entry was specified,
    // treat it as unavailable (matches upstream test behavior).
    SchedulerCmd::Unavailable
}

fn split_command(cmdline: &str) -> Vec<String> {
    cmdline
        .split_whitespace()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
}

fn run_scheduler_output(cmdline: &str, extra_arg: Option<&str>) -> Result<(bool, String)> {
    let parts = split_command(cmdline);
    if parts.is_empty() {
        return Ok((false, String::new()));
    }

    let mut cmd = if parts[0] == "test-tool" {
        let mut c = Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit")));
        c.arg("test-tool");
        c.args(&parts[1..]);
        c
    } else {
        let mut c = Command::new(&parts[0]);
        c.args(&parts[1..]);
        c
    };
    if let Some(arg) = extra_arg {
        cmd.arg(arg);
    }

    let out = cmd
        .output()
        .with_context(|| format!("failed to spawn scheduler command '{}'", parts[0]))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
    ))
}

fn run_scheduler_status(cmdline: &str, extra_arg: Option<&str>) -> Result<bool> {
    let parts = split_command(cmdline);
    if parts.is_empty() {
        return Ok(false);
    }

    let mut cmd = if parts[0] == "test-tool" {
        let mut c = Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit")));
        c.arg("test-tool");
        c.args(&parts[1..]);
        c
    } else {
        let mut c = Command::new(&parts[0]);
        c.args(&parts[1..]);
        c
    };
    if let Some(arg) = extra_arg {
        cmd.arg(arg);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to spawn scheduler command '{}'", parts[0]))?;
    Ok(status.success())
}

fn write_crontab_via_scheduler(cmdline: &str, content: &str) -> Result<()> {
    let tmp_path = std::env::temp_dir().join(format!(
        "grit-maint-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::write(&tmp_path, content)?;

    let success = run_scheduler_status(cmdline, Some(&tmp_path.to_string_lossy()))?;
    let _ = fs::remove_file(&tmp_path);
    if !success {
        bail!("scheduler command failed");
    }
    Ok(())
}

fn install_crontab(grit_bin: &std::path::Path) -> Result<()> {
    let grit = grit_bin.display();

    let scheduler = match scheduler_command("crontab") {
        SchedulerCmd::Unavailable => bail!("scheduler 'crontab' unavailable"),
        SchedulerCmd::Command(cmd) => cmd,
    };

    // Read existing crontab.
    let existing = run_scheduler_output(&scheduler, Some("-l"))
        .map(|(_, out)| out)
        .unwrap_or_default();

    // Remove any existing grit maintenance lines.
    let filtered: Vec<&str> = existing
        .lines()
        .filter(|l| !l.contains("grit maintenance"))
        .collect();

    let mut new_crontab = filtered.join("\n");
    if !new_crontab.is_empty() && !new_crontab.ends_with('\n') {
        new_crontab.push('\n');
    }

    // Add hourly, daily, weekly schedules.
    new_crontab.push_str(&format!(
        "0 * * * * {grit} maintenance run --schedule=hourly\n"
    ));
    new_crontab.push_str(&format!(
        "0 0 * * * {grit} maintenance run --schedule=daily\n"
    ));
    new_crontab.push_str(&format!(
        "0 0 * * 0 {grit} maintenance run --schedule=weekly\n"
    ));

    if write_crontab_via_scheduler(&scheduler, &new_crontab).is_ok() {
        eprintln!("Scheduled maintenance via crontab");
        return Ok(());
    }
    bail!("failed to install crontab entries")
}

// ── maintenance stop ─────────────────────────────────────────────────

fn run_stop(args: &StopArgs) -> Result<()> {
    let scheduler = args.scheduler.as_deref().unwrap_or(detect_scheduler());

    match scheduler {
        "crontab" => remove_crontab()?,
        _ => {
            eprintln!("Only crontab scheduler is currently supported for stop");
            remove_crontab()?;
        }
    }

    Ok(())
}

fn remove_crontab() -> Result<()> {
    let scheduler = match scheduler_command("crontab") {
        SchedulerCmd::Unavailable => bail!("scheduler 'crontab' unavailable"),
        SchedulerCmd::Command(cmd) => cmd,
    };

    let existing = run_scheduler_output(&scheduler, Some("-l"))
        .map(|(_, out)| out)
        .unwrap_or_default();

    let filtered: Vec<&str> = existing
        .lines()
        .filter(|l| !l.contains("grit maintenance"))
        .collect();

    let new_crontab = filtered.join("\n") + "\n";

    if write_crontab_via_scheduler(&scheduler, &new_crontab).is_ok() {
        eprintln!("Removed grit maintenance from crontab");
        return Ok(());
    }
    bail!("failed to update crontab entries")
}

// ── maintenance register / unregister ────────────────────────────────

/// Path to the global maintenance config file.
fn maintenance_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("git")
        .join("maintenance.ini")
}

fn run_register() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let repo_path = repo.git_dir.display().to_string();

    // Also set maintenance.auto = false in the repo config so regular
    // auto-gc doesn't overlap with scheduled maintenance.
    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };
    config.set("maintenance.auto", "false")?;
    config.write()?;

    // Register in global maintenance list.
    let maint_path = maintenance_config_path();
    if let Some(parent) = maint_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut repos = load_registered_repos(&maint_path);
    if !repos.contains(&repo_path) {
        repos.push(repo_path.clone());
        save_registered_repos(&maint_path, &repos)?;
        eprintln!("Registered '{}' for maintenance", repo_path);
    } else {
        eprintln!("'{}' already registered", repo_path);
    }

    Ok(())
}

fn run_unregister() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let repo_path = repo.git_dir.display().to_string();

    let maint_path = maintenance_config_path();
    let mut repos = load_registered_repos(&maint_path);
    let before = repos.len();
    repos.retain(|r| r != &repo_path);

    if repos.len() < before {
        save_registered_repos(&maint_path, &repos)?;
        eprintln!("Unregistered '{}' from maintenance", repo_path);
    } else {
        eprintln!("'{}' was not registered", repo_path);
    }

    Ok(())
}

fn load_registered_repos(path: &std::path::Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|c| {
            c.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect()
        })
        .unwrap_or_default()
}

fn save_registered_repos(path: &std::path::Path, repos: &[String]) -> Result<()> {
    let content = repos.join("\n") + "\n";
    fs::write(path, content).context("failed to write maintenance config")?;
    Ok(())
}

fn git_binary() -> String {
    std::env::var("REAL_GIT").unwrap_or_else(|_| "/usr/bin/git".to_string())
}
