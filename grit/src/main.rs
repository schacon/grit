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
    /// Show what revision and author last modified each line of a file.
    Annotate(commands::annotate::Args),
    /// Use binary search to find the commit that introduced a bug.
    Bisect(commands::bisect::Args),
    /// Show what revision and author last modified each line of a file.
    Blame(commands::blame::Args),
    /// Apply a patch to files and/or to the index.
    Apply(commands::apply::Args),
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
    /// Remove untracked files from the working tree.
    Clean(commands::clean::Args),
    /// Clone a repository into a new directory.
    Clone(commands::clone::Args),
    /// Switch branches or restore working tree files.
    Checkout(commands::checkout::Args),
    /// Create an empty Git repository or reinitialize an existing one.
    Init(commands::init::Args),
    /// Add or parse structured trailers in commit messages.
    #[command(name = "interpret-trailers")]
    InterpretTrailers(commands::interpret_trailers::Args),
    /// Compute object ID and optionally create an object from a file.
    #[command(name = "hash-object")]
    HashObject(commands::hash_object::Args),
    /// Provide contents or details of repository objects.
    #[command(name = "cat-file")]
    CatFile(commands::cat_file::Args),
    /// Display gitattributes information.
    #[command(name = "check-attr")]
    CheckAttr(commands::check_attr::Args),
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
    /// Stash the changes in a dirty working directory away.
    Stash(commands::stash::Args),
    /// Show the working tree status.
    Status(commands::status::Args),
    /// Reapply commits on top of another base tip.
    Rebase(commands::rebase::Args),
    /// Read tree information into the index.
    #[command(name = "read-tree")]
    ReadTree(commands::read_tree::Args),
    /// Manage set of tracked repositories.
    Remote(commands::remote::Args),
    /// Reuse recorded resolution of conflicted merges.
    Rerere(commands::rerere::Args),
    /// Check out files from the index into the working tree.
    #[command(name = "checkout-index")]
    CheckoutIndex(commands::checkout_index::Args),
    /// Create a new commit object.
    #[command(name = "commit-tree")]
    CommitTree(commands::commit_tree::Args),
    /// Update the object name stored in a ref safely.
    #[command(name = "update-ref")]
    UpdateRef(commands::update_ref::Args),
    /// Show canonical name/email from .mailmap.
    #[command(name = "check-mailmap")]
    CheckMailmap(commands::check_mailmap::Args),
    /// Debug gitignore and exclude rules.
    #[command(name = "check-ignore")]
    CheckIgnore(commands::check_ignore::Args),
    /// Count unpacked objects and disk usage.
    #[command(name = "count-objects")]
    CountObjects(commands::count_objects::Args),
    /// Give an object a human readable name based on an available ref.
    Describe(commands::describe::Args),
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
    /// Download objects and refs from another repository.
    Fetch(commands::fetch::Args),
    /// Output information on refs.
    #[command(name = "for-each-ref")]
    ForEachRef(commands::for_each_ref::Args),
    /// Join two or more development histories together.
    Merge(commands::merge::Args),
    /// Find best common ancestors.
    #[command(name = "merge-base")]
    MergeBase(commands::merge_base::Args),
    /// Name commits relative to refs.
    #[command(name = "name-rev")]
    NameRev(commands::name_rev::Args),
    /// Add or inspect object notes.
    Notes(commands::notes::Args),
    /// Run a three-way file merge.
    #[command(name = "merge-file")]
    MergeFile(commands::merge_file::Args),
    /// Show three-way merge without touching index/worktree.
    #[command(name = "merge-tree")]
    MergeTree(commands::merge_tree::Args),
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
    /// Verify a commit object.
    #[command(name = "verify-commit")]
    VerifyCommit(commands::verify_commit::Args),
    /// Validate packed Git archive files.
    #[command(name = "verify-pack")]
    VerifyPack(commands::verify_pack::Args),
    /// Verify a tag object.
    #[command(name = "verify-tag")]
    VerifyTag(commands::verify_tag::Args),
    /// Display version information.
    Version(commands::version::Args),
    /// Show logs with raw diff (no merges).
    Whatchanged(commands::whatchanged::Args),
    /// Produce a merge commit message.
    #[command(name = "fmt-merge-msg")]
    FmtMergeMsg(commands::fmt_merge_msg::Args),
    /// Prepare patches for e-mail submission.
    #[command(name = "format-patch")]
    FormatPatch(commands::format_patch::Args),
    /// Verify connectivity and validity of objects in the database.
    Fsck(commands::fsck::Args),
    /// Cleanup unnecessary files and optimize the repository.
    Gc(commands::gc::Args),
    /// Search tracked files for a pattern.
    Grep(commands::grep::Args),
    /// Pack unpacked objects in a repository.
    Repack(commands::repack::Args),
    /// Create, list, delete refs to replace objects.
    Replace(commands::replace::Args),
    /// Read a tag object from stdin, validate strictly, and write to ODB.
    #[command(name = "mktag")]
    Mktag(commands::mktag::Args),
    /// Remove unreachable loose objects.
    Prune(commands::prune::Args),
    /// Remove loose objects that are already stored in pack files.
    #[command(name = "prune-packed")]
    PrunePacked(commands::prune_packed::Args),
    /// Create, list, delete or verify a tag object.
    Tag(commands::tag::Args),
    /// Restore working tree files.
    Restore(commands::restore::Args),
    /// Revert some existing commits.
    Revert(commands::revert::Args),
    /// Summarize git log output.
    Shortlog(commands::shortlog::Args),
    /// Show various types of objects (commits, trees, blobs, tags).
    Show(commands::show::Args),
    /// Show branches and their commits.
    #[command(name = "show-branch")]
    ShowBranch(commands::show_branch::Args),
    /// Remove files from the working tree and from the index.
    Rm(commands::rm::Args),
    /// Move or rename a file, a directory, or a symlink.
    Mv(commands::mv::Args),
    /// Pack loose refs into packed-refs.
    #[command(name = "pack-refs")]
    PackRefs(commands::pack_refs::Args),
    /// Compute unique IDs for patches.
    #[command(name = "patch-id")]
    PatchId(commands::patch_id::Args),
    /// Build a tree object from ls-tree formatted text.
    #[command(name = "mktree")]
    MkTree(commands::mktree::Args),
    /// Show a Git logical variable.
    Var(commands::var::Args),
    /// Manage reflog information.
    Reflog(commands::reflog::Args),
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
    /// Manage multiple working trees.
    Worktree(commands::worktree::Args),
    /// Manage sparse checkout patterns.
    #[command(name = "sparse-checkout")]
    SparseCheckout(commands::sparse_checkout::Args),
    /// Create an archive of files from a named tree.
    Archive(commands::archive::Args),
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
        Command::Annotate(args) => commands::annotate::run(args),
        Command::Bisect(args) => commands::bisect::run(args),
        Command::Blame(args) => commands::blame::run(args),
        Command::Apply(args) => commands::apply::run(args),
        Command::Branch(args) => commands::branch::run(args),
        Command::Cherry(args) => commands::cherry::run(args),
        Command::CherryPick(args) => commands::cherry_pick::run(args),
        Command::CheckRefFormat(args) => commands::check_ref_format::run(args),
        Command::Clean(args) => commands::clean::run(args),
        Command::Clone(args) => commands::clone::run(args),
        Command::Checkout(args) => commands::checkout::run(args),
        Command::Init(args) => commands::init::run(args),
        Command::InterpretTrailers(args) => commands::interpret_trailers::run(args),
        Command::HashObject(args) => commands::hash_object::run(args),
        Command::CatFile(args) => commands::cat_file::run(args),
        Command::CheckAttr(args) => commands::check_attr::run(args),
        Command::Commit(args) => commands::commit::run(args),
        Command::Config(args) => commands::config::run(args),
        Command::Log(args) => commands::log::run(args),
        Command::UpdateIndex(args) => commands::update_index::run(args),
        Command::LsFiles(args) => commands::ls_files::run(args),
        Command::LsRemote(args) => commands::ls_remote::run(args),
        Command::WriteTree(args) => commands::write_tree::run(args),
        Command::LsTree(args) => commands::ls_tree::run(args),
        Command::Stash(args) => commands::stash::run(args),
        Command::Status(args) => commands::status::run(args),
        Command::Rebase(args) => commands::rebase::run(args),
        Command::ReadTree(args) => commands::read_tree::run(args),
        Command::Remote(args) => commands::remote::run(args),
        Command::Rerere(args) => commands::rerere::run(args),
        Command::CheckoutIndex(args) => commands::checkout_index::run(args),
        Command::CommitTree(args) => commands::commit_tree::run(args),
        Command::UpdateRef(args) => commands::update_ref::run(args),
        Command::CheckMailmap(args) => commands::check_mailmap::run(args),
        Command::CheckIgnore(args) => commands::check_ignore::run(args),
        Command::CountObjects(args) => commands::count_objects::run(args),
        Command::Describe(args) => commands::describe::run(args),
        Command::Diff(args) => commands::diff::run(args),
        Command::DiffFiles(args) => commands::diff_files::run(args),
        Command::DiffTree(args) => commands::diff_tree::run(args),
        Command::DiffIndex(args) => commands::diff_index::run(args),
        Command::Fetch(args) => commands::fetch::run(args),
        Command::ForEachRef(args) => commands::for_each_ref::run(args),
        Command::Merge(args) => commands::merge::run(args),
        Command::MergeBase(args) => commands::merge_base::run(args),
        Command::NameRev(args) => commands::name_rev::run(args),
        Command::Notes(args) => commands::notes::run(args),
        Command::MergeFile(args) => commands::merge_file::run(args),
        Command::MergeTree(args) => commands::merge_tree::run(args),
        Command::RevList(args) => commands::rev_list::run(args),
        Command::RevParse(args) => commands::rev_parse::run(args),
        Command::ShowIndex(args) => commands::show_index::run(args),
        Command::ShowRef(args) => commands::show_ref::run(args),
        Command::SymbolicRef(args) => commands::symbolic_ref::run(args),
        Command::VerifyCommit(args) => commands::verify_commit::run(args),
        Command::VerifyPack(args) => commands::verify_pack::run(args),
        Command::VerifyTag(args) => commands::verify_tag::run(args),
        Command::Version(args) => commands::version::run(args),
        Command::Whatchanged(args) => commands::whatchanged::run(args),
        Command::FmtMergeMsg(args) => commands::fmt_merge_msg::run(args),
        Command::FormatPatch(args) => commands::format_patch::run(args),
        Command::Fsck(args) => commands::fsck::run(args),
        Command::Gc(args) => commands::gc::run(args),
        Command::Grep(args) => commands::grep::run(args),
        Command::Repack(args) => commands::repack::run(args),
        Command::Replace(args) => commands::replace::run(args),
        Command::Mktag(args) => commands::mktag::run(args),
        Command::Prune(args) => commands::prune::run(args),
        Command::PrunePacked(args) => commands::prune_packed::run(args),
        Command::Tag(args) => commands::tag::run(args),
        Command::Restore(args) => commands::restore::run(args),
        Command::Revert(args) => commands::revert::run(args),
        Command::Shortlog(args) => commands::shortlog::run(args),
        Command::Show(args) => commands::show::run(args),
        Command::ShowBranch(args) => commands::show_branch::run(args),
        Command::Rm(args) => commands::rm::run(args),
        Command::Mv(args) => commands::mv::run(args),
        Command::PackRefs(args) => commands::pack_refs::run(args),
        Command::PatchId(args) => commands::patch_id::run(args),
        Command::MkTree(args) => commands::mktree::run(args),
        Command::Var(args) => commands::var::run(args),
        Command::Reflog(args) => commands::reflog::run(args),
        Command::Reset(args) => commands::reset::run(args),
        Command::Stripspace(args) => commands::stripspace::run(args),
        Command::Switch(args) => commands::switch::run(args),
        Command::UnpackFile(args) => commands::unpack_file::run(args),
        Command::UnpackObjects(args) => commands::unpack_objects::run(args),
        Command::Worktree(args) => commands::worktree::run(args),
        Command::SparseCheckout(args) => commands::sparse_checkout::run(args),
        Command::Archive(args) => commands::archive::run(args),
    }
}
