//! `grit merge-file` — three-way file merge.
//!
//! Merges `<current-file>` (ours), `<base-file>` (ancestor), and
//! `<other-file>` (theirs) line-by-line.  The result is written back to
//! `<current-file>` unless `-p` / `--stdout` is given.
//!
//! Exit codes follow git: 0 = clean merge, 1 = conflicts present, >1 = error.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::merge_file::{is_binary, merge, ConflictStyle, MergeFavor, MergeInput};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

/// Arguments for `grit merge-file`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Run a three-way file merge",
    long_about = "Incorporates all changes that lead from <base-file> to <other-file>\n\
                  into <current-file>. The result ordinarily goes into <current-file>."
)]
pub struct Args {
    /// Send results to standard output instead of overwriting <current-file>.
    #[arg(short = 'p', long = "stdout")]
    pub stdout: bool,

    /// Do not warn about conflicts.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Use a diff3 based merge.
    #[arg(long = "diff3", conflicts_with = "zdiff3")]
    pub diff3: bool,

    /// Use a zealous diff3 based merge.
    #[arg(long = "zdiff3", conflicts_with = "diff3")]
    pub zdiff3: bool,

    /// For conflicts, use our version.
    #[arg(long = "ours", conflicts_with_all = &["theirs", "union"])]
    pub ours: bool,

    /// For conflicts, use their version.
    #[arg(long = "theirs", conflicts_with_all = &["ours", "union"])]
    pub theirs: bool,

    /// For conflicts, use a union version.
    #[arg(long = "union", conflicts_with_all = &["ours", "theirs"])]
    pub union: bool,

    /// Set labels for file1 / orig-file / file2 (up to 3 times).
    #[arg(short = 'L', value_name = "name", action = clap::ArgAction::Append, num_args = 1)]
    pub label: Vec<String>,

    /// Use this many characters for conflict markers.
    #[arg(long = "marker-size", value_name = "n")]
    pub marker_size: Option<usize>,

    /// Current file (ours, will be overwritten unless -p).
    #[arg(value_name = "current-file")]
    pub current: PathBuf,

    /// Base file (ancestor).
    #[arg(value_name = "base-file")]
    pub base: PathBuf,

    /// Other file (theirs).
    #[arg(value_name = "other-file")]
    pub other: PathBuf,
}

/// Run the `merge-file` command.
///
/// Returns `Ok(())` on clean merge, but exits with code 1 when conflicts are
/// present (handled in [`run_with_exit_code`]).
///
/// # Errors
///
/// Returns an error when files cannot be read or written, or when binary
/// files are passed.
pub fn run(args: Args) -> Result<()> {
    std::process::exit(run_inner(args)?);
}

/// Inner implementation; returns the process exit code.
pub fn run_inner(args: Args) -> Result<i32> {
    if args.label.len() > 3 {
        bail!("too many labels on the command line");
    }

    let current_bytes = fs::read(&args.current)
        .with_context(|| format!("cannot read '{}'", args.current.display()))?;
    let base_bytes =
        fs::read(&args.base).with_context(|| format!("cannot read '{}'", args.base.display()))?;
    let other_bytes =
        fs::read(&args.other).with_context(|| format!("cannot read '{}'", args.other.display()))?;

    // Binary detection.
    for (data, path) in [
        (&current_bytes, &args.current),
        (&base_bytes, &args.base),
        (&other_bytes, &args.other),
    ] {
        if is_binary(data) {
            bail!("Cannot merge binary files: {}", path.display());
        }
    }

    // Labels default to file names.
    let label_ours = args
        .label
        .first()
        .map(|s| s.as_str())
        .unwrap_or_else(|| args.current.to_str().unwrap_or("ours"));
    let label_base = args
        .label
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or_else(|| args.base.to_str().unwrap_or("base"));
    let label_theirs = args
        .label
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or_else(|| args.other.to_str().unwrap_or("theirs"));

    let favor = if args.ours {
        MergeFavor::Ours
    } else if args.theirs {
        MergeFavor::Theirs
    } else if args.union {
        MergeFavor::Union
    } else {
        MergeFavor::None
    };

    let style = if args.diff3 {
        ConflictStyle::Diff3
    } else if args.zdiff3 {
        ConflictStyle::ZealousDiff3
    } else {
        ConflictStyle::Merge
    };

    let input = MergeInput {
        base: &base_bytes,
        ours: &current_bytes,
        theirs: &other_bytes,
        label_ours,
        label_base,
        label_theirs,
        favor,
        style,
        marker_size: args.marker_size.unwrap_or(0),
    };

    let result = merge(&input).context("merge failed")?;

    if result.conflicts > 0 && !args.quiet {
        eprintln!(
            "warning: conflicts during merge of {}",
            args.current.display()
        );
    }

    if args.stdout {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        out.write_all(&result.content)
            .context("writing to stdout")?;
    } else {
        fs::write(&args.current, &result.content)
            .with_context(|| format!("cannot write '{}'", args.current.display()))?;
    }

    if result.conflicts > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}
