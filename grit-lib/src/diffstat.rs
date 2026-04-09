//! Git-compatible `--stat` / diffstat layout (width, name truncation, bar scaling).
//!
//! Matches the width algorithm in Git's `show_stats()` (`diff.c`).

use std::io::{Result as IoResult, Write};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Visible terminal width of `s`, skipping ANSI CSI sequences (like Git `utf8_strnwidth(..., 1)`).
#[must_use]
pub fn display_width_minus_ansi(s: &str) -> usize {
    let mut w = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        w = w.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(0));
    }
    w
}

/// `term_columns()` approximation: `COLUMNS` env, then `stty size`, then 80.
#[must_use]
pub fn terminal_columns() -> usize {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<usize>() {
            if w > 0 {
                return w;
            }
        }
    }
    if let Ok(output) = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::null())
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() == 2 {
            if let Ok(w) = parts[1].parse::<usize>() {
                if w > 0 {
                    return w;
                }
            }
        }
    }
    80
}

/// Default total width for `format-patch` diffstat (`MAIL_DEFAULT_WRAP` in Git).
pub const FORMAT_PATCH_STAT_WIDTH: usize = 72;

#[derive(Debug, Clone)]
pub struct FileStatInput {
    pub path_display: String,
    pub insertions: usize,
    pub deletions: usize,
    pub is_binary: bool,
}

/// Options for laying out diffstat lines (Git `diff_options` stat fields).
#[derive(Debug, Clone)]
pub struct DiffstatOptions<'a> {
    /// Total display width for the stat block (after subtracting `line_prefix` when using terminal width).
    pub total_width: usize,
    /// Prefix printed before each stat line (graph + color); only affects width budget when
    /// `subtract_prefix_from_terminal` is true.
    pub line_prefix: &'a str,
    /// When true, width budget is `terminal_columns() - display_width_minus_ansi(line_prefix)`.
    pub subtract_prefix_from_terminal: bool,
    /// Cap filename area (`diff.statNameWidth` / `--stat-name-width`).
    pub stat_name_width: Option<usize>,
    /// Cap graph (+/-) area (`diff.statGraphWidth` / `--stat-graph-width`).
    pub stat_graph_width: Option<usize>,
    /// Max files to show; extra files omitted with a `...` line.
    pub stat_count: Option<usize>,
    /// ANSI SGR before `+` run (empty = no color).
    pub color_add: &'a str,
    /// ANSI SGR before `-` run (empty = no color).
    pub color_del: &'a str,
    /// ANSI reset after colored bar segments (typically `\x1b[m`).
    pub color_reset: &'a str,
    /// Extra columns allocated to the +/- bar (Git `log --graph --stat` uses one more than plain diffstat).
    pub graph_bar_slack: usize,
    /// When subtracting `line_prefix` from `COLUMNS`, add this many columns back (colored graph `|`).
    pub graph_prefix_budget_slack: usize,
}

fn scale_linear(it: usize, width: usize, max_change: usize) -> usize {
    if it == 0 || max_change == 0 {
        return 0;
    }
    if width <= 1 {
        return if it > 0 { 1 } else { 0 };
    }
    1 + (it * (width - 1) / max_change)
}

fn decimal_width(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        format!("{n}").len()
    }
}

/// Truncate a path to fit `area_width` display columns (Git `show_stats` name scaling).
fn truncate_path_for_name_area(path: &str, area_width: usize) -> (String, usize) {
    let full_w = path.width();
    if full_w <= area_width {
        return (path.to_string(), full_w);
    }
    let mut len = area_width;
    len = len.saturating_sub(3);
    let mut byte_start = 0usize;
    let mut name_w = full_w;
    while name_w > len {
        let ch = path[byte_start..].chars().next().unwrap_or('\u{fffd}');
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        name_w = name_w.saturating_sub(cw);
        byte_start += ch.len_utf8();
    }
    let rest = &path[byte_start..];
    if let Some(slash_idx) = rest.find('/') {
        let after = &rest[slash_idx..];
        let after_w = after.width();
        if after_w <= area_width {
            return (format!("...{}", after), after_w);
        }
    }
    let s = format!("...{}", rest);
    (s.clone(), s.width())
}

