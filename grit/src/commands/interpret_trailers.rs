//! `grit interpret-trailers` — add or parse structured trailers in commit messages.

use anyhow::{bail, Context, Result};
use grit_lib::interpret_trailers::{
    complete_line, process_trailers, NewTrailerArg, ProcessTrailerOptions, TrailerIfExists,
    TrailerIfMissing, TrailerWhere,
};
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Read, Write};
/// Arguments for `grit interpret-trailers` (manual parse to match Git's per-`--trailer` state).
#[derive(Debug, Default)]
pub struct ParsedCli {
    pub opts: ProcessTrailerOptions,
    pub in_place: bool,
    pub trailer_specs: Vec<NewTrailerArg>,
    pub files: Vec<String>,
}

pub fn parse_interpret_trailers_argv(args: &[String]) -> Result<ParsedCli> {
    let mut out = ParsedCli::default();
    let mut i = 0usize;
    let mut cli_where = TrailerWhere::Default;
    let mut cli_if_exists = TrailerIfExists::Default;
    let mut cli_if_missing = TrailerIfMissing::Default;

    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--in-place" | "-i" => {
                out.in_place = true;
                i += 1;
            }
            "--trim-empty" => {
                out.opts.trim_empty = true;
                i += 1;
            }
            "--parse" => {
                out.opts.only_trailers = true;
                out.opts.only_input = true;
                out.opts.unfold = true;
                i += 1;
            }
            "--only-trailers" => {
                out.opts.only_trailers = true;
                i += 1;
            }
            "--only-input" => {
                out.opts.only_input = true;
                i += 1;
            }
            "--unfold" => {
                out.opts.unfold = true;
                i += 1;
            }
            "--no-divider" => {
                out.opts.no_divider = true;
                i += 1;
            }
            "--where" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("option '--where' requires a value"))?;
                cli_where = parse_where_value(v)?;
                i += 2;
            }
            "--no-where" => {
                cli_where = TrailerWhere::Default;
                i += 1;
            }
            "--if-exists" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("option '--if-exists' requires a value"))?;
                cli_if_exists = parse_if_exists_value(v)?;
                i += 2;
            }
            "--no-if-exists" => {
                cli_if_exists = TrailerIfExists::Default;
                i += 1;
            }
            "--if-missing" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("option '--if-missing' requires a value"))?;
                cli_if_missing = parse_if_missing_value(v)?;
                i += 2;
            }
            "--no-if-missing" => {
                cli_if_missing = TrailerIfMissing::Default;
                i += 1;
            }
            "--trailer" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("option '--trailer' requires a value"))?;
                out.trailer_specs.push(NewTrailerArg {
                    text: v.clone(),
                    where_: cli_where,
                    if_exists: cli_if_exists,
                    if_missing: cli_if_missing,
                });
                i += 2;
            }
            "--" => {
                out.files.extend(args[i + 1..].iter().cloned());
                break;
            }
            x if x.starts_with('-') && x != "-" => {
                bail!("unknown option '{x}'");
            }
            _ => {
                out.files.push(a.clone());
                i += 1;
            }
        }
    }

    if out.opts.only_input
        && out.opts.only_trailers
        && out.opts.unfold
        && !out.trailer_specs.is_empty()
    {
        bail!("--trailer with --only-input does not make sense");
    }

    Ok(out)
}

fn parse_where_value(v: &str) -> Result<TrailerWhere> {
    grit_lib::interpret_trailers::trailer_where_from_str(v)
        .ok_or_else(|| anyhow::anyhow!("unknown --where value '{v}'"))
}

fn parse_if_exists_value(v: &str) -> Result<TrailerIfExists> {
    grit_lib::interpret_trailers::trailer_if_exists_from_str(v)
        .ok_or_else(|| anyhow::anyhow!("unknown --if-exists value '{v}'"))
}

fn parse_if_missing_value(v: &str) -> Result<TrailerIfMissing> {
    grit_lib::interpret_trailers::trailer_if_missing_from_str(v)
        .ok_or_else(|| anyhow::anyhow!("unknown --if-missing value '{v}'"))
}

/// Run `interpret-trailers` from already-split argv (subcommand name stripped).
pub fn run_from_argv(args: &[String]) -> Result<()> {
    let parsed = parse_interpret_trailers_argv(args)?;
    run_parsed(parsed)
}

fn discover_git_dir() -> Option<std::path::PathBuf> {
    Repository::discover(None).ok().map(|r| r.git_dir)
}

fn run_parsed(parsed: ParsedCli) -> Result<()> {
    let git_dir = discover_git_dir();

    if parsed.files.is_empty() {
        if parsed.in_place {
            bail!("no input file given for in-place editing");
        }
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .context("reading stdin")?;
        let input = complete_line(&input);
        let output = process_trailers(
            &input,
            &parsed.opts,
            &parsed.trailer_specs,
            git_dir.as_deref(),
        );
        io::stdout().write_all(output.as_bytes())?;
        return Ok(());
    }

    for file in &parsed.files {
        let raw = fs::read_to_string(file).with_context(|| format!("reading '{file}'"))?;
        let input = complete_line(&raw);
        let output = process_trailers(
            &input,
            &parsed.opts,
            &parsed.trailer_specs,
            git_dir.as_deref(),
        );
        if parsed.in_place {
            fs::write(file, &output).with_context(|| format!("writing '{file}'"))?;
        } else {
            io::stdout().write_all(output.as_bytes())?;
        }
    }
    Ok(())
}
