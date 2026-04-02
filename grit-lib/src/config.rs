//! Git-compatible configuration file parser and accessor.
//!
//! Supports the standard Git config file format:
//!
//! ```text
//! [section]
//!     key = value
//! [section "subsection"]
//!     key = value
//! ```
//!
//! # Multi-file layering
//!
//! Git reads configuration from several files in priority order:
//!
//! 1. System (`/etc/gitconfig`)
//! 2. Global (`~/.gitconfig` or `$XDG_CONFIG_HOME/git/config`)
//! 3. Local (`.git/config`)
//! 4. Worktree (`.git/config.worktree`)
//! 5. Command-line (`-c key=value` or `GIT_CONFIG_*`)
//!
//! [`ConfigSet`] merges all layers; last-wins for single-valued keys.
//!
//! # Include directives
//!
//! `[include] path = <path>` and `[includeIf "<condition>"] path = <path>`
//! are supported. Conditions: `gitdir:`, `gitdir/i:`, `onbranch:`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The scope (origin) of a configuration value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigScope {
    /// System-wide configuration (`/etc/gitconfig`).
    System,
    /// Per-user global configuration (`~/.gitconfig` or XDG).
    Global,
    /// Repository-local configuration (`.git/config`).
    Local,
    /// Per-worktree configuration (`.git/config.worktree`).
    Worktree,
    /// Command-line overrides (`-c key=value`).
    Command,
}

impl fmt::Display for ConfigScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::Global => write!(f, "global"),
            Self::Local => write!(f, "local"),
            Self::Worktree => write!(f, "worktree"),
            Self::Command => write!(f, "command"),
        }
    }
}

/// A single configuration entry with its origin metadata.
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    /// Fully-qualified key in canonical form: `section.subsection.name`
    /// (section and name lowercased; subsection preserves case).
    pub key: String,
    /// The raw string value, or `None` for a boolean-true bare key.
    pub value: Option<String>,
    /// Which scope this entry came from.
    pub scope: ConfigScope,
    /// The file this entry was read from (if file-backed).
    pub file: Option<PathBuf>,
    /// One-based line number in the source file.
    pub line: usize,
}

/// A parsed configuration file that preserves the raw text for round-trip
/// editing (set/unset/rename-section/remove-section).
#[derive(Debug, Clone)]
pub struct ConfigFile {
    /// The path to this config file on disk.
    pub path: PathBuf,
    /// The scope this file represents.
    pub scope: ConfigScope,
    /// Parsed entries (in file order).
    pub entries: Vec<ConfigEntry>,
    /// Raw lines of the file (for round-trip editing).
    raw_lines: Vec<String>,
}

/// A merged view across all configuration scopes.
///
/// Entries are stored in file-order within each scope; scopes are layered
/// in priority order (system < global < local < worktree < command).
#[derive(Debug, Clone, Default)]
pub struct ConfigSet {
    /// All entries across all scopes, in load order.
    entries: Vec<ConfigEntry>,
}

// ── Canonical key helpers ────────────────────────────────────────────

/// Normalise a config key to canonical form.
///
/// - Section name is lowercased.
/// - Variable name (last dot-separated component) is lowercased.
/// - Subsection (middle components) preserves original case.
///
/// Returns `Err` if the key has fewer than two dot-separated parts.
///
/// # Examples
///
/// - `core.bare` → `core.bare`
/// - `Section.SubSection.Key` → `section.SubSection.key`
/// - `CORE.BARE` → `core.bare`
pub fn canonical_key(raw: &str) -> Result<String> {
    // Reject keys containing newlines
    if raw.contains('\n') || raw.contains('\r') {
        return Err(Error::ConfigError(format!("invalid key: '{}'" , raw.replace('\n', "\\n"))));
    }

    let first_dot = raw
        .find('.')
        .ok_or_else(|| Error::ConfigError(format!("key does not contain a section: '{raw}'")))?;
    let last_dot = raw
        .rfind('.')
        .ok_or_else(|| Error::ConfigError(format!("key does not contain a section: '{raw}'")))?;

    if last_dot == raw.len() - 1 {
        return Err(Error::ConfigError(format!(
            "key does not contain variable name: '{raw}'"
        )));
    }

    let section = &raw[..first_dot];
    let name = &raw[last_dot + 1..];

    // Validate section name: must be alphanumeric or hyphen
    if section.is_empty() || !section.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return Err(Error::ConfigError(format!("invalid key (bad section): '{raw}'")));
    }

    // Validate variable name: must start with alpha, rest alphanumeric or hyphen
    if name.is_empty()
        || !name.chars().next().unwrap().is_ascii_alphabetic()
        || !name.chars().all(|c| c.is_alphanumeric() || c == '-')
    {
        return Err(Error::ConfigError(format!("invalid key (bad variable name): '{raw}'")));
    }

    if first_dot == last_dot {
        // No subsection: section.name
        Ok(format!(
            "{}.{}",
            section.to_lowercase(),
            name.to_lowercase()
        ))
    } else {
        // section.subsection.name
        let subsection = &raw[first_dot + 1..last_dot];
        Ok(format!(
            "{}.{}.{}",
            section.to_lowercase(),
            subsection,
            name.to_lowercase()
        ))
    }
}

