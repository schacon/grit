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
use crate::refs;
use crate::wildmatch::{wildmatch, WM_CASEFOLD, WM_PATHNAME};

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

/// Where a [`ConfigFile`] was loaded from for Git include semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigIncludeOrigin {
    /// Normal path on disk (`-f`, global/local config files, etc.).
    Disk,
    /// `--file -` (stdin).
    Stdin,
    /// Synthetic file built from `GIT_CONFIG_PARAMETERS` / `git -c`.
    CommandLine,
    /// `git config --blob=…`.
    Blob,
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
    /// Source kind for `[include]` resolution (Git `CONFIG_ORIGIN_*`).
    pub include_origin: ConfigIncludeOrigin,
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

/// Context for evaluating `[includeIf]` conditions (`gitdir:`, `onbranch:`).
#[derive(Debug, Clone, Default)]
pub struct IncludeContext {
    /// Git directory path used for `gitdir:` matching (may contain unresolved symlinks).
    pub git_dir: Option<PathBuf>,
    /// When true, `git -c include.path=relative` fails instead of ignoring the include.
    pub command_line_relative_include_is_error: bool,
}

/// Options controlling how [`ConfigSet::load_with_options`] merges files and includes.
#[derive(Debug, Clone)]
pub struct LoadConfigOptions {
    /// Load `/etc/gitconfig` (unless `GIT_CONFIG_NOSYSTEM` is set).
    pub include_system: bool,
    /// Expand `[include]` / `[includeIf]` while reading file-backed layers.
    pub process_includes: bool,
    /// Expand includes for synthetic command-line config built from `GIT_CONFIG_PARAMETERS`.
    pub command_includes: bool,
    pub include_ctx: IncludeContext,
}

