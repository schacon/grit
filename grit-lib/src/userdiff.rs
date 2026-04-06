//! User-defined and built-in diff function-name matching.
//!
//! This module implements the subset of Git's `userdiff` behavior needed for
//! hunk-header function context extraction.

use crate::config::ConfigSet;
use crate::crlf::{get_file_attrs, AttrRule};
use regex::{Regex, RegexBuilder};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

const USERDIFF_SOURCE: &str = include_str!("../../git/userdiff.c");

#[derive(Debug, Clone)]
struct FuncRule {
    matcher: RuleMatcher,
    negate: bool,
}

#[derive(Debug, Clone)]
enum RuleMatcher {
    Rust(Regex),
    Posix { pattern: String, ignore_case: bool },
}

#[derive(Debug, Clone)]
struct BuiltinPattern {
    pattern: String,
    ignore_case: bool,
}

/// Compiled function-name matcher used for diff hunk headers.
#[derive(Debug, Clone)]
pub struct FuncnameMatcher {
    rules: Vec<FuncRule>,
}

impl FuncnameMatcher {
    /// Match a source line against configured funcname rules.
    ///
    /// Returns the text to show after the hunk header when matched.
    #[must_use]
    pub fn match_line(&self, line: &str) -> Option<String> {
        let mut text = line;
        if let Some(stripped) = text.strip_suffix('\n') {
            text = stripped;
            if let Some(stripped_cr) = text.strip_suffix('\r') {
                text = stripped_cr;
            }
        }

        for rule in &self.rules {
            let matched_text = match &rule.matcher {
                RuleMatcher::Rust(regex) => {
                    let Some(caps) = regex.captures(text) else {
                        continue;
                    };
                    caps.get(1)
                        .or_else(|| caps.get(0))
                        .map(|m| m.as_str())
                        .unwrap_or_default()
                        .trim_end_matches(char::is_whitespace)
                        .to_owned()
                }
                RuleMatcher::Posix {
                    pattern,
                    ignore_case,
                } => {
                    if !posix_line_matches(pattern, *ignore_case, text) {
                        continue;
                    }
                    text.trim_end_matches(char::is_whitespace).to_owned()
                }
            };
            if rule.negate {
                return None;
            }
            return Some(matched_text);
        }
        None
    }
}

/// Resolve a function-name matcher for `rel_path` from attributes + config.
///
/// Returns `Ok(None)` when no diff driver is configured for the path.
pub fn matcher_for_path(
    config: &ConfigSet,
    rules: &[AttrRule],
    rel_path: &str,
) -> Result<Option<FuncnameMatcher>, String> {
    let attrs = get_file_attrs(rules, rel_path, config);
    let Some(driver) = attrs.diff_driver else {
        return Ok(None);
    };
    matcher_for_driver(config, &driver)
}

/// Resolve a function-name matcher for a named diff driver.
///
/// Returns `Ok(None)` when the driver has no built-in or configured funcname
/// pattern.
pub fn matcher_for_driver(
    config: &ConfigSet,
    driver: &str,
) -> Result<Option<FuncnameMatcher>, String> {
    if let Some(pattern) = config.get(&format!("diff.{driver}.xfuncname")) {
        return compile_matcher(&pattern, true, false).map(Some);
    }
    if let Some(pattern) = config.get(&format!("diff.{driver}.funcname")) {
        return compile_matcher(&pattern, false, false).map(Some);
    }
    if let Some(builtin) = builtin_patterns().get(driver) {
        return compile_matcher(&builtin.pattern, true, builtin.ignore_case).map(Some);
    }
    Ok(None)
}

fn compile_matcher(
    pattern: &str,
    extended: bool,
    ignore_case: bool,
) -> Result<FuncnameMatcher, String> {
    let lines: Vec<&str> = pattern.split('\n').collect();
    if lines.is_empty() {
        return Ok(FuncnameMatcher { rules: Vec::new() });
    }

    let mut rules = Vec::with_capacity(lines.len());
    for (idx, raw) in lines.iter().enumerate() {
        let mut line = *raw;
        let negate = line.starts_with('!');
        if negate {
            if idx == lines.len() - 1 {
                return Err(format!("Last expression must not be negated: {line}"));
            }
            line = &line[1..];
        }

        let rust_pattern = if extended {
            fix_charclass_escapes(line)
        } else {
            bre_to_ere(line)
        };
        let posix_pattern = if extended {
            line.to_owned()
        } else {
            bre_to_ere(line)
        };

        validate_posix_regex_via_grep(&posix_pattern, ignore_case)
            .map_err(|_| format!("Invalid regexp to look for hunk header: {line}"))?;

        let matcher = RegexBuilder::new(&rust_pattern)
            .case_insensitive(ignore_case)
            .build()
            .map(RuleMatcher::Rust)
            .unwrap_or_else(|_| RuleMatcher::Posix {
                pattern: posix_pattern,
                ignore_case,
            });
        rules.push(FuncRule { matcher, negate });
    }

    Ok(FuncnameMatcher { rules })
}

