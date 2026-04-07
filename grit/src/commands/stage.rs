//! `grit stage` — alias for `grit add`.
//!
//! Delegates entirely to the `add` command.

use anyhow::Result;
use clap::Args as ClapArgs;

use super::add;

/// Arguments for `grit stage` (identical to `grit add`).
#[derive(Debug, ClapArgs)]
#[command(about = "Add file contents to the index (alias for 'add')")]
pub struct Args {
    /// Files to add. Use '.' to add everything.
    #[arg(required_unless_present_any = ["update", "all"])]
    pub pathspec: Vec<String>,

    /// Update tracked files (don't add new files).
    #[arg(short = 'u', long = "update")]
    pub update: bool,

    /// Add, modify, and remove index entries to match the working tree.
    #[arg(short = 'A', long = "all", alias = "no-ignore-removal")]
    pub all: bool,

    /// Record only the intent to add a path (placeholder entry).
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Dry run — show what would be added.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Be verbose.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Allow adding otherwise ignored files.
    #[arg(short = 'f', long = "force")]
    pub force: bool,
}

/// Run the `stage` command by delegating to `add`.
pub fn run(args: Args) -> Result<()> {
    add::run(add::Args {
        pathspec: args.pathspec,
        update: args.update,
        all: args.all,
        no_all: false,
        intent_to_add: args.intent_to_add,
        dry_run: args.dry_run,
        verbose: args.verbose,
        force: args.force,
        patch: false,
        interactive: false,
        edit: false,
        chmod: None,
        renormalize: false,
        refresh: false,
        ignore_errors: false,
        ignore_missing: false,
        no_warn_embedded_repo: false,
        pathspec_from_file: None,
        pathspec_file_nul: false,
    })
}