impl Default for LoadConfigOptions {
    fn default() -> Self {
        Self {
            include_system: true,
            process_includes: true,
            command_includes: true,
            include_ctx: IncludeContext::default(),
        }
    }
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
        return Err(Error::ConfigError(format!(
            "invalid key: '{}'",
            raw.replace('\n', "\\n")
        )));
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
        return Err(Error::ConfigError(format!(
            "invalid key (bad section): '{raw}'"
        )));
    }

    // Validate variable name: must start with alpha, rest alphanumeric or hyphen
    if name.is_empty()
        || !name.chars().next().unwrap().is_ascii_alphabetic()
        || !name.chars().all(|c| c.is_alphanumeric() || c == '-')
    {
        return Err(Error::ConfigError(format!(
            "invalid key (bad variable name): '{raw}'"
        )));
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

/// Display path for config diagnostics (matches [`config_error_path_display`] for public callers).
#[must_use]
pub fn config_file_display_for_error(path: &Path) -> String {
    config_error_path_display(path)
}

fn config_error_path_display(path: &Path) -> String {
    if path.file_name().and_then(|s| s.to_str()) == Some("config")
        && path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            == Some(".git")
    {
        return ".git/config".to_owned();
    }
    path.display().to_string()
}

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
    /// If there is content after `]` (an inline key=value), it is returned
    /// via the `inline_remainder` parameter.
    fn try_parse_section_with_remainder<'a>(
        &mut self,
        line: &'a str,
        inline_remainder: &mut Option<&'a str>,
    ) -> bool {
        let trimmed = line.trim();
        if !trimmed.starts_with('[') {
            return false;
        }
        // Find the closing `]` — but for subsection headers like
        // [section "sub\"escaped"], we need to skip escaped chars
        // inside quotes.
        let end = {
            let bytes = trimmed.as_bytes();
            let mut i = 1; // skip opening '['
            let mut in_quotes = false;
            let mut found = None;
            while i < bytes.len() {
                if in_quotes {
                    if bytes[i] == b'\\' {
                        i += 2; // skip escaped char
                        continue;
                    }
                    if bytes[i] == b'"' {
                        in_quotes = false;
                    }
                } else {
                    if bytes[i] == b'"' {
                        in_quotes = true;
                    }
                    if bytes[i] == b']' {
                        found = Some(i);
                        break;
                    }
                }
                i += 1;
            }
            match found {
                Some(i) => i,
                None => return false,
            }
        };
        let inside = &trimmed[1..end];
        // Check for subsection: [section "subsection"]
        if let Some(quote_start) = inside.find('"') {
            self.section = inside[..quote_start].trim().to_owned();
            let rest = &inside[quote_start + 1..];
            // Find unescaped closing quote
            let mut sub = String::new();
            let mut chars = rest.chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    if let Some(escaped) = chars.next() {
                        sub.push(escaped);
                    }
                } else if ch == '"' {
                    break;
                } else {
                    sub.push(ch);
                }
            }
            self.subsection = Some(sub);
        } else {
            self.section = inside.trim().to_owned();
            self.subsection = None;
        }
        // Check for inline content after the closing `]`
        let after = trimmed[end + 1..].trim();
        if !after.is_empty() && !after.starts_with('#') && !after.starts_with(';') {
            *inline_remainder = Some(after);
        } else {
            *inline_remainder = None;
        }
        true
    }

    /// Parse a section header line (without inline remainder tracking).
    fn try_parse_section(&mut self, line: &str) -> bool {
        let mut _remainder = None;
        self.try_parse_section_with_remainder(line, &mut _remainder)
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
/// Escape a subsection name for writing in a config section header.
/// In subsection names, `"` and `\` must be escaped.
fn escape_subsection(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out
}

fn escape_value(s: &str) -> String {
    // Quote leading `-` so values are not mistaken for config options (Git does this for
    // submodule paths like `-sub` in `.gitmodules`).
    let needs_quoting = s.starts_with('-')
        || s.starts_with(' ')
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

/// Format a comment suffix for appending to a config value line.
///
/// Git's `--comment` flag normalises the comment:
/// - If the comment already starts with `#` (possibly preceded by whitespace/tab),
///   it is used as-is.
/// - Otherwise, ` # ` is prepended.
fn format_comment_suffix(comment: Option<&str>) -> String {
    match comment {
        None => String::new(),
        Some(c) => {
            if c.starts_with(' ') || c.starts_with('\t') {
                // Comment has its own leading whitespace separator
                c.to_owned()
            } else if c.starts_with('#') {
                // Comment starts with #, just prepend a space separator
                format!(" {c}")
            } else {
                // Plain text comment, prepend " # "
                format!(" # {c}")
            }
        }
    }
}

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
        let raw_lines: Vec<String> = content
            .lines()
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .map(String::from)
            .collect();
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

            let mut inline_remainder = None;
            if parser.try_parse_section_with_remainder(line, &mut inline_remainder) {
                // Check if there's an inline key=value after the section header
                if let Some(remainder) = inline_remainder {
                    if let Some((key, value)) = parser.try_parse_entry(remainder) {
                        if key == "fetch.negotiationalgorithm" && value.is_none() {
                            let file_disp = config_error_path_display(path);
                            return Err(Error::Message(format!(
                                "error: missing value for 'fetch.negotiationalgorithm'\n\
fatal: bad config variable 'fetch.negotiationalgorithm' in file '{file_disp}' at line {}",
                                start_idx + 1
                            )));
                        }
                        entries.push(ConfigEntry {
                            key,
                            value,
                            scope,
                            file: Some(path.to_path_buf()),
                            line: start_idx + 1,
                        });
                    }
                }
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
                if key == "fetch.negotiationalgorithm" && value.is_none() {
                    let file_disp = config_error_path_display(path);
                    return Err(Error::Message(format!(
                        "error: missing value for 'fetch.negotiationalgorithm'\n\
fatal: bad config variable 'fetch.negotiationalgorithm' in file '{file_disp}' at line {}",
                        start_idx + 1
                    )));
                }
                entries.push(ConfigEntry {
                    key,
                    value,
                    scope,
                    file: Some(path.to_path_buf()),
                    line: start_idx + 1,
                });
            } else if logical_line.trim().is_empty() {
                continue;
            } else {
                let file_disp = config_error_path_display(path);
                return Err(Error::Message(format!(
                    "fatal: bad config line {} in file {file_disp}",
                    start_idx + 1
                )));
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            scope,
            entries,
            raw_lines,
            include_origin: ConfigIncludeOrigin::Disk,
        })
    }

    /// Parse like [`Self::parse`] but record a non-disk include origin (blob, stdin, command line).
    pub fn parse_with_origin(
        path: &Path,
        content: &str,
        scope: ConfigScope,
        include_origin: ConfigIncludeOrigin,
    ) -> Result<Self> {
        let mut f = Self::parse(path, content, scope)?;
        f.include_origin = include_origin;
        Ok(f)
    }

    /// Build a synthetic [`ConfigFile`] from `GIT_CONFIG_PARAMETERS` / `git -c` payloads.
    ///
    /// Unlike [`Self::parse`], this accepts flat `key=value` assignments without `[section]`
    /// headers, matching how Git injects command-line configuration.
    pub fn from_git_config_parameters(path: &Path, raw: &str) -> Result<Self> {
        let mut entries = Vec::new();
        let pseudo_path = path.to_path_buf();
        for entry in parse_config_parameters(raw) {
            if let Some((key, val)) = entry.split_once('=') {
                let canon = canonical_key(key.trim())?;
                entries.push(ConfigEntry {
                    key: canon,
                    value: Some(val.to_owned()),
                    scope: ConfigScope::Command,
                    file: Some(pseudo_path.clone()),
                    line: 0,
                });
            } else {
                let canon = canonical_key(entry.trim())?;
                entries.push(ConfigEntry {
                    key: canon,
                    value: None,
                    scope: ConfigScope::Command,
                    file: Some(pseudo_path.clone()),
                    line: 0,
                });
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            scope: ConfigScope::Command,
            entries,
            raw_lines: Vec::new(),
            include_origin: ConfigIncludeOrigin::CommandLine,
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
        self.set_with_comment(key, value, None)
    }

    /// Set a value in this config file, optionally appending an inline comment.
    pub fn set_with_comment(
        &mut self,
        key: &str,
        value: &str,
        comment: Option<&str>,
    ) -> Result<()> {
        let canon = canonical_key(key)?;
        let raw_var = raw_variable_name(key);
        let comment_suffix = format_comment_suffix(comment);

        // Find the last entry with this key to replace in-place.
        let existing_idx = self.entries.iter().rposition(|e| e.key == canon);

        if let Some(idx) = existing_idx {
            let line_idx = self.entries[idx].line - 1;
            let raw_line = &self.raw_lines[line_idx];
            if is_section_header_with_inline_entry(raw_line) {
                // Entry is on the same line as a section header — split it
                let header_only = extract_section_header(raw_line);
                self.raw_lines[line_idx] = header_only;
                let new_line = format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);
                self.raw_lines.insert(line_idx + 1, new_line);
                // Re-parse to fix up entries and line numbers
                let content = self.raw_lines.join("\n");
                let reparsed = Self::parse(&self.path, &content, self.scope)?;
                self.entries = reparsed.entries;
                self.raw_lines = reparsed.raw_lines;
            } else {
                self.raw_lines[line_idx] =
                    format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);
                self.entries[idx].value = Some(value.to_owned());
            }
        } else {
            // Need to add: find or create the section
            let (section, subsection, _var) = split_key(&canon)?;
            let (raw_sec, raw_sub) = raw_section_parts(key);
            let section_line = self.find_or_create_section_preserving_case(
                &section,
                subsection.as_deref(),
                &raw_sec,
                raw_sub.as_deref(),
            );
            let new_line = format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);

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
    pub fn replace_all(
        &mut self,
        key: &str,
        value: &str,
        value_pattern: Option<&str>,
    ) -> Result<()> {
        self.replace_all_with_comment(key, value, value_pattern, None)
    }

    /// Replace all occurrences, optionally appending an inline comment.
    ///
    /// Value patterns starting with `!` are treated as negated regex
    /// (matching values that do NOT match the pattern).
    pub fn replace_all_with_comment(
        &mut self,
        key: &str,
        value: &str,
        value_pattern: Option<&str>,
        comment: Option<&str>,
    ) -> Result<()> {
        let canon = canonical_key(key)?;
        let comment_suffix = format_comment_suffix(comment);

        // Parse optional regex pattern, handling `!` negation
        let (re, negated) = match value_pattern {
            Some(pat) => {
                let (neg, actual_pat) = if let Some(rest) = pat.strip_prefix('!') {
                    (true, rest)
                } else {
                    (false, pat)
                };
                let compiled = regex::Regex::new(actual_pat)
                    .map_err(|e| Error::ConfigError(format!("invalid value-pattern regex: {e}")))?;
                (Some(compiled), neg)
            }
            None => (None, false),
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
                    let matched = re.is_match(v);
                    if negated {
                        !matched
                    } else {
                        matched
                    }
                } else {
                    true
                }
            })
            .map(|(i, _)| i)
            .collect();

        if matching_indices.is_empty() {
            // No matching entries — add a new one at the end of the section
            return self.add_value_with_comment(key, value, comment);
        }

        let raw_var = raw_variable_name(key);

        if matching_indices.len() == 1 {
            // Single match: update in-place (preserves position)
            let match_idx = matching_indices[0];
            let line_idx = self.entries[match_idx].line - 1;
            let raw_line = &self.raw_lines[line_idx];
            if is_section_header_with_inline_entry(raw_line) {
                let header = extract_section_header(raw_line);
                self.raw_lines[line_idx] = header;
                let new_line = format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);
                self.raw_lines.insert(line_idx + 1, new_line);
            } else {
                self.raw_lines[line_idx] =
                    format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);
            }
        } else {
            // Multiple matches: remove ALL, then add one new entry at end of section
            for &idx in matching_indices.iter().rev() {
                let line_idx = self.entries[idx].line - 1;
                self.remove_entry_line(line_idx);
            }

            // Re-parse after removals
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;

            // Add the new entry at the end of the section
            let (section, subsection, _var) = split_key(&canon)?;
            let (raw_sec, raw_sub) = raw_section_parts(key);
            let section_line = self.find_or_create_section_preserving_case(
                &section,
                subsection.as_deref(),
                &raw_sec,
                raw_sub.as_deref(),
            );
            let new_line = format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);
            let insert_at = self.last_line_in_section(section_line) + 1;
            self.raw_lines.insert(insert_at, new_line);
        }

        // Re-parse
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

    /// Remove an entry at the given raw line index.
    ///
    /// If the line is a section header with an inline entry, only the inline
    /// portion is removed (the header is kept). Otherwise the entire line is
    /// removed. Also removes continuation lines following the entry.
    /// Remove an entry at the given raw line index.
    ///
    /// If the line is a section header with an inline entry, only the inline
    /// portion is removed (the header is kept). Otherwise the entire line
    /// (and any continuation lines) is removed.
    fn remove_entry_line(&mut self, line_idx: usize) {
        if is_section_header_with_inline_entry(&self.raw_lines[line_idx]) {
            // Keep the section header, strip the inline entry
            let header = extract_section_header(&self.raw_lines[line_idx]);
            self.raw_lines[line_idx] = header;
        } else {
            // Check if this line has continuation lines and remove them too
            let mut lines_to_remove = 1;
            let mut check_line = self.raw_lines[line_idx].clone();
            while value_line_continues(&check_line)
                && (line_idx + lines_to_remove) < self.raw_lines.len()
            {
                check_line = self.raw_lines[line_idx + lines_to_remove].clone();
                lines_to_remove += 1;
            }
            for _ in 0..lines_to_remove {
                self.raw_lines.remove(line_idx);
            }
        }
    }

    /// Unset (remove) only the last occurrence of a key.
    ///
    /// Returns the number of entries removed (0 or 1).
    pub fn unset_last(&mut self, key: &str) -> Result<usize> {
        let canon = canonical_key(key)?;
        let last_idx = self.entries.iter().rposition(|e| e.key == canon);

        if let Some(idx) = last_idx {
            let line_idx = self.entries[idx].line - 1;
            self.remove_entry_line(line_idx);
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
            self.remove_entry_line(idx);
        }

        if count > 0 {
            let content = self.raw_lines.join("\n");
            let reparsed = Self::parse(&self.path, &content, self.scope)?;
            self.entries = reparsed.entries;
            self.raw_lines = reparsed.raw_lines;
        }

        Ok(count)
    }

    /// Unset entries matching a key and optional value-pattern regex.
    ///
    /// If `value_pattern` is `None`, removes all entries with the given key.
    /// If `value_pattern` is `Some(pat)`, only removes entries whose value matches the regex.
    ///
    /// When `preserve_empty_section_header` is `true`, a section header is kept even if the
    /// section has no remaining keys (Git's `config unset --all`). When `false`, empty sections
    /// are stripped (`config --unset`, `config --unset-all`, and value-pattern unsets).
    pub fn unset_matching(
        &mut self,
        key: &str,
        value_pattern: Option<&str>,
        preserve_empty_section_header: bool,
    ) -> Result<usize> {
        let canon = canonical_key(key)?;
        let re = match value_pattern {
            Some(pat) => Some(
                regex::Regex::new(pat)
                    .map_err(|e| Error::ConfigError(format!("invalid value-pattern regex: {e}")))?,
            ),
            None => None,
        };

        let line_indices: Vec<usize> = self
            .entries
            .iter()
            .filter(|e| {
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
            .map(|e| e.line - 1)
            .collect();

        let count = line_indices.len();
        for &idx in line_indices.iter().rev() {
            self.remove_entry_line(idx);
        }

        if count > 0 {
            if !preserve_empty_section_header {
                // Remove empty section headers (sections with no remaining entries and no comments)
                self.remove_empty_section_headers();
            }

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
        self.add_value_with_comment(key, value, None)
    }

    /// Append a new value with an optional inline comment.
    pub fn add_value_with_comment(
        &mut self,
        key: &str,
        value: &str,
        comment: Option<&str>,
    ) -> Result<()> {
        let canon = canonical_key(key)?;
        let raw_var = raw_variable_name(key);
        let comment_suffix = format_comment_suffix(comment);
        let (section, subsection, _var) = split_key(&canon)?;
        let (raw_sec, raw_sub) = raw_section_parts(key);

        let section_line = self.find_or_create_section_preserving_case(
            &section,
            subsection.as_deref(),
            &raw_sec,
            raw_sub.as_deref(),
        );
        let new_line = format!("\t{} = {}{}", raw_var, escape_value(value), comment_suffix);
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
    /// Remove section headers that have no remaining entries or comments.
    fn remove_empty_section_headers(&mut self) {
        let section_re = regex::Regex::new(r"^\s*\[").unwrap();
        let comment_re = regex::Regex::new(r"^\s*(#|;)").unwrap();

        let mut to_remove: Vec<usize> = Vec::new();
        let len = self.raw_lines.len();

        for i in 0..len {
            let line = &self.raw_lines[i];
            if !section_re.is_match(line) {
                continue;
            }
            // Don't remove section headers that have inline key=value entries
            if is_section_header_with_inline_entry(line) {
                continue;
            }
            // Check if this section header is followed only by blank lines,
            // comments, or another section header (or end of file).
            let mut has_entries = false;
            for j in (i + 1)..len {
                let next = self.raw_lines[j].trim();
                if next.is_empty() {
                    continue;
                }
                if section_re.is_match(&self.raw_lines[j]) {
                    break;
                }
                if comment_re.is_match(&self.raw_lines[j]) {
                    // Has comments — keep the section
                    has_entries = true;
                    break;
                }
                // Has a key-value entry
                has_entries = true;
                break;
            }
            if !has_entries {
                to_remove.push(i);
            }
        }

        // Remove in reverse to preserve indices
        for &idx in to_remove.iter().rev() {
            self.raw_lines.remove(idx);
        }

        // Also remove trailing blank lines
        while self.raw_lines.last().is_some_and(|l| l.trim().is_empty()) {
            self.raw_lines.pop();
        }
    }

    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on write failure.
    pub fn write(&self) -> Result<()> {
        let content = self.raw_lines.join("\n");
        let trimmed = content.trim();
        if trimmed.is_empty() {
            // Write empty file if no content
            fs::write(&self.path, "")?;
        } else {
            // Ensure trailing newline
            let content = if content.ends_with('\n') {
                content
            } else {
                format!("{content}\n")
            };
            fs::write(&self.path, content)?;
        }
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
            Some(sub) => {
                let escaped = escape_subsection(sub);
                format!("[{} \"{}\"]", section, escaped)
            }
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
            Some(sub) => {
                let escaped = escape_subsection(sub);
                format!("[{} \"{}\"]", raw_section, escaped)
            }
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

    /// All merged entries in load order (for listing keys such as `alias.*`).
    #[must_use]
    pub fn entries(&self) -> &[ConfigEntry] {
        &self.entries
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

    /// Last (highest-priority) [`ConfigEntry`] for a key, including origin metadata.
    ///
    /// Bare boolean keys are returned with [`ConfigEntry::value`] set to `None` (same as `get`,
    /// which maps them to `"true"` for string lookups).
    #[must_use]
    pub fn get_last_entry(&self, key: &str) -> Option<ConfigEntry> {
        let canon = canonical_key(key).ok()?;
        self.entries.iter().rev().find(|e| e.key == canon).cloned()
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
            .map(|e| e.value.clone().unwrap_or_default())
            .collect()
    }

    /// All raw values for a key in load order, preserving `None` for bare boolean keys.
    ///
    /// Matches Git's multi-value list where `NULL` means a value-less / boolean-true key.
    #[must_use]
    pub fn get_all_raw(&self, key: &str) -> Vec<Option<String>> {
        let canon = match canonical_key(key) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        self.entries
            .iter()
            .filter(|e| e.key == canon)
            .map(|e| e.value.clone())
            .collect()
    }

    /// Get a boolean value, interpreting `true`/`yes`/`on`/`1` as true and
    /// `false`/`no`/`off`/`0` as false.
    pub fn get_bool(&self, key: &str) -> Option<std::result::Result<bool, String>> {
        self.get(key).map(|v| parse_bool(&v))
    }

    /// Whether pathnames in human-readable output should fully C-quote non-ASCII bytes as octal.
    ///
    /// Maps to Git's `quote_path_fully` (`core.quotepath`, default true). When false, UTF-8 and
    /// other high bytes are emitted literally; only ASCII specials are escaped. Also honors
    /// `core.quotePath` as an alternate spelling.
    #[must_use]
    pub fn quote_path_fully(&self) -> bool {
        let from_key = |key: &str| self.get_bool(key).and_then(|r| r.ok());
        from_key("core.quotepath")
            .or_else(|| from_key("core.quotePath"))
            .unwrap_or(true)
    }

    /// Resolved `core.logAllRefUpdates` using this merged set (includes `git -c` / env), then Git's
    /// bare-repo default when the key is unset everywhere.
    #[must_use]
    pub fn effective_log_refs_config(&self, git_dir: &Path) -> refs::LogRefsConfig {
        if let Some(v) = self.get("core.logAllRefUpdates") {
            let lower = v.trim().to_ascii_lowercase();
            let parsed = match lower.as_str() {
                "always" => Some(refs::LogRefsConfig::Always),
                "1" | "true" | "yes" | "on" => Some(refs::LogRefsConfig::Normal),
                "0" | "false" | "no" | "off" | "never" => Some(refs::LogRefsConfig::None),
                _ => None,
            };
            if let Some(c) = parsed {
                return c;
            }
        }
        refs::effective_log_refs_config(git_dir)
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
        let re = regex::Regex::new(pattern).map_err(|e| format!("invalid key pattern: {e}"))?;
        Ok(self
            .entries
            .iter()
            .filter(|e| re.is_match(&e.key))
            .collect())
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
        let mut opts = LoadConfigOptions::default();
        opts.include_system = include_system;
        opts.include_ctx.git_dir = git_dir.map(PathBuf::from);
        Self::load_with_options(git_dir, &opts)
    }

    /// Load the standard configuration cascade with explicit include and scope control.
    ///
    /// See [`LoadConfigOptions`] for `GIT_CONFIG_PARAMETERS` / `-c` include behaviour.
    pub fn load_with_options(git_dir: Option<&Path>, opts: &LoadConfigOptions) -> Result<Self> {
        let mut set = Self::new();
        let proc = opts.process_includes;
        let ctx = opts.include_ctx.clone();

        // System config
        if opts.include_system && std::env::var("GIT_CONFIG_NOSYSTEM").is_err() {
            let system_path = std::env::var("GIT_CONFIG_SYSTEM")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/etc/gitconfig"));
            match ConfigFile::from_path(&system_path, ConfigScope::System) {
                Ok(Some(f)) => Self::merge_with_includes(&mut set, &f, proc, 0, &ctx)?,
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }

        // Global config (Git merges every existing file: XDG then ~/.gitconfig).
        for path in global_config_paths() {
            match ConfigFile::from_path(&path, ConfigScope::Global) {
                Ok(Some(f)) => Self::merge_with_includes(&mut set, &f, proc, 0, &ctx)?,
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }

        // Local config
        if let Some(gd) = git_dir {
            let local_path = gd.join("config");
            match ConfigFile::from_path(&local_path, ConfigScope::Local) {
                Ok(Some(f)) => Self::merge_with_includes(&mut set, &f, proc, 0, &ctx)?,
                Ok(None) => {}
                Err(e) => return Err(e),
            }

            // Worktree config
            let wt_path = gd.join("config.worktree");
            match ConfigFile::from_path(&wt_path, ConfigScope::Worktree) {
                Ok(Some(f)) => Self::merge_with_includes(&mut set, &f, proc, 0, &ctx)?,
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }

        // Environment overrides: optional file
        if let Ok(path) = std::env::var("GIT_CONFIG") {
            match ConfigFile::from_path(Path::new(&path), ConfigScope::Command) {
                Ok(Some(f)) => {
                    if proc {
                        Self::merge_with_includes(&mut set, &f, proc, 0, &ctx)?;
                    } else {
                        set.merge(&f);
                    }
                }
                Ok(None) => {}
                Err(e) => return Err(e),
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

        // GIT_CONFIG_PARAMETERS — used by `git -c key=value`.
        if let Ok(params) = std::env::var("GIT_CONFIG_PARAMETERS") {
            if proc && opts.command_includes && !params.trim().is_empty() {
                let pseudo = Path::new(":GIT_CONFIG_PARAMETERS");
                let cmd_file = ConfigFile::from_git_config_parameters(pseudo, &params)?;
                Self::merge_with_includes(&mut set, &cmd_file, proc, 0, &ctx)?;
            } else if !params.trim().is_empty() {
                for entry in parse_config_parameters(&params) {
                    if let Some((key, val)) = entry.split_once('=') {
                        let _ = set.add_command_override(key.trim(), val);
                    } else {
                        let _ = set.add_command_override(entry.trim(), "true");
                    }
                }
            }
        }

        Ok(set)
    }

    /// Read configuration the way Git's `read_early_config` / `do_git_config_sequence` does:
    /// system (unless disabled), global files in Git order, optional repository `config` /
    /// `config.worktree`, then `GIT_CONFIG_PARAMETERS`.
    ///
    /// When `git_dir` is `None` (no discovered repository, e.g. `GIT_CEILING_DIRECTORIES`), only
    /// non-repo layers are read — matching Git when discovery returns no gitdir (t1309 ceiling #2).
    ///
    /// Returns all values for `key` in load order (Git's `read_early_config` callback runs once per
    /// occurrence).
    ///
    /// This matches upstream ordering for `test-tool config read_early_config` (t1309, t1305).
    pub fn read_early_config(git_dir: Option<&Path>, key: &str) -> Result<Vec<String>> {
        let mut set = Self::new();
        let ctx = IncludeContext {
            git_dir: git_dir.map(PathBuf::from),
            command_line_relative_include_is_error: false,
        };

        // System
        if std::env::var("GIT_CONFIG_NOSYSTEM").is_err() {
            let system_path = std::env::var("GIT_CONFIG_SYSTEM")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/etc/gitconfig"));
            if let Ok(Some(f)) = ConfigFile::from_path(&system_path, ConfigScope::System) {
                Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
            }
        }

        // Global: all existing candidates (Git merges every readable file).
        for path in global_config_paths() {
            if let Ok(Some(f)) = ConfigFile::from_path(&path, ConfigScope::Global) {
                Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
            }
        }

        if let Some(gd) = git_dir {
            let common_dir = crate::repo::common_git_dir_for_config(gd);
            // Local (commondir) — skip when format is newer than supported (t1309).
            let local_path = common_dir.join("config");
            if let Some(msg) = crate::repo::early_config_ignore_repo_reason(&common_dir) {
                eprintln!("warning: ignoring git dir '{}': {}", gd.display(), msg);
            } else if let Ok(Some(f)) = ConfigFile::from_path(&local_path, ConfigScope::Local) {
                set.merge_file_with_includes(&f, true, &ctx)?;
            }

            // Worktree-specific config (when enabled for this repo).
            let wt_path = gd.join("config.worktree");
            if crate::repo::worktree_config_enabled(&common_dir) {
                if let Ok(Some(f)) = ConfigFile::from_path(&wt_path, ConfigScope::Worktree) {
                    Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
                }
            }
        }

        // GIT_CONFIG_PARAMETERS — same as full load (`load_with_options` default).
        if let Ok(params) = std::env::var("GIT_CONFIG_PARAMETERS") {
            if !params.trim().is_empty() {
                let pseudo = Path::new(":GIT_CONFIG_PARAMETERS");
                let cmd_file = ConfigFile::from_git_config_parameters(pseudo, &params)?;
                Self::merge_with_includes(&mut set, &cmd_file, true, 0, &ctx)?;
            }
        }

        Ok(set.get_all(key))
    }

    /// Merge a single config file, optionally expanding `[include]` / `[includeIf]`.
    ///
    /// Used by `grit config -f` and scoped reads; [`ConfigSet::load_with_options`] uses the same
    /// internal routine for the standard cascade.
    pub fn merge_file_with_includes(
        &mut self,
        file: &ConfigFile,
        process_includes: bool,
        ctx: &IncludeContext,
    ) -> Result<()> {
        Self::merge_with_includes(self, file, process_includes, 0, ctx)
    }

    /// Load only the repository's own `config` file (plus any `[include]` targets).
    ///
    /// Unlike [`Self::load`], this ignores system/global config and environment
    /// overrides. Used for receive-side options (e.g. `transfer.fsckObjects`) so a
    /// pusher's global configuration cannot weaken the remote repository's policy.
    pub fn load_repo_local_only(git_dir: &Path) -> Result<Self> {
        let mut set = Self::new();
        let local_path = git_dir.join("config");
        let ctx = IncludeContext {
            git_dir: Some(git_dir.to_path_buf()),
            command_line_relative_include_is_error: false,
        };
        if let Ok(Some(f)) = ConfigFile::from_path(&local_path, ConfigScope::Local) {
            Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
        }
        Ok(set)
    }

    /// Load configuration the way Git loads **protected** config (e.g. `uploadpack.packObjectsHook`).
    ///
    /// This matches Git's `read_protected_config`: system (optional), global files only (no
    /// repository or worktree `config`), then command-line overrides from `GIT_CONFIG_COUNT` /
    /// `GIT_CONFIG_PARAMETERS`. It does **not** read `$GIT_CONFIG` (Git omits that for protected
    /// config).
    ///
    /// Global file order matches Git: XDG `git/config` first (when present), then `~/.gitconfig`,
    /// unless `GIT_CONFIG_GLOBAL` is set (single file). When both global files exist, both are
    /// merged so later entries win for duplicate keys.
    pub fn load_protected(include_system: bool) -> Result<Self> {
        let mut set = Self::new();
        let ctx = IncludeContext {
            git_dir: None,
            command_line_relative_include_is_error: false,
        };

        if include_system && std::env::var("GIT_CONFIG_NOSYSTEM").is_err() {
            let system_path = std::env::var("GIT_CONFIG_SYSTEM")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/etc/gitconfig"));
            if let Ok(Some(f)) = ConfigFile::from_path(&system_path, ConfigScope::System) {
                Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
            }
        }

        if let Ok(p) = std::env::var("GIT_CONFIG_GLOBAL") {
            let path = PathBuf::from(p);
            if let Ok(Some(f)) = ConfigFile::from_path(&path, ConfigScope::Global) {
                Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
            }
        } else {
            let mut global_paths = Vec::new();
            if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                global_paths.push(PathBuf::from(xdg).join("git/config"));
            } else if let Some(home) = home_dir() {
                global_paths.push(home.join(".config/git/config"));
            }
            if let Some(home) = home_dir() {
                global_paths.push(home.join(".gitconfig"));
            }
            for path in global_paths {
                if let Ok(Some(f)) = ConfigFile::from_path(&path, ConfigScope::Global) {
                    Self::merge_with_includes(&mut set, &f, true, 0, &ctx)?;
                }
            }
        }

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

        if let Ok(params) = std::env::var("GIT_CONFIG_PARAMETERS") {
            for entry in parse_config_parameters(&params) {
                if let Some((key, val)) = entry.split_once('=') {
                    let _ = set.add_command_override(key.trim(), val);
                } else {
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
        depth: usize,
        ctx: &IncludeContext,
    ) -> Result<()> {
        // Mirror Git behavior and stop runaway include recursion.
        // t0017 expects the diagnostic to contain this exact phrase.
        const MAX_INCLUDE_DEPTH: usize = 10;
        if depth > MAX_INCLUDE_DEPTH {
            return Err(Error::ConfigError(
                "exceeded maximum include depth".to_owned(),
            ));
        }
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
                    if !evaluate_include_condition(cond, file, ctx) {
                        continue;
                    }
                }

                let resolved = match resolve_include_file_path(&inc_path, file, ctx) {
                    Ok(p) => p,
                    Err(Error::ConfigError(msg)) if msg.is_empty() => continue,
                    Err(e) => return Err(e),
                };
                if let Ok(Some(inc_file)) = ConfigFile::from_path(&resolved, file.scope) {
                    Self::merge_with_includes(set, &inc_file, process_includes, depth + 1, ctx)?;
                }
            }
        }

        Ok(())
    }
}

// ── Type coercion helpers ───────────────────────────────────────────

/// Parse a Git boolean value.
///
/// Accepts: `true`, `yes`, `on`, `1` as true.
/// Accepts: `false`, `no`, `off`, `0` (and explicit empty value) as false.
///
/// Note: bare config keys are represented as `None` in [`ConfigEntry`] and
/// are normalized to `"true"` by higher-level readers (`ConfigSet::get`).
/// This parser only sees string values and therefore treats `""` as an
/// explicitly empty assignment (`key =`), which Git interprets as false.
pub fn parse_bool(s: &str) -> std::result::Result<bool, String> {
    match s.to_lowercase().as_str() {
        "true" | "yes" | "on" => Ok(true),
        "" => Ok(false),
        "false" | "no" | "off" => Ok(false),
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

/// Why [`parse_git_config_int_strict`] failed (mirrors Git `errno` after `git_parse_signed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitConfigIntStrictError {
    /// `EINVAL` — trailing junk, unknown unit suffix, or not a number.
    InvalidUnit,
    /// `ERANGE` — value does not fit in `i64` after scaling.
    OutOfRange,
}

/// Parse a signed decimal integer with optional `k`/`m`/`g` multiplier suffix, requiring the
/// entire input (trimmed) to be consumed — same constraints as Git's `git_parse_signed` used by
/// `git_config_int` (so `no` and `1foo` are rejected, unlike [`parse_i64`]).
pub fn parse_git_config_int_strict(raw: &str) -> std::result::Result<i64, GitConfigIntStrictError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(GitConfigIntStrictError::InvalidUnit);
    }

    let bytes = s.as_bytes();
    let mut idx = 0usize;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        idx = 1;
    }
    if idx >= bytes.len() {
        return Err(GitConfigIntStrictError::InvalidUnit);
    }
    let digit_start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == digit_start {
        return Err(GitConfigIntStrictError::InvalidUnit);
    }

    let num_part =
        std::str::from_utf8(&bytes[..idx]).map_err(|_| GitConfigIntStrictError::InvalidUnit)?;
    let suffix =
        std::str::from_utf8(&bytes[idx..]).map_err(|_| GitConfigIntStrictError::InvalidUnit)?;
    let mult: i64 = match suffix {
        "" => 1,
        "k" | "K" => 1024,
        "m" | "M" => 1024 * 1024,
        "g" | "G" => 1024_i64
            .checked_mul(1024)
            .and_then(|x| x.checked_mul(1024))
            .ok_or(GitConfigIntStrictError::OutOfRange)?,
        _ => return Err(GitConfigIntStrictError::InvalidUnit),
    };

    let val: i64 = num_part
        .parse()
        .map_err(|_| GitConfigIntStrictError::InvalidUnit)?;
    val.checked_mul(mult)
        .ok_or(GitConfigIntStrictError::OutOfRange)
}

const DIFF_CONTEXT_KEY: &str = "diff.context";

fn format_bad_numeric_diff_context(
    value: &str,
    err: GitConfigIntStrictError,
    entry: &ConfigEntry,
) -> String {
    let detail = match err {
        GitConfigIntStrictError::InvalidUnit => "invalid unit",
        GitConfigIntStrictError::OutOfRange => "out of range",
    };
    if entry.scope == ConfigScope::Command || entry.file.is_none() {
        return format!(
            "fatal: bad numeric config value '{value}' for '{DIFF_CONTEXT_KEY}': {detail}"
        );
    }
    let path = entry
        .file
        .as_deref()
        .map(config_error_path_display)
        .unwrap_or_default();
    format!("fatal: bad numeric config value '{value}' for '{DIFF_CONTEXT_KEY}' in file {path}: {detail}")
}

fn format_bad_diff_context_variable(entry: &ConfigEntry) -> String {
    if entry.scope == ConfigScope::Command || entry.file.is_none() {
        return format!("fatal: unable to parse '{DIFF_CONTEXT_KEY}' from command-line config");
    }
    let path = entry
        .file
        .as_deref()
        .map(config_error_path_display)
        .unwrap_or_default();
    format!(
        "fatal: bad config variable '{DIFF_CONTEXT_KEY}' in file '{path}' at line {}",
        entry.line
    )
}

/// Read `diff.context` from a loaded [`ConfigSet`] with Git-compatible validation.
///
/// Returns `Ok(None)` when the key is unset. When set, the value must be a non-negative integer
/// acceptable to Git's diff machinery (same rules as `git diff` / `git log -p`).
pub fn resolve_diff_context_lines(cfg: &ConfigSet) -> std::result::Result<Option<usize>, String> {
    let Some(entry) = cfg.get_last_entry(DIFF_CONTEXT_KEY) else {
        return Ok(None);
    };
    let value_src = entry.value.as_deref().unwrap_or("").trim();
    match parse_git_config_int_strict(value_src) {
        Ok(n) if n < 0 => Err(format_bad_diff_context_variable(&entry)),
        Ok(n) => Ok(Some(usize::try_from(n).map_err(|_| {
            format_bad_numeric_diff_context(value_src, GitConfigIntStrictError::OutOfRange, &entry)
        })?)),
        Err(e) => Err(format_bad_numeric_diff_context(value_src, e, &entry)),
    }
}

/// Parse a Git color value and return the ANSI escape sequence.
///
/// Matches Git's `color_parse_mem` (`git/color.c`): whitespace-separated words,
/// optional leading `reset`, up to two color tokens (foreground then background),
/// then graphic rendition attributes. Attribute codes are accumulated as a
/// bitmask keyed by SGR number (so `bold` sets bit 1, `nobold` sets bit 22).
pub fn parse_color(s: &str) -> std::result::Result<String, String> {
    const COLOR_BACKGROUND_OFFSET: i32 = 10;
    const COLOR_FOREGROUND_ANSI: i32 = 30;
    const COLOR_FOREGROUND_RGB: i32 = 38;
    const COLOR_FOREGROUND_256: i32 = 38;
    const COLOR_FOREGROUND_BRIGHT_ANSI: i32 = 90;

    #[derive(Clone, Copy, Default)]
    struct Color {
        kind: u8,
        value: u8,
        red: u8,
        green: u8,
        blue: u8,
    }

    const COLOR_UNSPECIFIED: u8 = 0;
    const COLOR_NORMAL: u8 = 1;
    const COLOR_ANSI: u8 = 2;
    const COLOR_256: u8 = 3;
    const COLOR_RGB: u8 = 4;

    fn color_empty(c: &Color) -> bool {
        c.kind == COLOR_UNSPECIFIED || c.kind == COLOR_NORMAL
    }

    fn parse_ansi_color(name: &str) -> Option<Color> {
        let color_names = [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ];
        let color_offset = COLOR_FOREGROUND_ANSI;

        if name.eq_ignore_ascii_case("default") {
            return Some(Color {
                kind: COLOR_ANSI,
                value: (9 + color_offset) as u8,
                ..Default::default()
            });
        }

        let (name, color_offset) = if name.len() >= 6 && name[..6].eq_ignore_ascii_case("bright") {
            (&name[6..], COLOR_FOREGROUND_BRIGHT_ANSI)
        } else {
            (name, COLOR_FOREGROUND_ANSI)
        };

        for (i, cn) in color_names.iter().enumerate() {
            if name.eq_ignore_ascii_case(cn) {
                return Some(Color {
                    kind: COLOR_ANSI,
                    value: (i as i32 + color_offset) as u8,
                    ..Default::default()
                });
            }
        }
        None
    }

    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    fn get_hex_color(chars: &[u8], width: usize) -> Option<(u8, usize)> {
        assert!(width == 1 || width == 2);
        if chars.len() < width {
            return None;
        }
        let v = if width == 2 {
            let hi = hex_val(chars[0])?;
            let lo = hex_val(chars[1])?;
            (hi << 4) | lo
        } else {
            let n = hex_val(chars[0])?;
            (n << 4) | n
        };
        Some((v, width))
    }

    fn parse_single_color(word: &str) -> Option<Color> {
        if word.eq_ignore_ascii_case("normal") {
            return Some(Color {
                kind: COLOR_NORMAL,
                ..Default::default()
            });
        }

        let bytes = word.as_bytes();
        if (bytes.len() == 7 || bytes.len() == 4) && bytes.first() == Some(&b'#') {
            let width = if bytes.len() == 7 { 2 } else { 1 };
            let mut idx = 1;
            let (r, n1) = get_hex_color(&bytes[idx..], width)?;
            idx += n1;
            let (g, n2) = get_hex_color(&bytes[idx..], width)?;
            idx += n2;
            let (b, n3) = get_hex_color(&bytes[idx..], width)?;
            idx += n3;
            if idx != bytes.len() {
                return None;
            }
            return Some(Color {
                kind: COLOR_RGB,
                red: r,
                green: g,
                blue: b,
                ..Default::default()
            });
        }

        if let Some(c) = parse_ansi_color(word) {
            return Some(c);
        }

        let Ok(val) = word.parse::<i64>() else {
            return None;
        };
        if val < -1 {
            return None;
        }
        if val < 0 {
            return Some(Color {
                kind: COLOR_NORMAL,
                ..Default::default()
            });
        }
        if val < 8 {
            return Some(Color {
                kind: COLOR_ANSI,
                value: (val as i32 + COLOR_FOREGROUND_ANSI) as u8,
                ..Default::default()
            });
        }
        if val < 16 {
            return Some(Color {
                kind: COLOR_ANSI,
                value: (val as i32 - 8 + COLOR_FOREGROUND_BRIGHT_ANSI) as u8,
                ..Default::default()
            });
        }
        if val < 256 {
            return Some(Color {
                kind: COLOR_256,
                value: val as u8,
                ..Default::default()
            });
        }
        None
    }

    fn parse_attr(word: &str) -> Option<u8> {
        const ATTRS: [(&str, u8, u8); 8] = [
            ("bold", 1, 22),
            ("dim", 2, 22),
            ("italic", 3, 23),
            ("ul", 4, 24),
            ("underline", 4, 24),
            ("blink", 5, 25),
            ("reverse", 7, 27),
            ("strike", 9, 29),
        ];

        let mut negate = false;
        let mut rest = word;
        if let Some(stripped) = rest.strip_prefix("no") {
            negate = true;
            rest = stripped;
            if let Some(s) = rest.strip_prefix('-') {
                rest = s;
            }
        }

        for (name, val, neg) in ATTRS {
            if rest == name {
                return Some(if negate { neg } else { val });
            }
        }
        None
    }

    fn append_color_output(out: &mut String, c: &Color, background: bool) {
        let offset = if background {
            COLOR_BACKGROUND_OFFSET
        } else {
            0
        };
        match c.kind {
            COLOR_UNSPECIFIED | COLOR_NORMAL => {}
            COLOR_ANSI => {
                use std::fmt::Write;
                let _ = write!(out, "{}", i32::from(c.value) + offset);
            }
            COLOR_256 => {
                use std::fmt::Write;
                let _ = write!(out, "{};5;{}", COLOR_FOREGROUND_256 + offset, c.value);
            }
            COLOR_RGB => {
                use std::fmt::Write;
                let _ = write!(
                    out,
                    "{};2;{};{};{}",
                    COLOR_FOREGROUND_RGB + offset,
                    c.red,
                    c.green,
                    c.blue
                );
            }
            _ => {}
        }
    }

    let s = s.trim();
    if s.is_empty() {
        return Ok(String::new());
    }

    let mut has_reset = false;
    let mut attr: u64 = 0;
    let mut fg = Color::default();
    let mut bg = Color::default();
    fg.kind = COLOR_UNSPECIFIED;
    bg.kind = COLOR_UNSPECIFIED;

    for word in s.split_whitespace() {
        if word.eq_ignore_ascii_case("reset") {
            has_reset = true;
            continue;
        }

        if let Some(c) = parse_single_color(word) {
            if fg.kind == COLOR_UNSPECIFIED {
                fg = c;
                continue;
            }
            if bg.kind == COLOR_UNSPECIFIED {
                bg = c;
                continue;
            }
            return Err(format!("bad color value '{s}'"));
        }

        if let Some(code) = parse_attr(word) {
            attr |= 1u64 << u64::from(code);
            continue;
        }

        return Err(format!("bad color value '{s}'"));
    }

    if !has_reset && attr == 0 && color_empty(&fg) && color_empty(&bg) {
        return Err(format!("bad color value '{s}'"));
    }

    let mut out = String::from("\x1b[");
    let mut sep = if has_reset { 1u32 } else { 0u32 };

    let mut attr_bits = attr;
    let mut i = 0u32;
    while attr_bits != 0 {
        let bit = 1u64 << i;
        if attr_bits & bit == 0 {
            i += 1;
            continue;
        }
        attr_bits &= !bit;
        if sep > 0 {
            out.push(';');
        }
        sep += 1;
        use std::fmt::Write;
        let _ = write!(out, "{i}");
        i += 1;
    }

    if !color_empty(&fg) {
        if sep > 0 {
            out.push(';');
        }
        sep += 1;
        append_color_output(&mut out, &fg, false);
    }
    if !color_empty(&bg) {
        if sep > 0 {
            out.push(';');
        }
        append_color_output(&mut out, &bg, true);
    }
    out.push('m');
    Ok(out)
}

/// Match a URL against a URL pattern from config.
pub fn url_matches(pattern_url: &str, target_url: &str) -> bool {
    let pattern = pattern_url.trim_end_matches('/');
    let target = target_url.trim_end_matches('/');
    if target == pattern {
        return true;
    }
    if let Some(rest) = target.strip_prefix(pattern) {
        return rest.starts_with('/') || rest.is_empty();
    }
    let pattern_slash = format!("{}/", pattern);
    target.starts_with(&pattern_slash)
}

/// Get the best URL match for a specific key.
pub fn get_urlmatch_entries<'a>(
    entries: &'a [ConfigEntry],
    section: &str,
    variable: &str,
    url: &str,
) -> Vec<&'a ConfigEntry> {
    let section_lower = section.to_lowercase();
    let variable_lower = variable.to_lowercase();
    let mut matches: Vec<(usize, &'a ConfigEntry)> = Vec::new();

    for entry in entries {
        let key = &entry.key;
        let first_dot = match key.find('.') {
            Some(i) => i,
            None => continue,
        };
        let last_dot = match key.rfind('.') {
            Some(i) => i,
            None => continue,
        };
        let entry_section = &key[..first_dot];
        let entry_variable = &key[last_dot + 1..];
        if entry_section.to_lowercase() != section_lower
            || entry_variable.to_lowercase() != variable_lower
        {
            continue;
        }
        if first_dot == last_dot {
            matches.push((0, entry));
        } else {
            let subsection = &key[first_dot + 1..last_dot];
            if url_matches(subsection, url) {
                matches.push((subsection.len(), entry));
            }
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0));
    matches.into_iter().map(|(_, e)| e).collect()
}

/// Get all matching variables in a section for a given URL.
pub fn get_urlmatch_all_in_section(
    entries: &[ConfigEntry],
    section: &str,
    url: &str,
) -> Vec<(String, String, ConfigScope)> {
    let section_lower = section.to_lowercase();
    let mut matches: Vec<(String, usize, String, String, ConfigScope)> = Vec::new();

    for entry in entries {
        let key = &entry.key;
        let first_dot = match key.find('.') {
            Some(i) => i,
            None => continue,
        };
        let last_dot = match key.rfind('.') {
            Some(i) => i,
            None => continue,
        };
        let entry_section = &key[..first_dot];
        if entry_section.to_lowercase() != section_lower {
            continue;
        }
        let entry_variable = &key[last_dot + 1..];
        let val = entry.value.as_deref().unwrap_or("true");
        if first_dot == last_dot {
            let canonical = format!("{}.{}", section_lower, entry_variable);
            matches.push((
                entry_variable.to_lowercase(),
                0,
                val.to_owned(),
                canonical,
                entry.scope,
            ));
        } else {
            let subsection = &key[first_dot + 1..last_dot];
            if url_matches(subsection, url) {
                let canonical = format!("{}.{}", section_lower, entry_variable);
                matches.push((
                    entry_variable.to_lowercase(),
                    subsection.len(),
                    val.to_owned(),
                    canonical,
                    entry.scope,
                ));
            }
        }
    }

    let mut best: std::collections::BTreeMap<String, (usize, String, String, ConfigScope)> =
        std::collections::BTreeMap::new();
    for (var, specificity, val, canonical, scope) in matches {
        let entry = best
            .entry(var)
            .or_insert((0, String::new(), String::new(), scope));
        if specificity >= entry.0 {
            *entry = (specificity, val, canonical, scope);
        }
    }
    best.into_values()
        .map(|(_, val, canonical, scope)| (canonical, val, scope))
        .collect()
}

/// Parse a Git path value (expand `~/` to home directory).
/// Parse a path value. Returns the resolved path string.
/// Does NOT handle :(optional) prefix — use `parse_path_optional` for that.
pub fn parse_path(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    s.to_owned()
}

/// Parse a path value that may have an `:(optional)` prefix.
///
/// Returns `Some(path)` if the path should be used, `None` if the path
/// is optional and does not exist (meaning the entry should be skipped).
pub fn parse_path_optional(s: &str) -> Option<String> {
    if let Some(rest) = s.strip_prefix(":(optional)") {
        let resolved = parse_path(rest);
        if std::path::Path::new(&resolved).exists() {
            Some(resolved)
        } else {
            None // optional and missing → skip
        }
    } else {
        Some(parse_path(s))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Parse `GIT_CONFIG_PARAMETERS` payloads.
///
/// We support the common formats seen in tests and wrappers:
/// - single-quoted entries: `'key=value'`
/// - double-quoted entries: `"key=value"`
/// - unquoted `key=value` tokens separated by whitespace
///
/// Backslash escapes are interpreted minimally inside double quotes.
pub(crate) fn parse_config_parameters(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;

    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                buf.push(ch);
            }
            continue;
        }
        if in_double {
            if ch == '"' {
                in_double = false;
                continue;
            }
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    let mapped = match next {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '"' => '"',
                        '\\' => '\\',
                        other => other,
                    };
                    buf.push(mapped);
                }
                continue;
            }
            buf.push(ch);
            continue;
        }

        if ch == '\'' {
            in_single = true;
            continue;
        }
        if ch == '"' {
            in_double = true;
            continue;
        }

        if ch.is_whitespace() {
            if !buf.is_empty() {
                out.push(std::mem::take(&mut buf));
            }
            continue;
        }

        buf.push(ch);
    }

    if !buf.is_empty() {
        out.push(buf);
    }

    out
}

/// Return candidate paths for the global config file, in priority order.
/// Public accessor for the ordered list of global config file paths.
pub fn global_config_paths_pub() -> Vec<PathBuf> {
    global_config_paths()
}

fn global_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // $GIT_CONFIG_GLOBAL overrides
    if let Ok(p) = std::env::var("GIT_CONFIG_GLOBAL") {
        paths.push(PathBuf::from(p));
        return paths;
    }

    // Git order: XDG `git/config` first, then `~/.gitconfig` (see `git_global_config_paths`).
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("git/config"));
    } else if let Some(home) = home_dir() {
        paths.push(home.join(".config/git/config"));
    }
    if let Some(home) = home_dir() {
        paths.push(home.join(".gitconfig"));
    }

    paths
}

