//! Pattern-expression token extraction for `git grep`-compatible argv parsing.
//!
//! Git parses boolean pattern operators (`--not`, `--and`, `--or`, `(`, `)`) and `-e` in the
//! **options** section only. We peel those off before `clap` sees them, while copying through
//! every real grep flag (and its value) unchanged.

use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;

/// A shell token that participates in Git's boolean pattern expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternToken {
    Atom(String),
    Not,
    And,
    Open,
    Close,
}

fn flag_base(flag: &str) -> &str {
    flag.split_once('=').map(|(a, _)| a).unwrap_or(flag)
}

/// True if this grep option consumes a separate argv value (or `=value` on the same token).
fn grep_option_takes_value(flag: &str) -> bool {
    let base = flag_base(flag);
    matches!(
        base,
        "-f" | "--file"
            | "-A"
            | "--after-context"
            | "-B"
            | "--before-context"
            | "-C"
            | "--context"
            | "-m"
            | "--max-count"
            | "--threads"
            | "--color"
            | "--max-depth"
    )
}

/// Copy one grep option (and its `=value` or following argument) to `out`. Returns new index.
fn copy_option(out: &mut Vec<String>, rest: &[String], i: usize) -> usize {
    let flag = rest[i].as_str();
    if grep_option_takes_value(flag) {
        if flag.contains('=') {
            out.push(rest[i].clone());
            return i + 1;
        }
        if i + 1 < rest.len() {
            out.push(rest[i].clone());
            out.push(rest[i + 1].clone());
            return i + 2;
        }
    }
    out.push(rest[i].clone());
    i + 1
}

/// Peel pattern-expression tokens from the options section; pass everything else through.
pub(crate) fn extract_pattern_tokens(rest: &[String]) -> Result<(Vec<PatternToken>, Vec<String>)> {
    let mut tokens = Vec::new();
    let mut out = Vec::with_capacity(rest.len());
    let mut i = 0usize;
    while i < rest.len() {
        let a = rest[i].as_str();
        if a == "--" {
            out.extend(rest[i..].iter().cloned());
            return Ok((tokens, out));
        }
        match a {
            "-e" | "--regexp" => {
                if i + 1 < rest.len() {
                    tokens.push(PatternToken::Atom(rest[i + 1].clone()));
                    i += 2;
                } else {
                    out.push(rest[i].clone());
                    i += 1;
                }
            }
            "-f" | "--file" => {
                if i + 1 < rest.len() {
                    let path = &rest[i + 1];
                    let content = if path == "-" {
                        let mut s = String::new();
                        std::io::stdin()
                            .read_to_string(&mut s)
                            .context("cannot read patterns from stdin")?;
                        s
                    } else {
                        let p = Path::new(path);
                        let resolved = if p.is_absolute() {
                            p.to_path_buf()
                        } else {
                            std::env::current_dir()
                                .unwrap_or_else(|_| Path::new(".").to_path_buf())
                                .join(p)
                        };
                        std::fs::read_to_string(&resolved)
                            .with_context(|| format!("cannot read pattern file: '{path}'"))?
                    };
                    for line in content.lines() {
                        if !line.is_empty() {
                            tokens.push(PatternToken::Atom(line.to_string()));
                        }
                    }
                    i += 2;
                } else {
                    out.push(rest[i].clone());
                    i += 1;
                }
            }
            "--not" => {
                tokens.push(PatternToken::Not);
                i += 1;
            }
            "--and" => {
                tokens.push(PatternToken::And);
                i += 1;
            }
            "--or" => {
                i += 1;
            }
            "(" => {
                tokens.push(PatternToken::Open);
                i += 1;
            }
            ")" => {
                tokens.push(PatternToken::Close);
                i += 1;
            }
            _ if a.starts_with('-') && a != "-" => {
                i = copy_option(&mut out, rest, i);
            }
            _ => {
                out.extend(rest[i..].iter().cloned());
                break;
            }
        }
    }
    Ok((tokens, out))
}
