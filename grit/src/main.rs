//! `grit` — Git plumbing reimplementation in Rust.
//!
//! This binary uses manual pre-dispatch to avoid building a clap parser for
//! all 143+ subcommands on every invocation.  Global options (-C, --git-dir,
//! --work-tree, -c) are extracted from argv by hand, then only the specific
//! subcommand's clap `Args` struct is parsed.

use anyhow::{bail, Result};
use clap::{Args, FromArgMatches, Parser};
use std::path::PathBuf;

mod commands;
pub mod pathspec;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Global options parsed from argv before the subcommand.
#[derive(Default)]
struct GlobalOpts {
    git_dir: Option<PathBuf>,
    work_tree: Option<PathBuf>,
    change_dir: Option<PathBuf>,
    config_overrides: Vec<String>,
    bare: bool,
}

/// Extract global options and return (globals, subcommand_name, remaining_args).
///
/// We scan argv[1..] for global flags that appear before the subcommand.
/// The first non-flag argument is the subcommand name.
fn extract_globals(args: &[String]) -> Result<(GlobalOpts, Option<String>, Vec<String>)> {
    let mut opts = GlobalOpts::default();
    let mut subcmd = None;
    let mut rest = Vec::new();
    let mut i = 0;
    let items = &args[1..]; // skip argv[0]

    while i < items.len() {
        let arg = &items[i];

        // -C <dir>
        if arg == "-C" {
            i += 1;
            if i < items.len() {
                opts.change_dir = Some(PathBuf::from(&items[i]));
            }
            i += 1;
            continue;
        }

        // --git-dir=<val> or --git-dir <val>
        if let Some(val) = arg.strip_prefix("--git-dir=") {
            opts.git_dir = Some(PathBuf::from(val));
            i += 1;
            continue;
        }
        if arg == "--git-dir" {
            i += 1;
            if i < items.len() {
                opts.git_dir = Some(PathBuf::from(&items[i]));
            }
            i += 1;
            continue;
        }

        // --work-tree=<val> or --work-tree <val>
        if let Some(val) = arg.strip_prefix("--work-tree=") {
            opts.work_tree = Some(PathBuf::from(val));
            i += 1;
            continue;
        }
        if arg == "--work-tree" {
            i += 1;
            if i < items.len() {
                opts.work_tree = Some(PathBuf::from(&items[i]));
            }
            i += 1;
            continue;
        }

        // -c key=value
        if arg == "-c" {
            i += 1;
            if i < items.len() {
                opts.config_overrides.push(items[i].clone());
            }
            i += 1;
            continue;
        }

        // --bare
        if arg == "--bare" {
            opts.bare = true;
            i += 1;
            continue;
        }

        // --version / -v / --help / -h  → treat as pseudo-subcommands
        if arg == "--version" || arg == "-v" {
            subcmd = Some("version".to_owned());
            rest = items[i + 1..].to_vec();
            break;
        }
        if arg == "--help" || arg == "-h" || arg == "help" {
            subcmd = Some("help".to_owned());
            rest = items[i + 1..].to_vec();
            break;
        }

        // First non-flag argument is the subcommand
        if !arg.starts_with('-') {
            subcmd = Some(arg.clone());
            rest = items[i + 1..].to_vec();
            break;
        }

        // Unknown global flag — pass through
        bail!("unknown option: {arg}");
    }

    Ok((opts, subcmd, rest))
}

/// Apply global options (env vars, chdir).
fn apply_globals(opts: &GlobalOpts) -> Result<()> {
    if let Some(dir) = &opts.change_dir {
        std::env::set_current_dir(dir)?;
    }
    if let Some(git_dir) = &opts.git_dir {
        std::env::set_var("GIT_DIR", git_dir);
    }
    if let Some(wt) = &opts.work_tree {
        std::env::set_var("GIT_WORK_TREE", wt);
    }
    if !opts.config_overrides.is_empty() {
        let params: String = opts
            .config_overrides
            .iter()
            .map(|kv| format!("'{}'", kv))
            .collect::<Vec<_>>()
            .join(" ");
        std::env::set_var("GIT_CONFIG_PARAMETERS", params);
    }
    Ok(())
}