fn builtin_patterns() -> &'static BTreeMap<String, BuiltinPattern> {
    static BUILTIN_PATTERNS: OnceLock<BTreeMap<String, BuiltinPattern>> = OnceLock::new();
    BUILTIN_PATTERNS.get_or_init(parse_builtin_patterns)
}

fn parse_builtin_patterns() -> BTreeMap<String, BuiltinPattern> {
    let mut map = BTreeMap::new();
    let mut offset = 0usize;
    while let Some((is_ipattern, macro_start, args_start)) = find_next_macro(offset) {
        let Some((args, end_offset)) = parse_macro_args(args_start) else {
            break;
        };
        offset = end_offset.max(macro_start + 1);
        if args.len() < 2 {
            continue;
        }

        let Some(name) = decode_concat_c_strings(&args[0]) else {
            continue;
        };
        if name.is_empty() || name == "default" {
            continue;
        }
        let Some(pattern) = decode_concat_c_strings(&args[1]) else {
            continue;
        };

        map.insert(
            name,
            BuiltinPattern {
                pattern,
                ignore_case: is_ipattern,
            },
        );
    }
    map
}

fn find_next_macro(offset: usize) -> Option<(bool, usize, usize)> {
    let src = USERDIFF_SOURCE.as_bytes();
    let mut i = offset;
    while i < src.len() {
        if src[i].is_ascii_alphabetic() || src[i] == b'_' {
            let start = i;
            i += 1;
            while i < src.len() && (src[i].is_ascii_alphanumeric() || src[i] == b'_') {
                i += 1;
            }
            let ident = &USERDIFF_SOURCE[start..i];
            if (ident == "PATTERNS" || ident == "IPATTERN")
                && i < src.len()
                && src[i] == b'('
                && start > 0
                && !src[start - 1].is_ascii_alphanumeric()
                && src[start - 1] != b'_'
            {
                return Some((ident == "IPATTERN", start, i + 1));
            }
        } else {
            i += 1;
        }
    }
    None
}

fn parse_macro_args(mut i: usize) -> Option<(Vec<String>, usize)> {
    let src = USERDIFF_SOURCE.as_bytes();
    let mut depth = 1usize;
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < src.len() {
        let ch = src[i] as char;
        let next = src.get(i + 1).copied().map(char::from);

        if in_string {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if in_line_comment {
            current.push(ch);
            if ch == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            current.push(ch);
            if ch == '*' && next == Some('/') {
                current.push('/');
                i += 2;
                in_block_comment = false;
            } else {
                i += 1;
            }
            continue;
        }

        if ch == '/' && next == Some('/') {
            current.push('/');
            current.push('/');
            i += 2;
            in_line_comment = true;
            continue;
        }
        if ch == '/' && next == Some('*') {
            current.push('/');
            current.push('*');
            i += 2;
            in_block_comment = true;
            continue;
        }
        if ch == '"' {
            in_string = true;
            current.push(ch);
            i += 1;
            continue;
        }
        if ch == '(' {
            depth += 1;
            current.push(ch);
            i += 1;
            continue;
        }
        if ch == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                args.push(current.trim().to_owned());
                return Some((args, i + 1));
            }
            current.push(ch);
            i += 1;
            continue;
        }
        if ch == ',' && depth == 1 {
            args.push(current.trim().to_owned());
            current.clear();
            i += 1;
            continue;
        }

        current.push(ch);
        i += 1;
    }

    None
}

