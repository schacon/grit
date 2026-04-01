//! `grit rev-list` command.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{
    collect_revision_specs_with_stdin, render_commit, rev_list, tag_targets, OrderingMode,
    OutputMode, RevListOptions,
};

/// Arguments for `grit rev-list`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit rev-list`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;

    let mut options = RevListOptions::default();
    let mut abbrev_len = 7usize;
    let mut revision_specs = Vec::new();
    let mut read_stdin = false;
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
                "--all" => options.all_refs = true,
                "--first-parent" => options.first_parent = true,
                "--ancestry-path" => options.ancestry_path = true,
                "--simplify-by-decoration" => options.simplify_by_decoration = true,
                "--topo-order" => options.ordering = OrderingMode::Topo,
                "--date-order" => options.ordering = OrderingMode::Date,
                "--reverse" => options.reverse = true,
                "--count" => options.count = true,
                "--parents" => options.output_mode = OutputMode::Parents,
                "--quiet" => options.quiet = true,
                "--stdin" => read_stdin = true,
                "--end-of-options" => end_of_options = true,
                "-n" => {
                    let Some(value) = args.args.get(i + 1) else {
                        bail!("-n requires an argument");
                    };
                    options.max_count = Some(parse_non_negative(value, "-n")?);
                    i += 1;
                }
                _ if arg.starts_with("--max-count=") => {
                    let value = arg.trim_start_matches("--max-count=");
                    options.max_count = Some(parse_non_negative(value, "--max-count")?);
                }
                _ if arg.starts_with("--skip=") => {
                    let value = arg.trim_start_matches("--skip=");
                    options.skip = parse_non_negative(value, "--skip")?;
                }
                _ if arg.starts_with("--format=") => {
                    let value = arg.trim_start_matches("--format=").to_owned();
                    options.output_mode = OutputMode::Format(value);
                }
                _ if arg.starts_with("--abbrev=") => {
                    let value = arg.trim_start_matches("--abbrev=");
                    abbrev_len = parse_non_negative(value, "--abbrev")?;
                }
                _ if arg.starts_with("-n") && arg.len() > 2 => {
                    let value = &arg[2..];
                    options.max_count = Some(parse_non_negative(value, "-n")?);
                }
                _ if arg.starts_with('-')
                    && arg.len() > 1
                    && arg[1..].chars().all(|ch| ch.is_ascii_digit()) =>
                {
                    options.max_count = Some(parse_non_negative(&arg[1..], "-<n>")?);
                }
                _ if arg.starts_with("--ancestry-path=") => {
                    let value = arg.trim_start_matches("--ancestry-path=");
                    let oid =
                        grit_lib::rev_parse::resolve_revision(&repo, value).with_context(|| {
                            format!("could not get commit for --ancestry-path argument {value}")
                        })?;
                    options.ancestry_path = true;
                    options.ancestry_path_bottoms.push(oid);
                }
                _ => bail!("unsupported option: {arg}"),
            }
            i += 1;
            continue;
        }
        revision_specs.push(arg.clone());
        i += 1;
    }

    if options.simplify_by_decoration {
        // Decoration subset: keep commits pointed to by tags only.
        let decorated = tag_targets(&repo.git_dir).context("failed to list tag refs")?;
        if decorated.is_empty() {
            options.simplify_by_decoration = false;
        }
    }

    let (positive_specs, negative_specs, stdin_all_refs) =
        collect_revision_specs_with_stdin(&revision_specs, read_stdin)
            .context("failed to parse revision arguments")?;
    if stdin_all_refs {
        options.all_refs = true;
    }

    let result =
        rev_list(&repo, &positive_specs, &negative_specs, &options).context("rev-list failed")?;

    if options.count {
        println!("{}", result.commits.len());
        return Ok(());
    }
    if options.quiet {
        return Ok(());
    }

    for oid in result.commits {
        match &options.output_mode {
            OutputMode::Format(_) => {
                println!("commit {oid}");
                let rendered = render_commit(&repo, oid, &options.output_mode, abbrev_len)?;
                println!("{rendered}");
            }
            _ => {
                let rendered = render_commit(&repo, oid, &options.output_mode, abbrev_len)?;
                println!("{rendered}");
            }
        }
    }
    Ok(())
}

fn parse_non_negative(text: &str, flag: &str) -> Result<usize> {
    let value = text
        .parse::<isize>()
        .with_context(|| format!("{flag} requires an integer"))?;
    if value < 0 {
        return Ok(usize::MAX);
    }
    Ok(value as usize)
}
