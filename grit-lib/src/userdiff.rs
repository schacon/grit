//! User-defined and built-in diff function-name matching.
//!
//! This module implements the subset of Git's `userdiff` behavior needed for
//! hunk-header function context extraction.

use crate::config::ConfigSet;
use crate::crlf::{get_file_attrs, AttrRule, DiffAttr};
use regex::{Regex, RegexBuilder};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Built-in diff driver funcname patterns (same strings as Git's userdiff builtin drivers).
const BUILTIN_PATTERN_DEFS: &[(&str, &str, bool)] = &[
    (
        "ada",
        r"!^(.*[ 	])?(is[ 	]+new|renames|is[ 	]+separate)([ 	].*)?$
!^[ 	]*with[ 	].*$
^[ 	]*((procedure|function)[ 	]+.*)$
^[ 	]*((package|protected|task)[ 	]+.*)$",
        true,
    ),
    (
        "bash",
        r"^[ 	]*((([a-zA-Z_][a-zA-Z0-9_]*[ 	]*\([ 	]*\))|(function[ 	]+[a-zA-Z_][a-zA-Z0-9_]*(([ 	]*\([ 	]*\))|([ 	]+)))).*$)",
        false,
    ),
    (
        "bibtex",
        r#"(@[a-zA-Z]{1,}[ 	]*\{{0,1}[ 	]*[^ 	"@',\#}{~%]*).*$"#,
        false,
    ),
    (
        "cpp",
        r"!^[ 	]*[A-Za-z_][A-Za-z_0-9]*:[[:space:]]*($|/[/*])
^((::[[:space:]]*)?[A-Za-z_].*)$",
        false,
    ),
    (
        "csharp",
        r"!(^|[ 	]+)(do|while|for|foreach|if|else|new|default|return|switch|case|throw|catch|using|lock|fixed)([ 	(]+|$)
^[ 	]*(([][[:alnum:]@_.](<[][[:alnum:]@_, 	<>]+>)?)+([ 	]+([][[:alnum:]@_.](<[][[:alnum:]@_, 	<>]+>)?)+)+[ 	]*\([^;]*)$
^[ 	]*(([][[:alnum:]@_.](<[][[:alnum:]@_, 	<>]+>)?)+([ 	]+([][[:alnum:]@_.](<[][[:alnum:]@_, 	<>]+>)?)+)+[^;=:,()]*)$
^[ 	]*(((static|public|internal|private|protected|new|unsafe|sealed|abstract|partial)[ 	]+)*(class|enum|interface|struct|record)[ 	]+.*)$
^[ 	]*(namespace[ 	]+.*)$",
        false,
    ),
    (
        "css",
        r"![:;][[:space:]]*$
^[:[@.#]?[_a-z0-9].*$",
        true,
    ),
    (
        "dts",
        r"!;
!=
^[ 	]*((/[ 	]*\{|&?[a-zA-Z_]).*)",
        false,
    ),
    (
        "elixir",
        r"^[ 	]*((def(macro|module|impl|protocol|p)?|test)[ 	].*)$",
        false,
    ),
    (
        "fortran",
        r#"!^([C*]|[ 	]*!)
!^[ 	]*MODULE[ 	]+PROCEDURE[ 	]
^[ 	]*((END[ 	]+)?(PROGRAM|MODULE|BLOCK[ 	]+DATA|([^!'" 	]+[ 	]+)*(SUBROUTINE|FUNCTION))[ 	]+[A-Z].*)$"#,
        true,
    ),
    (
        "fountain",
        r"^((\.[^.]|(int|ext|est|int\.?/ext|i/e)[. ]).*)$",
        true,
    ),
    (
        "golang",
        r"^[ 	]*(func[ 	]*.*(\{[ 	]*)?)
^[ 	]*(type[ 	].*(struct|interface)[ 	]*(\{[ 	]*)?)",
        false,
    ),
    ("html", r"^[ 	]*(<[Hh][1-6]([ 	].*)?>.*)$", false),
    ("ini", r"^[ 	]*\[[^]]+\]", false),
    (
        "java",
        r"!^[ 	]*(catch|do|for|if|instanceof|new|return|switch|throw|while)
^[ 	]*(([a-z-]+[ 	]+)*(class|enum|interface|record)[ 	]+.*)$
^[ 	]*(([A-Za-z_<>&][][?&<>.,A-Za-z_0-9]*[ 	]+)+[A-Za-z_][A-Za-z_0-9]*[ 	]*\([^;]*)$",
        false,
    ),
    (
        "kotlin",
        r"^[ 	]*(([a-z]+[ 	]+)*(fun|class|interface)[ 	]+.*)$",
        false,
    ),
    ("markdown", r"^ {0,3}#{1,6}[ 	].*", false),
    (
        "matlab",
        r"^[[:space:]]*((classdef|function)[[:space:]].*)$|^(%%%?|##)[[:space:]].*$",
        false,
    ),
    (
        "objc",
        r"!^[ 	]*(do|for|if|else|return|switch|while)
^[ 	]*([-+][ 	]*\([ 	]*[A-Za-z_][A-Za-z_0-9* 	]*\)[ 	]*[A-Za-z_].*)$
^[ 	]*(([A-Za-z_][A-Za-z_0-9]*[ 	]+)+[A-Za-z_][A-Za-z_0-9]*[ 	]*\([^;]*)$
^(@(implementation|interface|protocol)[ 	].*)$",
        false,
    ),
    (
        "pascal",
        r"^(((class[ 	]+)?(procedure|function)|constructor|destructor|interface|implementation|initialization|finalization)[ 	]*.*)$
^(.*=[ 	]*(class|record).*)$",
        false,
    ),
    (
        "perl",
        r"^package .*
^sub [[:alnum:]_':]+[ 	]*(\([^)]*\)[ 	]*)?(:[^;#]*)?(\{[ 	]*)?(#.*)?$
^(BEGIN|END|INIT|CHECK|UNITCHECK|AUTOLOAD|DESTROY)[ 	]*(\{[ 	]*)?(#.*)?$
^=head[0-9] .*",
        false,
    ),
    (
        "php",
        r"^[	 ]*(((public|protected|private|static|abstract|final)[	 ]+)*function.*)$
^[	 ]*((((final|abstract)[	 ]+)?class|enum|interface|trait).*)$",
        false,
    ),
    ("python", r"^[ 	]*((class|(async[ 	]+)?def)[ 	].*)$", false),
    (
        "r",
        r"^[ 	]*([a-zA-z][a-zA-Z0-9_.]*[ 	]*(<-|=)[ 	]*function.*)$",
        false,
    ),
    ("ruby", r"^[ 	]*((class|module|def)[ 	].*)$", false),
    (
        "rust",
        r#"^[	 ]*((pub(\([^\)]+\))?[	 ]+)?((async|const|unsafe|extern([	 ]+"[^"]+"))[	 ]+)?(struct|enum|union|mod|trait|fn|impl|macro_rules!)[< 	]+[^;]*)$"#,
        false,
    ),
    (
        "scheme",
        r"^[	 ]*(\(((define|def(struct|syntax|class|method|rules|record|proto|alias)?)[-*/ 	]|(library|module|struct|class)[*+ 	]).*)$",
        false,
    ),
    (
        "tex",
        r"^(\\((sub)*section|chapter|part)\*{0,1}\{.*)$",
        false,
    ),
];

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
    let attrs = get_file_attrs(rules, rel_path, false, config);
    let DiffAttr::Driver(ref driver) = attrs.diff_attr else {
        return Ok(None);
    };
    matcher_for_driver(config, driver)
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
    BUILTIN_PATTERN_DEFS
        .iter()
        .filter(|(name, _, _)| !name.is_empty() && *name != "default")
        .map(|(name, pattern, ignore_case)| {
            (
                (*name).to_owned(),
                BuiltinPattern {
                    pattern: (*pattern).to_owned(),
                    ignore_case: *ignore_case,
                },
            )
        })
        .collect()
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