/// Wrapper to parse a clap `Args` struct as if it were a top-level `Parser`.
///
/// Each subcommand's Args struct derives `clap::Args`, not `clap::Parser`.
/// This wrapper lets us parse it standalone from a slice of arguments.
#[derive(Debug, Parser)]
#[command(name = "grit", disable_help_subcommand = true)]
struct ArgsWrapper<T: Args> {
    #[command(flatten)]
    inner: T,
}

/// Parse a command's clap Args from the remaining arguments.
fn parse_cmd_args<T: Args + FromArgMatches>(subcmd: &str, rest: &[String]) -> T {
    let mut argv = vec![format!("grit {subcmd}")];
    argv.extend(rest.iter().cloned());
    let wrapper = ArgsWrapper::<T>::parse_from(argv);
    wrapper.inner
}

fn run() -> Result<()> {
    // Check env vars that clap would have handled
    if std::env::var("GIT_DIR").is_ok() && std::env::var("GIT_DIR").unwrap().is_empty() {
        // ignore empty GIT_DIR
    }

    let args: Vec<String> = std::env::args().collect();
    let (opts, subcmd, rest) = extract_globals(&args)?;

    let subcmd = match subcmd {
        Some(s) => s,
        None => {
            eprintln!("grit: a Git plumbing reimplementation in Rust");
            eprintln!("usage: grit <command> [<args>]");
            std::process::exit(1);
        }
    };

    apply_globals(&opts)?;

    dispatch(&subcmd, &rest, &opts)
}

/// Preprocess diff arguments: expand `-U<N>` to `--unified=<N>` so that
/// clap does not swallow it into the trailing var-arg positional.
fn preprocess_diff_args(rest: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        if arg == "-U" {
            // `-U <N>` with a space — merge into `--unified=<N>`
            if let Some(val) = iter.next() {
                result.push(format!("--unified={val}"));
            } else {
                result.push(arg.clone());
            }
        } else if let Some(n) = arg.strip_prefix("-U") {
            // `-U<N>` without a space
            result.push(format!("--unified={n}"));
        } else {
            result.push(arg.clone());
        }
    }
    result
}

/// Preprocess log arguments: convert `-<N>` shorthand to `-n <N>`.
fn preprocess_log_args(rest: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for arg in rest {
        if arg.starts_with('-') && arg.len() > 1 && arg[1..].chars().all(|c| c.is_ascii_digit()) {
            result.push("-n".to_string());
            result.push(arg[1..].to_string());
        } else {
            result.push(arg.clone());
        }
    }
    result
}

