//! `grit gc` — repository housekeeping.
//!
//! Runs [`prune_packed_objects`](grit_lib::prune_packed::prune_packed_objects),
//! [`pack_refs`](crate::commands::pack_refs), optional [`reflog`](grit_lib::reflog) expiry
//! (`gc.reflogExpire`, `gc.reflogExpireUnreachable`), optional [`commit-graph`](crate::commands::commit_graph) writes
//! (`gc.writeCommitGraph`), and [`repack`](crate::commands::repack) **`-d -l`** to pack objects.
//! Missing: cruft / keep-largest-pack parity and **`--aggressive`** repack tuning.

use crate::commands::pack_refs;
use crate::grit_exe;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::prune_packed::{prune_packed_objects, PrunePackedOptions};
use grit_lib::reflog::{expire_reflog, expire_reflog_unreachable, list_reflog_refs};
use grit_lib::repo::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Arguments for `grit gc`.
#[derive(Debug, ClapArgs)]
#[command(about = "Cleanup unnecessary files and optimize the local repository")]
pub struct Args {
    /// More aggressive optimization (accepted; repack tuning not wired yet).
    #[arg(long)]
    pub aggressive: bool,

    /// Only run if [`gc.auto`](https://git-scm.com/docs/git-config#Documentation/git-config.txt-gcauto)
    /// heuristics say housekeeping is needed.
    #[arg(long)]
    pub auto: bool,

    /// Suppress informational messages (including auto-gc notices).
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Show progress even when stderr is not a terminal (accepted for tests).
    #[arg(long = "no-quiet")]
    pub no_quiet: bool,

    /// Force even if another gc may be running (bypasses `gc.pid` checks).
    #[arg(long)]
    pub force: bool,

    /// Do not prune loose objects that are already packed.
    #[arg(long)]
    pub no_prune: bool,

    /// Detach to background (accepted; always runs in foreground in grit).
    #[arg(long)]
    pub detach: bool,

    #[arg(long = "no-detach")]
    pub no_detach: bool,

    /// Cruft pack options (accepted; no-op until repack supports them).
    #[arg(long)]
    pub cruft: bool,

    #[arg(long = "no-cruft")]
    pub no_cruft: bool,

    #[arg(long = "keep-largest-pack")]
    pub keep_largest_pack: bool,
}

/// Run `grit gc`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let quiet = args.quiet && !args.no_quiet;

    if args.auto {
        if !need_to_gc(&repo, &cfg) {
            return Ok(());
        }
        match run_hook(&repo, "pre-auto-gc", &[], None) {
            HookResult::Failed(_) => return Ok(()),
            HookResult::Success | HookResult::NotFound => {}
        }
        if !quiet {
            eprintln!("Auto packing the repository for optimum performance.");
            eprintln!("See \"git help gc\" for manual housekeeping.");
        }
    }

    let _gc_pid_guard = acquire_gc_pid(&repo.git_dir, args.force)?;

    let objects_dir = repo.git_dir.join("objects");
    if !args.no_prune {
        let opts = PrunePackedOptions {
            dry_run: false,
            quiet,
        };
        prune_packed_objects(&objects_dir, opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    pack_refs::run(pack_refs::Args {
        all: true,
        prune: true,
        no_prune: false,
    })?;

    run_repack_for_gc(&repo, quiet)?;

    run_reflog_expire_for_gc(&repo, &cfg)?;
    run_reflog_expire_unreachable_for_gc(&repo, &cfg)?;
    run_commit_graph_for_gc(&repo, &cfg, quiet)?;

    Ok(())
}

fn gc_hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Writes `<git-dir>/gc.pid` while gc runs; removed on drop. Matches Git’s stale checks (12h, host, `kill(pid,0)`).
struct GcPidGuard {
    path: PathBuf,
}

impl Drop for GcPidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_gc_pid(git_dir: &Path, force: bool) -> Result<GcPidGuard> {
    let pid_path = git_dir.join("gc.pid");
    if !force {
        check_or_clear_stale_gc_pid(&pid_path)?;
    }
    let my_pid = std::process::id();
    let host = gc_hostname();
    fs::write(&pid_path, format!("{my_pid} {host}\n"))?;
    Ok(GcPidGuard { path: pid_path })
}

fn check_or_clear_stale_gc_pid(pid_path: &Path) -> Result<()> {
    let meta = match fs::metadata(pid_path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    let age_secs = SystemTime::now()
        .duration_since(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH))
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX);
    if age_secs > 12 * 3600 {
        let _ = fs::remove_file(pid_path);
        return Ok(());
    }
    let contents = fs::read_to_string(pid_path)?;
    let mut parts = contents.split_whitespace();
    let Some(pid_s) = parts.next() else {
        let _ = fs::remove_file(pid_path);
        return Ok(());
    };
    let Some(locking_host) = parts.next() else {
        let _ = fs::remove_file(pid_path);
        return Ok(());
    };
    let Ok(foreign_pid) = pid_s.parse::<u32>() else {
        let _ = fs::remove_file(pid_path);
        return Ok(());
    };
    let my_host = gc_hostname();
    if locking_host != my_host {
        bail!("gc is already running on machine {locking_host}");
    }
    #[cfg(unix)]
    {
        if grit_lib::unix_process::pid_is_alive(foreign_pid) {
            bail!("gc is already running on machine {locking_host}");
        }
        let _ = fs::remove_file(pid_path);
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        bail!("gc.pid file exists; use --force to bypass");
    }
}