/// Write diffstat lines and summary, matching Git's layout.
pub fn write_diffstat_block(
    out: &mut impl Write,
    files: &[FileStatInput],
    opts: &DiffstatOptions<'_>,
) -> IoResult<()> {
    if files.is_empty() {
        return Ok(());
    }

    let limit = opts.stat_count.unwrap_or(files.len()).min(files.len());
    let shown = &files[..limit];

    let mut max_len = 0usize;
    let mut max_change = 0usize;
    let mut number_width = 0usize;
    let mut bin_width = 0usize;

    for f in shown {
        let w = f.path_display.width();
        if max_len < w {
            max_len = w;
        }
        if f.is_binary {
            let w = 14 + decimal_width(f.insertions) + decimal_width(f.deletions);
            if bin_width < w {
                bin_width = w;
            }
            number_width = number_width.max(3);
            continue;
        }
        let ch = f.insertions + f.deletions;
        if max_change < ch {
            max_change = ch;
        }
    }

    let mut width = if opts.subtract_prefix_from_terminal {
        terminal_columns()
            .saturating_sub(display_width_minus_ansi(opts.line_prefix))
            .saturating_add(opts.graph_prefix_budget_slack)
    } else {
        opts.total_width
    };

    number_width = number_width.max(decimal_width(max_change));

    if width < 16 + 6 + number_width {
        width = 16 + 6 + number_width;
    }

    let mut graph_width = if max_change + 4 > bin_width {
        max_change
    } else {
        bin_width.saturating_sub(4)
    };
    if let Some(cap) = opts.stat_graph_width {
        if cap > 0 && cap < graph_width {
            graph_width = cap;
        }
    }

    let mut name_width = match opts.stat_name_width {
        Some(nw) if nw > 0 && nw < max_len => nw,
        _ => max_len,
    };

    if name_width + number_width + 6 + graph_width > width {
        let mut gw = graph_width;
        let target_gw = width * 3 / 8;
        if gw > target_gw.saturating_sub(number_width).saturating_sub(6) {
            gw = target_gw.saturating_sub(number_width).saturating_sub(6);
            if gw < 6 {
                gw = 6;
            }
        }
        graph_width = gw;
        if let Some(cap) = opts.stat_graph_width {
            if graph_width > cap {
                graph_width = cap;
            }
        }
        if name_width
            > width
                .saturating_sub(number_width)
                .saturating_sub(6)
                .saturating_sub(graph_width)
        {
            name_width = width
                .saturating_sub(number_width)
                .saturating_sub(6)
                .saturating_sub(graph_width);
        } else {
            graph_width = width
                .saturating_sub(number_width)
                .saturating_sub(6)
                .saturating_sub(name_width);
        }
    }

    graph_width = graph_width.saturating_add(opts.graph_bar_slack);

    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for f in shown {
        let prefix = opts.line_prefix;
        if f.is_binary {
            let (display_name, _) = truncate_path_for_name_area(&f.path_display, name_width);
            if prefix.is_empty() {
                writeln!(
                    out,
                    " {:<nw_name$} | {:>nw$} {} -> {} bytes",
                    display_name,
                    "Bin",
                    f.deletions,
                    f.insertions,
                    nw_name = name_width,
                    nw = number_width
                )?;
            } else {
                writeln!(
                    out,
                    "{prefix}{:<nw_name$} | {:>nw$} {} -> {} bytes",
                    display_name,
                    "Bin",
                    f.deletions,
                    f.insertions,
                    nw_name = name_width,
                    nw = number_width
                )?;
            }
            continue;
        }

        let added = f.insertions;
        let deleted = f.deletions;
        let (display_name, _) = truncate_path_for_name_area(&f.path_display, name_width);

        let mut add = added;
        let mut del = deleted;
        if graph_width <= max_change && max_change > 0 {
            let total_scaled = scale_linear(added + del, graph_width, max_change);
            let mut total = total_scaled;
            if total < 2 && add > 0 && del > 0 {
                total = 2;
            }
            if add < del {
                add = scale_linear(add, graph_width, max_change);
                del = total.saturating_sub(add);
            } else {
                del = scale_linear(del, graph_width, max_change);
                add = total.saturating_sub(del);
            }
        }

        total_ins = total_ins.saturating_add(added);
        total_del = total_del.saturating_add(deleted);

        let total = added + del;
        if prefix.is_empty() {
            write!(
                out,
                " {:<nw_name$} | {:>nw$}",
                display_name,
                total,
                nw_name = name_width,
                nw = number_width
            )?;
        } else {
            write!(
                out,
                "{prefix}{:<nw_name$} | {:>nw$}",
                display_name,
                total,
                nw_name = name_width,
                nw = number_width
            )?;
        }
        if total > 0 {
            write!(out, " ")?;
        }
        if add > 0 {
            if !opts.color_add.is_empty() {
                write!(out, "{}", opts.color_add)?;
            }
            write!(out, "{}", "+".repeat(add))?;
            if !opts.color_add.is_empty() && !opts.color_reset.is_empty() {
                write!(out, "{}", opts.color_reset)?;
            }
        }
        if del > 0 {
            if !opts.color_del.is_empty() {
                write!(out, "{}", opts.color_del)?;
            }
            write!(out, "{}", "-".repeat(del))?;
            if !opts.color_del.is_empty() && !opts.color_reset.is_empty() {
                write!(out, "{}", opts.color_reset)?;
            }
        }
        writeln!(out)?;
    }

    if files.len() > limit {
        if opts.line_prefix.is_empty() {
            writeln!(out, " ...")?;
        } else {
            writeln!(out, "{}...", opts.line_prefix)?;
        }
    }

    let files_changed = files.len();
    let mut summary = if opts.line_prefix.is_empty() {
        format!(
            " {} file{} changed",
            files_changed,
            if files_changed == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{}{} file{} changed",
            opts.line_prefix,
            files_changed,
            if files_changed == 1 { "" } else { "s" }
        )
    };
    if total_ins > 0 {
        summary.push_str(&format!(
            ", {} insertion{}(+)",
            total_ins,
            if total_ins == 1 { "" } else { "s" }
        ));
    }
    if total_del > 0 {
        summary.push_str(&format!(
            ", {} deletion{}(-)",
            total_del,
            if total_del == 1 { "" } else { "s" }
        ));
    }
    if total_ins == 0 && total_del == 0 {
        summary.push_str(", 0 insertions(+), 0 deletions(-)");
    }
    writeln!(out, "{summary}")?;

    Ok(())
}
