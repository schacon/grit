//! `grit` — Git plumbing reimplementation in Rust.
//!
//! This binary is a thin CLI shim: it parses the command line, resolves
//! global options, and delegates to the appropriate command handler in
//! the `commands` module.  All Git-compatible logic lives in `grit-lib`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

/// Gust: a Git plumbing reimplementation.
#[derive(Debug, Parser)]
#[command(
    name = "grit",
    version,
    about = "Git plumbing reimplementation in Rust"
)]
struct Cli {
    /// Override the path to the git directory.
    #[arg(long = "git-dir", env = "GIT_DIR")]
    git_dir: Option<PathBuf>,

    /// Run as if started in this directory (Git's `-C`).
    /// Named `change_dir` to avoid clap field-name collision with `grit init [DIRECTORY]`.
    #[arg(short = 'C', global = true, value_name = "PATH")]
    change_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add file contents to the index.
    Add(commands::add::Args),
    /// List, create, or delete branches.
    Branch(commands::branch::Args),
    /// Find commits not yet applied upstream.
    Cherry(commands::cherry::Args),
    /// Apply the changes introduced by existing commits.
    #[command(name = "cherry-pick")]
    CherryPick(commands::cherry_pick::Args),
    /// Verify that a ref name is valid.
    #[command(name = "check-ref-format")]
    CheckRefFormat(commands::check_ref_format::Args),
    /// Switch branches or restore working tree files.
    Checkout(commands::checkout::Args),
    /// Create an empty Git repository or reinitialize an existing one.
    Init(commands::init::Args),
    /// Compute object ID and optionally create an object from a file.
    #[command(name = "hash-object")]
    HashObject(commands::hash_object::Args),
    /// Provide contents or details of repository objects.
    #[command(name = "cat-file")]
    CatFile(commands::cat_file::Args),
    /// Record changes to the repository.
    Commit(commands::commit::Args),
    /// Get and set repository or global options.
    Config(commands::config::Args),
    /// Show commit logs.
    Log(commands::log::Args),
    /// Register file contents in the working tree to the index.
    #[command(name = "update-index")]
    UpdateIndex(commands::update_index::Args),
    /// Show information about files in the index and working tree.
    #[command(name = "ls-files")]
    LsFiles(commands::ls_files::Args),
    /// Create a tree object from the current index.
    #[command(name = "write-tree")]
    WriteTree(commands::write_tree::Args),
    /// List references in a remote (or local) repository.
    #[command(name = "ls-remote")]
    LsRemote(commands::ls_remote::Args),
    /// List the contents of a tree object.
    #[command(name = "ls-tree")]
    LsTree(commands::ls_tree::Args),
    /// Show the working tree status.
    Status(commands::status::Args),
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
    /// Debug gitignore and exclude rules.
    #[command(name = "check-ignore")]
    CheckIgnore(commands::check_ignore::Args),
    /// Count unpacked objects and disk usage.
    #[command(name = "count-objects")]
    CountObjects(commands::count_objects::Args),
    /// Show changes between commits, commit and working tree, etc.
    Diff(commands::diff::Args),
    /// Compare working tree files against the index.
    #[command(name = "diff-files")]
    DiffFiles(commands::diff_files::Args),
    /// Compare the content and mode of blobs found via two tree objects.
    #[command(name = "diff-tree")]
    DiffTree(commands::diff_tree::Args),
    /// Compare a tree against working tree or index.
    #[command(name = "diff-index")]
    DiffIndex(commands::diff_index::Args),
    /// Output information on refs.
    #[command(name = "for-each-ref")]
    ForEachRef(commands::for_each_ref::Args),
    /// Find best common ancestors.
    #[command(name = "merge-base")]
    MergeBase(commands::merge_base::Args),
    /// Name commits relative to refs.
    #[command(name = "name-rev")]
    NameRev(commands::name_rev::Args),
    /// Run a three-way file merge.
    #[command(name = "merge-file")]
    MergeFile(commands::merge_file::Args),
    /// List commit objects in reverse chronological order.
    #[command(name = "rev-list")]
    RevList(commands::rev_list::Args),
    /// Pick out and massage revision parameters.
    #[command(name = "rev-parse")]
    RevParse(commands::rev_parse::Args),
    /// Show packed archive index.
    #[command(name = "show-index")]
    ShowIndex(commands::show_index::Args),
    /// List references in a local repository.
    #[command(name = "show-ref")]
    ShowRef(commands::show_ref::Args),
    /// Read, modify, and delete symbolic refs.
    #[command(name = "symbolic-ref")]
    SymbolicRef(commands::symbolic_ref::Args),
    /// Validate packed Git archive files.
    #[command(name = "verify-pack")]
    VerifyPack(commands::verify_pack::Args),
    /// Produce a merge commit message.
    #[command(name = "fmt-merge-msg")]
    FmtMergeMsg(commands::fmt_merge_msg::Args),
    /// Cleanup unnecessary files and optimize the repository.
    Gc(commands::gc::Args),
    /// Pack unpacked objects in a repository.
    Repack(commands::repack::Args),
    /// Read a tag object from stdin, validate strictly, and write to ODB.
    #[command(name = "mktag")]
    Mktag(commands::mktag::Args),
    /// Remove loose objects that are already stored in pack files.
    #[command(name = "prune-packed")]
    PrunePacked(commands::prune_packed::Args),
    /// Create, list, delete or verify a tag object.
    Tag(commands::tag::Args),
    /// Restore working tree files.
    Restore(commands::restore::Args),
    /// Show various types of objects (commits, trees, blobs, tags).
    Show(commands::show::Args),
    /// Remove files from the working tree and from the index.
    Rm(commands::rm::Args),
    /// Move or rename a file, a directory, or a symlink.
    Mv(commands::mv::Args),
    /// Build a tree object from ls-tree formatted text.
    #[command(name = "mktree")]
    MkTree(commands::mktree::Args),
    /// Show a Git logical variable.
    Var(commands::var::Args),
    /// Reset current HEAD to the specified state.
    Reset(commands::reset::Args),
    /// Remove unnecessary whitespace.
    Stripspace(commands::stripspace::Args),
    /// Switch branches.
    Switch(commands::switch::Args),
    /// Write a blob object to a temporary file and print its path.
    #[command(name = "unpack-file")]
    UnpackFile(commands::unpack_file::Args),
    /// Unpack objects from a pack stream into the object database.
    #[command(name = "unpack-objects")]
    UnpackObjects(commands::unpack_objects::Args),
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
        Command::Add(args) => commands::add::run(args),
        Command::Branch(args) => commands::branch::run(args),
        Command::Cherry(args) => commands::cherry::run(args),
        Command::CherryPick(args) => commands::cherry_pick::run(args),
        Command::CheckRefFormat(args) => commands::check_ref_format::run(args),
        Command::Checkout(args) => commands::checkout::run(args),
        Command::Init(args) => commands::init::run(args),
        Command::HashObject(args) => commands::hash_object::run(args),
        Command::CatFile(args) => commands::cat_file::run(args),
        Command::Commit(args) => commands::commit::run(args),
        Command::Config(args) => commands::config::run(args),
        Command::Log(args) => commands::log::run(args),
        Command::UpdateIndex(args) => commands::update_index::run(args),
        Command::LsFiles(args) => commands::ls_files::run(args),
        Command::LsRemote(args) => commands::ls_remote::run(args),
        Command::WriteTree(args) => commands::write_tree::run(args),
        Command::LsTree(args) => commands::ls_tree::run(args),
        Command::Status(args) => commands::status::run(args),
        Command::ReadTree(args) => commands::read_tree::run(args),
        Command::CheckoutIndex(args) => commands::checkout_index::run(args),
        Command::CommitTree(args) => commands::commit_tree::run(args),
        Command::UpdateRef(args) => commands::update_ref::run(args),
        Command::CheckIgnore(args) => commands::check_ignore::run(args),
        Command::CountObjects(args) => commands::count_objects::run(args),
        Command::Diff(args) => commands::diff::run(args),
        Command::DiffFiles(args) => commands::diff_files::run(args),
        Command::DiffTree(args) => commands::diff_tree::run(args),
        Command::DiffIndex(args) => commands::diff_index::run(args),
        Command::ForEachRef(args) => commands::for_each_ref::run(args),
        Command::MergeBase(args) => commands::merge_base::run(args),
        Command::NameRev(args) => commands::name_rev::run(args),
        Command::MergeFile(args) => commands::merge_file::run(args),
        Command::RevList(args) => commands::rev_list::run(args),
        Command::RevParse(args) => commands::rev_parse::run(args),
        Command::ShowIndex(args) => commands::show_index::run(args),
        Command::ShowRef(args) => commands::show_ref::run(args),
        Command::SymbolicRef(args) => commands::symbolic_ref::run(args),
        Command::VerifyPack(args) => commands::verify_pack::run(args),
        Command::FmtMergeMsg(args) => commands::fmt_merge_msg::run(args),
        Command::Gc(args) => commands::gc::run(args),
        Command::Repack(args) => commands::repack::run(args),
        Command::Mktag(args) => commands::mktag::run(args),
        Command::PrunePacked(args) => commands::prune_packed::run(args),
        Command::Tag(args) => commands::tag::run(args),
        Command::Restore(args) => commands::restore::run(args),
        Command::Show(args) => commands::show::run(args),
        Command::Rm(args) => commands::rm::run(args),
        Command::Mv(args) => commands::mv::run(args),
        Command::MkTree(args) => commands::mktree::run(args),
        Command::Var(args) => commands::var::run(args),
        Command::Reset(args) => commands::reset::run(args),
        Command::Stripspace(args) => commands::stripspace::run(args),
        Command::Switch(args) => commands::switch::run(args),
        Command::UnpackFile(args) => commands::unpack_file::run(args),
        Command::UnpackObjects(args) => commands::unpack_objects::run(args),
    }
}