/// Return the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// True when Git would treat the config source as `CONFIG_ORIGIN_FILE` for includes.
fn include_source_is_disk_file(file: &ConfigFile) -> bool {
    file.include_origin == ConfigIncludeOrigin::Disk
}

/// Resolve an include file path (Git `handle_path_include` semantics).
///
/// Relative paths are only allowed when the including config came from a real on-disk file.
fn resolve_include_file_path(
    path: &str,
    file: &ConfigFile,
    ctx: &IncludeContext,
) -> Result<PathBuf> {
    let expanded = parse_path(path);
    let p = Path::new(&expanded);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    if !include_source_is_disk_file(file) {
        if file.include_origin == ConfigIncludeOrigin::CommandLine {
            if ctx.command_line_relative_include_is_error {
                return Err(Error::ConfigError(
                    "relative config includes must come from files".to_owned(),
                ));
            }
            return Err(Error::ConfigError(String::new()));
        }
        return Err(Error::ConfigError(
            "relative config includes must come from files".to_owned(),
        ));
    }
    let base = match file.path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        Some(_) | None => Path::new("."),
    };
    Ok(base.join(p))
}

fn is_dir_sep(b: u8) -> bool {
    b == b'/' || b == b'\\'
}

fn add_trailing_starstar_for_dir(pat: &mut String) {
    let bytes = pat.as_bytes();
    if !bytes.is_empty() && is_dir_sep(*bytes.last().unwrap()) {
        pat.push_str("**");
    }
}

