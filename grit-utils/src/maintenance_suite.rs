//! `grit gc` maintenance benchmarks vs system `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use time::OffsetDateTime;

use crate::bench_env::isolated_env_prefix;
use crate::binary::{grit_source_commit, tool_version};
use crate::fixture::{remove_dir_robust, scratch_dir};
use crate::machine::{collect_machine_info, format_timestamp};
use crate::network_fixture::{ensure_fixture, NetworkFixtureKind, NetworkProfile};
use crate::scenarios::{run_scenario, Driver, RunConfig, Scenario};
use crate::schema::{BenchReport, ToolVersions, SCHEMA_VERSION};

fn git_env(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "gc.auto")
        .env("GIT_CONFIG_VALUE_0", "0");
}

fn clone_deep_worktree(git: &Path, bare: &Path) -> Result<PathBuf> {
    let dest = scratch_dir().join("gc-medium-worktree");
    remove_dir_robust(&dest);
    let mut cmd = Command::new(git);
    cmd.args([
        "clone",
        bare.to_str().context("bare path utf-8")?,
        dest.to_str().context("dest utf-8")?,
    ]);
    git_env(&mut cmd);
    let out = cmd.output().context("git clone")?;
    if !out.status.success() {
        anyhow::bail!("git clone failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(dest)
}

fn seed_loose_objects(git: &Path, repo: &Path) -> Result<()> {
    for i in 0..40 {
        let mut cmd = Command::new(git);
        cmd.args(["commit", "--allow-empty", "-m", &format!("gc-bench-{i}")])
            .current_dir(repo);
        git_env(&mut cmd);
        cmd.env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com");
        let out = cmd.output().context("git commit")?;
        if !out.status.success() {
            anyhow::bail!(
                "seed commit failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    Ok(())
}

fn prepare_gc_fixture(git: &Path, bare: &Path) -> Result<PathBuf> {
    let work = clone_deep_worktree(git, bare)?;
    seed_loose_objects(git, &work)?;
    Ok(work)
}

/// GC, repack, and commit-graph scenarios on the deep-history worktree.
pub fn run_maintenance_suite(
    hyperfine: &Path,
    git: &Path,
    grit: &Path,
    cfg: &RunConfig,
    smoke: bool,
    timestamp: OffsetDateTime,
) -> Result<BenchReport> {
    let profile = if smoke {
        NetworkProfile::Smoke
    } else {
        NetworkProfile::Production
    };
    let bare = ensure_fixture(git, profile, NetworkFixtureKind::DeepHistory)?;
    let worktree = prepare_gc_fixture(git, &bare)?;
    let mut scenarios = Vec::new();

    let _ = cfg.isolated_config.then(isolated_env_prefix);

    scenarios.push(run_scenario(
        hyperfine,
        git,
        grit,
        cfg,
        &Scenario {
            id: "gc-medium".into(),
            group: "maintenance".into(),
            fixture: bare.display().to_string(),
            description: "full grit gc vs git gc on deep-history worktree with loose objects"
                .into(),
            grit_argv: vec!["gc".into()],
            git_argv: vec!["gc".into(), "--quiet".into()],
            driver: Driver::Cli,
            prepare_kind: None,
            grit_via_shell: false,
            git_via_shell: false,
        },
        &worktree,
    )?);

    scenarios.push(run_scenario(
        hyperfine,
        git,
        grit,
        cfg,
        &Scenario {
            id: "repack-medium".into(),
            group: "maintenance".into(),
            fixture: bare.display().to_string(),
            description: "grit gc repack --bitmaps vs git repack -adb".into(),
            grit_argv: vec!["gc".into(), "repack".into(), "--bitmaps".into()],
            git_argv: vec!["repack".into(), "-adb".into()],
            driver: Driver::Cli,
            prepare_kind: None,
            grit_via_shell: false,
            git_via_shell: false,
        },
        &worktree,
    )?);

    scenarios.push(run_scenario(
        hyperfine,
        git,
        grit,
        cfg,
        &Scenario {
            id: "commit-graph-write".into(),
            group: "maintenance".into(),
            fixture: bare.display().to_string(),
            description:
                "grit gc commit-graph vs git commit-graph write --reachable --changed-paths".into(),
            grit_argv: vec!["gc".into(), "commit-graph".into()],
            git_argv: vec![
                "commit-graph".into(),
                "write".into(),
                "--reachable".into(),
                "--changed-paths".into(),
            ],
            driver: Driver::Cli,
            prepare_kind: None,
            grit_via_shell: false,
            git_via_shell: false,
        },
        &worktree,
    )?);

    Ok(BenchReport {
        schema_version: SCHEMA_VERSION,
        timestamp: format_timestamp(timestamp),
        machine: collect_machine_info(&scratch_dir())?,
        tools: ToolVersions {
            git: tool_version(git),
            grit: tool_version(grit),
            grit_commit: grit_source_commit(),
        },
        scenarios,
    })
}
