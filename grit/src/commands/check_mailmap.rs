//! `grit check-mailmap` — show canonical names/emails from .mailmap.
//!
//! Reads the `.mailmap` file (if present) in the repository root and maps
//! author/committer identities to their canonical forms.
//!
//! Usage:
//!   grit check-mailmap "Name <email>"
//!   grit check-mailmap --stdin < identities.txt

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

/// Arguments for `grit check-mailmap`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Show canonical name/email from .mailmap",
    override_usage = "grit check-mailmap [--stdin] <contact>..."
)]
pub struct Args {
    /// Read contacts from stdin, one per line.
    #[arg(long = "stdin")]
    pub stdin: bool,

    /// Contact strings to look up (format: "Name <email>" or "<email>").
    pub contacts: Vec<String>,
}

/// A single mailmap entry.
#[derive(Debug, Clone)]
struct MailmapEntry {
    /// Canonical name (None = keep original).
    canonical_name: Option<String>,
    /// Canonical email (None = keep original).
    canonical_email: Option<String>,
    /// Match on this name (None = match any name with the email).
    match_name: Option<String>,
    /// Match on this email.
    match_email: String,
}

/// Parse a .mailmap file into entries.
fn parse_mailmap(content: &str) -> Vec<MailmapEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(entry) = parse_mailmap_line(line) {
            entries.push(entry);
        }
    }

    entries
}

/// Parse a single .mailmap line.
///
/// Formats:
///   <canonical-email>                          — map old email
///   canonical-name <canonical-email>           — map old email, set name
///   <canonical-email> <match-email>            — map email→email
///   canonical-name <canonical-email> <match-email>  — map email, set name
///   canonical-name <canonical-email> match-name <match-email>
fn parse_mailmap_line(line: &str) -> Option<MailmapEntry> {
    let emails = extract_emails(line);

    match emails.len() {
        1 => {
            // One email: "Name <canonical-email>" or "<canonical-email>"
            let email = &emails[0];
            let before = line[..email.start].trim();
            let canonical_name = if before.is_empty() {
                None
            } else {
                Some(before.to_string())
            };
            Some(MailmapEntry {
                canonical_name,
                canonical_email: Some(email.value.clone()),
                match_name: None,
                match_email: email.value.clone(),
            })
        }
        2 => {
            // Two emails: various forms with canonical + match email.
            let canonical_email = &emails[0];
            let match_email = &emails[1];

            let before_first = line[..canonical_email.start].trim();
            let between =
                line[canonical_email.end..match_email.start].trim();

            let canonical_name = if before_first.is_empty() {
                None
            } else {
                Some(before_first.to_string())
            };

            let match_name = if between.is_empty() {
                None
            } else {
                Some(between.to_string())
            };

            Some(MailmapEntry {
                canonical_name,
                canonical_email: Some(canonical_email.value.clone()),
                match_name,
                match_email: match_email.value.clone(),
            })
        }
        _ => None,
    }
}

/// Extracted email with position info.
struct EmailSpan {
    value: String,
    start: usize,
    end: usize,
}

/// Extract all <email> spans from a line.
fn extract_emails(line: &str) -> Vec<EmailSpan> {
    let mut emails = Vec::new();
    let mut search_from = 0;

    while let Some(start) = line[search_from..].find('<') {
        let abs_start = search_from + start;
        if let Some(end) = line[abs_start..].find('>') {
            let abs_end = abs_start + end + 1;
            let email = line[abs_start + 1..abs_end - 1].to_string();
            emails.push(EmailSpan {
                value: email,
                start: abs_start,
                end: abs_end,
            });
            search_from = abs_end;
        } else {
            break;
        }
    }

    emails
}

/// Parse a contact string "Name <email>" or "<email>".
fn parse_contact(contact: &str) -> (Option<String>, Option<String>) {
    let contact = contact.trim();
    if let Some(lt) = contact.find('<') {
        if let Some(gt) = contact.find('>') {
            let name = contact[..lt].trim();
            let email = contact[lt + 1..gt].trim();
            return (
                if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                },
                if email.is_empty() {
                    None
                } else {
                    Some(email.to_string())
                },
            );
        }
    }
    // No angle brackets — treat whole thing as name.
    (Some(contact.to_string()), None)
}

/// Look up a contact in the mailmap and return the canonical form.
fn map_contact(
    name: Option<&str>,
    email: Option<&str>,
    mailmap: &[MailmapEntry],
) -> (String, String) {
    let orig_name = name.unwrap_or("");
    let orig_email = email.unwrap_or("");

    for entry in mailmap.iter().rev() {
        // Check email match (case-insensitive).
        if !entry.match_email.eq_ignore_ascii_case(orig_email) {
            continue;
        }

        // Check name match if specified.
        if let Some(ref match_name) = entry.match_name {
            if !match_name.eq_ignore_ascii_case(orig_name) {
                continue;
            }
        }

        // Match found — apply canonical values.
        let result_name = entry
            .canonical_name
            .as_deref()
            .unwrap_or(orig_name);
        let result_email = entry
            .canonical_email
            .as_deref()
            .unwrap_or(orig_email);

        return (result_name.to_string(), result_email.to_string());
    }

    (orig_name.to_string(), orig_email.to_string())
}

/// Run the `check-mailmap` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)?;

    // Load .mailmap from worktree root.
    let mailmap_content = if let Some(ref wt) = repo.work_tree {
        let mailmap_path = wt.join(".mailmap");
        if mailmap_path.exists() {
            fs::read_to_string(&mailmap_path)
                .with_context(|| format!("reading {}", mailmap_path.display()))?
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let mailmap = parse_mailmap(&mailmap_content);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.stdin {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.context("reading stdin")?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (name, email) = parse_contact(line);
            let (cn, ce) = map_contact(name.as_deref(), email.as_deref(), &mailmap);
            writeln!(out, "{cn} <{ce}>")?;
        }
    } else {
        if args.contacts.is_empty() {
            bail!("usage: grit check-mailmap [--stdin] <contact>...");
        }
        for contact in &args.contacts {
            let (name, email) = parse_contact(contact);
            let (cn, ce) = map_contact(name.as_deref(), email.as_deref(), &mailmap);
            writeln!(out, "{cn} <{ce}>")?;
        }
    }

    Ok(())
}
