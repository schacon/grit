//! Recursive `git fetch` into submodules (Git `fetch_submodules` / `submodule.c`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigFile;
use grit_lib::diff::zero_oid;
use grit_lib::fetch_submodules::{
    collect_changed_submodules_for_fetch, is_submodule_active_for_fetch,
    might_have_submodules_to_fetch, parse_fetch_recurse_submodules_arg,
    submodule_git_dir_for_fetch, submodule_has_all_commits, ChangedSubmoduleFetch,
    FetchRecurseSubmodules,
};
use grit_lib::index::MODE_GITLINK;
use grit_lib::name_rev;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;

use crate::commands::fetch::Args as FetchArgs;
use crate::commands::submodule::{get_default_remote_for_path, parse_gitmodules_with_repo};
use crate::grit_exe;

fn parse_gitmodules_file(work_tree: &Path) -> Option<ConfigFile> {
    let p = work_tree.join(".gitmodules");
    let content = std::fs::read_to_string(&p).ok()?;
    ConfigFile::parse(&p, &content, grit_lib::config::ConfigScope::Local).ok()
}

fn gitmodules_fetch_recurse_value(gm: &ConfigFile, submodule_name: &str) -> Option<String> {
    let key_lc = format!("submodule.{submodule_name}.fetchrecursesubmodules");
    for e in &gm.entries {
        if e.key == key_lc {
            return e.value.clone();
        }
    }
    None
}

fn effective_submodule_fetch_recurse(
    name: &str,
    cmd: FetchRecurseSubmodules,
    default: FetchRecurseSubmodules,
    config: &grit_lib::config::ConfigSet,
    gm: Option<&ConfigFile>,
) -> Result<FetchRecurseSubmodules> {
    if cmd != FetchRecurseSubmodules::Default {
        return Ok(cmd);
    }
    let key = format!("submodule.{name}.fetchRecurseSubmodules");
    let key_alt = format!("submodule.{name}.fetchrecursesubmodules");
    let from_config = config.get(&key).or_else(|| config.get(&key_alt));
    let from_gm = gm.and_then(|g| gitmodules_fetch_recurse_value(g, name));
    let raw = from_config.or_else(|| from_gm.clone());
    if let Some(v) = raw {
        return parse_fetch_recurse_submodules_arg(&key, v.trim()).map_err(|e| anyhow::anyhow!(e));
    }
    Ok(default)
}

fn fetch_parallel_job_count(args: &FetchArgs, config: &grit_lib::config::ConfigSet) -> usize {
    if let Some(j) = args.jobs {
        if j == 0 {
            return std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
        }
        return j;
    }
    if let Some(v) = config
        .get("submodule.fetchjobs")
        .or_else(|| config.get("submodule.fetchJobs"))
    {
        if let Ok(n) = v.trim().parse::<usize>() {
            if n == 0 {
                return std::thread::available_parallelism()
                    .map(|x| x.get())
                    .unwrap_or(1);
            }
            if n > 0 {
                return n;
            }
        }
    }
    if let Some(v) = config.get("fetch.parallel") {
        if let Ok(n) = v.trim().parse::<usize>() {
            if n == 0 {
                return std::thread::available_parallelism()
                    .map(|x| x.get())
                    .unwrap_or(1);
            }
            if n > 0 {
                return n;
            }
        }
    }
    1
}

fn trace_parallel_tasks(n: usize) {
    if let Ok(trace_val) = std::env::var("GIT_TRACE") {
        if trace_val.is_empty() || trace_val == "0" || trace_val.eq_ignore_ascii_case("false") {
            return;
        }
        let line = format!("run_processes_parallel: preparing to run up to {n} tasks\n");
        crate::write_git_trace(&trace_val, &line);
    }
}

fn forward_parent_fetch_flags(cmd: FetchRecurseSubmodules, args: &FetchArgs) -> Vec<String> {
    let mut v = Vec::new();
    if args.dry_run {
        v.push("--dry-run".to_string());
    }
    if args.quiet {
        v.push("-q".to_string());
    }
    if args.no_write_fetch_head {
        v.push("--no-write-fetch-head".to_string());
    }
    match cmd {
        FetchRecurseSubmodules::On => v.push("--recurse-submodules".to_string()),
        FetchRecurseSubmodules::Off => v.push("--no-recurse-submodules".to_string()),
        FetchRecurseSubmodules::OnDemand => v.push("--recurse-submodules=on-demand".to_string()),
        FetchRecurseSubmodules::Default => {}
    }
    v
}

