//! `grit check-ignore` - debug gitignore / exclude matching.

use anyhow::{anyhow, bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::ignore::{normalize_repo_relative, IgnoreMatcher};
use grit_lib::index::Index;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;

/// Arguments for `grit check-ignore`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit check-ignore`.
pub fn run(args: Args) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    let repo = Repository::discover(None)
        .context("not a git repository (or any of the parent directories)")?;
    let work_tree = repo
        .work_tree
        .as_ref()
        .ok_or_else(|| anyhow!("this operation must be run in a work tree"))?;

    let parsed = parse_args(&args.args)?;
    validate_args(&parsed)?;

    let index = if parsed.no_index {
        None
    } else {
        Some(Index::load(&repo.index_path()).context("failed to read index")?)
    };
    let index_ref = index.as_ref();

    let mut matcher =
        IgnoreMatcher::from_repository(&repo).context("failed to load ignore rules")?;

    let mut queries = if parsed.stdin {
        read_stdin_paths(parsed.nul_terminated)?
    } else {
        parsed.paths.clone()
    };

    if queries.is_empty() && parsed.stdin {
        std::process::exit(1);
    }

    let mut out = io::stdout().lock();
    let mut matched_count = 0usize;
    for raw_path in queries.drain(..) {
        let repo_rel = normalize_repo_relative(&repo, &cwd, &raw_path)
            .map_err(|err| anyhow!(err.to_string()))?;
        let abs = work_tree.join(Path::new(&repo_rel));
        let is_dir = fs::metadata(&abs).map(|m| m.is_dir()).unwrap_or(false);

        let (ignored, matched) = matcher
            .check_path(&repo, index_ref, &repo_rel, is_dir)
            .map_err(|err| anyhow!(err.to_string()))?;

        let reportable_match = matched
            .as_ref()
            .map(|rule| parsed.verbose || !rule.negative)
            .unwrap_or(false);
        if reportable_match {
            matched_count += 1;
        }

        if parsed.quiet {
            continue;
        }

        if parsed.verbose {
            if let Some(matched_rule) = matched {
                write_verbose_record(
                    &mut out,
                    parsed.nul_terminated,
                    &matched_rule.source_display,
                    matched_rule.line_number,
                    &matched_rule.pattern_text,
                    &raw_path,
                )?;
            } else if parsed.non_matching {
                write_verbose_non_match(&mut out, parsed.nul_terminated, &raw_path)?;
            }
        } else if ignored {
            write_plain_record(&mut out, parsed.nul_terminated, &raw_path)?;
        }
    }
    out.flush().context("failed to flush output")?;

    if matched_count > 0 {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

#[derive(Debug, Default)]
struct ParsedArgs {
    quiet: bool,
    verbose: bool,
    stdin: bool,
    nul_terminated: bool,
    non_matching: bool,
    no_index: bool,
    paths: Vec<String>,
}

fn parse_args(raw: &[String]) -> Result<ParsedArgs> {
    let mut parsed = ParsedArgs::default();
    let mut i = 0usize;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "--" => {
                parsed.paths.extend(raw.iter().skip(i + 1).cloned());
                break;
            }
            "-q" | "--quiet" => parsed.quiet = true,
            "-v" | "--verbose" => parsed.verbose = true,
            "--stdin" => parsed.stdin = true,
            "-z" => parsed.nul_terminated = true,
            "-n" | "--non-matching" => parsed.non_matching = true,
            "--no-index" => parsed.no_index = true,
            _ if arg.starts_with('-') => bail!("unsupported option: {arg}"),
            _ => parsed.paths.push(arg.clone()),
        }
        i += 1;
    }
    Ok(parsed)
}

fn validate_args(args: &ParsedArgs) -> Result<()> {
    if args.stdin && !args.paths.is_empty() {
        bail!("cannot specify pathnames with --stdin");
    }
    if !args.stdin {
        if args.nul_terminated {
            bail!("-z only makes sense with --stdin");
        }
        if args.paths.is_empty() {
            bail!("no path specified");
        }
    }
    if args.quiet {
        if args.verbose {
            bail!("cannot have both --quiet and --verbose");
        }
        if args.stdin || args.paths.len() != 1 {
            bail!("--quiet is only valid with a single pathname");
        }
    }
    if args.non_matching && !args.verbose {
        bail!("--non-matching is only valid with --verbose");
    }
    Ok(())
}

fn read_stdin_paths(nul_terminated: bool) -> Result<Vec<String>> {
    if nul_terminated {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .context("failed to read stdin")?;
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for chunk in buf.split(|b| *b == b'\0') {
            if chunk.is_empty() {
                continue;
            }
            paths.push(String::from_utf8_lossy(chunk).to_string());
        }
        return Ok(paths);
    }

    let stdin = io::stdin();
    let mut paths = Vec::new();
    for line in stdin.lock().lines() {
        let line = line.context("failed reading stdin line")?;
        if line.is_empty() {
            continue;
        }
        paths.push(line);
    }
    Ok(paths)
}

fn write_plain_record(out: &mut dyn Write, nul_terminated: bool, path: &str) -> Result<()> {
    if nul_terminated {
        out.write_all(path.as_bytes())?;
        out.write_all(b"\0")?;
    } else {
        writeln!(out, "{path}")?;
    }
    Ok(())
}

fn write_verbose_record(
    out: &mut dyn Write,
    nul_terminated: bool,
    source: &str,
    line_number: usize,
    pattern: &str,
    path: &str,
) -> Result<()> {
    if nul_terminated {
        out.write_all(source.as_bytes())?;
        out.write_all(b"\0")?;
        out.write_all(line_number.to_string().as_bytes())?;
        out.write_all(b"\0")?;
        out.write_all(pattern.as_bytes())?;
        out.write_all(b"\0")?;
        out.write_all(path.as_bytes())?;
        out.write_all(b"\0")?;
    } else {
        writeln!(out, "{source}:{line_number}:{pattern}\t{path}")?;
    }
    Ok(())
}

fn write_verbose_non_match(out: &mut dyn Write, nul_terminated: bool, path: &str) -> Result<()> {
    if nul_terminated {
        out.write_all(b"\0\0\0")?;
        out.write_all(path.as_bytes())?;
        out.write_all(b"\0")?;
    } else {
        writeln!(out, "::\t{path}")?;
    }
    Ok(())
}