/// Dispatch to the appropriate command handler.
///
/// Each arm only constructs the clap parser for that specific command.
fn dispatch(subcmd: &str, rest: &[String], opts: &GlobalOpts) -> Result<()> {
    match subcmd {
        "add" => commands::add::run(parse_cmd_args(subcmd, rest)),
        "am" => commands::am::run(parse_cmd_args(subcmd, rest)),
        "annotate" => commands::annotate::run(parse_cmd_args(subcmd, rest)),
        "apply" => commands::apply::run(parse_cmd_args(subcmd, rest)),
        "archive" => commands::archive::run(parse_cmd_args(subcmd, rest)),
        "backfill" => commands::backfill::run(parse_cmd_args(subcmd, rest)),
        "bisect" => commands::bisect::run(parse_cmd_args(subcmd, rest)),
        "blame" => commands::blame::run(parse_cmd_args(subcmd, rest)),
        "branch" => commands::branch::run(parse_cmd_args(subcmd, rest)),
        "bugreport" => commands::bugreport::run(parse_cmd_args(subcmd, rest)),
        "bundle" => commands::bundle::run(parse_cmd_args(subcmd, rest)),
        "cat-file" => commands::cat_file::run(parse_cmd_args(subcmd, rest)),
        "check-attr" => commands::check_attr::run(parse_cmd_args(subcmd, rest)),
        "check-ignore" => commands::check_ignore::run(parse_cmd_args(subcmd, rest)),
        "check-mailmap" => commands::check_mailmap::run(parse_cmd_args(subcmd, rest)),
        "check-ref-format" => commands::check_ref_format::run(parse_cmd_args(subcmd, rest)),
        "checkout" => commands::checkout::run(parse_cmd_args(subcmd, rest)),
        "checkout-index" => commands::checkout_index::run(parse_cmd_args(subcmd, rest)),
        "cherry" => commands::cherry::run(parse_cmd_args(subcmd, rest)),
        "cherry-pick" => commands::cherry_pick::run(parse_cmd_args(subcmd, rest)),
        "clean" => commands::clean::run(parse_cmd_args(subcmd, rest)),
        "clone" => commands::clone::run(parse_cmd_args(subcmd, rest)),
        "column" => commands::column::run(parse_cmd_args(subcmd, rest)),
        "commit" => commands::commit::run(parse_cmd_args(subcmd, rest)),
        "commit-graph" => commands::commit_graph::run(parse_cmd_args(subcmd, rest)),
        "commit-tree" => commands::commit_tree::run(parse_cmd_args(subcmd, rest)),
        "config" => commands::config::run(parse_cmd_args(subcmd, rest)),
        "count-objects" => commands::count_objects::run(parse_cmd_args(subcmd, rest)),
        "credential" => commands::credential::run(parse_cmd_args(subcmd, rest)),
        "credential-cache" => commands::credential_cache::run(parse_cmd_args(subcmd, rest)),
        "credential-store" => commands::credential_store::run(parse_cmd_args(subcmd, rest)),
        "daemon" => commands::daemon::run(parse_cmd_args(subcmd, rest)),
        "describe" => commands::describe::run(parse_cmd_args(subcmd, rest)),
        "diagnose" => commands::diagnose::run(parse_cmd_args(subcmd, rest)),
        "diff" => commands::diff::run(parse_cmd_args(subcmd, &preprocess_diff_args(rest))),
        "diff-files" => commands::diff_files::run(parse_cmd_args(subcmd, rest)),
        "diff-index" => commands::diff_index::run(parse_cmd_args(subcmd, rest)),
        "diff-pairs" => commands::diff_pairs::run(parse_cmd_args(subcmd, rest)),
        "diff-tree" => commands::diff_tree::run(parse_cmd_args(subcmd, rest)),
        "difftool" => commands::difftool::run(parse_cmd_args(subcmd, rest)),
        "fast-export" => commands::fast_export::run(parse_cmd_args(subcmd, rest)),
        "fast-import" => commands::fast_import::run(parse_cmd_args(subcmd, rest)),
        "fetch" => commands::fetch::run(parse_cmd_args(subcmd, rest)),
        "fetch-pack" => commands::fetch_pack::run(parse_cmd_args(subcmd, rest)),
        "filter-branch" => commands::filter_branch::run(parse_cmd_args(subcmd, rest)),
        "fmt-merge-msg" => commands::fmt_merge_msg::run(parse_cmd_args(subcmd, rest)),
        "for-each-ref" => commands::for_each_ref::run(parse_cmd_args(subcmd, rest)),
        "for-each-repo" => commands::for_each_repo::run(parse_cmd_args(subcmd, rest)),
        "format-patch" => commands::format_patch::run(parse_cmd_args(subcmd, rest)),
        "fsck" => commands::fsck::run(parse_cmd_args(subcmd, rest)),
        "gc" => commands::gc::run(parse_cmd_args(subcmd, rest)),
        "get-tar-commit-id" => commands::get_tar_commit_id::run(parse_cmd_args(subcmd, rest)),
        "grep" => commands::grep::run(parse_cmd_args(subcmd, rest)),
        "hash-object" => commands::hash_object::run(parse_cmd_args(subcmd, rest)),
        "help" => commands::help::run(parse_cmd_args(subcmd, rest)),
        "history" => commands::history::run(parse_cmd_args(subcmd, rest)),
        "hook" => commands::hook::run(parse_cmd_args(subcmd, rest)),
        "http-backend" => commands::http_backend::run(parse_cmd_args(subcmd, rest)),
        "http-fetch" => commands::http_fetch::run(parse_cmd_args(subcmd, rest)),
        "http-push" => commands::http_push::run(parse_cmd_args(subcmd, rest)),
        "index-pack" => commands::index_pack::run(parse_cmd_args(subcmd, rest)),
        "init" => commands::init::run(parse_cmd_args(subcmd, rest), opts.bare),
        "interpret-trailers" => commands::interpret_trailers::run(parse_cmd_args(subcmd, rest)),
        "last-modified" => commands::last_modified::run(parse_cmd_args(subcmd, rest)),
        "log" => {
            let rest = preprocess_log_args(rest);
            commands::log::run(parse_cmd_args(subcmd, &rest))
        }
        "ls-files" => commands::ls_files::run(parse_cmd_args(subcmd, rest)),
        "ls-remote" => commands::ls_remote::run(parse_cmd_args(subcmd, rest)),
        "ls-tree" => commands::ls_tree::run(parse_cmd_args(subcmd, rest)),
        "mailinfo" => commands::mailinfo::run(parse_cmd_args(subcmd, rest)),
        "mailsplit" => commands::mailsplit::run(parse_cmd_args(subcmd, rest)),
        "maintenance" => commands::maintenance::run(parse_cmd_args(subcmd, rest)),
        "merge" => commands::merge::run(parse_cmd_args(subcmd, rest)),
        "merge-base" => commands::merge_base::run(parse_cmd_args(subcmd, rest)),
        "merge-file" => commands::merge_file::run(parse_cmd_args(subcmd, rest)),
        "merge-index" => commands::merge_index::run(parse_cmd_args(subcmd, rest)),
        "merge-one-file" => commands::merge_one_file::run(parse_cmd_args(subcmd, rest)),
        "merge-tree" => commands::merge_tree::run(parse_cmd_args(subcmd, rest)),
        "mergetool" => commands::mergetool::run(parse_cmd_args(subcmd, rest)),
        "mktag" => commands::mktag::run(parse_cmd_args(subcmd, rest)),
        "mktree" => commands::mktree::run(parse_cmd_args(subcmd, rest)),
        "multi-pack-index" => commands::multi_pack_index::run(parse_cmd_args(subcmd, rest)),
        "mv" => commands::mv::run(parse_cmd_args(subcmd, rest)),
        "name-rev" => commands::name_rev::run(parse_cmd_args(subcmd, rest)),
        "notes" => commands::notes::run(parse_cmd_args(subcmd, rest)),
        "pack-objects" => commands::pack_objects::run(parse_cmd_args(subcmd, rest)),
        "pack-redundant" => commands::pack_redundant::run(parse_cmd_args(subcmd, rest)),
        "pack-refs" => commands::pack_refs::run(parse_cmd_args(subcmd, rest)),
        "patch-id" => commands::patch_id::run(parse_cmd_args(subcmd, rest)),
        "prune" => commands::prune::run(parse_cmd_args(subcmd, rest)),
        "prune-packed" => commands::prune_packed::run(parse_cmd_args(subcmd, rest)),
        "pull" => commands::pull::run(parse_cmd_args(subcmd, rest)),
        "push" => commands::push::run(parse_cmd_args(subcmd, rest)),
        "range-diff" => commands::range_diff::run(parse_cmd_args(subcmd, rest)),
        "read-tree" => commands::read_tree::run(parse_cmd_args(subcmd, rest)),
        "rebase" => commands::rebase::run(parse_cmd_args(subcmd, rest)),
        "receive-pack" => commands::receive_pack::run(parse_cmd_args(subcmd, rest)),
        "reflog" => {
            let rest = preprocess_log_args(rest);
            commands::reflog::run(parse_cmd_args(subcmd, &rest))
        }
        "refs" => commands::refs::run(parse_cmd_args(subcmd, rest)),
        "remote" => commands::remote::run(parse_cmd_args(subcmd, rest)),
        "repack" => commands::repack::run(parse_cmd_args(subcmd, rest)),
        "replace" => commands::replace::run(parse_cmd_args(subcmd, rest)),
        "replay" => commands::replay::run(parse_cmd_args(subcmd, rest)),
        "repo" => commands::repo::run(parse_cmd_args(subcmd, rest)),
        "rerere" => commands::rerere::run(parse_cmd_args(subcmd, rest)),
        "reset" => commands::reset::run(parse_cmd_args(subcmd, rest)),
        "restore" => commands::restore::run(parse_cmd_args(subcmd, rest)),
        "rev-list" => commands::rev_list::run(parse_cmd_args(subcmd, rest)),
        "rev-parse" => commands::rev_parse::run(parse_cmd_args(subcmd, rest)),
        "revert" => commands::revert::run(parse_cmd_args(subcmd, rest)),
        "rm" => commands::rm::run(parse_cmd_args(subcmd, rest)),
        "send-pack" => commands::send_pack::run(parse_cmd_args(subcmd, rest)),
        "sh-i18n" => commands::sh_i18n::run(parse_cmd_args(subcmd, rest)),
        "sh-setup" => commands::sh_setup::run(parse_cmd_args(subcmd, rest)),
        "shell" => commands::shell::run(parse_cmd_args(subcmd, rest)),
        "shortlog" => commands::shortlog::run(parse_cmd_args(subcmd, rest)),
        "show" => commands::show::run(parse_cmd_args(subcmd, rest)),
        "show-branch" => commands::show_branch::run(parse_cmd_args(subcmd, rest)),
        "show-index" => commands::show_index::run(parse_cmd_args(subcmd, rest)),
        "show-ref" => commands::show_ref::run(parse_cmd_args(subcmd, rest)),
        "sparse-checkout" => commands::sparse_checkout::run(parse_cmd_args(subcmd, rest)),
        "stage" => commands::stage::run(parse_cmd_args(subcmd, rest)),
        "stash" => commands::stash::run(parse_cmd_args(subcmd, rest)),
        "status" => commands::status::run(parse_cmd_args(subcmd, rest)),
        "stripspace" => commands::stripspace::run(parse_cmd_args(subcmd, rest)),
        "submodule" => commands::submodule::run(parse_cmd_args(subcmd, rest)),
        "switch" => commands::switch::run(parse_cmd_args(subcmd, rest)),
        "symbolic-ref" => commands::symbolic_ref::run(parse_cmd_args(subcmd, rest)),
        "tag" => commands::tag::run(parse_cmd_args(subcmd, rest)),
        "unpack-file" => commands::unpack_file::run(parse_cmd_args(subcmd, rest)),
        "unpack-objects" => commands::unpack_objects::run(parse_cmd_args(subcmd, rest)),
        "update-index" => commands::update_index::run(parse_cmd_args(subcmd, rest)),
        "update-ref" => commands::update_ref::run(parse_cmd_args(subcmd, rest)),
        "update-server-info" => commands::update_server_info::run(parse_cmd_args(subcmd, rest)),
        "upload-archive" => commands::upload_archive::run(parse_cmd_args(subcmd, rest)),
        "upload-pack" => commands::upload_pack::run(parse_cmd_args(subcmd, rest)),
        "var" => commands::var::run(parse_cmd_args(subcmd, rest)),
        "verify-commit" => commands::verify_commit::run(parse_cmd_args(subcmd, rest)),
        "verify-pack" => commands::verify_pack::run(parse_cmd_args(subcmd, rest)),
        "verify-tag" => commands::verify_tag::run(parse_cmd_args(subcmd, rest)),
        "version" => commands::version::run(parse_cmd_args(subcmd, rest)),
        "whatchanged" => commands::whatchanged::run(parse_cmd_args(subcmd, rest)),
        "worktree" => commands::worktree::run(parse_cmd_args(subcmd, rest)),
        "write-tree" => commands::write_tree::run(parse_cmd_args(subcmd, rest)),
        _ => bail!("grit: '{subcmd}' is not a grit command. See 'grit help'."),
    }
}