/// Prepare a `gitdir:` / `gitdir/i:` pattern (Git `prepare_include_condition_pattern`).
fn prepare_gitdir_pattern(condition: &str, file: &ConfigFile) -> Result<(String, usize)> {
    // Git `interpolate_path`: expand `~/` in the condition before pattern rules.
    let mut pat = parse_path(condition);
    if pat.starts_with("./") || pat.starts_with(".\\") {
        if !include_source_is_disk_file(file) {
            return Err(Error::ConfigError(
                "relative config include conditionals must come from files".to_owned(),
            ));
        }
        let parent = file.path.parent().ok_or_else(|| {
            Error::ConfigError(
                "relative config include conditionals must come from files".to_owned(),
            )
        })?;
        let real = parent.canonicalize().map_err(Error::Io)?;
        let mut dir = real.to_string_lossy().into_owned();
        if !dir.ends_with('/') && !dir.ends_with('\\') {
            dir.push('/');
        }
        let rest = &pat[2..];
        pat = format!("{dir}{rest}");
        let prefix_len = dir.len();
        add_trailing_starstar_for_dir(&mut pat);
        return Ok((pat, prefix_len));
    }
    let p = Path::new(&pat);
    if !p.is_absolute() {
        pat.insert_str(0, "**/");
    }
    add_trailing_starstar_for_dir(&mut pat);
    Ok((pat, 0))
}

