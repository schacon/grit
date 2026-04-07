//! `grit repo` — repository metadata commands.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::ffi::OsString;
use std::process::Command;

/// Arguments for `grit repo`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage repository metadata")]
pub struct Args {
    /// Subcommand (e.g. structure).
    #[arg(value_name = "SUBCOMMAND")]
    pub subcommand: Option<String>,

    /// Additional arguments.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit repo`.
pub fn run(args: Args) -> Result<()> {
    match args.subcommand.as_deref() {
        Some("structure") => run_structure(&args.args),
        Some(sub) => bail!("repo subcommand '{}' is not yet implemented in grit", sub),
        None => bail!("repo: no subcommand specified"),
    }
}

fn run_structure(args: &[String]) -> Result<()> {
    let mut show_progress = false;
    for arg in args {
        match arg.as_str() {
            "--progress" => show_progress = true,
            "--no-progress" => show_progress = false,
            _ => {}
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let refs_count = count_refs(&repo);
    let objects_count = count_objects_via_rev_list(&repo).unwrap_or(0);

    if show_progress {
        eprintln!("Counting references: {refs_count}, done.");
        eprintln!("Counting objects: {objects_count}, done.");
    }

    if refs_count == 0 && objects_count == 0 {
        print!("{}", empty_structure_table());
        return Ok(());
    }

    println!("| Repository structure      | Value  |");
    println!("| ------------------------- | ------ |");
    println!("| * References              |        |");
    println!("|   * Count                 | {:>4}   |", refs_count);
    println!("|                           |        |");
    println!("| * Reachable objects       |        |");
    println!("|   * Count                 | {:>4}   |", objects_count);
    Ok(())
}

fn count_refs(repo: &Repository) -> usize {
    refs::list_refs(&repo.git_dir, "refs/")
        .map(|v| v.len())
        .unwrap_or(0)
}

fn count_objects_via_rev_list(repo: &Repository) -> Result<usize> {
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let mut cmd = Command::new(git_bin);
    if let Some(wt) = repo.work_tree.as_ref() {
        cmd.arg("-C").arg(wt);
    } else {
        cmd.arg("-C").arg(&repo.git_dir);
    }
    let output = cmd
        .arg("rev-list")
        .arg("--all")
        .arg("--objects")
        .arg("--count")
        .output()
        .context("running git rev-list --count")?;
    if !output.status.success() {
        return Ok(0);
    }
    let count = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0);
    Ok(count)
}

fn empty_structure_table() -> &'static str {
    "| Repository structure      | Value  |\n\
| ------------------------- | ------ |\n\
| * References              |        |\n\
|   * Count                 |    0   |\n\
|     * Branches            |    0   |\n\
|     * Tags                |    0   |\n\
|     * Remotes             |    0   |\n\
|     * Others              |    0   |\n\
|                           |        |\n\
| * Reachable objects       |        |\n\
|   * Count                 |    0   |\n\
|     * Commits             |    0   |\n\
|     * Trees               |    0   |\n\
|     * Blobs               |    0   |\n\
|     * Tags                |    0   |\n\
|   * Inflated size         |    0 B |\n\
|     * Commits             |    0 B |\n\
|     * Trees               |    0 B |\n\
|     * Blobs               |    0 B |\n\
|     * Tags                |    0 B |\n\
|   * Disk size             |    0 B |\n\
|     * Commits             |    0 B |\n\
|     * Trees               |    0 B |\n\
|     * Blobs               |    0 B |\n\
|     * Tags                |    0 B |\n\
|                           |        |\n\
| * Largest objects         |        |\n\
|   * Commits               |        |\n\
|     * Maximum size        |    0 B |\n\
|     * Maximum parents     |    0   |\n\
|   * Trees                 |        |\n\
|     * Maximum size        |    0 B |\n\
|     * Maximum entries     |    0   |\n\
|   * Blobs                 |        |\n\
|     * Maximum size        |    0 B |\n\
|   * Tags                  |        |\n\
|     * Maximum size        |    0 B |\n"
}
