//! CLI entry for `git-cloud`: initialize harness task DB, sync from CSV, and run the cloud agent loop.

mod ansi;
mod cursor;
mod db;
mod git_ops;
mod harness;
mod orchestrator;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;

use crate::ansi::YELLOW;

/// Loads `REPO/.env` into the process environment (does not override existing vars).
///
/// Missing files are ignored. Parse/load errors are printed to stderr.
fn load_dotenv(repo: &Path) {
    let path = repo.join(".env");
    if !path.is_file() {
        return;
    }
    match dotenvy::from_path(&path) {
        Ok(_) => {}
        Err(e) => {
            eprintln!(
                "{}warning: failed to load {}: {}{}",
                YELLOW,
                path.display(),
                e,
                crate::ansi::RESET
            );
        }
    }
}

/// Orchestrate Cursor Cloud agents to drive grit harness tests.
///
/// Variables such as `CURSOR_API_KEY` and `GIT_CLOUD_*` can be placed in `.env` at the
/// repository root (the directory selected by `--repo`, or `git rev-parse --show-toplevel`).
#[derive(Parser, Debug)]
#[command(name = "git-cloud", version, about)]
struct Cli {
    /// Create `.git/cloud.sqlite` and seed tasks from `data/test-files.csv`.
    #[arg(long)]
    init: bool,

    /// Overwrite an existing database (only with `--init`).
    #[arg(long)]
    force: bool,

    /// Repository root (defaults to `git rev-parse --show-toplevel` from cwd).
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Poll cloud agents, merge results, run tests, push, and spawn new agents.
    Run,
    /// Mark tasks `completed` in SQLite when `data/test-files.csv` has `fully_passing=true`.
    Update,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_root = git_ops::resolve_repo_root(cli.repo.as_deref())?;
    load_dotenv(&repo_root);

    if cli.init {
        orchestrator::init_db(&repo_root, cli.force)?;
    }

    match cli.command {
        Some(Command::Run) => {
            orchestrator::run_loop(&repo_root)?;
        }
        Some(Command::Update) => {
            orchestrator::sync_completed_from_csv(&repo_root)?;
        }
        None => {
            if !cli.init {
                anyhow::bail!("expected `--init`, `run`, or `update` (see --help)");
            }
        }
    }

    Ok(())
}