struct SubmoduleFetchWork {
    /// Directory for the child process (`submodule/` when checked out, else the git dir).
    process_cwd: std::path::PathBuf,
    /// Pass `--work-tree=.` only when `process_cwd` is the git dir (unpopulated module).
    work_tree_dot: bool,
    /// Path relative to the repo whose index listed this submodule (super or nested).
    display_path: String,
    remote: String,
    default_token: &'static str,
    oids: Vec<ObjectId>,
}

fn head_tree_oid(repo: &Repository) -> Option<ObjectId> {
    let head = resolve_head(&repo.git_dir).ok()?;
    let oid = head.oid()?;
    let obj = repo.odb.read(oid).ok()?;
    if obj.kind != grit_lib::objects::ObjectKind::Commit {
        return None;
    }
    let c = grit_lib::objects::parse_commit(&obj.data).ok()?;
    Some(c.tree)
}

/// After a successful top-level `fetch`, recurse into submodules like Git's `fetch_submodules`.
pub(crate) fn recursive_fetch_submodules_after_fetch(
    super_git_dir: &Path,
    config: &grit_lib::config::ConfigSet,
    args: &FetchArgs,
    cmd_recurse: FetchRecurseSubmodules,
) -> Result<()> {
    let Some(record) = crate::fetch_submodule_record::take_fetch_submodule_record() else {
        return Ok(());
    };
    let cwd = std::env::current_dir().context("cwd")?;
    let repo = Repository::discover(Some(cwd.as_path())).context("open repository")?;
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    if !might_have_submodules_to_fetch(work_tree, super_git_dir) {
        return Ok(());
    }
    let gm_file = parse_gitmodules_file(work_tree);

    let positive = grit_lib::fetch_submodules::merge_tips_for_changed_walk(
        &record.submodule_commits.borrow(),
        &record.tips_after.borrow(),
    );
    let negative: Vec<String> = record.tips_before.iter().map(|o| o.to_hex()).collect();
    let mut changed_list = collect_changed_submodules_for_fetch(&repo, &positive, &negative)?;
    let mut changed_by_name: HashMap<String, ChangedSubmoduleFetch> = HashMap::new();
    for c in changed_list.drain(..) {
        changed_by_name.insert(c.name.clone(), c);
    }

    let default_child = if let Some(ref s) = args.recurse_submodules_default {
        parse_fetch_recurse_submodules_arg("--recurse-submodules-default", s)
            .map_err(|e| anyhow::anyhow!(e))?
    } else {
        FetchRecurseSubmodules::OnDemand
    };

    let mut filtered_changed: HashMap<String, ChangedSubmoduleFetch> = HashMap::new();
    for (name, cs) in changed_by_name {
        if !is_submodule_active_for_fetch(&repo, config, &cs.super_oid, &cs.path, &name) {
            continue;
        }
        let Some(gd) = submodule_git_dir_for_fetch(&repo, &cs.path) else {
            continue;
        };
        let odb = Odb::new(&gd.join("objects"));
        if submodule_has_all_commits(&odb, &cs.new_commits)? {
            continue;
        }
        filtered_changed.insert(name, cs);
    }
    let changed_by_name = filtered_changed;

    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut work: Vec<SubmoduleFetchWork> = Vec::new();

    let head_tree = head_tree_oid(&repo);

    let index_path = repo.index_path();
    let index = repo.load_index_at(&index_path).ok();
    if let Some(ref idx) = index {
        for e in &idx.entries {
            if e.stage() != 0 || e.mode != MODE_GITLINK {
                continue;
            }
            let path = String::from_utf8_lossy(&e.path).to_string();
            let Some(sm) = modules.iter().find(|m| m.path == path) else {
                continue;
            };
            let tree_for_active = head_tree.unwrap_or_else(zero_oid);
            if !is_submodule_active_for_fetch(&repo, config, &tree_for_active, &path, &sm.name) {
                continue;
            }
            let mode = effective_submodule_fetch_recurse(
                &sm.name,
                cmd_recurse,
                default_child,
                config,
                gm_file.as_ref(),
            )?;
            let include = match mode {
                FetchRecurseSubmodules::Off => false,
                FetchRecurseSubmodules::On => true,
                FetchRecurseSubmodules::OnDemand | FetchRecurseSubmodules::Default => {
                    changed_by_name.contains_key(&sm.name)
                }
            };
            if !include {
                continue;
            }
            let Some(gd) = submodule_git_dir_for_fetch(&repo, &path) else {
                let abs = work_tree.join(&path);
                if abs.is_dir()
                    && std::fs::read_dir(&abs)
                        .map(|d| d.count() > 0)
                        .unwrap_or(false)
                {
                    bail!("Could not access submodule '{path}'");
                }
                continue;
            };
            if seen.insert(sm.name.clone()) {
                let remote = get_default_remote_for_path(&path)?;
                let abs_sm = work_tree.join(&path);
                let (process_cwd, work_tree_dot) = if abs_sm.join(".git").exists() {
                    (abs_sm, false)
                } else {
                    (gd.clone(), true)
                };
                work.push(SubmoduleFetchWork {
                    process_cwd,
                    work_tree_dot,
                    display_path: path.clone(),
                    remote,
                    default_token: if mode == FetchRecurseSubmodules::On {
                        "yes"
                    } else {
                        "on-demand"
                    },
                    oids: Vec::new(),
                });
            }
        }
    }

    let mut changed_names: Vec<String> = changed_by_name.keys().cloned().collect();
    changed_names.sort();
    for name in changed_names {
        if seen.contains(&name) {
            continue;
        }
        let Some(cs) = changed_by_name.get(&name) else {
            continue;
        };
        if !is_submodule_active_for_fetch(&repo, config, &cs.super_oid, &cs.path, &name) {
            continue;
        }
        let mode = effective_submodule_fetch_recurse(
            &name,
            cmd_recurse,
            default_child,
            config,
            gm_file.as_ref(),
        )?;
        let include = match mode {
            FetchRecurseSubmodules::Off => false,
            FetchRecurseSubmodules::On => true,
            FetchRecurseSubmodules::OnDemand | FetchRecurseSubmodules::Default => true,
        };
        if !include {
            continue;
        }
        let Some(gd) = submodule_git_dir_for_fetch(&repo, &cs.path) else {
            bail!(
                "Could not access submodule '{}' at commit {}",
                cs.path,
                cs.super_oid.to_hex()
            );
        };
        seen.insert(name.clone());
        let remote = get_default_remote_for_path(&cs.path)?;
        let abs_sm = work_tree.join(&cs.path);
        let (process_cwd, work_tree_dot) = if abs_sm.join(".git").exists() {
            (abs_sm, false)
        } else {
            (gd.clone(), true)
        };
        work.push(SubmoduleFetchWork {
            process_cwd,
            work_tree_dot,
            display_path: cs.path.clone(),
            remote,
            default_token: "on-demand",
            oids: cs.new_commits.clone(),
        });
    }

    let jobs = fetch_parallel_job_count(args, config).max(1);
    trace_parallel_tasks(jobs);

    let grit_bin = grit_exe::grit_executable();
    let prefix_raw = args.submodule_prefix.as_deref().unwrap_or("");
    let prefix_trim = prefix_raw.trim_end_matches('/');
    let forward = forward_parent_fetch_flags(cmd_recurse, args);

    for w in work {
        let full_prefix = if prefix_trim.is_empty() {
            format!("{}/", w.display_path)
        } else {
            format!("{prefix_trim}/{}/", w.display_path)
        };
        let stderr_path = if prefix_trim.is_empty() {
            w.display_path.clone()
        } else {
            format!("{prefix_trim}/{}", w.display_path)
        };
        if !args.quiet {
            if w.oids.is_empty() {
                eprintln!("Fetching submodule {stderr_path}");
            } else {
                let abbrev = cs_super_abbrev(super_git_dir, &changed_by_name, &stderr_path);
                eprintln!("Fetching submodule {stderr_path} at commit {abbrev}");
            }
        }

        let mut argv: Vec<String> = vec!["fetch".into()];
        argv.extend(forward.clone());
        argv.push("--recurse-submodules-default".into());
        argv.push(w.default_token.to_string());
        argv.push(format!("--submodule-prefix={full_prefix}"));
        if w.work_tree_dot {
            argv.push("--work-tree=.".into());
        }
        argv.push(w.remote.clone());
        for o in &w.oids {
            argv.push(o.to_hex());
        }

        let trace_argv: Vec<String> = std::iter::once("git".to_string())
            .chain(argv.iter().cloned())
            .collect();
        crate::trace2_emit_git_subcommand_argv(&trace_argv);

        let status = std::process::Command::new(&grit_bin)
            .current_dir(&w.process_cwd)
            .args(&argv)
            .status()
            .with_context(|| format!("submodule fetch {}", w.display_path))?;
        if !status.success() {
            bail!("submodule fetch failed for {}", w.display_path);
        }
    }

    Ok(())
}

fn cs_super_abbrev(
    super_git_dir: &Path,
    changed: &HashMap<String, ChangedSubmoduleFetch>,
    path: &str,
) -> String {
    for cs in changed.values() {
        if cs.path == path {
            return short_oid(&cs.super_oid, super_git_dir);
        }
    }
    "???????".to_string()
}

fn short_oid(oid: &ObjectId, _super_git_dir: &Path) -> String {
    name_rev::abbrev_oid(*oid, 7)
}
