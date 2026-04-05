//! `grit mailinfo` — extract patch from email message.
//!
//! Reads an email message from stdin, separates it into a commit message
//! (written to `<msg>`) and a patch (written to `<patch>`), and prints
//! author/subject metadata to stdout.  This is the plumbing behind
//! `git am`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// Arguments for `grit mailinfo`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Extract patch from a single email message",
    override_usage = "grit mailinfo [OPTIONS] <msg> <patch>"
)]
pub struct Args {
    /// Keep non-UTF-8 charsets as-is (do not re-encode).
    #[arg(short = 'k', long)]
    pub keep: bool,

    /// Keep everything in the body before a `---` line.
    #[arg(short = 'b', long)]
    pub keep_body: bool,

    /// Do not strip brackets from Subject.
    #[arg(short = 'u', long)]
    pub encoding: bool,

    /// Scissors mode: look for `-- >8 --` and discard everything before.
    #[arg(long)]
    pub scissors: bool,

    /// Disable scissors mode.
    #[arg(long = "no-scissors")]
    pub no_scissors: bool,

    /// Decode quoted-printable (default; ignored for compat).
    #[arg(long = "quoted-cr", hide = true)]
    pub quoted_cr: Option<String>,

    /// File to write the commit-message body to.
    pub msg: PathBuf,

    /// File to write the patch (diff) to.
    pub patch: PathBuf,
}

/// Run `grit mailinfo`.
pub fn run(args: Args) -> Result<()> {
    let stdin = io::stdin();
    let lines: Vec<String> = stdin
        .lock()
        .lines()
        .collect::<Result<Vec<_>, _>>()
        .context("reading stdin")?;

    let mut from = String::new();
    let mut subject = String::new();
    let mut date = String::new();
    let mut message_id = String::new();

    let mut body_lines: Vec<String> = Vec::new();
    let mut patch_lines: Vec<String> = Vec::new();
    let mut in_headers = true;
    let mut in_patch = false;
    let mut past_scissors = false;

    for line in &lines {
        if in_headers {
            if line.is_empty() {
                in_headers = false;
                continue;
            }
            if let Some(rest) = strip_header(line, "From:") {
                from = rest.trim().to_string();
            } else if let Some(rest) = strip_header(line, "Subject:") {
                subject = clean_subject(rest.trim(), args.keep);
            } else if let Some(rest) = strip_header(line, "Date:") {
                date = rest.trim().to_string();
            } else if let Some(rest) = strip_header(line, "Message-Id:") {
                message_id = rest.trim().to_string();
            } else if let Some(rest) = strip_header(line, "Message-ID:") {
                message_id = rest.trim().to_string();
            }
            continue;
        }

        // Handle scissors: discard everything before `-- >8 --`
        if args.scissors && !past_scissors
            && (line.contains("-- >8 --") || line.contains("-->8--")) {
                body_lines.clear();
                patch_lines.clear();
                past_scissors = true;
                continue;
            }

        if !in_patch {
            // Detect the start of the patch
            if line.starts_with("diff --git ")
                || line.starts_with("diff -")
                || line.starts_with("--- ")
                || line.starts_with("Index: ")
            {
                in_patch = true;
                patch_lines.push(line.clone());
            } else if line == "---" && !args.keep_body {
                // Separator between body and diffstat/patch — skip the `---` line,
                // body is everything before it.
                in_patch = true;
            } else {
                body_lines.push(line.clone());
            }
        } else {
            patch_lines.push(line.clone());
        }
    }

    // Write the commit message body
    let mut msg_file = std::fs::File::create(&args.msg).context("creating msg file")?;
    for line in &body_lines {
        writeln!(msg_file, "{}", line)?;
    }

    // Write the patch
    let mut patch_file = std::fs::File::create(&args.patch).context("creating patch file")?;
    for line in &patch_lines {
        writeln!(patch_file, "{}", line)?;
    }

    // Print metadata to stdout
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if !from.is_empty() {
        let (name, email) = parse_author(&from);
        writeln!(out, "Author: {}", name)?;
        writeln!(out, "Email: {}", email)?;
    }
    if !subject.is_empty() {
        writeln!(out, "Subject: {}", subject)?;
    }
    if !date.is_empty() {
        writeln!(out, "Date: {}", date)?;
    }
    if !message_id.is_empty() {
        writeln!(out, "Message-Id: {}", message_id)?;
    }

    Ok(())
}

/// Case-insensitive header stripping.
fn strip_header<'a>(line: &'a str, header: &str) -> Option<&'a str> {
    if line.len() >= header.len() && line[..header.len()].eq_ignore_ascii_case(header) {
        Some(&line[header.len()..])
    } else {
        None
    }
}

/// Clean up a Subject line: remove `[PATCH ...]` prefixes and similar.
fn clean_subject(s: &str, keep: bool) -> String {
    if keep {
        return s.to_string();
    }
    let mut result = s.to_string();
    // Strip leading [PATCH ...] or [RFC ...] brackets
    while result.starts_with('[') {
        if let Some(end) = result.find(']') {
            result = result[end + 1..].trim_start().to_string();
        } else {
            break;
        }
    }
    // Strip leading "Re: " etc.
    loop {
        let lower = result.to_lowercase();
        if lower.starts_with("re: ") || lower.starts_with("re:") {
            result = result[3..].trim_start().to_string();
        } else {
            break;
        }
    }
    result
}

/// Parse an author field like `Name <email>` or just `email`.
fn parse_author(from: &str) -> (String, String) {
    if let Some(start) = from.find('<') {
        if let Some(end) = from.find('>') {
            let name = from[..start].trim().trim_matches('"').to_string();
            let email = from[start + 1..end].to_string();
            return (name, email);
        }
    }
    // No angle brackets — treat the whole thing as email
    (String::new(), from.to_string())
}
