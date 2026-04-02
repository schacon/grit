//! `grit mailsplit` — split mbox into individual messages.
//!
//! Reads an mbox-format file and splits it into numbered message files
//! in the output directory, printing the count to stdout.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Arguments for `grit mailsplit`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Split mbox into individual messages",
    override_usage = "grit mailsplit -o<dir> [--keep-cr] <mbox>..."
)]
pub struct Args {
    /// Output directory for split messages (e.g., `-odir`).
    #[arg(short = 'o', long = "output")]
    pub output: PathBuf,

    /// Preserve CR at end of lines.
    #[arg(long = "keep-cr")]
    pub keep_cr: bool,

    /// Skip first N messages.
    #[arg(short = 'd', long = "skip", default_value = "0")]
    pub skip: usize,

    /// The mbox file(s) to split. Uses stdin if omitted.
    pub mbox: Vec<PathBuf>,
}

/// Run `grit mailsplit`.
pub fn run(args: Args) -> Result<()> {
    // Ensure the output directory exists
    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating output dir {:?}", args.output))?;

    let mut count: usize = 0;

    if args.mbox.is_empty() {
        // Read from stdin
        let data = {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        };
        count = split_mbox(&data, &args.output, count, args.skip, args.keep_cr)?;
    } else {
        for mbox_path in &args.mbox {
            let data = fs::read_to_string(mbox_path)
                .with_context(|| format!("reading mbox {:?}", mbox_path))?;
            count = split_mbox(&data, &args.output, count, args.skip, args.keep_cr)?;
        }
    }

    println!("{}", count);
    Ok(())
}

/// Split mbox content into numbered files.
/// Returns the total count of messages written (cumulative from `start`).
fn split_mbox(
    data: &str,
    output: &std::path::Path,
    start: usize,
    skip: usize,
    _keep_cr: bool,
) -> Result<usize> {
    let mut messages: Vec<String> = Vec::new();
    let mut current: Option<String> = None;

    for line in data.lines() {
        if is_mbox_from_line(line) {
            // Start a new message
            if let Some(msg) = current.take() {
                messages.push(msg);
            }
            current = Some(format!("{}\n", line));
        } else if let Some(ref mut msg) = current {
            msg.push_str(line);
            msg.push('\n');
        }
        // Lines before the first From line are ignored (mbox preamble)
    }
    // Don't forget the last message
    if let Some(msg) = current.take() {
        messages.push(msg);
    }

    let mut count = start;
    for (i, msg) in messages.into_iter().enumerate() {
        if i < skip {
            continue;
        }
        count += 1;
        let filename = format!("{:04}", count);
        let path = output.join(&filename);
        let mut f = fs::File::create(&path)
            .with_context(|| format!("creating {:?}", path))?;
        f.write_all(msg.as_bytes())?;
    }

    Ok(count)
}

/// Check if a line is an mbox "From " separator.
///
/// Traditional mbox format: lines starting with "From " followed by an
/// email address and a date are message separators.
fn is_mbox_from_line(line: &str) -> bool {
    // Git uses a simple heuristic: line starts with "From "
    line.starts_with("From ")
}
