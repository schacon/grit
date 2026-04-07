//! `grit merge-resolve` — plumbing helper matching `git-merge-resolve.sh`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;

use crate::commands::{diff_index, merge_index, read_tree, update_index, write_tree};

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    if diff_index::index_cached_differs_from_head(&repo)? {
        eprintln!("Error: Your local changes to the following files would be overwritten by merge");
        std::process::exit(2);
    }

    let (base, head, remote) = parse_merge_resolve_args(&args.args)?;
    update_index::run_refresh_quiet(&repo)?;

    read_tree::run(read_tree::Args {
        merge: true,
        index_only: false,
        update: true,
        reset: false,
        prefix: None,
        aggressive: true,
        dry_run: false,
        exclude_per_directory: None,
        empty: false,
        super_prefix: None,
        recurse_submodules: false,
        trees: vec![base, head, remote],
    })?;

    let index = repo.load_index().context("loading index")?;
    if write_tree::write_tree_from_index(&repo.odb, &index, "", false).is_ok() {
        return Ok(());
    }

    eprintln!("Simple merge failed, trying Automatic merge.");
    merge_index::run(merge_index::Args {
        merge_program: "git-merge-one-file".to_string(),
        all: true,
        one_shot: false,
        quiet: false,
        files: vec![],
    })
}

fn parse_merge_resolve_args(args: &[String]) -> Result<(String, String, String)> {
    let sep = args
        .iter()
        .position(|s| s == "--")
        .ok_or_else(|| anyhow::anyhow!("usage: git merge-resolve <base>... -- <head> <remote>"))?;
    let before = &args[..sep];
    let after = &args[sep + 1..];
    if before.is_empty() {
        bail!("usage: git merge-resolve <base>... -- <head> <remote>");
    }
    if after.len() != 2 {
        bail!("merge-resolve expects exactly one head and one remote after --");
    }
    if before.len() != 1 {
        bail!("merge-resolve: multiple merge bases are not supported yet");
    }
    Ok((before[0].clone(), after[0].clone(), after[1].clone()))
}