// ── Parser ──────────────────────────────────────────────────────────

/// State tracked while parsing a config file line-by-line.
struct Parser {
    section: String,
    subsection: Option<String>,
}

impl Parser {
    fn new() -> Self {
        Self {
            section: String::new(),
            subsection: None,
        }
    }

    /// Build the canonical key for a variable name in the current section.
    fn make_key(&self, name: &str) -> String {
        let sec = self.section.to_lowercase();
        let var = name.to_lowercase();
        match &self.subsection {
            Some(sub) => format!("{sec}.{sub}.{var}"),
            None => format!("{sec}.{var}"),
        }
    }

    /// Parse a section header line like `[section]` or `[section "subsection"]`.
    ///
    /// Returns `true` if the line was a section header.
    fn try_parse_section(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        if !trimmed.starts_with('[') {
            return false;
        }
        let end = match trimmed.find(']') {
            Some(i) => i,
            None => return false,
        };
        let inside = &trimmed[1..end];
        // Check for subsection: [section "subsection"]
        if let Some(quote_start) = inside.find('"') {
            self.section = inside[..quote_start].trim().to_owned();
            let rest = &inside[quote_start + 1..];
            if let Some(quote_end) = rest.find('"') {
                self.subsection = Some(rest[..quote_end].to_owned());
            } else {
                self.subsection = Some(rest.to_owned());
            }
        } else {
            self.section = inside.trim().to_owned();
            self.subsection = None;
        }
        true
    }

    /// Parse a `key = value` or bare `key` line.
    ///
    /// Returns `Some((canonical_key, value))` if this is a variable line.
    fn try_parse_entry(&self, line: &str) -> Option<(String, Option<String>)> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            return None;
        }
        if trimmed.starts_with('[') {
            return None;
        }
        if self.section.is_empty() {
            return None;
        }

        if let Some(eq_pos) = trimmed.find('=') {
            let raw_name = trimmed[..eq_pos].trim();
            let raw_value = trimmed[eq_pos + 1..].trim();
            // Strip inline comment (not inside quotes)
            let value = strip_inline_comment(raw_value);
            let value = unescape_value(&value);
            let key = self.make_key(raw_name);
            Some((key, Some(value)))
        } else {
            // Bare key (boolean true)
            let raw_name = strip_inline_comment(trimmed);
            let key = self.make_key(raw_name.trim());
            Some((key, None))
        }
    }
}

/// Check if a value line ends with a continuation backslash.
///
/// This checks the value portion (after `=`) for a trailing `\` that is
/// outside quotes and outside an inline comment. If the `\` is after
/// a `#` or `;` that starts a comment, it does NOT count as continuation.
fn value_line_continues(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return false;
    }
    // Find the value portion (after '=')
    // If no '=', this is a bare key — no continuation
    let value_part = match trimmed.find('=') {
        Some(pos) => &trimmed[pos + 1..],
        None => return false,
    };
    // Walk the value portion tracking quotes and comments
    let mut in_quote = false;
    let mut last_was_backslash = false;
    let mut in_comment = false;
    for ch in value_part.chars() {
        if in_comment {
            // Inside comment, backslash doesn't matter
            last_was_backslash = false;
            continue;
        }
        match ch {
            '"' if !last_was_backslash => {
                in_quote = !in_quote;
                last_was_backslash = false;
            }
            '\\' if !last_was_backslash => {
                last_was_backslash = true;
                continue;
            }
            '#' | ';' if !in_quote && !last_was_backslash => {
                in_comment = true;
                last_was_backslash = false;
            }
            _ => {
                last_was_backslash = false;
            }
        }
    }
    // The line continues if it ends with an unescaped backslash outside comments
    last_was_backslash && !in_comment
}

