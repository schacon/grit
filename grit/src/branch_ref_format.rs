//! `git branch --format` subset for t3203: atoms and simple nested-safe `%(if)...%(then)...%(else)...%(end)`.

use grit_lib::config::ConfigSet;
use grit_lib::merge_base::count_symmetric_ahead_behind;
use grit_lib::objects::ObjectId;
use grit_lib::refs::read_head;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
pub struct BranchFormatContext<'a> {
    pub repo: &'a Repository,
    pub refname_display: &'a str,
    pub oid: ObjectId,
    pub full_refname: Option<&'a str>,
    /// When false, `%(color:…)` atoms expand to empty (non-TTY auto).
    pub emit_format_color: bool,
}

#[derive(Debug)]
pub enum BranchFormatError {
    Fatal(String),
}

pub fn expand_branch_format(
    ctx: &BranchFormatContext<'_>,
    format: &str,
    omit_empty: bool,
) -> Result<String, BranchFormatError> {
    let expanded = expand_all(ctx, format)?;
    Ok(
        if omit_empty && expanded.chars().all(|c| c.is_whitespace()) {
            String::new()
        } else {
            expanded
        },
    )
}

fn expand_all(ctx: &BranchFormatContext<'_>, s: &str) -> Result<String, BranchFormatError> {
    let head_ref = read_head(&ctx.repo.git_dir).ok().flatten();
    let mut out = String::new();
    let mut i = 0usize;
    let b = s.as_bytes();
    while i < s.len() {
        if i + 1 < s.len() && b[i] == b'%' && b[i + 1] == b'%' {
            out.push('%');
            i += 2;
            continue;
        }
        if i + 1 < s.len() && b[i] == b'%' && b[i + 1] == b'(' {
            let (n, piece) = expand_delimited(ctx, &s[i..], &head_ref)?;
            out.push_str(&piece);
            i += n;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or_default();
        out.push(ch);
        i += ch.len_utf8();
    }
    Ok(out)
}

fn expand_delimited(
    ctx: &BranchFormatContext<'_>,
    s: &str,
    head_ref: &Option<String>,
) -> Result<(usize, String), BranchFormatError> {
    if !s.starts_with("%(") {
        return Ok((1, "%".to_owned()));
    }
    let inner = &s[2..];
    let close = find_matching_paren(inner)
        .ok_or_else(|| BranchFormatError::Fatal("unterminated format atom".into()))?;
    let atom = &inner[..close];
    let total_atom = 2 + close + 1;

    if atom == "then" || atom == "else" || atom == "end" {
        return Err(BranchFormatError::Fatal(format!(
            "format: %({atom}) atom used without an %(if) atom"
        )));
    }

    if let Some(rest) = atom.strip_prefix("if") {
        let tail = &s[total_atom..];
        let (body, consumed_tail) = expand_if(ctx, rest, tail)?;
        return Ok((total_atom + consumed_tail, body));
    }

    Ok((total_atom, expand_atom(ctx, atom, head_ref)?))
}

fn expand_if(
    ctx: &BranchFormatContext<'_>,
    after_if_colon: &str,
    tail: &str,
) -> Result<(String, usize), BranchFormatError> {
    let modifier = after_if_colon.strip_prefix(':').unwrap_or("").trim();

    let then_pos = find_at_if_depth(tail, "%(then)").ok_or_else(|| {
        BranchFormatError::Fatal("format: %(if) atom used without a %(then) atom".into())
    })?;
    let cond_fmt = &tail[..then_pos];
    let after_then = &tail[then_pos + "%(then)".len()..];

    let (else_at, end_at) = find_else_and_end(after_then)?;
    let (then_fmt, else_fmt) = match else_at {
        Some(e) => (&after_then[..e], &after_then[e + "%(else)".len()..end_at]),
        None => (&after_then[..end_at], ""),
    };

    let cond_val = expand_all(ctx, cond_fmt)?;
    let take_then = if modifier.is_empty() {
        !cond_val.is_empty()
    } else if let Some(v) = modifier.strip_prefix("equals=") {
        cond_val == v
    } else if let Some(v) = modifier.strip_prefix("notequals=") {
        cond_val != v
    } else {
        return Err(BranchFormatError::Fatal(format!(
            "unrecognized %(if) argument: {modifier}"
        )));
    };

    let body = if take_then {
        expand_all(ctx, then_fmt)?
    } else {
        expand_all(ctx, else_fmt)?
    };

    let consumed = then_pos + "%(then)".len() + end_at + "%(end)".len();
    Ok((body, consumed))
}

fn find_else_and_end(s: &str) -> Result<(Option<usize>, usize), BranchFormatError> {
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut else_at = None::<usize>;
    while i < s.len() {
        if let Some(j) = scan_if_open(s, i) {
            depth += 1;
            i = j;
            continue;
        }
        if depth > 0 && s[i..].starts_with("%(end)") {
            depth -= 1;
            i += "%(end)".len();
            continue;
        }
        if depth == 0 && else_at.is_none() && s[i..].starts_with("%(else)") {
            else_at = Some(i);
            i += "%(else)".len();
            continue;
        }
        if depth == 0 && s[i..].starts_with("%(end)") {
            return Ok((else_at, i));
        }
        i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    Err(BranchFormatError::Fatal(
        "format: %(if) atom used without a %(end) atom".into(),
    ))
}

fn find_at_if_depth(s: &str, pat: &str) -> Option<usize> {
    let mut i = 0usize;
    let mut depth = 0usize;
    while i < s.len() {
        if let Some(j) = scan_if_open(s, i) {
            depth += 1;
            i = j;
            continue;
        }
        if depth > 0 && s[i..].starts_with("%(end)") {
            depth -= 1;
            i += "%(end)".len();
            continue;
        }
        if depth == 0 && s[i..].starts_with(pat) {
            return Some(i);
        }
        i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

fn scan_if_open(s: &str, i: usize) -> Option<usize> {
    if !s[i..].starts_with("%(") {
        return None;
    }
    let inner = &s[i + 2..];
    let close = find_matching_paren(inner)?;
    let atom = &inner[..close];
    if atom.starts_with("if") {
        Some(i + 2 + close + 1)
    } else {
        None
    }
}

fn find_matching_paren(s: &str) -> Option<usize> {
    let mut d = 1usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => d += 1,
            ')' => {
                d -= 1;
                if d == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn expand_atom(
    ctx: &BranchFormatContext<'_>,
    atom: &str,
    head_ref: &Option<String>,
) -> Result<String, BranchFormatError> {
    let (base, modifier) = atom
        .find(':')
        .map(|p| (&atom[..p], Some(&atom[p + 1..])))
        .unwrap_or((atom, None));

    match base {
        "refname" => match modifier {
            Some("short") => Ok(short_ref_display(ctx.refname_display)),
            Some(m) => Err(BranchFormatError::Fatal(format!(
                "unrecognized %(refname) argument: {m}"
            ))),
            None => Ok(ctx.refname_display.to_owned()),
        },
        "HEAD" => {
            let is_head = ctx.full_refname.is_none()
                || head_ref
                    .as_deref()
                    .zip(ctx.full_refname)
                    .is_some_and(|(h, r)| h == r);
            Ok(if is_head {
                "*".to_owned()
            } else {
                " ".to_owned()
            })
        }
        "objectname" => match modifier {
            None => Ok(ctx.oid.to_hex()),
            Some("short") => Ok(ctx.oid.to_hex()[..7].to_owned()),
            Some(m) if m.starts_with("short=") => {
                let n: usize = m["short=".len()..].parse().unwrap_or(7);
                let n = n.max(4).min(40);
                Ok(ctx.oid.to_hex()[..n].to_owned())
            }
            Some(other) => Err(BranchFormatError::Fatal(format!(
                "unrecognized %(objectname) argument: {other}"
            ))),
        },
        "ahead-behind" => {
            let Some(spec) = modifier else {
                return Err(BranchFormatError::Fatal(
                    "expected format: %(ahead-behind:<committish>)".to_owned(),
                ));
            };
            let base = resolve_revision(ctx.repo, spec)
                .map_err(|_| BranchFormatError::Fatal(format!("failed to find '{spec}'")))?;
            let (a, b) = count_symmetric_ahead_behind(ctx.repo, ctx.oid, base)
                .map_err(|e| BranchFormatError::Fatal(e.to_string()))?;
            Ok(format!("{a} {b}"))
        }
        "color" => {
            if !ctx.emit_format_color {
                return Ok(String::new());
            }
            let slot = modifier.unwrap_or("");
            let cfg = ConfigSet::load(Some(&ctx.repo.git_dir), true).ok();
            if matches!(
                slot,
                "reset" | "bold" | "red" | "green" | "yellow" | "blue" | "magenta" | "cyan"
            ) {
                let key = format!("color.{slot}");
                let raw = cfg
                    .as_ref()
                    .and_then(|c| c.get(&key))
                    .unwrap_or_else(|| slot.to_string());
                return Ok(grit_lib::config::parse_color(&raw).unwrap_or_default());
            }
            let key = format!("color.branch.{slot}");
            let default = match slot {
                "current" => "green",
                "local" => "normal",
                "remote" => "red",
                "plain" => "normal",
                "upstream" => "blue",
                "worktree" => "cyan",
                _ => "",
            };
            let raw = cfg
                .as_ref()
                .and_then(|c| c.get(&key))
                .unwrap_or_else(|| default.to_string());
            Ok(grit_lib::config::parse_color(&raw).unwrap_or_default())
        }
        "rest" => Err(BranchFormatError::Fatal("invalid atom: %(rest)".to_owned())),
        _ => Err(BranchFormatError::Fatal(format!(
            "unsupported format atom: {base}"
        ))),
    }
}

fn short_ref_display(full: &str) -> String {
    for prefix in ["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Some(s) = full.strip_prefix(prefix) {
            return s.to_owned();
        }
    }
    full.to_owned()
}
