//! `grit interpret-trailers` — add or parse structured trailers in commit messages.
//!
//! Trailers are key-value pairs at the end of a commit message, separated from
//! the body by a blank line.  Format: `Key: Value` or `Key #Value`.
//!
//! Examples:
//!   Signed-off-by: Name <email>
//!   Reviewed-by: Name <email>

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::{self, Read, Write};

/// Arguments for `grit interpret-trailers`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Add or parse trailers in commit messages",
    override_usage = "grit interpret-trailers [OPTIONS] [<file>...]"
)]
pub struct Args {
    /// Add a trailer (can be specified multiple times).
    #[arg(long = "trailer", value_name = "token")]
    pub trailer: Vec<String>,

    /// Only output the trailers (parse mode).
    #[arg(long = "parse")]
    pub parse: bool,

    /// Edit files in-place instead of writing to stdout.
    #[arg(long = "in-place")]
    pub in_place: bool,

    /// Only output trailers that consist of a key-value pair.
    #[arg(long = "only-trailers")]
    pub only_trailers: bool,

    /// Do not apply the standard cleanup rules to the trailer value.
    #[arg(long = "no-divider")]
    pub no_divider: bool,

    /// Trim trailing whitespace from trailers.
    #[arg(long = "trim-empty")]
    pub trim_empty: bool,

    /// Remove duplicate trailers with the same key and value.
    #[arg(long = "if-exists", value_name = "action")]
    pub if_exists: Option<String>,

    /// What to do if a trailer with the same key doesn't exist.
    #[arg(long = "if-missing", value_name = "action")]
    pub if_missing: Option<String>,

    /// Where to place new trailers: end (default), start, after, before.
    #[arg(long = "where", value_name = "placement")]
    pub placement: Option<String>,

    /// Input files (reads from stdin if none given).
    pub files: Vec<String>,
}

/// Run the `interpret-trailers` command.
pub fn run(args: Args) -> Result<()> {
    if args.files.is_empty() {
        // Read from stdin.
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .context("reading stdin")?;

        let output = process_message(&input, &args);
        io::stdout().write_all(output.as_bytes())?;
    } else {
        for file in &args.files {
            let input =
                fs::read_to_string(file).with_context(|| format!("reading '{file}'"))?;
            let output = process_message(&input, &args);

            if args.in_place {
                fs::write(file, &output)
                    .with_context(|| format!("writing '{file}'"))?;
            } else {
                io::stdout().write_all(output.as_bytes())?;
            }
        }
    }

    Ok(())
}

/// A parsed trailer: key-separator-value.
#[derive(Debug, Clone)]
struct Trailer {
    key: String,
    separator: String,
    value: String,
}

impl Trailer {
    fn parse(line: &str) -> Option<Self> {
        // Try "Key: Value" first, then "Key #Value".
        if let Some(colon_pos) = line.find(": ") {
            let key = line[..colon_pos].trim().to_string();
            let value = line[colon_pos + 2..].trim().to_string();
            if is_valid_trailer_key(&key) {
                return Some(Trailer {
                    key,
                    separator: ": ".to_string(),
                    value,
                });
            }
        }
        if let Some(hash_pos) = line.find(" #") {
            let key = line[..hash_pos].trim().to_string();
            let value = line[hash_pos + 2..].trim().to_string();
            if is_valid_trailer_key(&key) {
                return Some(Trailer {
                    key,
                    separator: " #".to_string(),
                    value,
                });
            }
        }
        None
    }

    fn format(&self) -> String {
        format!("{}{}{}", self.key, self.separator, self.value)
    }
}

/// Check if a string is a valid trailer key (alphanumeric + hyphens, no spaces).
fn is_valid_trailer_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Parse a trailer token from --trailer argument.
/// Accepts "Key: Value", "Key=Value", "Key:Value".
fn parse_trailer_token(token: &str) -> Trailer {
    // Try "Key: Value" first.
    if let Some(pos) = token.find(": ") {
        return Trailer {
            key: token[..pos].trim().to_string(),
            separator: ": ".to_string(),
            value: token[pos + 2..].trim().to_string(),
        };
    }
    // Try "Key:Value".
    if let Some(pos) = token.find(':') {
        return Trailer {
            key: token[..pos].trim().to_string(),
            separator: ": ".to_string(),
            value: token[pos + 1..].trim().to_string(),
        };
    }
    // Try "Key=Value".
    if let Some(pos) = token.find('=') {
        return Trailer {
            key: token[..pos].trim().to_string(),
            separator: ": ".to_string(),
            value: token[pos + 1..].trim().to_string(),
        };
    }
    // Fallback: treat whole thing as key with empty value.
    Trailer {
        key: token.trim().to_string(),
        separator: ": ".to_string(),
        value: String::new(),
    }
}