/// Git `include_by_gitdir` tries `strbuf_realpath` first, then `strbuf_add_absolute_path` if no match.
fn git_dir_match_texts(git_dir: &Path) -> (String, String) {
    let real = git_dir
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| git_dir.to_string_lossy().into_owned());
    let abs = if git_dir.is_absolute() {
        git_dir.to_string_lossy().into_owned()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(git_dir).to_string_lossy().into_owned()
    } else {
        git_dir.to_string_lossy().into_owned()
    };
    (real, abs)
}

fn include_by_gitdir(
    condition: &str,
    file: &ConfigFile,
    ctx: &IncludeContext,
    icase: bool,
) -> bool {
    let Some(git_dir) = ctx.git_dir.as_ref() else {
        return false;
    };
    let (pattern, prefix) = match prepare_gitdir_pattern(condition, file) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let flags = WM_PATHNAME | if icase { WM_CASEFOLD } else { 0 };
    let (text_real, text_abs) = git_dir_match_texts(git_dir);
    let try_match = |text: &str| -> bool {
        let t = text.as_bytes();
        let p = pattern.as_bytes();
        if prefix > 0 {
            if t.len() < prefix {
                return false;
            }
            let pre = &p[..prefix];
            let te = &t[..prefix];
            let ok = if icase {
                pre.eq_ignore_ascii_case(te)
            } else {
                pre == te
            };
            if !ok {
                return false;
            }
            return wildmatch(&p[prefix..], &t[prefix..], flags);
        }
        wildmatch(p, t, flags)
    };
    if try_match(&text_real) {
        return true;
    }
    text_real != text_abs && try_match(&text_abs)
}

