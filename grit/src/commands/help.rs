//! `grit help` — display help information.
//!
//! Lists available commands or shows help for a specific command.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Write};

/// Arguments for `grit help`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display help information")]
pub struct Args {
    /// List all available commands.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Command to show help for.
    pub command: Option<String>,
}

/// All commands known to grit, in alphabetical order.
const ALL_COMMANDS: &[&str] = &[
    "add",
    "annotate",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "commit-tree",
    "config",
    "count-objects",
    "describe",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "fetch",
    "fmt-merge-msg",
    "for-each-ref",
    "format-patch",
    "fsck",
    "gc",
    "grep",
    "hash-object",
    "help",
    "init",
    "interpret-trailers",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "merge",
    "merge-base",
    "merge-file",
    "merge-tree",
    "mktag",
    "mktree",
    "mv",
    "name-rev",
    "notes",
    "pack-refs",
    "patch-id",
    "prune",
    "prune-packed",
    "read-tree",
    "rebase",
    "reflog",
    "remote",
    "repack",
    "replace",
    "rerere",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "shortlog",
    "show",
    "show-branch",
    "show-index",
    "show-ref",
    "sparse-checkout",
    "stash",
    "status",
    "stripspace",
    "switch",
    "symbolic-ref",
    "tag",
    "unpack-file",
    "unpack-objects",
    "update-index",
    "update-ref",
    "var",
    "verify-commit",
    "verify-pack",
    "verify-tag",
    "version",
    "whatchanged",
    "worktree",
    "write-tree",
];

/// Run `grit help`.
pub fn run(args: Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.all {
        writeln!(out, "usage: grit <command> [<args>]")?;
        writeln!(out)?;
        writeln!(out, "Available commands:")?;
        for cmd in ALL_COMMANDS {
            writeln!(out, "   {cmd}")?;
        }
        writeln!(out)?;
        writeln!(
            out,
            "See 'grit help <command>' or 'grit <command> --help' for more information."
        )?;
        return Ok(());
    }

    if let Some(cmd) = &args.command {
        // Delegate to `grit <command> --help` by re-exec'ing ourselves.
        let exe = std::env::current_exe().unwrap_or_else(|_| "grit".into());
        let status = std::process::Command::new(&exe)
            .arg(cmd)
            .arg("--help")
            .status();

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => {
                // Command might not exist — show a helpful message
                let code = s.code().unwrap_or(1);
                if code == 2 {
                    writeln!(
                        io::stderr(),
                        "'{cmd}' is not a grit command. See 'grit help -a'."
                    )?;
                }
                std::process::exit(code);
            }
            Err(e) => {
                writeln!(io::stderr(), "error: failed to run help for '{cmd}': {e}")?;
                std::process::exit(1);
            }
        }
    } else {
        // No command specified — show general usage
        let exe = std::env::current_exe().unwrap_or_else(|_| "grit".into());
        let status = std::process::Command::new(&exe).arg("--help").status();
        match status {
            Ok(_) => Ok(()),
            Err(e) => {
                writeln!(io::stderr(), "error: {e}")?;
                std::process::exit(1);
            }
        }
    }
}
