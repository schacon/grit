//! `git maintenance` — Git-compatible maintenance (`git/builtin/gc.c`).

use crate::explicit_exit::ExplicitExit;
use crate::grit_exe;
use crate::trace2_emit_child_start_json;
use crate::trace2_region_json;
use anyhow::{bail, Context, Result};
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SchedulePriority {
    None = 0,
    Weekly = 1,
    Daily = 2,
    Hourly = 3,
}

impl SchedulePriority {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "hourly" => Some(Self::Hourly),
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskId {
    Prefetch,
    LooseObjects,
    IncrementalRepack,
    GeometricRepack,
    Gc,
    CommitGraph,
    PackRefs,
    ReflogExpire,
    WorktreePrune,
    RerereGc,
}

impl TaskId {
    fn name(self) -> &'static str {
        match self {
            Self::Prefetch => "prefetch",
            Self::LooseObjects => "loose-objects",
            Self::IncrementalRepack => "incremental-repack",
            Self::GeometricRepack => "geometric-repack",
            Self::Gc => "gc",
            Self::CommitGraph => "commit-graph",
            Self::PackRefs => "pack-refs",
            Self::ReflogExpire => "reflog-expire",
            Self::WorktreePrune => "worktree-prune",
            Self::RerereGc => "rerere-gc",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "prefetch" => Self::Prefetch,
            "loose-objects" => Self::LooseObjects,
            "incremental-repack" => Self::IncrementalRepack,
            "geometric-repack" => Self::GeometricRepack,
            "gc" => Self::Gc,
            "commit-graph" => Self::CommitGraph,
            "pack-refs" => Self::PackRefs,
            "reflog-expire" => Self::ReflogExpire,
            "worktree-prune" => Self::WorktreePrune,
            "rerere-gc" => Self::RerereGc,
            _ => return None,
        })
    }

    fn idx(self) -> usize {
        self as usize
    }
}

const N_TASK: usize = 10;

fn all_tasks() -> [TaskId; N_TASK] {
    [
        TaskId::Prefetch,
        TaskId::LooseObjects,
        TaskId::IncrementalRepack,
        TaskId::GeometricRepack,
        TaskId::Gc,
        TaskId::CommitGraph,
        TaskId::PackRefs,
        TaskId::ReflogExpire,
        TaskId::WorktreePrune,
        TaskId::RerereGc,
    ]
}

#[derive(Clone, Copy)]
struct MaintBits(u8);

impl MaintBits {
    const SCH: u8 = 1;
    const MAN: u8 = 2;

    fn new(bits: u8) -> Self {
        Self(bits)
    }
    fn has(self, m: u8) -> bool {
        self.0 & m != 0
    }
}

#[derive(Clone)]
struct Strategy {
    flags: [MaintBits; N_TASK],
    schedules: [SchedulePriority; N_TASK],
}

impl Strategy {
    fn empty() -> Self {
        Self {
            flags: [MaintBits::new(0); N_TASK],
            schedules: [SchedulePriority::None; N_TASK],
        }
    }

    fn geometric() -> Self {
        let mut s = Self::empty();
        let mut set = |t: TaskId, bits: u8, sch: SchedulePriority| {
            s.flags[t.idx()] = MaintBits::new(bits);
            s.schedules[t.idx()] = sch;
        };
        let both = MaintBits::SCH | MaintBits::MAN;
        set(TaskId::CommitGraph, both, SchedulePriority::Hourly);
        set(TaskId::GeometricRepack, both, SchedulePriority::Daily);
        set(TaskId::PackRefs, both, SchedulePriority::Daily);
        set(TaskId::RerereGc, both, SchedulePriority::Weekly);
        set(TaskId::ReflogExpire, both, SchedulePriority::Weekly);
        set(TaskId::WorktreePrune, both, SchedulePriority::Weekly);
        s
    }

    fn gc_only() -> Self {
        let mut s = Self::empty();
        s.flags[TaskId::Gc.idx()] = MaintBits::new(MaintBits::SCH | MaintBits::MAN);
        s.schedules[TaskId::Gc.idx()] = SchedulePriority::Daily;
        s
    }

    fn incremental() -> Self {
        let mut s = Self::empty();
        let mut sch = |t: TaskId, sc: SchedulePriority| {
            s.flags[t.idx()] = MaintBits::new(MaintBits::SCH);
            s.schedules[t.idx()] = sc;
        };
        sch(TaskId::CommitGraph, SchedulePriority::Hourly);
        sch(TaskId::Prefetch, SchedulePriority::Hourly);
        sch(TaskId::IncrementalRepack, SchedulePriority::Daily);
        sch(TaskId::LooseObjects, SchedulePriority::Daily);
        sch(TaskId::PackRefs, SchedulePriority::Weekly);
        s.flags[TaskId::Gc.idx()] = MaintBits::new(MaintBits::MAN);
        s
    }
}

fn parse_strategy(name: &str) -> Result<Strategy> {
    match name.to_ascii_lowercase().as_str() {
        "gc" => Ok(Strategy::gc_only()),
        "incremental" => Ok(Strategy::incremental()),
        "geometric" => Ok(Strategy::geometric()),
        "none" => Ok(Strategy::empty()),
        other => bail!("unknown maintenance strategy: '{other}'"),
    }
}