fn current_branch_short_name(git_dir: Option<&Path>) -> Option<String> {
    let gd = git_dir?;
    let target = refs::read_symbolic_ref(gd, "HEAD").ok()??;
    let rest = target.strip_prefix("refs/heads/")?;
    Some(rest.to_owned())
}

fn include_by_onbranch(condition: &str, ctx: &IncludeContext) -> bool {
    let Some(short) = current_branch_short_name(ctx.git_dir.as_deref()) else {
        return false;
    };
    let mut pattern = condition.to_owned();
    add_trailing_starstar_for_dir(&mut pattern);
    wildmatch(pattern.as_bytes(), short.as_bytes(), WM_PATHNAME)
}

/// Evaluate an `[includeIf]` condition.
///
/// Supports `gitdir:`, `gitdir/i:`, and `onbranch:` like Git. Unknown prefixes are false.
fn evaluate_include_condition(condition: &str, file: &ConfigFile, ctx: &IncludeContext) -> bool {
    if let Some(rest) = condition.strip_prefix("gitdir/i:") {
        return include_by_gitdir(rest, file, ctx, true);
    }
    if let Some(rest) = condition.strip_prefix("gitdir:") {
        return include_by_gitdir(rest, file, ctx, false);
    }
    if let Some(rest) = condition.strip_prefix("onbranch:") {
        return include_by_onbranch(rest, ctx);
    }
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

/// Check if a raw line is a section header that also contains an inline key=value.
fn is_section_header_with_inline_entry(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return false;
    }
    let end = match trimmed.find(']') {
        Some(i) => i,
        None => return false,
    };
    let after = trimmed[end + 1..].trim();
    // Has non-comment content after the ]
    !after.is_empty() && !after.starts_with('#') && !after.starts_with(';')
}