/// Find where the trailer block starts in the message.
/// Returns (body_end, trailer_start) indices into lines.
fn find_trailer_block(lines: &[&str]) -> (usize, usize) {
    // Walk backwards from the end, skipping trailing blank lines.
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }

    if end == 0 {
        return (0, 0);
    }

    // Check if the last non-blank lines form a trailer block.
    let mut trailer_start = end;
    let mut i = end;
    while i > 0 {
        i -= 1;
        let line = lines[i].trim();
        if line.is_empty() {
            // Blank line before trailers = separator.
            break;
        }
        if Trailer::parse(line).is_some() {
            trailer_start = i;
        } else {
            // Non-trailer, non-blank line — stop.
            trailer_start = end; // no trailer block found at this level
            break;
        }
    }

    // Body ends at the blank line before trailers (or at trailer_start if no blank line).
    let body_end = if trailer_start > 0 && trailer_start < end {
        // Check if line before trailer_start is blank.
        if trailer_start > 0
            && lines
                .get(trailer_start - 1)
                .map_or(false, |l| l.trim().is_empty())
        {
            trailer_start - 1
        } else {
            trailer_start
        }
    } else {
        end
    };

    (body_end, trailer_start)
}

/// Process a commit message: parse existing trailers, add new ones.
fn process_message(input: &str, args: &Args) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let (body_end, trailer_start) = find_trailer_block(&lines);

    // Parse existing trailers.
    let mut existing_trailers: Vec<Trailer> = Vec::new();
    if trailer_start < lines.len() {
        for line in &lines[trailer_start..] {
            if let Some(t) = Trailer::parse(line.trim()) {
                existing_trailers.push(t);
            }
        }
    }

    // Parse new trailers from --trailer args.
    let new_trailers: Vec<Trailer> = args.trailer.iter().map(|t| parse_trailer_token(t)).collect();

    if args.parse {
        // --parse: only output trailers.
        let mut out = String::new();
        for t in &existing_trailers {
            if !args.only_trailers || (!t.key.is_empty() && !t.value.is_empty()) {
                out.push_str(&t.format());
                out.push('\n');
            }
        }
        for t in &new_trailers {
            if !args.only_trailers || (!t.key.is_empty() && !t.value.is_empty()) {
                out.push_str(&t.format());
                out.push('\n');
            }
        }
        return out;
    }

    // Determine placement, if-exists, if-missing from args.
    let placement = args.placement.as_deref().unwrap_or("end");
    let if_exists_action = args.if_exists.as_deref().unwrap_or("addIfDifferent");
    let if_missing_action = args.if_missing.as_deref().unwrap_or("add");

    // Filter new trailers based on --if-exists and --if-missing.
    let mut trailers_to_add: Vec<Trailer> = Vec::new();
    for new_t in &new_trailers {
        if args.trim_empty && new_t.value.is_empty() {
            continue;
        }

        let key_lower = new_t.key.to_lowercase();
        let existing_with_same_key: Vec<&Trailer> = existing_trailers
            .iter()
            .filter(|t| t.key.to_lowercase() == key_lower)
            .collect();

        if !existing_with_same_key.is_empty() {
            // Trailer with this key already exists — apply --if-exists.
            match if_exists_action {
                "addIfDifferentNeighbor" | "addIfDifferent" => {
                    let dominated = existing_with_same_key
                        .iter()
                        .any(|t| t.value == new_t.value);
                    if !dominated {
                        trailers_to_add.push(new_t.clone());
                    }
                }
                "add" => {
                    trailers_to_add.push(new_t.clone());
                }
                "replace" => {
                    // Remove existing trailers with matching key, add replacement.
                    existing_trailers.retain(|t| t.key.to_lowercase() != key_lower);
                    trailers_to_add.push(new_t.clone());
                }
                "donothing" => {
                    // Do nothing — skip this trailer.
                }
                _ => {
                    trailers_to_add.push(new_t.clone());
                }
            }
        } else {
            // Trailer with this key doesn't exist — apply --if-missing.
            match if_missing_action {
                "add" => {
                    trailers_to_add.push(new_t.clone());
                }
                "donothing" => {
                    // Don't add.
                }
                _ => {
                    trailers_to_add.push(new_t.clone());
                }
            }
        }
    }

    // Reconstruct message with trailers added.
    let mut out = String::new();

    // Body (up to body_end).
    for line in &lines[..body_end] {
        out.push_str(line);
        out.push('\n');
    }

    // Ensure blank line before trailers.
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }

    // Combine existing + new trailers respecting --where placement.
    match placement {
        "start" => {
            for t in &trailers_to_add {
                out.push_str(&t.format());
                out.push('\n');
            }
            for t in &existing_trailers {
                out.push_str(&t.format());
                out.push('\n');
            }
        }
        "before" => {
            // Insert new trailers before existing ones.
            for t in &trailers_to_add {
                out.push_str(&t.format());
                out.push('\n');
            }
            for t in &existing_trailers {
                out.push_str(&t.format());
                out.push('\n');
            }
        }
        "after" | "end" | _ => {
            // Append new trailers after existing ones (default).
            for t in &existing_trailers {
                out.push_str(&t.format());
                out.push('\n');
            }
            for t in &trailers_to_add {
                out.push_str(&t.format());
                out.push('\n');
            }
        }
    }

    out
}
