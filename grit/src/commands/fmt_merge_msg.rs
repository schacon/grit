//! `grit fmt-merge-msg` — format a merge commit message.
//!
//! Reads FETCH_HEAD-style merge information from stdin (or a file) and
//! produces a suitable merge commit message.
//!
//! # Example
//!
//! ```text
//! $ git fetch origin feature
//! $ grit fmt-merge-msg < .git/FETCH_HEAD
//! Merge branch 'feature' of https://example.com/repo
//! ```

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::fmt_merge_msg::{fmt_merge_msg, FmtMergeMsgOptions};
use std::io::Read;

/// Arguments for `grit fmt-merge-msg`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Use this text instead of branch names for the first line.
    #[arg(short = 'm', long)]
    pub message: Option<String>,

    /// Prepare the merge message as if merging into this branch.
    #[arg(long = "into-name", value_name = "BRANCH")]
    pub into_name: Option<String>,

    /// Read merge info from this file instead of stdin.
    #[arg(short = 'F', long = "file", value_name = "FILE")]
    pub file: Option<std::path::PathBuf>,

    /// Include one-line commit descriptions (at most N per parent; default 20).
    /// This flag is accepted for compatibility but the log body is not
    /// currently generated (only the title line is produced).
    #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "20")]
    pub log: Option<u32>,

    /// Do not include one-line descriptions (overrides --log).
    #[arg(long = "no-log")]
    pub no_log: bool,
}

/// Run `grit fmt-merge-msg`.
pub fn run(args: Args) -> Result<()> {
    let input = read_input(args.file.as_deref())?;

    let opts = FmtMergeMsgOptions {
        message: args.message,
        into_name: args.into_name,
    };

    let output = fmt_merge_msg(&input, &opts);
    print!("{output}");
    Ok(())
}

/// Read the merge info from a file or from stdin.
fn read_input(file: Option<&std::path::Path>) -> Result<String> {
    if let Some(path) = file {
        std::fs::read_to_string(path).with_context(|| format!("cannot open '{}'", path.display()))
    } else {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("could not read from stdin")?;
        Ok(buf)
    }
}
