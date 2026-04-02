//! `grit sh-setup` — shell setup helpers.
//!
//! Helper for shell scripts that provides common setup functions like
//! `die`, `require_work_tree`, `cd_to_toplevel`, etc.  Our stub prints
//! the shell helper functions that scripts can source.
//!
//!     . "$(grit --exec-path)/grit-sh-setup"

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit sh-setup`.
#[derive(Debug, ClapArgs)]
#[command(about = "Shell script setup helpers (stub)")]
pub struct Args {
    /// Arguments (unused, accepted for compatibility).
    #[arg(value_name = "ARG", num_args = 0.., trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit sh-setup`.
///
/// Outputs minimal shell helper functions that scripts can source.
pub fn run(_args: Args) -> Result<()> {
    // Provide minimal shell helper stubs
    print!(
        r#"# grit-sh-setup — shell helpers (stub)
die () {{
    echo >&2 "$@"
    exit 1
}}

require_work_tree () {{
    test "$(grit rev-parse --is-inside-work-tree 2>/dev/null)" = true ||
        die "fatal: $0 requires a working tree"
}}

cd_to_toplevel () {{
    cd "$(grit rev-parse --show-toplevel)" || die "cannot chdir to toplevel"
}}

require_clean_work_tree () {{
    grit diff --quiet HEAD -- || die "working tree is dirty"
}}
"#
    );
    Ok(())
}