/// Strip an inline comment (`#` or `;`) that is not inside quotes.
fn strip_inline_comment(s: &str) -> String {
    let mut in_quote = false;
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                result.push(ch);
            }
            '\\' if in_quote => {
                result.push(ch);
                if let Some(&next) = chars.peek() {
                    result.push(next);
                    chars.next();
                }
            }
            '#' | ';' if !in_quote => break,
            _ => result.push(ch),
        }
    }
    // Trim trailing whitespace that was before the comment
    let trimmed = result.trim_end();
    trimmed.to_owned()
}

/// Unescape a config value: handle `\"`, `\\`, `\n`, `\t`, and strip
/// surrounding quotes.
fn unescape_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => { /* strip quotes */ }
            '\\' => match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            },
            _ => result.push(ch),
        }
    }
    result
}

/// Escape a config value for writing back to a file.
///
/// Wraps in double quotes if the value contains leading/trailing whitespace,
/// internal quotes, backslashes, or special characters.
fn escape_value(s: &str) -> String {
    let needs_quoting = s.starts_with(' ')
        || s.starts_with('\t')
        || s.ends_with(' ')
        || s.ends_with('\t')
        || s.contains('"')
        || s.contains('\\')
        || s.contains('\n')
        || s.contains('#')
        || s.contains(';');

    if !needs_quoting {
        return s.to_owned();
    }

    let mut out = String::with_capacity(s.len() + 4);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

// ── ConfigFile ──────────────────────────────────────────────────────

impl ConfigFile {
    /// Parse a config file from its raw text content.
    ///
    /// # Parameters
    ///
    /// - `path` — the file path (stored for diagnostics and round-trip writes).
    /// - `content` — the raw text of the file.
    /// - `scope` — the [`ConfigScope`] this file represents.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ConfigError`] on malformed input.
    pub fn parse(path: &Path, content: &str, scope: ConfigScope) -> Result<Self> {
        let raw_lines: Vec<String> = content.lines().map(String::from).collect();
        let mut entries = Vec::new();
        let mut parser = Parser::new();

        let mut idx = 0;
        while idx < raw_lines.len() {
            let start_idx = idx;
            let line = &raw_lines[idx];
            idx += 1;

            // Pure comment lines don't continue even with trailing \
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }

            if parser.try_parse_section(line) {
                continue;
            }

            // For entry lines, we need to check continuation.
            // Build a logical line by joining continuations.
            let mut logical_line = line.clone();
            while value_line_continues(&logical_line) && idx < raw_lines.len() {
                // Remove the trailing backslash
                let t = logical_line.trim_end();
                logical_line = t[..t.len() - 1].to_string();
                // Append next line (trimmed of leading whitespace)
                let next = raw_lines[idx].trim_start();
                logical_line.push_str(next);
                idx += 1;
            }

            if let Some((key, value)) = parser.try_parse_entry(&logical_line) {
                entries.push(ConfigEntry {
                    key,
                    value,
                    scope,
                    file: Some(path.to_path_buf()),
                    line: start_idx + 1,
                });
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            scope,
            entries,
            raw_lines,
        })
    }

    /// Read and parse a config file from disk.
    ///
    /// Returns `Ok(None)` if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on read failure (other than not-found) or
    /// [`Error::ConfigError`] on parse failure.
    pub fn from_path(path: &Path, scope: ConfigScope) -> Result<Option<Self>> {
        match fs::read_to_string(path) {
            Ok(content) => Ok(Some(Self::parse(path, &content, scope)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Set a value in this config file, creating the section if needed.
    ///
    /// If the key already exists, its last occurrence is updated in-place.
    /// Otherwise a new entry is appended (creating the section header if
    /// necessary).
    ///
    /// # Parameters
    ///
    /// - `key` — canonical key (e.g. `core.bare`).
    /// - `value` — the value to set.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let canon = canonical_key(key)?;
        // Extract original-case variable name from the raw key
        let raw_var = raw_variable_name(key);

        // Find the last entry with this key to replace in-place.
        let existing_idx = self.entries.iter().rposition(|e| e.key == canon);

        if let Some(idx) = existing_idx {
            let line_idx = self.entries[idx].line - 1;
            // Rebuild the line, preserving the user's variable name case
            self.raw_lines[line_idx] = format!("\t{} = {}", raw_var, escape_value(value));
            self.entries[idx].value = Some(value.to_owned());
        } else {
            // Need to add: find or create the section
            let (section, subsection, _var) = split_key(&canon)?;
            // Extract original-case section/subsection from the raw key
            let (raw_sec, raw_sub) = raw_section_parts(key);
            let section_line = self.find_or_create_section_preserving_case(
                &section, subsection.as_deref(),
                &raw_sec, raw_sub.as_deref(),
            );
            let new_line = format!("\t{} = {}", raw_var, escape_value(value));

            // Insert after the section header (or last entry in section)
            let insert_at = self.last_line_in_section(section_line) + 1;
            self.raw_lines.insert(insert_at, new_line);

            // Re-parse to fix up line numbers
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;
        }

        Ok(())
    }

    /// Replace ALL occurrences of a key with a new value.
    ///
    /// Removes all but the last occurrence from the file, then updates
    /// the last occurrence with the new value (matching Git behaviour).
    pub fn replace_all(&mut self, key: &str, value: &str, value_pattern: Option<&str>) -> Result<()> {
        let canon = canonical_key(key)?;
        let raw_var = raw_variable_name(key);

        // Compile optional regex pattern
        let re = match value_pattern {
            Some(pat) => Some(
                regex::Regex::new(pat)
                    .map_err(|e| Error::ConfigError(format!("invalid value-pattern regex: {e}")))?),
            None => None,
        };

        // Find all matching entries (by key, and optionally by value pattern)
        let matching_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if e.key != canon {
                    return false;
                }
                if let Some(ref re) = re {
                    let v = e.value.as_deref().unwrap_or("");
                    re.is_match(v)
                } else {
                    true
                }
            })
            .map(|(i, _)| i)
            .collect();

        if matching_indices.is_empty() {
            // No matching entries — add a new one (same as set)
            return self.set(key, value);
        }

        // Keep the first matching entry, remove the rest
        let first_match = matching_indices[0];
        let lines_to_remove: Vec<usize> = matching_indices
            .iter()
            .skip(1)
            .map(|&i| self.entries[i].line - 1)
            .collect();

        // Update the first matching entry's line with the new value
        let first_line_idx = self.entries[first_match].line - 1;
        self.raw_lines[first_line_idx] = format!("\t{} = {}", raw_var, escape_value(value));
        self.entries[first_match].value = Some(value.to_owned());

        // Remove remaining matching lines from bottom to top
        for &line_idx in lines_to_remove.iter().rev() {
            self.raw_lines.remove(line_idx);
        }

        // Re-parse after modifications
        let content = self.raw_lines.join("\n");
        let reparsed = Self::parse(&self.path, &content, self.scope)?;
        self.entries = reparsed.entries;
        self.raw_lines = reparsed.raw_lines;

        Ok(())
    }

    /// Count how many entries exist for a key.
    pub fn count(&self, key: &str) -> Result<usize> {
        let canon = canonical_key(key)?;
        Ok(self.entries.iter().filter(|e| e.key == canon).count())
    }

    /// Unset (remove) only the last occurrence of a key.
    ///
    /// Returns the number of entries removed (0 or 1).
    pub fn unset_last(&mut self, key: &str) -> Result<usize> {
        let canon = canonical_key(key)?;
        let last_idx = self.entries.iter().rposition(|e| e.key == canon);

        if let Some(idx) = last_idx {
            let line_idx = self.entries[idx].line - 1;
            self.raw_lines.remove(line_idx);
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// Unset (remove) all occurrences of a key.
    ///
    /// # Parameters
    ///
    /// - `key` — canonical key (e.g. `core.bare`).
    ///
    /// # Returns
    ///
    /// The number of entries removed.
    pub fn unset(&mut self, key: &str) -> Result<usize> {
        let canon = canonical_key(key)?;
        let line_indices: Vec<usize> = self
            .entries
            .iter()
            .filter(|e| e.key == canon)
            .map(|e| e.line - 1)
            .collect();

        let count = line_indices.len();
        // Remove from bottom to top to keep indices valid
        for &idx in line_indices.iter().rev() {
            self.raw_lines.remove(idx);
        }

        if count > 0 {
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;
        }

        Ok(count)
    }

    /// Remove an entire section (and all its entries).
    ///
    /// # Parameters
    ///
    /// - `section` — section name (e.g. `"core"`, `"remote.origin"`).
    pub fn remove_section(&mut self, section: &str) -> Result<bool> {
        let (sec_name, sub_name) = parse_section_name(section);
        let sec_lower = sec_name.to_lowercase();

        // Find section header line and all lines that belong to it
        let mut start = None;
        let mut end = 0;
        let mut parser = Parser::new();

        for (idx, line) in self.raw_lines.iter().enumerate() {
            if parser.try_parse_section(line) {
                if parser.section.to_lowercase() == sec_lower
                    && parser.subsection.as_deref() == sub_name
                {
                    start = Some(idx);
                    end = idx;
                } else if start.is_some() {
                    break;
                }
            } else if start.is_some() {
                end = idx;
            }
        }

        if let Some(s) = start {
            self.raw_lines.drain(s..=end);
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Rename a section.
    ///
    /// # Parameters
    ///
    /// - `old_name` — current section name (e.g. `"branch.main"`).
    /// - `new_name` — new section name (e.g. `"branch.develop"`).
    pub fn rename_section(&mut self, old_name: &str, new_name: &str) -> Result<bool> {
        let (old_sec, old_sub) = parse_section_name(old_name);
        let (new_sec, new_sub) = parse_section_name(new_name);
        let old_lower = old_sec.to_lowercase();

        let mut found = false;
        let mut parser = Parser::new();

        for idx in 0..self.raw_lines.len() {
            let line = &self.raw_lines[idx];
            if parser.try_parse_section(line)
                && parser.section.to_lowercase() == old_lower
                && parser.subsection.as_deref() == old_sub
            {
                // Rewrite the section header
                let header = match new_sub {
                    Some(sub) => format!("[{} \"{}\"]", new_sec, sub),
                    None => format!("[{}]", new_sec),
                };
                self.raw_lines[idx] = header;
                found = true;
            }
        }

        if found {
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;
        }

        Ok(found)
    }

    /// Append a new value for a key without removing existing entries.
    ///
    /// This is the behaviour of `git config --add section.key value`.
    /// If the section doesn't exist, it is created.
    pub fn add_value(&mut self, key: &str, value: &str) -> Result<()> {
        let canon = canonical_key(key)?;
        let raw_var = raw_variable_name(key);
        let (section, subsection, _var) = split_key(&canon)?;
        let (raw_sec, raw_sub) = raw_section_parts(key);

        let section_line = self.find_or_create_section_preserving_case(
            &section, subsection.as_deref(),
            &raw_sec, raw_sub.as_deref(),
        );
        let new_line = format!("\t{} = {}", raw_var, escape_value(value));
        let insert_at = self.last_line_in_section(section_line) + 1;
        self.raw_lines.insert(insert_at, new_line);

        // Re-parse to fix up entries and line numbers
        let content = self.raw_lines.join("\n");
        let reparsed = Self::parse(&self.path, &content, self.scope)?;
        self.entries = reparsed.entries;
        self.raw_lines = reparsed.raw_lines;

        Ok(())
    }

    /// Write the (possibly modified) config back to disk.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on write failure.
    pub fn write(&self) -> Result<()> {
        let content = self.raw_lines.join("\n");
        // Ensure trailing newline
        let content = if content.ends_with('\n') {
            content
        } else {
            format!("{content}\n")
        };
        fs::write(&self.path, content)?;
        Ok(())
    }

    /// Find the line index of a section header, or create one.
    #[allow(dead_code)]
    fn find_or_create_section(&mut self, section: &str, subsection: Option<&str>) -> usize {
        let sec_lower = section.to_lowercase();
        let mut parser = Parser::new();

        for (idx, line) in self.raw_lines.iter().enumerate() {
            if parser.try_parse_section(line)
                && parser.section.to_lowercase() == sec_lower
                && parser.subsection.as_deref() == subsection
            {
                return idx;
            }
        }

        // Create new section at end of file
        let header = match subsection {
            Some(sub) => format!("[{} \"{}\"]", section, sub),
            None => format!("[{}]", section),
        };
        self.raw_lines.push(header);
        self.raw_lines.len() - 1
    }

    /// Find the line index of a section header (case-insensitive match),
    /// or create one using the original-case names from user input.
    fn find_or_create_section_preserving_case(
        &mut self,
        section: &str,
        subsection: Option<&str>,
        raw_section: &str,
        raw_subsection: Option<&str>,
    ) -> usize {
        let sec_lower = section.to_lowercase();
        let mut parser = Parser::new();

        for (idx, line) in self.raw_lines.iter().enumerate() {
            if parser.try_parse_section(line)
                && parser.section.to_lowercase() == sec_lower
                && parser.subsection.as_deref() == subsection
            {
                return idx;
            }
        }

        // Create new section at end of file, using original case
        let header = match raw_subsection {
            Some(sub) => format!("[{} \"{}\"]", raw_section, sub),
            None => format!("[{}]", raw_section),
        };
        self.raw_lines.push(header);
        self.raw_lines.len() - 1
    }

    /// Find the last line that belongs to the section starting at `section_line`.
    fn last_line_in_section(&self, section_line: usize) -> usize {
        let mut last = section_line;
        for idx in (section_line + 1)..self.raw_lines.len() {
            let trimmed = self.raw_lines[idx].trim();
            if trimmed.starts_with('[') {
                break;
            }
            last = idx;
        }
        last
    }
}

// ── ConfigSet ───────────────────────────────────────────────────────

impl ConfigSet {
    /// Create an empty config set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Merge entries from a [`ConfigFile`] into this set.
    ///
    /// Entries are appended; later values override earlier ones for
    /// single-value lookups.
    pub fn merge(&mut self, file: &ConfigFile) {
        self.entries.extend(file.entries.iter().cloned());
    }

    /// Add a command-line override (`-c key=value`).
    pub fn add_command_override(&mut self, key: &str, value: &str) -> Result<()> {
        let canon = canonical_key(key)?;
        self.entries.push(ConfigEntry {
            key: canon,
            value: Some(value.to_owned()),
            scope: ConfigScope::Command,
            file: None,
            line: 0,
        });
        Ok(())
    }

    /// Get the last (highest-priority) value for a key.
    ///
    /// # Parameters
    ///
    /// - `key` — the key to look up (will be canonicalized).
    ///
    /// # Returns
    ///
    /// `Some(value)` for the last matching entry, or `None` if not found.
    /// Bare boolean keys return `Some("true")`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        let canon = canonical_key(key).ok()?;
        self.entries
            .iter()
            .rev()
            .find(|e| e.key == canon)
            .map(|e| e.value.clone().unwrap_or_else(|| "true".to_owned()))
    }

    /// Get all values for a key (multi-valued; in load order).
    #[must_use]
    pub fn get_all(&self, key: &str) -> Vec<String> {
        let canon = match canonical_key(key) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        self.entries
            .iter()
            .filter(|e| e.key == canon)
            .map(|e| e.value.clone().unwrap_or_else(|| "true".to_owned()))
            .collect()
    }

    /// Get a boolean value, interpreting `true`/`yes`/`on`/`1` as true and
    /// `false`/`no`/`off`/`0` as false.
    pub fn get_bool(&self, key: &str) -> Option<std::result::Result<bool, String>> {
        self.get(key).map(|v| parse_bool(&v))
    }

    /// Get an integer value, supporting Git's `k`/`m`/`g` suffixes.
    pub fn get_i64(&self, key: &str) -> Option<std::result::Result<i64, String>> {
        self.get(key).map(|v| parse_i64(&v))
    }

    /// Get all entries matching a key pattern (regex).
    ///
    /// Used by `git config --get-regexp`. Returns an error if the pattern
    /// is not a valid regex.
    pub fn get_regexp(&self, pattern: &str) -> std::result::Result<Vec<&ConfigEntry>, String> {
        let re = regex::Regex::new(pattern)
            .map_err(|e| format!("invalid key pattern: {e}"))?;
        Ok(self.entries
            .iter()
            .filter(|e| re.is_match(&e.key))
            .collect())
    }

    /// List all entries in load order.
    #[must_use]
    pub fn entries(&self) -> &[ConfigEntry] {
        &self.entries
    }

    /// Load the standard Git configuration file cascade for a repository.
    ///
    /// # Parameters
    ///
    /// - `git_dir` — path to the `.git` directory (for local/worktree config).
    /// - `include_system` — whether to load system config.
    ///
    /// # Errors
    ///
    /// Returns errors from file I/O or parsing.
    pub fn load(git_dir: Option<&Path>, include_system: bool) -> Result<Self> {
        let mut set = Self::new();

        // System config
        if include_system {
            if let Ok(Some(f)) =
                ConfigFile::from_path(Path::new("/etc/gitconfig"), ConfigScope::System)
            {
                Self::merge_with_includes(&mut set, &f, true)?;
            }
        }

        // Global config
        for path in global_config_paths() {
            if let Ok(Some(f)) = ConfigFile::from_path(&path, ConfigScope::Global) {
                Self::merge_with_includes(&mut set, &f, true)?;
                break; // Only use the first found
            }
        }

        // Local config
        if let Some(gd) = git_dir {
            let local_path = gd.join("config");
            if let Ok(Some(f)) = ConfigFile::from_path(&local_path, ConfigScope::Local) {
                Self::merge_with_includes(&mut set, &f, true)?;
            }

            // Worktree config
            let wt_path = gd.join("config.worktree");
            if let Ok(Some(f)) = ConfigFile::from_path(&wt_path, ConfigScope::Worktree) {
                Self::merge_with_includes(&mut set, &f, true)?;
            }
        }

        // Environment overrides
        if let Ok(path) = std::env::var("GIT_CONFIG") {
            if let Ok(Some(f)) = ConfigFile::from_path(Path::new(&path), ConfigScope::Command) {
                set.merge(&f);
            }
        }

        // GIT_CONFIG_COUNT / GIT_CONFIG_KEY_N / GIT_CONFIG_VALUE_N
        if let Ok(count_str) = std::env::var("GIT_CONFIG_COUNT") {
            if let Ok(count) = count_str.parse::<usize>() {
                for i in 0..count {
                    let key_var = format!("GIT_CONFIG_KEY_{i}");
                    let val_var = format!("GIT_CONFIG_VALUE_{i}");
                    if let (Ok(key), Ok(val)) = (std::env::var(&key_var), std::env::var(&val_var)) {
                        let _ = set.add_command_override(&key, &val);
                    }
                }
            }
        }

        // GIT_CONFIG_PARAMETERS — single-quoted 'key=value' entries separated by spaces.
        // This is the format used by `git -c key=value`.
        if let Ok(params) = std::env::var("GIT_CONFIG_PARAMETERS") {
            for entry in parse_config_parameters(&params) {
                if let Some((key, val)) = entry.split_once('=') {
                    let _ = set.add_command_override(key.trim(), val.trim());
                } else {
                    // Bare key (boolean true)
                    let _ = set.add_command_override(entry.trim(), "true");
                }
            }
        }

        Ok(set)
    }

    /// Merge a file, processing `[include]` and `[includeIf]` directives.
    fn merge_with_includes(
        set: &mut Self,
        file: &ConfigFile,
        process_includes: bool,
    ) -> Result<()> {
        // First pass: find include paths
        let mut includes: Vec<(String, Option<String>)> = Vec::new();

        for entry in &file.entries {
            if entry.key == "include.path" {
                if let Some(ref val) = entry.value {
                    includes.push((val.clone(), None));
                }
            } else if entry.key.starts_with("includeif.") && entry.key.ends_with(".path") {
                // Extract condition from key: includeif.<condition>.path
                let mid = &entry.key["includeif.".len()..entry.key.len() - ".path".len()];
                if let Some(ref val) = entry.value {
                    includes.push((val.clone(), Some(mid.to_owned())));
                }
            }
        }

        // Merge the file's own entries
        set.merge(file);

        // Process includes
        if process_includes {
            for (inc_path, condition) in includes {
                if let Some(ref cond) = condition {
                    if !evaluate_include_condition(cond, file) {
                        continue;
                    }
                }

                let resolved = resolve_include_path(&inc_path, file.path.parent());
                if let Ok(Some(inc_file)) = ConfigFile::from_path(&resolved, file.scope) {
                    Self::merge_with_includes(set, &inc_file, true)?;
                }
            }
        }

        Ok(())
    }
}

// ── Type coercion helpers ───────────────────────────────────────────

/// Parse a Git boolean value.
///
/// Accepts: `true`, `yes`, `on`, `1` (and bare key / empty) as true.
/// Accepts: `false`, `no`, `off`, `0` as false.
pub fn parse_bool(s: &str) -> std::result::Result<bool, String> {
    match s.to_lowercase().as_str() {
        "true" | "yes" | "on" => Ok(true),
        "false" | "no" | "off" | "" => Ok(false),
        _ => {
            // Try parsing as integer: 0 → false, non-zero → true
            if let Ok(n) = s.parse::<i64>() {
                return Ok(n != 0);
            }
            Err(format!("bad boolean config value '{s}'"))
        }
    }
}

/// Parse a Git integer value with optional `k`/`m`/`g` suffix.
pub fn parse_i64(s: &str) -> std::result::Result<i64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty integer value".to_owned());
    }

    let (num_str, multiplier) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024_i64),
        Some(b'm' | b'M') => (&s[..s.len() - 1], 1024 * 1024),
        Some(b'g' | b'G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1_i64),
    };

    let base: i64 = num_str
        .parse()
        .map_err(|_| format!("invalid integer: '{s}'"))?;
    base.checked_mul(multiplier)
        .ok_or_else(|| format!("integer overflow: '{s}'"))
}

/// Parse a Git path value (expand `~/` to home directory).
pub fn parse_path(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    s.to_owned()
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Parse `GIT_CONFIG_PARAMETERS` — single-quoted `'key=value'` entries
/// separated by whitespace.
fn parse_config_parameters(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut iter = raw.chars().peekable();
    while let Some(&c) = iter.peek() {
        if c == '\'' {
            iter.next();
            let mut s = String::new();
            loop {
                match iter.next() {
                    Some('\'') | None => break,
                    Some(x) => s.push(x),
                }
            }
            if !s.is_empty() {
                out.push(s);
            }
        } else {
            iter.next();
        }
    }
    out
}

/// Return candidate paths for the global config file, in priority order.
fn global_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // $GIT_CONFIG_GLOBAL overrides
    if let Ok(p) = std::env::var("GIT_CONFIG_GLOBAL") {
        paths.push(PathBuf::from(p));
        return paths;
    }

    // $HOME/.gitconfig
    if let Some(home) = home_dir() {
        paths.push(home.join(".gitconfig"));
    }

    // $XDG_CONFIG_HOME/git/config
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("git/config"));
    } else if let Some(home) = home_dir() {
        paths.push(home.join(".config/git/config"));
    }

    paths
}

