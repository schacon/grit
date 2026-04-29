//! CLI wrappers for Git-compatible column layout.

use std::io::IsTerminal;

pub use grit_lib::git_column::{
    apply_column_cli_arg, merge_column_config, parse_column_tokens_into, print_columns, ColOpts,
    ColumnOptions,
};

/// Apply `finalize_colopts` semantics, resolving `auto` from stdout when no TTY state is supplied.
pub fn finalize_colopts(colopts: &mut ColOpts, stdout_is_tty: Option<bool>) {
    let is_tty = stdout_is_tty.unwrap_or_else(|| std::io::stdout().is_terminal());
    grit_lib::git_column::finalize_colopts(colopts, is_tty);
}

/// Width for layout: `$COLUMNS`, then `ioctl`, else 80, then minus one.
#[must_use]
pub fn term_columns_minus_one() -> usize {
    let mut n = 80usize;
    if let Ok(s) = std::env::var("COLUMNS") {
        if let Ok(v) = s.parse::<usize>() {
            if v > 0 {
                n = v;
            }
        }
    } else if let Ok(output) = std::process::Command::new("stty")
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
                    n = w;
                }
            }
        }
    }
    n.saturating_sub(1)
}
