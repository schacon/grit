//! `grit rerere` — reuse recorded resolution of conflicted merges.

use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::repo::Repository;
use grit_lib::rerere::{
    repo_rerere, rerere_clear, rerere_diff_for_path, rerere_forget_path, rerere_gc,
    rerere_post_commit, rerere_remaining_lines, rerere_status_lines, RerereAutoupdate,
};
use std::io::{self, Write};

/// Arguments for `grit rerere`.
#[derive(Debug, ClapArgs)]
#[command(about = "Reuse recorded resolution of conflicted merges")]
pub struct Args {
    #[arg(long = "rerere-autoupdate")]
    pub rerere_autoupdate: bool,

    #[arg(long = "no-rerere-autoupdate")]
    pub no_rerere_autoupdate: bool,

    #[command(subcommand)]
    pub subcmd: Option<RerereSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum RerereSubcommand {
    Forget { pathspec: String },
    Status,
    Diff,
    Clear,
    Remaining,
    Gc,
}

/// Run the `rerere` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)?;
    let rr = if args.no_rerere_autoupdate {
        RerereAutoupdate::No
    } else if args.rerere_autoupdate {
        RerereAutoupdate::Yes
    } else {
        RerereAutoupdate::FromConfig
    };

    match args.subcmd {
        None => {
            repo_rerere(&repo, rr)?;
            Ok(())
        }
        Some(RerereSubcommand::Forget { pathspec }) => {
            rerere_forget_path(&repo, &pathspec).map_err(|e| anyhow::anyhow!("{}", e))
        }
        Some(RerereSubcommand::Status) => {
            let lines = rerere_status_lines(&repo)?;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for l in lines {
                writeln!(out, "{l}")?;
            }
            Ok(())
        }
        Some(RerereSubcommand::Diff) => {
            let paths = rerere_status_lines(&repo)?;
            let stdout = io::stdout();
            let mut o = stdout.lock();
            for p in paths {
                if let Some(diff) = rerere_diff_for_path(&repo, &p)? {
                    write!(o, "{diff}")?;
                }
            }
            Ok(())
        }
        Some(RerereSubcommand::Clear) => rerere_clear(&repo.git_dir).map_err(|e| e.into()),
        Some(RerereSubcommand::Remaining) => {
            let lines = rerere_remaining_lines(&repo)?;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for l in lines {
                writeln!(out, "{l}")?;
            }
            Ok(())
        }
        Some(RerereSubcommand::Gc) => rerere_gc(&repo.git_dir).map_err(|e| e.into()),
    }
}

/// Used by `am` / legacy call sites.
#[allow(dead_code)]
pub fn auto_rerere(repo: &Repository) -> Result<bool> {
    repo_rerere(repo, RerereAutoupdate::FromConfig)?;
    Ok(false)
}

/// Invoked when `am` fails to apply a patch (index may lack unmerged stages).
pub fn auto_rerere_worktree(repo: &Repository) -> Result<bool> {
    repo_rerere(repo, RerereAutoupdate::FromConfig)?;
    Ok(false)
}

/// Used by `am --continue`.
pub fn record_postimage(repo: &Repository) -> Result<()> {
    rerere_post_commit(repo).map_err(|e| e.into())
}
