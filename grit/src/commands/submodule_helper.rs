//! `git submodule--helper` — internal plumbing used by shell scripts and tests.

use anyhow::{bail, Context, Result};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::repo::Repository;
use grit_lib::submodule_gitdir::{migrate_gitdir_configs, submodule_gitdir_filesystem_path};
use std::fs;

fn load_local_config(git_dir: &std::path::Path) -> Result<ConfigFile> {
    let config_path = git_dir.join("config");
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local).map_err(Into::into)
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local).map_err(Into::into)
    }
}

/// Run `git submodule--helper <subcommand> ...` (argv after the builtin name).
pub fn run(rest: &[String]) -> Result<()> {
    let sub = rest.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "gitdir" => {
            let name = rest
                .get(1)
                .map(String::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: git submodule--helper gitdir <name>"))?;
            let repo = Repository::discover(None).context("not a git repository")?;
            let work_tree = repo.work_tree.as_ref().context("bare repository")?;
            let cfg = load_local_config(&repo.git_dir)?;
            let path = submodule_gitdir_filesystem_path(work_tree, &repo.git_dir, &cfg, name)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{}", path.display());
            Ok(())
        }
        "migrate-gitdir-configs" => {
            let repo = Repository::discover(None).context("not a git repository")?;
            let work_tree = repo.work_tree.as_ref().context("bare repository")?;
            migrate_gitdir_configs(work_tree, &repo.git_dir).map_err(|e| anyhow::anyhow!("{e}"))
        }
        _ => bail!(
            "git submodule--helper: unknown subcommand '{sub}' (supported: gitdir, migrate-gitdir-configs)"
        ),
    }
}
