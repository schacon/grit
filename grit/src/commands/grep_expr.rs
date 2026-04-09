//! Boolean pattern expressions for `git grep` (`--and`, `--or`, `--not`, `(`, `)`).

use super::grep_pattern::PatternToken;
use anyhow::{bail, Result};
use regex::Regex;

/// Compiled boolean expression over pattern atoms (indices into `atoms`).
#[derive(Debug, Clone)]
pub(crate) enum GrepExpr {
    True,
    Atom(usize),
    Not(Box<GrepExpr>),
    And(Box<GrepExpr>, Box<GrepExpr>),
    Or(Box<GrepExpr>, Box<GrepExpr>),
}

pub(crate) struct CompiledGrep {
    pub atoms: Vec<Option<Regex>>,
    pub expr: GrepExpr,
}

/// Collect every atom index referenced by `expr` (for `grep_next_match`-style scans).
pub(crate) fn collect_atom_indices(expr: &GrepExpr, out: &mut Vec<usize>) {
    match expr {
        GrepExpr::True => {}
        GrepExpr::Atom(i) => out.push(*i),
        GrepExpr::Not(e) => collect_atom_indices(e, out),
        GrepExpr::And(a, b) | GrepExpr::Or(a, b) => {
            collect_atom_indices(a, out);
            collect_atom_indices(b, out);
        }
    }
}

/// Earliest match start among atoms (for column when not using expression tree).
pub(crate) fn earliest_atom_match(
    line: &str,
    atoms: &[Option<Regex>],
    indices: &[usize],
) -> Option<usize> {
    let mut best: Option<usize> = None;
    for &i in indices {
        if let Some(re) = atoms.get(i).and_then(|x| x.as_ref()) {
            if let Some(m) = re.find(line) {
                let s = m.start();
                best = Some(best.map_or(s, |e: usize| e.min(s)));
            }
        }
    }
    best
}

fn match_atom(re: &Regex, line: &str) -> bool {
    re.is_match(line)
}

/// Git-compatible boolean match with `col` / `icol` tracking for `--column` + `--invert-match`.
pub(crate) fn match_expr_eval(
    expr: &GrepExpr,
    line: &str,
    atoms: &[Option<Regex>],
    col: &mut isize,
    icol: &mut isize,
    column_mode: bool,
) -> bool {
    match expr {
        GrepExpr::True => true,
        GrepExpr::Atom(i) => {
            let Some(re) = atoms.get(*i).and_then(|x| x.as_ref()) else {
                return false;
            };
            let h = match_atom(re, line);
            if h {
                if let Some(m) = re.find(line) {
                    let p = m.start() as isize;
                    if *col < 0 || p < *col {
                        *col = p;
                    }
                }
            }
            h
        }
        GrepExpr::Not(inner) => !match_expr_eval(inner, line, atoms, icol, col, column_mode),
        GrepExpr::And(left, right) => {
            let mut h = match_expr_eval(left, line, atoms, col, icol, column_mode);
            if h || column_mode {
                h &= match_expr_eval(right, line, atoms, col, icol, column_mode);
            }
            h
        }
        GrepExpr::Or(left, right) => {
            if !column_mode {
                return match_expr_eval(left, line, atoms, col, icol, column_mode)
                    || match_expr_eval(right, line, atoms, col, icol, column_mode);
            }
            let h1 = match_expr_eval(left, line, atoms, col, icol, column_mode);
            let h2 = match_expr_eval(right, line, atoms, col, icol, column_mode);
            h1 || h2
        }
    }
}

/// Line matches the full expression (post-`invert_match` is applied in the caller).
pub(crate) fn line_matches_expr(
    expr: &GrepExpr,
    line: &str,
    atoms: &[Option<Regex>],
    column_mode: bool,
) -> bool {
    let mut col: isize = -1;
    let mut icol: isize = -1;
    match_expr_eval(expr, line, atoms, &mut col, &mut icol, column_mode)
}

fn tokens_use_expression_syntax(tokens: &[PatternToken]) -> bool {
    tokens.iter().any(|t| {
        matches!(
            t,
            PatternToken::Not | PatternToken::And | PatternToken::Open | PatternToken::Close
        )
    })
}

fn compile_atom_expr(
    tokens: &[PatternToken],
    i: &mut usize,
    atoms: &mut Vec<String>,
) -> Result<GrepExpr> {
    match tokens.get(*i) {
        Some(PatternToken::Atom(s)) => {
            let idx = atoms.len();
            atoms.push(s.clone());
            *i += 1;
            Ok(GrepExpr::Atom(idx))
        }
        Some(PatternToken::Open) => {
            *i += 1;
            let inner = compile_or_expr(tokens, i, atoms)?;
            match tokens.get(*i) {
                Some(PatternToken::Close) => {
                    *i += 1;
                    Ok(inner)
                }
                _ => bail!("unmatched ( for expression group"),
            }
        }
        Some(PatternToken::Close) | None => Ok(GrepExpr::True),
        Some(_) => bail!("not a pattern expression"),
    }
}

fn compile_not_expr(
    tokens: &[PatternToken],
    i: &mut usize,
    atoms: &mut Vec<String>,
) -> Result<GrepExpr> {
    if matches!(tokens.get(*i), Some(PatternToken::Not)) {
        *i += 1;
        let inner = compile_not_expr(tokens, i, atoms)?;
        if matches!(inner, GrepExpr::True) {
            bail!("--not followed by non pattern expression");
        }
        return Ok(GrepExpr::Not(Box::new(inner)));
    }
    compile_atom_expr(tokens, i, atoms)
}

fn compile_and_expr(
    tokens: &[PatternToken],
    i: &mut usize,
    atoms: &mut Vec<String>,
) -> Result<GrepExpr> {
    let mut left = compile_not_expr(tokens, i, atoms)?;
    while matches!(tokens.get(*i), Some(PatternToken::And)) {
        *i += 1;
        if matches!(left, GrepExpr::True) {
            bail!("--and not preceded by pattern expression");
        }
        let right = compile_and_expr(tokens, i, atoms)?;
        if matches!(right, GrepExpr::True) {
            bail!("--and not followed by pattern expression");
        }
        left = GrepExpr::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn compile_or_expr(
    tokens: &[PatternToken],
    i: &mut usize,
    atoms: &mut Vec<String>,
) -> Result<GrepExpr> {
    let mut left = compile_and_expr(tokens, i, atoms)?;
    loop {
        match tokens.get(*i) {
            Some(PatternToken::Close) | None => break,
            _ => {
                let right = compile_or_expr(tokens, i, atoms)?;
                if matches!(right, GrepExpr::True) {
                    bail!("not a pattern expression");
                }
                left = GrepExpr::Or(Box::new(left), Box::new(right));
            }
        }
    }
    Ok(left)
}

/// Parse pattern tokens into an expression and raw atom strings (one per compiled regex slot).
pub(crate) fn parse_pattern_tokens(tokens: &[PatternToken]) -> Result<(GrepExpr, Vec<String>)> {
    if tokens.is_empty() {
        return Ok((GrepExpr::True, Vec::new()));
    }
    let mut atoms = Vec::new();
    let mut i = 0usize;
    let expr = compile_or_expr(tokens, &mut i, &mut atoms)?;
    if i != tokens.len() {
        bail!("incomplete pattern expression group");
    }
    Ok((expr, atoms))
}
