//! `grit help` — display help information.
//!
//! Lists available commands or shows help for a specific command.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Write};

/// Config variable names for completion (from `git help --config-for-completion`).
const CONFIG_VARS_FOR_COMPLETION: &str = include_str!("config_vars.txt");

/// Config section names for completion (from `git help --config-sections-for-completion`).
const CONFIG_SECTIONS_FOR_COMPLETION: &str = include_str!("config_sections.txt");

/// Full config variable names with placeholders (from `git help --config`).
const CONFIG_VARS_ALL: &str = include_str!("config_vars_all.txt");

/// Arguments for `grit help`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display help information")]
pub struct Args {
    /// List all available commands.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// List config variable names for completion.
    #[arg(long = "config-for-completion", hide = true)]
    pub config_for_completion: bool,

    /// List all config variable names.
    #[arg(long = "config", hide = true)]
    pub config_list: bool,

    /// List config section names for completion.
    #[arg(long = "config-sections-for-completion", hide = true)]
    pub config_sections_for_completion: bool,

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

fn alias_names_for_help() -> Vec<String> {
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });

    let Ok(config) = grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) else {
        return Vec::new();
    };

    let mut names: Vec<String> = Vec::new();
    for entry in config.entries() {
        if !entry.key.starts_with("alias.") {
            continue;
        }
        let rest = &entry.key["alias.".len()..];
        if let Some(name) = rest.strip_suffix(".command") {
            if !name.is_empty() && !name.contains('.') {
                names.push(name.to_owned());
            }
            continue;
        }
        if let Some(name) = rest.strip_prefix('.') {
            if !name.is_empty() && !name.contains('.') {
                names.push(name.to_owned());
            }
            continue;
        }
        if !rest.is_empty() && !rest.contains('.') {
            names.push(rest.to_owned());
        }
    }

    names.sort();
    names.dedup();
    names
}

/// Run `grit help`.
pub fn run(args: Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.config_for_completion {
        print!("{}", CONFIG_VARS_FOR_COMPLETION);
        return Ok(());
    }

    if args.config_list {
        print!("{}", CONFIG_VARS_ALL);
        return Ok(());
    }

    if args.config_sections_for_completion {
        print!("{}", CONFIG_SECTIONS_FOR_COMPLETION);
        return Ok(());
    }

    if args.all {
        writeln!(out, "usage: grit <command> [<args>]")?;
        writeln!(out)?;
        writeln!(out, "Available commands:")?;
        for cmd in ALL_COMMANDS {
            writeln!(out, "   {cmd}")?;
        }
        for alias in alias_names_for_help() {
            writeln!(out, "   {alias}")?;
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
        // No command specified — show general usage directly
        writeln!(out, "usage: grit <command> [<args>]")?;
        writeln!(out)?;
        writeln!(out, "These are common grit commands:")?;
        writeln!(out)?;
        writeln!(out, "Commands:")?;
        for cmd in ALL_COMMANDS {
            writeln!(out, "   {cmd}")?;
        }
        for alias in alias_names_for_help() {
            writeln!(out, "   {alias}")?;
        }
        writeln!(out)?;
        writeln!(
            out,
            "See 'grit help <command>' or 'grit <command> --help' for more information."
        )?;
        Ok(())
    }
}
