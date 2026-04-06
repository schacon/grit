//! `grit show-branch` — show branches and their commits.
//!
//! Lists branch heads with abbreviated commit hash and subject line,
//! similar to `git show-branch` in its basic mode.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::merge_base::{independent_commits, merge_bases_octopus, resolve_commit_specs};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit show-branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show branches and their commits")]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run the `show-branch` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let mut mode = Mode::List;
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
                "--all" => {}
                "--merge-base" => mode = choose_mode(mode, Mode::MergeBase)?,
                "--independent" => mode = choose_mode(mode, Mode::Independent)?,
                _ => bail!("unsupported option: {arg}"),
            }
            i += 1;
            continue;
        }
        revisions.push(arg.clone());
        i += 1;
    }

    match mode {
        Mode::List => run_list_mode(&repo, revisions),
        Mode::MergeBase => run_merge_base_mode(&repo, revisions),
        Mode::Independent => run_independent_mode(&repo, revisions),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    MergeBase,
    Independent,
}

fn choose_mode(current: Mode, requested: Mode) -> Result<Mode> {
    if current == Mode::List || current == requested {
        return Ok(requested);
    }
    bail!("incompatible operation modes");
}

fn run_merge_base_mode(repo: &Repository, revisions: Vec<String>) -> Result<()> {
    if revisions.len() < 2 {
        bail!("usage: grit show-branch --merge-base <commit> <commit>...");
    }
    let commits = resolve_commit_specs(repo, &revisions)?;
    let mut bases = merge_bases_octopus(repo, &commits)?;
    if bases.is_empty() {
        std::process::exit(1);
    }
    bases.sort();
    println!("{}", bases[0]);
    Ok(())
}

fn run_independent_mode(repo: &Repository, revisions: Vec<String>) -> Result<()> {
    if revisions.is_empty() {
        bail!("usage: grit show-branch --independent <commit>...");
    }
    let commits = resolve_commit_specs(repo, &revisions)?;
    for oid in independent_commits(repo, &commits)? {
        println!("{oid}");
    }
    Ok(())
}

fn run_list_mode(repo: &Repository, branches_arg: Vec<String>) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    let current_branch = match &head {
        HeadState::Branch { short_name, .. } => Some(short_name.clone()),
        _ => None,
    };

    let mut branches: Vec<(String, ObjectId)> = Vec::new();

    if branches_arg.is_empty() {
        // List all local branches
        let heads_dir = repo.git_dir.join("refs/heads");
        collect_branches(&heads_dir, "", &mut branches)?;
        branches.sort_by(|a, b| a.0.cmp(&b.0));
    } else {
        for name in &branches_arg {
            let ref_path = repo.git_dir.join("refs/heads").join(name);
            if let Ok(content) = fs::read_to_string(&ref_path) {
                if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                    branches.push((name.clone(), oid));
                }
            }
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (name, oid) in &branches {
        let hex = oid.to_hex();
        let abbrev = &hex[..7.min(hex.len())];

        let subject = match repo.odb.read(oid) {
            Ok(obj) => match parse_commit(&obj.data) {
                Ok(commit) => commit.message.lines().next().unwrap_or("").to_owned(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        };

        let marker = if current_branch.as_deref() == Some(name.as_str()) {
            "* "
        } else {
            "  "
        };

        writeln!(out, "{marker}[{name}] {abbrev} {subject}")?;
    }

    Ok(())
}

/// Recursively collect branches from the heads directory.
fn collect_branches(dir: &Path, prefix: &str, out: &mut Vec<(String, ObjectId)>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            file_name
        } else {
            format!("{prefix}/{file_name}")
        };

        if path.is_dir() {
            collect_branches(&path, &full_name, out)?;
        } else if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                out.push((full_name, oid));
            }
        }
    }

    Ok(())
}