/// Return the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Resolve an include path relative to the including file's directory.
fn resolve_include_path(path: &str, base: Option<&Path>) -> PathBuf {
    let expanded = parse_path(path);
    let p = Path::new(&expanded);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(base) = base {
        base.join(p)
    } else {
        p.to_path_buf()
    }
}

/// Evaluate an `[includeIf]` condition.
///
/// Currently supports:
/// - `gitdir:<pattern>` / `gitdir/i:<pattern>` — match against the git dir.
/// - `onbranch:<pattern>` — match against the current branch.
fn evaluate_include_condition(condition: &str, _file: &ConfigFile) -> bool {
    // TODO: Implement gitdir: and onbranch: matching.
    // For now, we skip conditional includes (safe default: don't include).
    let _ = condition;
    false
}

/// Split a canonical key into (section, subsection, variable).
fn split_key(key: &str) -> Result<(String, Option<String>, String)> {
    let first_dot = key
        .find('.')
        .ok_or_else(|| Error::ConfigError(format!("invalid key: '{key}'")))?;
    let last_dot = key
        .rfind('.')
        .ok_or_else(|| Error::ConfigError(format!("invalid key: '{key}'")))?;

    let section = key[..first_dot].to_owned();
    let variable = key[last_dot + 1..].to_owned();

    let subsection = if first_dot == last_dot {
        None
    } else {
        Some(key[first_dot + 1..last_dot].to_owned())
    };

    Ok((section, subsection, variable))
}