pub fn run_from_argv(rest: &[String]) -> Result<()> {
    if rest.len() == 1 && (rest[0] == "-h" || rest[0] == "--help") {
        println!("usage: git maintenance <subcommand> [<options>]");
        return Err(ExplicitExit {
            code: 129,
            message: String::new(),
        }
        .into());
    }
    if rest.is_empty() {
        eprintln!("error: need a subcommand");
        eprintln!("usage: git maintenance <subcommand> [<options>]");
        return Err(ExplicitExit {
            code: 129,
            message: String::new(),
        }
        .into());
    }

    match rest[0].as_str() {
        "run" => cmd_run(&rest[1..]),
        "is-needed" => cmd_is_needed(&rest[1..]),
        "register" => cmd_register(&rest[1..]),
        "unregister" => cmd_unregister(&rest[1..]),
        "start" => cmd_start(&rest[1..]),
        "stop" => cmd_stop(),
        other => {
            eprintln!("error: unknown subcommand: `{other}'");
            eprintln!("usage: git maintenance <subcommand> [<options>]");
            Err(ExplicitExit {
                code: 129,
                message: String::new(),
            }
            .into())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SchedulerKind {
    Crontab,
    Systemd,
    Launchctl,
    Schtasks,
}

fn parse_scheduler_arg(s: &str) -> Result<SchedulerKind> {
    match s.to_ascii_lowercase().as_str() {
        "cron" | "crontab" => Ok(SchedulerKind::Crontab),
        "systemd" | "systemd-timer" => Ok(SchedulerKind::Systemd),
        "launchctl" => Ok(SchedulerKind::Launchctl),
        "schtasks" => Ok(SchedulerKind::Schtasks),
        _ => {
            eprintln!("error: unrecognized --scheduler argument '{s}'");
            Err(ExplicitExit {
                code: 129,
                message: String::new(),
            }
            .into())
        }
    }
}

fn systemctl_user_works() -> bool {
    let (_, cmd, ok) = scheduler_testing_lookup("systemctl");
    if ok && cmd != "systemctl" {
        return true;
    }
    Command::new("systemctl")
        .args(["--user", "list-timers"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn scheduler_testing_lookup(cmd: &str) -> (bool, String, bool) {
    let Ok(testing) = std::env::var("GIT_TEST_MAINT_SCHEDULER") else {
        return (false, cmd.to_string(), true);
    };
    for entry in testing.split(',') {
        let mut p = entry.splitn(2, ':');
        let Some(key) = p.next() else { continue };
        let Some(val) = p.next() else { continue };
        if key == cmd {
            return (true, val.to_string(), val != "false");
        }
    }
    (true, cmd.to_string(), true)
}

fn scheduler_available(sk: SchedulerKind) -> bool {
    let name = match sk {
        SchedulerKind::Crontab => "crontab",
        SchedulerKind::Systemd => "systemctl",
        SchedulerKind::Launchctl => "launchctl",
        SchedulerKind::Schtasks => "schtasks",
    };
    let (from_env, _, ok) = scheduler_testing_lookup(name);
    if from_env {
        return ok;
    }
    match sk {
        SchedulerKind::Crontab => {
            #[cfg(target_os = "macos")]
            {
                false
            }
            #[cfg(not(target_os = "macos"))]
            {
                Command::new("crontab")
                    .arg("-l")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok()
            }
        }
        SchedulerKind::Systemd => systemctl_user_works(),
        _ => true,
    }
}

fn resolve_auto_scheduler() -> SchedulerKind {
    #[cfg(target_os = "linux")]
    if systemctl_user_works() {
        return SchedulerKind::Systemd;
    }
    SchedulerKind::Crontab
}

fn cmd_stop() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    update_background_schedule(&repo, false, None)
}

fn cmd_start(rest: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let mut sched_arg: Option<String> = None;
    let mut i = 0usize;
    while i < rest.len() {
        let a = &rest[i];
        if let Some(v) = a.strip_prefix("--scheduler=") {
            sched_arg = Some(v.to_string());
            i += 1;
        } else if a == "--scheduler" {
            let Some(v) = rest.get(i + 1) else {
                bail!("option `--scheduler` requires a value");
            };
            sched_arg = Some(v.clone());
            i += 2;
        } else if a == "--no-scheduler" {
            eprintln!("error: unknown option `--no-scheduler`");
            return Err(ExplicitExit {
                code: 129,
                message: String::new(),
            }
            .into());
        } else {
            bail!("unknown option: {a}");
        }
    }

    let raw = sched_arg.as_deref().unwrap_or("auto");
    let sk = if raw.eq_ignore_ascii_case("auto") {
        resolve_auto_scheduler()
    } else {
        parse_scheduler_arg(raw)?
    };

    if !scheduler_available(sk) {
        bail!(
            "fatal: {} scheduler is not available",
            match sk {
                SchedulerKind::Crontab => "crontab",
                SchedulerKind::Systemd => "systemctl",
                SchedulerKind::Launchctl => "launchctl",
                SchedulerKind::Schtasks => "schtasks",
            }
        );
    }

    update_background_schedule(&repo, true, Some(sk))?;
    cmd_register(&[])
}

struct ScheduleLock {
    path: PathBuf,
}

impl ScheduleLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", std::process::id());
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "unable to create '{}.lock': File exists.\n\n\
Another scheduled git-maintenance(1) process seems to be running in this\n\
repository. Please make sure no other maintenance processes are running and\n\
then try again. If it still fails, a git-maintenance(1) process may have\n\
crashed in this repository earlier: remove the file manually to continue.",
                    path.display()
                );
            }
            Err(e) => Err(e.into()),
        }
    }
}

impl Drop for ScheduleLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn update_background_schedule(
    repo: &Repository,
    enable: bool,
    sk: Option<SchedulerKind>,
) -> Result<()> {
    let lock_path = repo.git_dir.join("objects").join("schedule.lock");
    fs::create_dir_all(lock_path.parent().unwrap())?;
    let _lock = ScheduleLock::acquire(lock_path)?;

    if enable {
        let Some(sk) = sk else {
            return Ok(());
        };
        match sk {
            SchedulerKind::Systemd => systemd_enable_timers()?,
            SchedulerKind::Crontab => crontab_install(repo)?,
            _ => {}
        }
    } else {
        let _ = systemd_disable_timers();
        crontab_clear(repo)?;
    }
    Ok(())
}

