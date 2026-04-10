//! `grit merge-base` - find best common ancestors for merges.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::merge_base::{
    independent_commits, is_ancestor, merge_base_fork_point, merge_bases_first_vs_rest,
    merge_bases_octopus, resolve_commit_specs,
};
use grit_lib::repo::Repository;

/// Arguments for `grit merge-base`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit merge-base`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;

    let mut show_all = false;
    let mut mode = Mode::Default;
    let mut revisions = Vec::new();
    let mut end_of_options = false;

    let mut i = 0usize;
    while i < args.args.len() {
        let arg = &args.args[i];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            i += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') {
            match arg.as_str() {
                "-a" | "--all" => show_all = true,
                "--octopus" => mode = choose_mode(mode, Mode::Octopus)?,
                "--independent" => mode = choose_mode(mode, Mode::Independent)?,
                "--is-ancestor" => mode = choose_mode(mode, Mode::IsAncestor)?,
                "--fork-point" => mode = choose_mode(mode, Mode::ForkPoint)?,
                _ => bail!("unsupported option: {arg}"),
            }
            i += 1;
            continue;
        }
        revisions.push(arg.clone());
        i += 1;
    }

    match mode {
        Mode::Default => run_default(&repo, show_all, revisions),
        Mode::Octopus => run_octopus(&repo, show_all, revisions),
        Mode::Independent => run_independent(&repo, show_all, revisions),
        Mode::IsAncestor => run_is_ancestor(&repo, show_all, revisions),
        Mode::ForkPoint => run_fork_point(&repo, show_all, revisions),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Default,
    Octopus,
    Independent,
    IsAncestor,
    ForkPoint,
}

fn choose_mode(current: Mode, requested: Mode) -> Result<Mode> {
    if current == Mode::Default || current == requested {
        return Ok(requested);
    }
    bail!("incompatible operation modes");
}

fn run_default(repo: &Repository, show_all: bool, revisions: Vec<String>) -> Result<()> {
    if revisions.len() < 2 {
        bail!("usage: grit merge-base [-a | --all] <commit> <commit>...");
    }
    let commits = resolve_commit_specs(repo, &revisions)?;
    let bases = if commits.len() == 2 {
        merge_bases_first_vs_rest(repo, commits[0], &commits[1..])?
    } else {
        merge_bases_octopus(repo, &commits)?
    };
    print_result(bases, show_all);
    Ok(())
}

fn run_octopus(repo: &Repository, show_all: bool, revisions: Vec<String>) -> Result<()> {
    if revisions.is_empty() {
        bail!("usage: grit merge-base [-a | --all] --octopus <commit>...");
    }
    let commits = resolve_commit_specs(repo, &revisions)?;
    let bases = merge_bases_octopus(repo, &commits)?;
    print_result(bases, show_all);
    Ok(())
}

fn run_independent(repo: &Repository, show_all: bool, revisions: Vec<String>) -> Result<()> {
    if show_all {
        bail!("options '--independent' and '--all' cannot be used together");
    }
    if revisions.is_empty() {
        bail!("usage: grit merge-base --independent <commit>...");
    }
    let commits = resolve_commit_specs(repo, &revisions)?;
    for oid in independent_commits(repo, &commits)? {
        println!("{oid}");
    }
    Ok(())
}

fn run_is_ancestor(repo: &Repository, show_all: bool, revisions: Vec<String>) -> Result<()> {
    if show_all {
        bail!("options '--is-ancestor' and '--all' cannot be used together");
    }
    if revisions.len() != 2 {
        bail!("--is-ancestor takes exactly two commits");
    }
    let commits = resolve_commit_specs(repo, &revisions)?;
    let yes = is_ancestor(repo, commits[0], commits[1])?;
    std::process::exit(if yes { 0 } else { 1 });
}

fn run_fork_point(repo: &Repository, show_all: bool, revisions: Vec<String>) -> Result<()> {
    if show_all {
        bail!("options '--fork-point' and '--all' cannot be used together");
    }
    if revisions.is_empty() || revisions.len() > 2 {
        bail!("usage: grit merge-base --fork-point <ref> [<commit>]");
    }

    let upstream_spec = revisions[0].clone();
    let commit_spec = revisions
        .get(1)
        .cloned()
        .unwrap_or_else(|| "HEAD".to_string());
    let commits = resolve_commit_specs(repo, &[upstream_spec.clone(), commit_spec])?;
    let upstream_oid = commits[0];
    let commit_oid = commits[1];

    if let Some(fork_point) =
        merge_base_fork_point(repo, &repo.git_dir, &upstream_spec, commit_oid)?
    {
        println!("{fork_point}");
        return Ok(());
    }

    let mut bases = merge_bases_first_vs_rest(repo, upstream_oid, &[commit_oid])?;
    if bases.is_empty() {
        std::process::exit(1);
    }
    bases.sort();
    println!("{}", bases[0]);
    Ok(())
}

fn print_result(mut oids: Vec<grit_lib::objects::ObjectId>, show_all: bool) {
    if oids.is_empty() {
        std::process::exit(1);
    }
    if show_all {
        for oid in oids {
            println!("{oid}");
        }
        return;
    }
    // Stable deterministic single-result behavior.
    oids.sort();
    println!("{}", oids[0]);
}
