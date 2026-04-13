//! Expand tab characters in commit log output to spaces, matching Git's `strbuf_add_tabexpand`.
//!
//! Git aligns tabs to multiples of `tab_width` using the display width of the preceding UTF-8
//! text (see `pretty.c`). If width cannot be determined, remaining tabs are copied literally.

use unicode_width::UnicodeWidthChar;

/// Sum Unicode display widths for `s`, or `None` if any codepoint has ambiguous width.
fn utf8_display_width(s: &str) -> Option<usize> {
    let mut w = 0usize;
    for ch in s.chars() {
        w = w.checked_add(UnicodeWidthChar::width(ch)?)?;
    }
    Some(w)
}

/// Replace tabs in `line` with spaces so each tab advances to the next multiple of `tab_width`.
///
/// `tab_width` must be positive. When expansion is disabled (`effective width` 0), callers should
/// print `line` unchanged instead of calling this function.
///
/// # Parameters
///
/// - `line` — single line without trailing newline.
/// - `tab_width` — tab stop distance (Git `--expand-tabs=N`, `N > 0`).
#[must_use]
pub fn expand_tabs_in_line(line: &str, tab_width: usize) -> String {
    debug_assert!(tab_width > 0);
    if tab_width == 0 {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find('\t') {
        let prefix = &rest[..pos];
        match utf8_display_width(prefix) {
            Some(width) => {
                out.push_str(prefix);
                let col = width % tab_width;
                let spaces = tab_width - col;
                out.extend(std::iter::repeat_n(' ', spaces));
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
        rest = &rest[pos + 1..];
    }
    out.push_str(rest);
    out
}

/// Expand tabs on every line of `msg`, preserving newlines (including a trailing final newline).
#[must_use]
pub fn expand_tabs_in_multiline_message(msg: &str, tab_width: usize) -> String {
    if tab_width == 0 {
        return msg.to_owned();
    }
    let mut out = String::with_capacity(msg.len());
    let mut start = 0usize;
    for (i, c) in msg.char_indices() {
        if c == '\n' {
            out.push_str(&expand_tabs_in_line(&msg[start..i], tab_width));
            out.push('\n');
            start = i + c.len_utf8();
        }
    }
    out.push_str(&expand_tabs_in_line(&msg[start..], tab_width));
    out
}

/// Indent with `indent` ASCII spaces, then optionally expand tabs in `line`.
///
/// Matches Git `pp_handle_indent`: fixed spaces plus tab expansion on the remainder when
/// `tab_width > 0`; otherwise the line is copied verbatim (tabs preserved).
#[must_use]
pub fn indent_and_expand_tabs(line: &str, indent: usize, tab_width: usize) -> String {
    let mut out = String::with_capacity(indent + line.len());
    out.extend(std::iter::repeat_n(' ', indent));
    if tab_width == 0 {
        out.push_str(line);
    } else {
        out.push_str(&expand_tabs_in_line(line, tab_width));
    }
    out
}

/// Default tab-expansion width for a named `--pretty` format (Git `cmt_fmt_map.expand_tabs_in_log`).
///
/// `format` is the resolved pretty name (`None` means Git's default, i.e. `medium`).
/// When `oneline` is true and no explicit format was given, Git uses the oneline defaults.
#[must_use]
pub fn default_expand_tabs_for_pretty_format(format: Option<&str>, oneline: bool) -> usize {
    if oneline && format.is_none() {
        return 0;
    }
    let fmt: &str = match format {
        None => "medium",
        Some(f) => f,
    };
    match fmt {
        "short" | "raw" | "email" | "oneline" | "reference" | "mboxrd" => 0,
        "medium" | "full" | "fuller" => 8,
        f if f.starts_with("format:") || f.starts_with("tformat:") => 8,
        _ => 8,
    }
}

/// Resolve effective tab width from CLI flags and pretty format (Git `rev_info.expand_tabs_in_log`).
///
/// Precedence: `--no-expand-tabs` forces 0; else `--expand-tabs[=N]` if present (`N` defaults to 8
/// when the flag is given without `=`, via the CLI layer); else format default.
#[must_use]
pub fn resolve_expand_tabs_in_log(
    no_expand_tabs: bool,
    expand_tabs: Option<usize>,
    format: Option<&str>,
    oneline: bool,
) -> usize {
    if no_expand_tabs {
        return 0;
    }
    if let Some(n) = expand_tabs {
        return n;
    }
    default_expand_tabs_for_pretty_format(format, oneline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tabs_ascii_simple() {
        assert_eq!(expand_tabs_in_line("a\tb", 8), "a       b");
        assert_eq!(expand_tabs_in_line("\tfoo", 8), "        foo");
    }

    #[test]
    fn expand_tabs_aligns_to_stop() {
        // 'abcd' has width 4; next stop at 8 => 4 spaces
        assert_eq!(expand_tabs_in_line("abcd\tx", 8), "abcd    x");
    }

    #[test]
    fn expand_tabs_multiline() {
        let s = expand_tabs_in_multiline_message("a\tb\nc\td\n", 8);
        assert_eq!(s, "a       b\nc       d\n");
    }

    #[test]
    fn indent_and_expand_combines() {
        let s = indent_and_expand_tabs("\ttitle", 4, 8);
        assert_eq!(s, format!("{}{}", " ".repeat(12), "title"));
    }

    #[test]
    fn indent_without_expand_preserves_tabs() {
        let s = indent_and_expand_tabs("\tx", 4, 0);
        assert_eq!(s, "    \tx");
    }
}