fn systemd_disable_timers() -> Result<()> {
    let (_, cmdline, ok) = scheduler_testing_lookup("systemctl");
    if !ok {
        return Ok(());
    }
    let parts: Vec<String> = cmdline.split_whitespace().map(|s| s.to_string()).collect();
    if parts.is_empty() {
        return Ok(());
    }
    for freq in ["hourly", "daily", "weekly"] {
        let mut c = parts.clone();
        c.extend([
            "--user".into(),
            "disable".into(),
            "--now".into(),
            format!("git-maintenance@{freq}.timer"),
        ]);
        let _ = run_cmd_quiet(&c);
    }
    Ok(())
}

fn run_cmd_quiet(argv: &[String]) -> Result<()> {
    if argv.is_empty() {
        return Ok(());
    }
    let st = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match st {
        Ok(s) if s.success() => Ok(()),
        _ => Ok(()),
    }
}

fn systemd_enable_timers() -> Result<()> {
    let (_, cmdline, _) = scheduler_testing_lookup("systemctl");
    let parts: Vec<String> = cmdline.split_whitespace().map(|s| s.to_string()).collect();
    if parts.is_empty() {
        bail!("failed to set up maintenance schedule");
    }
    let mut base = parts.clone();
    base.extend([
        "--user".into(),
        "list-timers".into(),
        "--all".into(),
        "--no-pager".into(),
    ]);
    run_cmd(&base)?;
    for freq in ["hourly", "daily", "weekly"] {
        let mut c = parts.clone();
        c.extend([
            "--user".into(),
            "enable".into(),
            "--now".into(),
            format!("git-maintenance@{freq}.timer"),
        ]);
        run_cmd(&c)?;
    }
    Ok(())
}

fn run_cmd(argv: &[String]) -> Result<()> {
    if argv.is_empty() {
        return Ok(());
    }
    let st = Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .with_context(|| format!("failed to run {}", argv[0]))?;
    if !st.success() {
        bail!("scheduler command failed");
    }
    Ok(())
}

const CRON_BEGIN: &str = "# BEGIN GIT MAINTENANCE SCHEDULE";
const CRON_END: &str = "# END GIT MAINTENANCE SCHEDULE";