/// Pack loose and packed objects into a new pack, then drop redundant packs (`-d`), same as
/// `maintenance`’s loose-objects task (`-l`).
fn run_repack_for_gc(repo: &Repository, quiet: bool) -> Result<()> {
    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let mut cmd = Command::new(grit_exe::grit_executable());
    cmd.current_dir(work_dir).args(["repack", "-d", "-l"]);
    if quiet {
        cmd.arg("-q");
    }
    let status = cmd.status().context("failed to run grit repack for gc")?;
    if !status.success() {
        eprintln!("warning: repack returned non-zero status");
    }
    Ok(())
}

/// Apply `gc.reflogExpire` to all reflogs (Git default **90** days when unset).
fn run_reflog_expire_for_gc(repo: &Repository, cfg: &ConfigSet) -> Result<()> {
    let raw = cfg
        .get("gc.reflogexpire")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "90".to_string());
    if raw == "never" || raw == "false" {
        return Ok(());
    }
    let days: u64 = raw.parse().unwrap_or(90);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("system time error: {e}"))?
        .as_secs() as i64;
    let cutoff = now.saturating_sub((days as i64).saturating_mul(86_400));

    let refs = list_reflog_refs(&repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    for refname in refs {
        expire_reflog(&repo.git_dir, &refname, Some(cutoff)).map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    Ok(())
}

/// Apply `gc.reflogExpireUnreachable` to all reflogs (Git default **30** days when unset).
fn run_reflog_expire_unreachable_for_gc(repo: &Repository, cfg: &ConfigSet) -> Result<()> {
    let raw = cfg
        .get("gc.reflogexpireunreachable")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "30".to_string());
    if raw == "never" || raw == "false" {
        return Ok(());
    }
    let days: u64 = raw.parse().unwrap_or(30);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("system time error: {e}"))?
        .as_secs() as i64;
    let cutoff = now.saturating_sub((days as i64).saturating_mul(86_400));

    let refs = list_reflog_refs(&repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    for refname in refs {
        expire_reflog_unreachable(repo, &repo.git_dir, &refname, Some(cutoff))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    Ok(())
}

/// Run `grit commit-graph write` when `gc.writeCommitGraph` is true (Git default: on).
fn run_commit_graph_for_gc(repo: &Repository, cfg: &ConfigSet, quiet: bool) -> Result<()> {
    let write_graph = cfg
        .get_bool("gc.writecommitgraph")
        .and_then(|r| r.ok())
        .unwrap_or(true);
    if !write_graph {
        return Ok(());
    }

    let work_dir = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let mut cmd = Command::new(grit_exe::grit_executable());
    cmd.current_dir(work_dir)
        .args(["commit-graph", "write", "--reachable", "--changed-paths"]);
    if quiet {
        cmd.arg("--no-progress");
    }
    let status = cmd
        .status()
        .context("failed to run grit commit-graph write for gc")?;
    if !status.success() {
        eprintln!("warning: commit-graph write returned non-zero status");
    }
    Ok(())
}

/// Rounded threshold matching Git’s `gc.auto` interpretation (`DIV_ROUND_UP(limit, 256) * 256`).
fn gc_auto_threshold(gc_auto: i32) -> usize {
    if gc_auto <= 0 {
        return 0;
    }
    ((gc_auto as usize).saturating_add(255) / 256) * 256
}

fn count_loose_object_files(objects_dir: &Path) -> usize {
    let Ok(rd) = fs::read_dir(objects_dir) else {
        return 0;
    };
    let mut n = 0usize;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) || !entry.path().is_dir()
        {
            continue;
        }
        let Ok(sub) = fs::read_dir(entry.path()) else {
            continue;
        };
        for f in sub.flatten() {
            let fname = f.file_name().to_string_lossy().to_string();
            if fname.len() == 38 && fname.chars().all(|c| c.is_ascii_hexdigit()) {
                n += 1;
            }
        }
    }
    n
}

fn count_local_pack_files(pack_dir: &Path) -> usize {
    let Ok(rd) = fs::read_dir(pack_dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("pack"))
        })
        .count()
}

/// Returns whether automatic gc should do work (loose object count or pack count over limits).
fn need_to_gc(repo: &Repository, cfg: &ConfigSet) -> bool {
    let gc_auto = cfg
        .get("gc.auto")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(6700);
    if gc_auto <= 0 {
        return false;
    }

    let threshold = gc_auto_threshold(gc_auto);
    let loose = count_loose_object_files(&repo.git_dir.join("objects"));
    if loose > threshold {
        return true;
    }

    let pack_limit = cfg
        .get("gc.autopacklimit")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(50);
    if pack_limit <= 0 {
        return false;
    }

    let pack_dir = repo.git_dir.join("objects").join("pack");
    let npacks = count_local_pack_files(&pack_dir);
    npacks > pack_limit as usize
}
