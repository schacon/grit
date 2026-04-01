//! `grit name-rev` — name commits relative to refs.
//!
//! Resolves each commit OID argument to a human-readable name derived from the
//! nearest ref tip.  Supports `--all` (name every reachable commit),
//! `--annotate-stdin` / `--stdin` (annotate OIDs in piped text), `--name-only`,
//! `--tags`, `--refs`, `--exclude`, `--no-undefined`, and `--always`.

use std::io::{self, BufRead};

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::name_rev::{
    abbrev_oid, annotate_line, build_name_map, lookup_name, object_exists, resolve_oid,
    walk_all_commits, NameRevOptions,
};
use grit_lib::repo::Repository;

/// Arguments for `grit name-rev`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments (options and commit OIDs).
    #[arg(
        value_name = "ARG",
        num_args = 0..,
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    pub args: Vec<String>,
}

/// Run `grit name-rev`.
///
/// # Errors
///
/// Returns errors from repository discovery, object lookup, or I/O.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;

    let mut options = NameRevOptions::default();
    let mut name_only = false;
    let mut all = false;
    let mut annotate_stdin = false;
    let mut allow_undefined = true;
    let mut always = false;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0usize;
    while i < args.args.len() {
        let arg = &args.args[i];
        match arg.as_str() {
            "--name-only" | "--no-name" => name_only = true,
            "--tags" => options.tags_only = true,
            "--all" => all = true,
            "--annotate-stdin" => annotate_stdin = true,
            "--stdin" => {
                // Deprecated alias; emit warning to stderr and behave the same.
                eprintln!(
                    "warning: --stdin is deprecated. \
                     Please use --annotate-stdin instead, which is functionally equivalent.\n\
                     This option will be removed in a future release."
                );
                annotate_stdin = true;
            }
            "--undefined" => allow_undefined = true,
            "--no-undefined" => allow_undefined = false,
            "--always" => always = true,
            "--no-always" => always = false,
            "--" => {
                // Everything after `--` is positional.
                positional.extend(args.args[i + 1..].iter().cloned());
                break;
            }
            _ if arg.starts_with("--refs=") => {
                options
                    .ref_filters
                    .push(arg.trim_start_matches("--refs=").to_owned());
            }
            _ if arg.starts_with("--exclude=") => {
                options
                    .exclude_filters
                    .push(arg.trim_start_matches("--exclude=").to_owned());
            }
            _ if arg.starts_with('-') => {
                bail!("unknown option: {arg}");
            }
            _ => {
                positional.push(arg.clone());
            }
        }
        i += 1;
    }

    // Validate mutual exclusivity.
    let mode_count = positional.len() + usize::from(all) + usize::from(annotate_stdin);
    if mode_count > 1 && !positional.is_empty() && (all || annotate_stdin) {
        bail!("Specify either a list, or --all, not both!");
    }

    // Tags are shortened when both --tags and --name-only are active together.
    options.shorten_tags = options.tags_only && name_only;

    // Build the OID → name map from all applicable refs.
    let name_map = build_name_map(&repo, &options).context("failed to build name map")?;

    if annotate_stdin {
        // Read stdin line by line and annotate each line.
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.context("failed to read stdin")?;
            let annotated =
                annotate_line(&repo, &name_map, &line, name_only).context("annotation failed")?;
            print!("{annotated}");
            if !annotated.ends_with('\n') {
                println!();
            }
        }
        return Ok(());
    }

    if all {
        let commits = walk_all_commits(&repo).context("failed to enumerate reachable commits")?;
        for oid in commits {
            let name = name_map.get(&oid).map(String::as_str);
            print_result(oid, &oid.to_hex(), name, always, allow_undefined, name_only)?;
        }
        return Ok(());
    }

    // Named commit OIDs / ref specs.
    for spec in &positional {
        let oid = match resolve_oid(&repo, spec) {
            Ok(id) => id,
            Err(_) => {
                eprintln!("Could not get sha1 for {spec}. Skipping.");
                continue;
            }
        };

        if !object_exists(&repo, oid) {
            eprintln!("Could not get object for {spec}. Skipping.");
            continue;
        }

        let name = lookup_name(&repo, &name_map, oid).context("name lookup failed")?;
        let caller_name = spec.as_str();
        print_result(
            oid,
            caller_name,
            name.map(String::as_str),
            always,
            allow_undefined,
            name_only,
        )?;
    }

    Ok(())
}

/// Print a single name-rev result line.
///
/// Format:
/// - Normal: `<caller_name> <name>\n`  (or `<caller_name> undefined\n` / error)
/// - `--name-only`: `<name>\n`
///
/// # Errors
///
/// Returns [`anyhow::Error`] when `allow_undefined` is false and no name was
/// found and `always` is also false.
fn print_result(
    oid: grit_lib::objects::ObjectId,
    caller_name: &str,
    name: Option<&str>,
    always: bool,
    allow_undefined: bool,
    name_only: bool,
) -> Result<()> {
    if !name_only {
        print!("{caller_name} ");
    }

    if let Some(n) = name {
        println!("{n}");
    } else if allow_undefined {
        println!("undefined");
    } else if always {
        println!("{}", abbrev_oid(oid, 7));
    } else {
        bail!("cannot describe '{}'", oid.to_hex());
    }
    Ok(())
}
