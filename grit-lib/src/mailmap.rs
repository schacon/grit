//! Parse `.mailmap` and resolve author/committer identities (Git-compatible).

use crate::config::ConfigSet;
use crate::error::Error as GustError;
use crate::objects::ObjectKind;
use crate::repo::Repository;
use crate::rev_parse::resolve_revision;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, GustError>;

/// One line from a `.mailmap` file after parsing.
#[derive(Debug, Clone)]
pub struct MailmapEntry {
    /// Canonical name (`None` = keep original).
    pub canonical_name: Option<String>,
    /// Canonical email (`None` = keep original).
    pub canonical_email: Option<String>,
    /// Match on this name (`None` = any name with the email).
    pub match_name: Option<String>,
    /// Match on this email.
    pub match_email: String,
}

struct EmailSpan {
    value: String,
    start: usize,
    end: usize,
}

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

fn parse_mailmap_line(line: &str) -> Option<MailmapEntry> {
    let emails = extract_emails(line);

    match emails.len() {
        1 => {
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
            let canonical_email = &emails[0];
            let match_email = &emails[1];

            let before_first = line[..canonical_email.start].trim();
            let between = line[canonical_email.end..match_email.start].trim();

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

/// Parse a `.mailmap` file body into entries (comments and blanks skipped).
#[must_use]
pub fn parse_mailmap(content: &str) -> Vec<MailmapEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(entry) = parse_mailmap_line(line) {
            entries.push(entry);
        }
    }

    entries
}

/// Parse a contact string `Name <email>` or `<email>`.
#[must_use]
pub fn parse_contact(contact: &str) -> (Option<String>, Option<String>) {
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
    if contact.contains('@') && !contact.chars().any(char::is_whitespace) {
        return (None, Some(contact.to_string()));
    }

    (Some(contact.to_string()), None)
}

/// Map `(name, email)` through the mailmap; last matching rule wins (Git order).
#[must_use]
pub fn map_contact(
    name: Option<&str>,
    email: Option<&str>,
    mailmap: &[MailmapEntry],
) -> (String, String) {
    let orig_name = name.unwrap_or("");
    let orig_email = email.unwrap_or("");

    for entry in mailmap.iter().rev() {
        if !entry.match_email.eq_ignore_ascii_case(orig_email) {
            continue;
        }

        if let Some(ref match_name) = entry.match_name {
            if !match_name.eq_ignore_ascii_case(orig_name) {
                continue;
            }
        }

        let result_name = entry.canonical_name.as_deref().unwrap_or(orig_name);
        let result_email = entry.canonical_email.as_deref().unwrap_or(orig_email);

        return (result_name.to_string(), result_email.to_string());
    }

    (orig_name.to_string(), orig_email.to_string())
}

/// Format a contact for display (`check-mailmap` style).
#[must_use]
pub fn render_contact(name: &str, email: &str) -> String {
    if email.is_empty() {
        return name.to_string();
    }
    if name.is_empty() {
        return format!("<{email}>");
    }
    format!("{name} <{email}>")
}

fn resolve_mailmap_path(base: &Path, value: &str) -> PathBuf {
    let candidate = Path::new(value);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    }
}

fn read_optional_mailmap_file(path: &Path) -> Result<String> {
    if path.exists() {
        fs::read_to_string(path)
            .map_err(|e| GustError::PathError(format!("reading {}: {e}", path.display())))
    } else {
        Ok(String::new())
    }
}

/// Read mailmap text from a blob revision (for `mailmap.blob` / CLI `--mailmap-blob`).
pub fn read_mailmap_blob(repo: &Repository, spec: &str) -> Result<String> {
    let oid = resolve_revision(repo, spec)
        .map_err(|e| GustError::PathError(format!("resolving mailmap blob '{spec}': {e}")))?;
    let obj = repo
        .odb
        .read(&oid)
        .map_err(|e| GustError::PathError(format!("reading mailmap blob '{spec}': {e}")))?;
    if obj.kind != ObjectKind::Blob {
        return Err(GustError::PathError(format!(
            "mailmap.blob '{spec}' does not resolve to a blob object"
        )));
    }
    Ok(String::from_utf8_lossy(&obj.data).into_owned())
}

/// Load and concatenate all configured mailmap sources for a repository.
pub fn load_mailmap_raw(repo: &Repository) -> Result<String> {
    let mut mailmap_content = String::new();

    if let Some(ref wt) = repo.work_tree {
        mailmap_content.push_str(&read_optional_mailmap_file(&wt.join(".mailmap"))?);
        if !mailmap_content.ends_with('\n') && !mailmap_content.is_empty() {
            mailmap_content.push('\n');
        }
    }

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let base_dir = repo
        .work_tree
        .as_deref()
        .unwrap_or(repo.git_dir.as_path())
        .to_path_buf();

    if let Some(file) = config.get("mailmap.file") {
        mailmap_content.push_str(&read_optional_mailmap_file(&resolve_mailmap_path(
            &base_dir, &file,
        ))?);
        if !mailmap_content.ends_with('\n') && !mailmap_content.is_empty() {
            mailmap_content.push('\n');
        }
    }
    if let Some(blob) = config.get("mailmap.blob") {
        mailmap_content.push_str(&read_mailmap_blob(repo, &blob)?);
        if !mailmap_content.ends_with('\n') && !mailmap_content.is_empty() {
            mailmap_content.push('\n');
        }
    }

    Ok(mailmap_content)
}

/// Parsed mailmap for the repository (default `.mailmap` + config).
pub fn load_mailmap(repo: &Repository) -> Result<Vec<MailmapEntry>> {
    Ok(parse_mailmap(&load_mailmap_raw(repo)?))
}