fn decode_concat_c_strings(expr: &str) -> Option<String> {
    let bytes = expr.as_bytes();
    let mut i = 0usize;
    let mut out = String::new();
    let mut found = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        if in_line_comment {
            if bytes[i] == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }

        found = true;
        i += 1;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if ch == '"' {
                i += 1;
                break;
            }
            if ch != '\\' {
                out.push(ch);
                i += 1;
                continue;
            }
            i += 1;
            if i >= bytes.len() {
                break;
            }
            let esc = bytes[i] as char;
            match esc {
                'a' => out.push('\u{0007}'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'v' => out.push('\u{000b}'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '?' => out.push('?'),
                'x' => {
                    i += 1;
                    let mut value: u32 = 0;
                    let mut consumed = 0usize;
                    while i < bytes.len() {
                        let ch = bytes[i] as char;
                        let Some(d) = ch.to_digit(16) else {
                            break;
                        };
                        value = value.saturating_mul(16).saturating_add(d);
                        consumed += 1;
                        i += 1;
                    }
                    if consumed > 0 {
                        if let Some(decoded) = char::from_u32(value) {
                            out.push(decoded);
                        }
                        i = i.saturating_sub(1);
                    } else {
                        out.push('x');
                    }
                }
                '0'..='7' => {
                    let mut value = esc.to_digit(8).unwrap_or_default();
                    let mut consumed = 0usize;
                    while consumed < 2 && i + 1 < bytes.len() {
                        let ch = bytes[i + 1] as char;
                        let Some(d) = ch.to_digit(8) else {
                            break;
                        };
                        value = value.saturating_mul(8).saturating_add(d);
                        consumed += 1;
                        i += 1;
                    }
                    if let Some(decoded) = char::from_u32(value) {
                        out.push(decoded);
                    }
                }
                other => out.push(other),
            }
            i += 1;
        }
    }

    if found {
        Some(out)
    } else {
        None
    }
}

fn bre_to_ere(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0usize;
    let mut in_bracket = false;

    while i < chars.len() {
        if in_bracket {
            if chars[i] == ']' && i > 0 {
                result.push(']');
                in_bracket = false;
                i += 1;
            } else if chars[i] == '[' {
                result.push('[');
                i += 1;
            } else if chars[i] == '\\' {
                // Preserve literal backslashes inside character classes.
                // Rust `regex` understands POSIX classes like `[:alnum:]`,
                // so we only need to escape unknown escapes.
                if i + 1 < chars.len() {
                    let next = chars[i + 1];
                    if next.is_ascii_alphabetic() {
                        result.push('\\');
                        result.push('\\');
                        result.push(next);
                        i += 2;
                    } else {
                        result.push('\\');
                        result.push(next);
                        i += 2;
                    }
                } else {
                    result.push('\\');
                    i += 1;
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '[' {
            result.push('[');
            in_bracket = true;
            i += 1;
            if i < chars.len() && (chars[i] == '^' || chars[i] == '!') {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                result.push(']');
                i += 1;
            }
        } else if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                '+' | '?' | '{' | '}' | '(' | ')' | '|' => {
                    result.push(chars[i + 1]);
                    i += 2;
                }
                _ => {
                    result.push(chars[i]);
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else if matches!(chars[i], '+' | '?' | '{' | '}' | '(' | ')' | '|') {
            result.push('\\');
            result.push(chars[i]);
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

fn fix_charclass_escapes(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0usize;
    let mut in_bracket = false;

    while i < chars.len() {
        if in_bracket {
            if chars[i] == ']' {
                result.push(']');
                in_bracket = false;
                i += 1;
            } else if chars[i] == '[' {
                result.push('[');
                i += 1;
            } else if chars[i] == '\\' && i + 1 < chars.len() {
                let next = chars[i + 1];
                if next.is_ascii_alphabetic() {
                    result.push('\\');
                    result.push('\\');
                    result.push(next);
                } else {
                    result.push('\\');
                    result.push(next);
                }
                i += 2;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '[' {
            result.push('[');
            in_bracket = true;
            i += 1;
            if i < chars.len() && (chars[i] == '^' || chars[i] == '!') {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                result.push(']');
                i += 1;
            }
        } else if chars[i] == '\\' && i + 1 < chars.len() {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

fn validate_posix_regex_via_grep(pattern: &str, ignore_case: bool) -> std::io::Result<()> {
    let mut cmd = Command::new("grep");
    cmd.arg("-E").arg("-q");
    if ignore_case {
        cmd.arg("-i");
    }
    cmd.arg("--").arg(pattern).arg("/dev/null");
    let status = cmd.status()?;
    if status.success() || status.code() == Some(1) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid regex",
        ))
    }
}

fn posix_line_matches(pattern: &str, ignore_case: bool, line: &str) -> bool {
    let mut cmd = Command::new("grep");
    cmd.arg("-E").arg("-q");
    if ignore_case {
        cmd.arg("-i");
    }
    cmd.arg("--").arg(pattern);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(line.as_bytes());
        let _ = stdin.write_all(b"\n");
    }

    child.wait().map(|status| status.success()).unwrap_or(false)
}