/// Extract the variable name from a canonical key.
#[allow(dead_code)]
fn variable_name_from_key(key: &str) -> &str {
    match key.rfind('.') {
        Some(i) => &key[i + 1..],
        None => key,
    }
}

/// Parse a section name that may contain a subsection (e.g. `"remote.origin"`).
///
/// Returns (section, subsection).
fn parse_section_name(name: &str) -> (&str, Option<&str>) {
    match name.find('.') {
        Some(i) => (&name[..i], Some(&name[i + 1..])),
        None => (name, None),
    }
}

/// Extract the original-case variable name from a raw (user-typed) key.
///
/// E.g. `"Section.Movie"` → `"Movie"`, `"a.b.CamelCase"` → `"CamelCase"`.
fn raw_variable_name(raw_key: &str) -> &str {
    match raw_key.rfind('.') {
        Some(i) => &raw_key[i + 1..],
        None => raw_key,
    }
}

/// Extract the original-case section and subsection from a raw (user-typed) key.
///
/// E.g. `"Section.key"` → `("Section", None)`,
///      `"Remote.origin.url"` → `("Remote", Some("origin"))`.
fn raw_section_parts(raw_key: &str) -> (String, Option<String>) {
    let first_dot = match raw_key.find('.') {
        Some(i) => i,
        None => return (raw_key.to_owned(), None),
    };
    // rfind always succeeds here since we already found at least one dot above.
    let last_dot = match raw_key.rfind('.') {
        Some(i) => i,
        None => return (raw_key[..first_dot].to_owned(), None),
    };
    let section = raw_key[..first_dot].to_owned();
    if first_dot == last_dot {
        (section, None)
    } else {
        let subsection = raw_key[first_dot + 1..last_dot].to_owned();
        (section, Some(subsection))
    }
}