/// Extract just the section header portion (up to and including `]` and any
/// comment after it, but not any inline key=value) from a raw line.
fn extract_section_header(line: &str) -> String {
    let trimmed = line.trim();
    let end = match trimmed.find(']') {
        Some(i) => i,
        None => return line.to_owned(),
    };
    // Preserve any comment on the section header itself (between ] and key),
    // but git doesn't really do this. Just return up to ].
    trimmed[..=end].to_owned()
}

#[cfg(test)]
mod get_regexp_tests {
    use super::{ConfigFile, ConfigScope, ConfigSet};
    use std::path::Path;

    fn set_from_snippet(text: &str) -> ConfigSet {
        let path = Path::new(".git/config");
        let file = ConfigFile::parse(path, text, ConfigScope::Local).expect("parse config snippet");
        let mut set = ConfigSet::new();
        set.merge(&file);
        set
    }

    #[test]
    fn get_regexp_matches_section_prefix_like_git_config() {
        let text = r#"
[user]
    email = alice@example.com
    name = Alice
[core]
    bare = false
"#;
        let set = set_from_snippet(text);
        let keys: Vec<_> = set
            .get_regexp("user")
            .expect("valid pattern")
            .into_iter()
            .map(|e| e.key.as_str())
            .collect();
        assert!(keys.contains(&"user.email"));
        assert!(keys.contains(&"user.name"));
        assert!(!keys.iter().any(|k| k.starts_with("core.")));
    }

