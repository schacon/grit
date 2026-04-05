//! `grit stripspace` — remove unnecessary whitespace.
//!
//! Reads text from stdin, applies whitespace stripping or comment-line
//! prefixing, and writes the result to stdout.  No git repository is
//! required for the default mode.

use anyhow::Result;
use clap::Args as ClapArgs;
use grit_lib::stripspace::{self, Mode};
use std::io::{self, Read, Write};

/// Arguments for `grit stripspace`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Remove unnecessary whitespace",
    override_usage = "grit stripspace [-s | --strip-comments]\n       grit stripspace [-c | --comment-lines]"
)]
pub struct Args {
    /// Skip and remove all lines starting with the comment character.
    #[arg(short = 's', long = "strip-comments", conflicts_with = "comment_lines")]
    pub strip_comments: bool,

    /// Prepend the comment character and a blank space to each line.
    #[arg(short = 'c', long = "comment-lines", conflicts_with = "strip_comments")]
    pub comment_lines: bool,
}

/// Run the `stripspace` command.
///
/// Reads all of stdin, applies the requested transformation, and writes the
/// result to stdout.
///
/// # Errors
///
/// Returns an error if reading stdin or writing stdout fails.
pub fn run(args: Args) -> Result<()> {
    let comment_char = resolve_comment_char()?;

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let mode = if args.strip_comments {
        Mode::StripComments(comment_char)
    } else if args.comment_lines {
        Mode::CommentLines(comment_char)
    } else {
        Mode::Default
    };

    let output = stripspace::process(&input, &mode);
    io::stdout().lock().write_all(&output)?;

    Ok(())
}

/// Resolve the comment character from the git config, defaulting to `"#"`.
///
/// Tries to discover a repository and read `core.commentchar`.  Falls back
/// to `"#"` when outside a repository or when the key is unset.
fn resolve_comment_char() -> Result<String> {
    let git_dir = grit_lib::repo::Repository::discover(None)
        .ok()
        .map(|r| r.git_dir);

    if let Some(ref dir) = git_dir {
        if let Ok(config) = grit_lib::config::ConfigSet::load(Some(dir.as_path()), false) {
            if let Some(val) = config.get("core.commentchar") {
                return validate_comment_char(&val);
            }
        }
    }

    Ok("#".to_owned())
}

fn validate_comment_char(raw: &str) -> Result<String> {
    if raw.is_empty() {
        anyhow::bail!("core.commentchar must have at least one character");
    }
    if raw.contains('\n') || raw.contains('\r') {
        anyhow::bail!("core.commentchar cannot contain newline");
    }
    Ok(raw.to_owned())
}
