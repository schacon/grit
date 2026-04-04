//! `grit fast-export` — export repository as a fast-import stream.
//!
//! Produces a stream on stdout suitable for `git fast-import`, walking
//! commits and emitting `commit`, `blob`, and `reset` directives.
//! Supports `--all` to export every ref.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit fast-export`.
#[derive(Debug, ClapArgs)]
#[command(about = "Export repository as fast-import stream")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-export` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    // Normalize newer option names for compatibility with older system Git.
    let normalized: Vec<String> = args.args.iter().map(|a| {
        if a == "--signed-tags=warn-verbatim" {
            // --signed-tags=warn-verbatim → --signed-tags=warn
            "--signed-tags=warn".to_string()
        } else {
            a.clone()
        }
    }).collect();
    git_passthrough::run("fast-export", &normalized)
}
