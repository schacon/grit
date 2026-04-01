//! `gust` — Git plumbing reimplementation in Rust.
//!
//! This binary is a thin CLI shim: it parses the command line, resolves
//! global options, and delegates to the appropriate command handler in
//! the `commands` module.  All Git-compatible logic lives in `gust-lib`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

/// Gust: a Git plumbing reimplementation.
#[derive(Debug, Parser)]
#[command(
    name = "gust",
    version,
    about = "Git plumbing reimplementation in Rust"
)]
struct Cli {
    /// Override the path to the git directory.
    #[arg(long = "git-dir", global = true, env = "GIT_DIR")]
    git_dir: Option<PathBuf>,

    /// Run as if started in this directory (Git's `-C`).
    /// Named `change_dir` to avoid clap field-name collision with `gust init [DIRECTORY]`.
    #[arg(short = 'C', global = true, value_name = "PATH")]
    change_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create an empty Git repository or reinitialize an existing one.
    Init(commands::init::Args),
    /// Compute object ID and optionally create an object from a file.
    #[command(name = "hash-object")]
    HashObject(commands::hash_object::Args),
    /// Provide contents or details of repository objects.
    #[command(name = "cat-file")]
    CatFile(commands::cat_file::Args),
    /// Register file contents in the working tree to the index.
    #[command(name = "update-index")]
    UpdateIndex(commands::update_index::Args),
    /// Show information about files in the index and working tree.
    #[command(name = "ls-files")]
    LsFiles(commands::ls_files::Args),
    /// Create a tree object from the current index.
    #[command(name = "write-tree")]
    WriteTree(commands::write_tree::Args),
    /// List the contents of a tree object.
    #[command(name = "ls-tree")]
    LsTree(commands::ls_tree::Args),
    /// Read tree information into the index.
    #[command(name = "read-tree")]
    ReadTree(commands::read_tree::Args),
    /// Check out files from the index into the working tree.
    #[command(name = "checkout-index")]
    CheckoutIndex(commands::checkout_index::Args),
    /// Create a new commit object.
    #[command(name = "commit-tree")]
    CommitTree(commands::commit_tree::Args),
    /// Update the object name stored in a ref safely.
    #[command(name = "update-ref")]
    UpdateRef(commands::update_ref::Args),
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Handle -C: change working directory before doing anything else.
    if let Some(dir) = &cli.change_dir {
        std::env::set_current_dir(dir)?;
    }

    // Pass git_dir override into env so library discovery picks it up.
    if let Some(git_dir) = &cli.git_dir {
        std::env::set_var("GIT_DIR", git_dir);
    }

    match cli.command {
        Command::Init(args) => commands::init::run(args),
        Command::HashObject(args) => commands::hash_object::run(args),
        Command::CatFile(args) => commands::cat_file::run(args),
        Command::UpdateIndex(args) => commands::update_index::run(args),
        Command::LsFiles(args) => commands::ls_files::run(args),
        Command::WriteTree(args) => commands::write_tree::run(args),
        Command::LsTree(args) => commands::ls_tree::run(args),
        Command::ReadTree(args) => commands::read_tree::run(args),
        Command::CheckoutIndex(args) => commands::checkout_index::run(args),
        Command::CommitTree(args) => commands::commit_tree::run(args),
        Command::UpdateRef(args) => commands::update_ref::run(args),
    }
}