fn crontab_install(repo: &Repository) -> Result<()> {
    let (_, cmdline, _) = scheduler_testing_lookup("crontab");
    let parts: Vec<String> = cmdline.split_whitespace().map(|s| s.to_string()).collect();
    if parts.is_empty() {
        bail!("failed to set up maintenance schedule");
    }

    let tmp = repo.git_dir.join("objects").join("cron-edit.txt");
    let mut list_cmd = Command::new(&parts[0]);
    list_cmd.args(&parts[1..]).arg("-l");
    list_cmd.stdin(Stdio::null());
    let existing = list_cmd
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let mut out = String::new();
    let mut skip = false;
    for line in existing.lines() {
        if line == CRON_BEGIN {
            skip = true;
            continue;
        }
        if line == CRON_END {
            skip = false;
            continue;
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }

    let exec = grit_exe::grit_executable();
    let exec_s = exec.to_string_lossy();
    let work = repo
        .work_tree
        .as_deref()
        .unwrap_or(&repo.git_dir)
        .canonicalize()
        .unwrap_or_else(|_| repo.git_dir.clone());
    let work_s = work.to_string_lossy();
    let minute = if std::env::var("GIT_TEST_MAINT_SCHEDULER").is_ok() {
        13u32
    } else {
        0u32
    };
    let extra = "-c credential.interactive=false -c core.askPass=true ";

    out.push_str(CRON_BEGIN);
    out.push('\n');
    out.push_str("# The following schedule was created by Git\n");
    out.push_str("# Any edits made in this region might be\n");
    out.push_str("# replaced in the future by a Git command.\n\n");
    out.push_str(&format!(
        "{minute} 1-23 * * * cd \"{work_s}\" && \"{exec_s}\" for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=hourly\n"
    ));
    out.push_str(&format!(
        "{minute} 0 * * 1-6 cd \"{work_s}\" && \"{exec_s}\" {extra}for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=daily\n"
    ));
    out.push_str(&format!(
        "{minute} 0 * * 0 cd \"{work_s}\" && \"{exec_s}\" {extra}for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=weekly\n"
    ));
    out.push('\n');
    out.push_str(CRON_END);
    out.push('\n');

    fs::write(&tmp, &out)?;
    let mut install = Command::new(&parts[0]);
    install.args(&parts[1..]).arg(&tmp);
    let st = install.status().context("crontab install")?;
    if !st.success() {
        bail!("failed to set up maintenance schedule");
    }
    Ok(())
}

fn crontab_clear(repo: &Repository) -> Result<()> {
    let (_, cmdline, _) = scheduler_testing_lookup("crontab");
    let parts: Vec<String> = cmdline.split_whitespace().map(|s| s.to_string()).collect();
    if parts.is_empty() {
        return Ok(());
    }
    let tmp = repo.git_dir.join("objects").join("cron-clear.txt");
    let mut list_cmd = Command::new(&parts[0]);
    list_cmd.args(&parts[1..]).arg("-l");
    list_cmd.stdin(Stdio::null());
    let existing = list_cmd
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut out = String::new();
    let mut skip = false;
    for line in existing.lines() {
        if line == CRON_BEGIN {
            skip = true;
            continue;
        }
        if line == CRON_END {
            skip = false;
            continue;
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    fs::write(&tmp, &out)?;
    let mut install = Command::new(&parts[0]);
    install.args(&parts[1..]).arg(&tmp);
    let _ = install.status();
    Ok(())
}

fn cmd_register(rest: &[String]) -> Result<()> {
    let mut config_file: Option<PathBuf> = None;
    let mut i = 0usize;
    while i < rest.len() {
        if rest[i] == "--config-file" {
            let Some(p) = rest.get(i + 1) else {
                bail!("option `--config-file` requires a value");
            };
            config_file = Some(PathBuf::from(p));
            i += 2;
        } else {
            bail!("unknown option: {}", rest[i]);
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let cfg_path = repo.git_dir.join("config");
    let content = fs::read_to_string(&cfg_path).unwrap_or_default();
    let mut local = ConfigFile::parse(&cfg_path, &content, ConfigScope::Local)?;
    local.set("maintenance.auto", "false")?;
    let has_strategy = local
        .entries
        .iter()
        .any(|e| e.key == "maintenance.strategy");
    if !has_strategy {
        local.set("maintenance.strategy", "incremental")?;
    }
    local.write()?;

    let maintpath = repo
        .work_tree
        .as_deref()
        .unwrap_or(&repo.git_dir)
        .canonicalize()
        .unwrap_or_else(|_| repo.git_dir.clone());
    let maintpath_s = maintpath.to_string_lossy().to_string();

    let global_path = config_file.unwrap_or_else(|| {
        grit_lib::config::global_config_paths_pub()
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("/tmp/.gitconfig"))
    });
    let g_content = fs::read_to_string(&global_path).unwrap_or_default();
    let mut global = ConfigFile::parse(&global_path, &g_content, ConfigScope::Global)?;
    let exists = global
        .entries
        .iter()
        .any(|e| e.key == "maintenance.repo" && e.value.as_deref() == Some(maintpath_s.as_str()));
    if !exists {
        global.add_value("maintenance.repo", &maintpath_s)?;
        global.write()?;
    }
    Ok(())
}

fn cmd_unregister(rest: &[String]) -> Result<()> {
    let mut config_file: Option<PathBuf> = None;
    let mut force = false;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "--config-file" => {
                let Some(p) = rest.get(i + 1) else {
                    bail!("option `--config-file` requires a value");
                };
                config_file = Some(PathBuf::from(p));
                i += 2;
            }
            "--force" => {
                force = true;
                i += 1;
            }
            _ => bail!("unknown option: {}", rest[i]),
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let maintpath = repo
        .work_tree
        .as_deref()
        .unwrap_or(&repo.git_dir)
        .canonicalize()
        .unwrap_or_else(|_| repo.git_dir.clone());
    let maintpath_s = maintpath.to_string_lossy().to_string();

    let global_path = config_file.unwrap_or_else(|| {
        grit_lib::config::global_config_paths_pub()
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("/tmp/.gitconfig"))
    });
    if !global_path.exists() {
        if force {
            return Ok(());
        }
        bail!("repository '{}' is not registered", maintpath_s);
    }
    let g_content = fs::read_to_string(&global_path).unwrap_or_default();
    let mut global = ConfigFile::parse(&global_path, &g_content, ConfigScope::Global)?;
    let had = global
        .entries
        .iter()
        .any(|e| e.key == "maintenance.repo" && e.value.as_deref() == Some(maintpath_s.as_str()));
    if !had && !force {
        bail!("repository '{}' is not registered", maintpath_s);
    }
    let n = global.unset_matching("maintenance.repo", Some(&regex::escape(&maintpath_s)), true)?;
    if n == 0 && !force {
        bail!("repository '{}' is not registered", maintpath_s);
    }
    global.write()?;
    Ok(())
}

fn cmd_is_needed(rest: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let mut auto = false;
    let mut tasks: Vec<TaskId> = Vec::new();
    let mut i = 0usize;
    while i < rest.len() {
        if rest[i] == "--auto" {
            auto = true;
            i += 1;
        } else if let Some(t) = rest[i].strip_prefix("--task=") {
            let Some(id) = TaskId::from_name(t) else {
                bail!("'{t}' is not a valid task");
            };
            tasks.push(id);
            i += 1;
        } else {
            bail!("unknown option: {}", rest[i]);
        }
    }
    if tasks.is_empty() {
        tasks.extend(all_tasks());
    }
    if auto {
        for t in &tasks {
            if task_auto_needed(&repo, &cfg, *t)? {
                return Ok(());
            }
        }
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_run(rest: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let mut auto = false;
    let mut detach: Option<bool> = None;
    let mut quiet = !atty::is(atty::Stream::Stderr);
    let mut schedule: Option<SchedulePriority> = None;
    let mut selected: Vec<TaskId> = Vec::new();

    let mut i = 0usize;
    while i < rest.len() {
        let a = &rest[i];
        if a == "--auto" {
            auto = true;
            i += 1;
        } else if a == "--quiet" {
            quiet = true;
            i += 1;
        } else if a == "--no-quiet" {
            quiet = false;
            i += 1;
        } else if a == "--detach" {
            detach = Some(true);
            i += 1;
        } else if a == "--no-detach" {
            detach = Some(false);
            i += 1;
        } else if let Some(v) = a.strip_prefix("--schedule=") {
            schedule =
                Some(SchedulePriority::parse(v).ok_or_else(|| {
                    anyhow::anyhow!("fatal: unrecognized --schedule argument '{v}'")
                })?);
            i += 1;
        } else if let Some(t) = a.strip_prefix("--task=") {
            let Some(id) = TaskId::from_name(t) else {
                bail!("'{t}' is not a valid task");
            };
            if selected.contains(&id) {
                bail!("task '{t}' cannot be selected multiple times");
            }
            selected.push(id);
            i += 1;
        } else {
            bail!("unknown option: {a}");
        }
    }

    if auto && schedule.is_some() {
        bail!("fatal: --auto and --schedule cannot be used together");
    }
    if !selected.is_empty() && schedule.is_some() {
        bail!("fatal: --task and --schedule cannot be used together");
    }

    let detach_effective = detach.unwrap_or_else(|| {
        cfg.get_bool("maintenance.autoDetach")
            .or_else(|| cfg.get_bool("maintenance.autodetach"))
            .or_else(|| cfg.get_bool("gc.autoDetach"))
            .or_else(|| cfg.get_bool("gc.autodetach"))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| {
                std::env::var("GIT_TEST_MAINT_AUTO_DETACH")
                    .ok()
                    .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                    .unwrap_or(true)
            })
    });

    if detach_effective {
        if let Ok(p) = std::env::var("GIT_TRACE2_EVENT") {
            if !p.is_empty() {
                let _ = trace2_region_json(&p, "maintenance", "detach");
            }
        }
        let grit = grit_exe::grit_executable();
        let mut c = Command::new(&grit);
        c.arg("maintenance").arg("run");
        if auto {
            c.arg("--auto");
        }
        if quiet {
            c.arg("--quiet");
        } else {
            c.arg("--no-quiet");
        }
        c.arg("--no-detach");
        for t in &selected {
            c.arg(format!("--task={}", t.name()));
        }
        if let Some(s) = schedule {
            c.arg(format!("--schedule={}", s.as_str()));
        }
        c.stdin(Stdio::null());
        c.stdout(Stdio::null());
        c.stderr(Stdio::null());
        let _ = c.spawn();
        return Ok(());
    }

    let tasks = if !selected.is_empty() {
        selected.clone()
    } else {
        build_task_list(&cfg, schedule)?
    };

    run_maintenance_tasks(&repo, &cfg, &tasks, auto, quiet)?;
    Ok(())
}

fn build_task_list(cfg: &ConfigSet, schedule: Option<SchedulePriority>) -> Result<Vec<TaskId>> {
    let strategy_name = cfg.get("maintenance.strategy").unwrap_or_else(|| {
        if schedule.is_some() {
            "none".into()
        } else {
            "geometric".into()
        }
    });
    let mut strategy = parse_strategy(&strategy_name)?;

    for t in all_tasks() {
        let key = format!("maintenance.{}.enabled", t.name());
        if let Some(Ok(b)) = cfg.get_bool(&key) {
            let i = t.idx();
            if b {
                strategy.flags[i] = if schedule.is_some() {
                    MaintBits::new(MaintBits::SCH)
                } else {
                    MaintBits::new(MaintBits::MAN)
                };
            } else {
                strategy.flags[i] = MaintBits::new(0);
            }
        }
    }

    let mut out = Vec::new();
    let want_sched = schedule.is_some();

    for t in all_tasks() {
        let i = t.idx();
        let f = strategy.flags[i].0;
        if want_sched {
            if f & MaintBits::SCH == 0 {
                continue;
            }
            let need = schedule.unwrap();
            if strategy.schedules[i] < need {
                continue;
            }
            let sk = format!("maintenance.{}.schedule", t.name());
            if let Some(sv) = cfg.get(&sk) {
                if let Some(ps) = SchedulePriority::parse(&sv) {
                    if ps < need {
                        continue;
                    }
                }
            }
        } else if f & MaintBits::MAN == 0 {
            continue;
        }
        out.push(t);
    }
    Ok(out)
}

struct MaintLock(PathBuf);

impl MaintLock {
    fn new(repo: &Repository) -> Result<Option<Self>> {
        let p = repo.git_dir.join("objects").join("maintenance.lock");
        match fs::OpenOptions::new().create_new(true).write(true).open(&p) {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", std::process::id());
                Ok(Some(Self(p)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl Drop for MaintLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn run_maintenance_tasks(
    repo: &Repository,
    cfg: &ConfigSet,
    tasks: &[TaskId],
    auto: bool,
    quiet: bool,
) -> Result<()> {
    let Some(_lock) = MaintLock::new(repo)? else {
        if !auto && !quiet {
            eprintln!(
                "warning: lock file '{}.lock' exists, skipping maintenance",
                repo.git_dir.join("objects").join("maintenance").display()
            );
        }
        return Ok(());
    };

    for t in tasks {
        trace2_region(repo, "maintenance foreground", t.name(), || {
            run_foreground(repo, cfg, *t, auto, quiet)
        })?;
    }
    for t in tasks {
        trace2_region(repo, "maintenance", t.name(), || {
            run_background(repo, cfg, *t, auto, quiet)
        })?;
    }
    Ok(())
}

fn trace2_region(
    repo: &Repository,
    category: &str,
    label: &str,
    f: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let _ = repo;
    if let Ok(p) = std::env::var("GIT_TRACE2_EVENT") {
        if !p.is_empty() {
            let _ = trace2_region_json(&p, category, label);
        }
    }
    f()
}

fn emit_child(argv: &[&str]) {
    if let Ok(p) = std::env::var("GIT_TRACE2_EVENT") {
        if !p.is_empty() {
            let mut v = vec!["git".to_string()];
            v.extend(argv.iter().map(|s| s.to_string()));
            let _ = trace2_emit_child_start_json(&p, &v);
        }
    }
}

fn run_grit(repo: &Repository, argv: &[&str]) -> Result<()> {
    emit_child(argv);
    let work = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let grit = grit_exe::grit_executable();
    let st = Command::new(&grit)
        .current_dir(work)
        .args(argv)
        .status()
        .with_context(|| format!("failed: grit {}", argv.first().unwrap_or(&"")))?;
    if !st.success() {
        bail!("command failed: {}", argv.join(" "));
    }
    Ok(())
}

fn run_foreground(
    repo: &Repository,
    cfg: &ConfigSet,
    t: TaskId,
    auto: bool,
    _quiet: bool,
) -> Result<()> {
    match t {
        TaskId::PackRefs => {
            if auto && !pack_refs_auto_needed(repo) {
                return Ok(());
            }
            let mut a = vec!["pack-refs", "--all", "--prune"];
            if auto {
                a.push("--auto");
            }
            run_grit(repo, &a)?;
        }
        TaskId::ReflogExpire => {
            if auto && !reflog_expire_needed(repo, cfg) {
                return Ok(());
            }
            run_grit(repo, &["reflog", "expire", "--all"])?;
        }
        TaskId::Gc => {
            if auto && !crate::commands::gc::need_to_gc(repo, cfg) {
                return Ok(());
            }
            run_grit(repo, &["pack-refs", "--all", "--prune"])?;
            if cfg
                .get_bool("gc.reflogExpire")
                .and_then(|r| r.ok())
                .unwrap_or(true)
            {
                run_grit(repo, &["reflog", "expire", "--all"])?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn run_background(
    repo: &Repository,
    cfg: &ConfigSet,
    t: TaskId,
    auto: bool,
    quiet: bool,
) -> Result<()> {
    match t {
        TaskId::Prefetch => {
            if auto {
                return Ok(());
            }
            run_prefetch(repo, quiet)?;
        }
        TaskId::LooseObjects => {
            if auto && !loose_auto(cfg) {
                return Ok(());
            }
            run_grit(
                repo,
                &["prune-packed", if quiet { "--quiet" } else { "--no-quiet" }],
            )?;
            pack_loose(repo, quiet)?;
        }
        TaskId::IncrementalRepack => {
            if auto && !incr_auto(repo, cfg) {
                return Ok(());
            }
            run_grit(
                repo,
                &[
                    "multi-pack-index",
                    "repack",
                    "--no-progress",
                    "--batch-size=2147483647",
                ],
            )?;
        }
        TaskId::GeometricRepack => {
            if auto && !geom_auto(repo, cfg) {
                return Ok(());
            }
            geom_repack(repo, cfg, quiet)?;
        }
        TaskId::Gc => {
            let mut a = vec!["gc"];
            if auto {
                a.push("--auto");
            }
            if quiet {
                a.push("--quiet");
            } else {
                a.push("--no-quiet");
            }
            a.extend(["--no-detach", "--skip-foreground-tasks"]);
            run_grit(repo, &a)?;
        }
        TaskId::CommitGraph => {
            if !core_commit_graph(cfg) {
                return Ok(());
            }
            if auto && !cg_auto(repo, cfg)? {
                return Ok(());
            }
            let prog = if quiet { "--no-progress" } else { "--progress" };
            run_grit(
                repo,
                &["commit-graph", "write", "--split", "--reachable", prog],
            )?;
        }
        TaskId::PackRefs | TaskId::ReflogExpire => {}
        TaskId::WorktreePrune => {
            if auto && !wt_auto(repo, cfg) {
                return Ok(());
            }
            let exp = cfg
                .get("gc.worktreePruneExpire")
                .unwrap_or_else(|| "3.months.ago".into());
            run_grit(repo, &["worktree", "prune", "--expire", exp.as_str()])?;
        }
        TaskId::RerereGc => {
            if auto && !rerere_auto(repo, cfg) {
                return Ok(());
            }
            run_grit(repo, &["rerere", "gc"])?;
        }
    }
    Ok(())
}

fn core_commit_graph(cfg: &ConfigSet) -> bool {
    cfg.get_bool("core.commitGraph")
        .and_then(|r| r.ok())
        .unwrap_or(true)
}

fn cfg_i32(cfg: &ConfigSet, key: &str, d: i32) -> i32 {
    cfg.get_i64(key)
        .and_then(|r| r.ok())
        .map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
        .unwrap_or(d)
}

fn loose_auto(cfg: &ConfigSet) -> bool {
    let lim = cfg_i32(cfg, "maintenance.loose-objects.auto", 100);
    if lim == 0 {
        return false;
    }
    if lim < 0 {
        return true;
    }
    Repository::discover(None)
        .map(|r| loose_count_at_least(&r, lim as usize))
        .unwrap_or(false)
}

fn loose_count_at_least(repo: &Repository, limit: usize) -> bool {
    let objects = repo.git_dir.join("objects");
    let mut n = 0usize;
    let Ok(rd) = fs::read_dir(&objects) else {
        return false;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(sub) = fs::read_dir(e.path()) else {
            continue;
        };
        for f in sub.flatten() {
            let fname = f.file_name().to_string_lossy().to_string();
            if fname.len() == 38 && fname.chars().all(|c| c.is_ascii_hexdigit()) {
                n += 1;
                if n >= limit {
                    return true;
                }
            }
        }
    }
    n >= limit
}

fn incr_auto(repo: &Repository, cfg: &ConfigSet) -> bool {
    let lim = cfg_i32(cfg, "maintenance.incremental-repack.auto", 10);
    if lim == 0 {
        return false;
    }
    if lim < 0 {
        return true;
    }
    if !cfg
        .get_bool("core.multiPackIndex")
        .and_then(|r| r.ok())
        .unwrap_or(false)
    {
        return false;
    }
    let pack_dir = repo.git_dir.join("objects").join("pack");
    let midx = pack_dir.join("multi-pack-index");
    let Ok(rd) = fs::read_dir(&pack_dir) else {
        return false;
    };
    let mut c = 0;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with("pack-") && n.ends_with(".pack") && !midx.exists() {
            c += 1;
        }
        if c >= lim as usize {
            return true;
        }
    }
    c >= lim as usize
}

fn geom_auto(repo: &Repository, cfg: &ConfigSet) -> bool {
    let v = cfg_i32(cfg, "maintenance.geometric-repack.auto", 100);
    if v == 0 {
        return false;
    }
    if v < 0 {
        return true;
    }
    if count_packs(repo) >= 2 {
        return true;
    }
    Repository::discover(None)
        .map(|r| loose_count_at_least(&r, v as usize))
        .unwrap_or(false)
}

fn count_packs(repo: &Repository) -> usize {
    let d = repo.git_dir.join("objects").join("pack");
    let Ok(rd) = fs::read_dir(&d) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with("pack-") && n.ends_with(".pack") && !n.starts_with("pack-loose-")
        })
        .count()
}

fn geom_repack(repo: &Repository, cfg: &ConfigSet, quiet: bool) -> Result<()> {
    let split = cfg_i32(cfg, "maintenance.geometric-repack.splitFactor", 2).max(2);
    let n = count_packs(repo);
    let q = if quiet { "--quiet" } else { "--no-quiet" };
    if n >= 2 {
        let g = format!("--geometric={split}");
        run_grit(repo, &["repack", "-d", "-l", g.as_str(), q, "--write-midx"])?;
    } else {
        run_grit(
            repo,
            &[
                "repack",
                "-d",
                "-l",
                "--cruft",
                "--cruft-expiration=2.weeks.ago",
                q,
                "--write-midx",
            ],
        )?;
    }
    Ok(())
}

fn cg_auto(repo: &Repository, cfg: &ConfigSet) -> Result<bool> {
    let limit = cfg_i32(cfg, "maintenance.commit-graph.auto", 100);
    if limit == 0 {
        return Ok(false);
    }
    if limit < 0 {
        return Ok(true);
    }
    let in_g = graph_oids(repo)?;
    let mut count = 0i32;
    walk_commits(repo, |oid| {
        let h = oid.as_bytes();
        let mut arr = [0u8; 20];
        arr.copy_from_slice(h);
        if !in_g.contains(&arr) {
            count += 1;
        }
        count < limit
    })?;
    Ok(count >= limit)
}

fn graph_oids(repo: &Repository) -> Result<HashSet<[u8; 20]>> {
    let p = repo
        .git_dir
        .join("objects")
        .join("info")
        .join("commit-graph");
    if !p.exists() {
        return Ok(HashSet::new());
    }
    let data = fs::read(&p)?;
    read_graph_oids(&data)
}

fn read_graph_oids(data: &[u8]) -> Result<HashSet<[u8; 20]>> {
    let mut set = HashSet::new();
    if data.len() < 8 || &data[0..4] != b"CGPH" {
        return Ok(set);
    }
    let nchunks = data[6] as usize;
    let toc = 8usize;
    let mut oid_off = None;
    for i in 0..nchunks {
        let o = toc + i * 12;
        if o + 12 > data.len() {
            break;
        }
        let id = u32::from_be_bytes(data[o..o + 4].try_into()?);
        let off = u64::from_be_bytes(data[o + 4..o + 12].try_into()?) as usize;
        if id == 0x4f49444c {
            oid_off = Some(off);
            break;
        }
    }
    let Some(lookup) = oid_off else {
        return Ok(set);
    };
    if data.len() < 20 {
        return Ok(set);
    }
    let body_len = data.len() - 20;
    if lookup + 20 > body_len {
        return Ok(set);
    }
    let body = &data[..body_len];
    let n = (body.len() - lookup) / 20;
    for i in 0..n {
        let s = lookup + i * 20;
        let mut h = [0u8; 20];
        h.copy_from_slice(&body[s..s + 20]);
        set.insert(h);
    }
    Ok(set)
}

fn walk_commits(repo: &Repository, mut visit: impl FnMut(ObjectId) -> bool) -> Result<()> {
    let odb = Odb::new(&repo.git_dir.join("objects"));
    let mut stack: Vec<ObjectId> = Vec::new();
    collect_tips(repo, &mut stack)?;
    let mut seen: HashSet<ObjectId> = HashSet::new();
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        if !visit(oid) {
            break;
        }
        let c = parse_commit(&obj.data)?;
        for p in c.parents {
            stack.push(p);
        }
    }
    Ok(())
}

fn collect_tips(repo: &Repository, stack: &mut Vec<ObjectId>) -> Result<()> {
    collect_ref_dir(&repo.git_dir.join("refs"), stack)?;
    let packed = repo.git_dir.join("packed-refs");
    if let Ok(content) = fs::read_to_string(&packed) {
        for line in content.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some(hex) = line.split_whitespace().next() {
                if let Ok(oid) = ObjectId::from_hex(hex) {
                    stack.push(oid);
                }
            }
        }
    }
    let head = fs::read_to_string(repo.git_dir.join("HEAD")).unwrap_or_default();
    let head = head.trim();
    if let Some(r) = head.strip_prefix("ref: ") {
        let p = repo.git_dir.join(r.trim());
        if let Ok(c) = fs::read_to_string(p) {
            if let Ok(oid) = ObjectId::from_hex(c.trim()) {
                stack.push(oid);
            }
        }
    } else if let Ok(oid) = ObjectId::from_hex(head) {
        stack.push(oid);
    }
    Ok(())
}

fn collect_ref_dir(dir: &Path, stack: &mut Vec<ObjectId>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for e in fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            collect_ref_dir(&p, stack)?;
        } else if let Ok(c) = fs::read_to_string(&p) {
            if let Ok(oid) = ObjectId::from_hex(c.trim()) {
                stack.push(oid);
            }
        }
    }
    Ok(())
}

fn pack_refs_auto_needed(repo: &Repository) -> bool {
    let heads = repo.git_dir.join("refs").join("heads");
    let n = fs::read_dir(&heads).map(|d| d.count()).unwrap_or(0);
    n > 1 || repo.git_dir.join("packed-refs").is_file()
}

fn reflog_expire_needed(repo: &Repository, cfg: &ConfigSet) -> bool {
    let lim = cfg_i32(cfg, "maintenance.reflog-expire.auto", 100);
    if lim == 0 {
        return false;
    }
    if lim < 0 {
        return true;
    }
    let log = repo.git_dir.join("logs").join("HEAD");
    let Ok(content) = fs::read_to_string(&log) else {
        return false;
    };
    let n = content.lines().filter(|l| !l.is_empty()).count();
    n >= lim as usize
}

fn wt_auto(repo: &Repository, cfg: &ConfigSet) -> bool {
    let lim = cfg_i32(cfg, "maintenance.worktree-prune.auto", 1);
    if lim <= 0 {
        return lim < 0;
    }
    let wt = repo.git_dir.join("worktrees");
    if !wt.is_dir() {
        return false;
    }
    fs::read_dir(&wt)
        .map(|d| d.count() >= lim as usize)
        .unwrap_or(false)
}

fn rerere_auto(repo: &Repository, cfg: &ConfigSet) -> bool {
    let lim = cfg_i32(cfg, "maintenance.rerere-gc.auto", 1);
    if lim <= 0 {
        return lim < 0;
    }
    let p = repo.git_dir.join("rr-cache");
    p.is_dir() && fs::read_dir(&p).map(|d| d.count() > 0).unwrap_or(false)
}

fn task_auto_needed(repo: &Repository, cfg: &ConfigSet, t: TaskId) -> Result<bool> {
    Ok(match t {
        TaskId::Gc => crate::commands::gc::need_to_gc(repo, cfg),
        TaskId::CommitGraph => cg_auto(repo, cfg)?,
        TaskId::LooseObjects => loose_auto(cfg),
        TaskId::IncrementalRepack => incr_auto(repo, cfg),
        TaskId::GeometricRepack => geom_auto(repo, cfg),
        TaskId::ReflogExpire => reflog_expire_needed(repo, cfg),
        TaskId::WorktreePrune => wt_auto(repo, cfg),
        TaskId::RerereGc => rerere_auto(repo, cfg),
        TaskId::PackRefs => pack_refs_auto_needed(repo),
        TaskId::Prefetch => false,
    })
}

fn run_prefetch(repo: &Repository, quiet: bool) -> Result<()> {
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let path = repo.git_dir.join("config");
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(());
    };
    let mut current: Option<String> = None;
    let mut remotes: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('[') {
            if let Some(sec) = rest.strip_suffix(']') {
                current = Some(sec.trim().to_string());
            }
        } else if let Some((k, _v)) = t.split_once('=') {
            if let Some(ref sec) = current {
                if let Some(name) = sec
                    .strip_prefix("remote \"")
                    .and_then(|s| s.strip_suffix('"'))
                {
                    if k.trim() == "url" && !name.is_empty() {
                        remotes.push(name.to_string());
                    }
                }
            }
        }
    }
    remotes.sort();
    remotes.dedup();
    for r in remotes {
        if cfg
            .get(&format!("remote.{r}.skipFetchAll"))
            .map(|v| v == "true")
            .unwrap_or(false)
        {
            continue;
        }
        let q = if quiet { "--quiet" } else { "" };
        let args = vec![
            "fetch",
            r.as_str(),
            "--prefetch",
            "--prune",
            "--no-tags",
            "--no-write-fetch-head",
            "--recurse-submodules=no",
            q,
        ];
        let args: Vec<&str> = args.into_iter().filter(|s| !s.is_empty()).collect();
        run_grit(repo, &args)?;
    }
    Ok(())
}

fn pack_loose(repo: &Repository, quiet: bool) -> Result<()> {
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let mut batch = cfg_i32(&cfg, "maintenance.loose-objects.batchSize", 50000);
    if batch == 0 {
        batch = i32::MAX;
    } else if batch > 0 {
        batch -= 1;
    }
    let work = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let base = if repo.work_tree.is_some() {
        ".git/objects/pack/loose"
    } else {
        "objects/pack/loose"
    };
    let grit = grit_exe::grit_executable();
    let mut cmd = Command::new(&grit);
    cmd.current_dir(work)
        .arg("pack-objects")
        .arg(if quiet { "--quiet" } else { "--no-quiet" })
        .arg(base)
        .stdin(Stdio::piped());
    let mut child = cmd.spawn().context("pack-objects")?;
    let mut stdin = child.stdin.take().context("stdin")?;
    let objects = repo.git_dir.join("objects");
    let mut count = 0i32;
    if let Ok(rd) = fs::read_dir(&objects) {
        'outer: for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(sub) = fs::read_dir(e.path()) else {
                continue;
            };
            for f in sub.flatten() {
                let fname = f.file_name().to_string_lossy().to_string();
                if fname.len() == 38 && fname.chars().all(|c| c.is_ascii_hexdigit()) {
                    writeln!(stdin, "{name}{fname}")?;
                    count += 1;
                    if count > batch {
                        break 'outer;
                    }
                }
            }
        }
    }
    drop(stdin);
    let st = child.wait()?;
    if !st.success() {
        bail!("pack-objects failed");
    }
    Ok(())
}

pub(crate) fn run_auto_after_commit(repo: &Repository, quiet: bool) -> Result<()> {
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    if cfg.get_bool("maintenance.auto").and_then(|r| r.ok()) == Some(false) {
        return Ok(());
    }
    let auto_detach = cfg
        .get_bool("maintenance.autoDetach")
        .or_else(|| cfg.get_bool("maintenance.autodetach"))
        .or_else(|| cfg.get_bool("gc.autoDetach"))
        .or_else(|| cfg.get_bool("gc.autodetach"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| {
            std::env::var("GIT_TEST_MAINT_AUTO_DETACH")
                .ok()
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true)
        });
    emit_child(&[
        "maintenance",
        "run",
        "--auto",
        if quiet { "--quiet" } else { "--no-quiet" },
        if auto_detach {
            "--detach"
        } else {
            "--no-detach"
        },
    ]);
    let work = repo.work_tree.as_deref().unwrap_or(&repo.git_dir);
    let grit = grit_exe::grit_executable();
    let mut c = Command::new(&grit);
    c.current_dir(work)
        .args(["maintenance", "run", "--auto"])
        .arg(if quiet { "--quiet" } else { "--no-quiet" })
        .arg(if auto_detach {
            "--detach"
        } else {
            "--no-detach"
        });
    let _ = c.status();
    Ok(())
}