    #[test]
    fn get_regexp_returns_all_multi_value_entries_in_order() {
        let text = r#"
[remote "origin"]
    url = https://example.com/repo.git
    fetch = +refs/heads/*:refs/remotes/origin/*
    push = +refs/heads/main:refs/heads/main
    push = +refs/heads/develop:refs/heads/develop
"#;
        let set = set_from_snippet(text);
        let matches = set.get_regexp("remote.origin").expect("valid pattern");
        let push_vals: Vec<_> = matches
            .iter()
            .filter(|e| e.key == "remote.origin.push")
            .map(|e| e.value.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(push_vals.len(), 2);
        assert_eq!(push_vals[0], "+refs/heads/main:refs/heads/main");
        assert_eq!(push_vals[1], "+refs/heads/develop:refs/heads/develop");
    }

    #[test]
    fn get_regexp_dot_matches_any_key() {
        let text = r#"
[a]
    x = 1
[b]
    y = 2
"#;
        let set = set_from_snippet(text);
        let m = set.get_regexp(".").expect("valid pattern");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn get_regexp_no_match_returns_empty_vec() {
        let set = set_from_snippet("[user]\n\tname = x\n");
        let m = set.get_regexp("zzz").expect("valid pattern");
        assert!(m.is_empty());
    }

    #[test]
    fn get_regexp_invalid_pattern_is_error() {
        let set = set_from_snippet("[user]\n\tname = x\n");
        let err = set.get_regexp("(").expect_err("unclosed group");
        assert!(err.contains("invalid key pattern"), "got: {err}");
    }
}
