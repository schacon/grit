//! `grit rev-parse` - pick out and massage revision parameters.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::rev_parse::{
    abbreviate_object_id, discover_optional, is_inside_git_dir, is_inside_work_tree,
    resolve_revision, show_prefix, to_relative_path,
};
use std::env;

/// Arguments for `grit rev-parse`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit rev-parse`.
pub fn run(args: Args) -> Result<()> {
    let cwd = env::current_dir().context("failed to read current directory")?;
    let mut verify = false;
    let mut quiet = false;
    let mut sq_quote = false;
    let mut short_len: Option<usize> = None;
    let mut show_is_inside_work_tree = false;
    let mut show_is_inside_git_dir = false;
    let mut show_is_bare = false;
    let mut show_toplevel = false;
    let mut show_prefix_flag = false;
    let mut show_git_dir = false;
    let mut prefix: Option<String> = None;
    let mut default_rev: Option<String> = None;
    let mut revisions = Vec::new();
    let mut forced_paths = Vec::new();
    let mut saw_path_separator = false;
    let mut end_of_options = false;

    let mut i = 0usize;
    while i < args.args.len() {
        let arg = &args.args[i];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            saw_path_separator = true;
            i += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') {
            if arg == "--verify" {
                verify = true;
            } else if arg == "--quiet" || arg == "-q" {
                quiet = true;
            } else if arg == "--is-inside-work-tree" {
                show_is_inside_work_tree = true;
            } else if arg == "--is-inside-git-dir" {
                show_is_inside_git_dir = true;
            } else if arg == "--is-bare-repository" {
                show_is_bare = true;
            } else if arg == "--show-toplevel" {
                show_toplevel = true;
            } else if arg == "--show-prefix" {
                show_prefix_flag = true;
            } else if arg == "--git-dir" {
                show_git_dir = true;
            } else if arg == "--prefix" {
                i += 1;
                let value = args
                    .args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--prefix requires an argument"))?;
                prefix = Some(value.clone());
            } else if let Some(value) = arg.strip_prefix("--prefix=") {
                prefix = Some(value.to_owned());
            } else if let Some(value) = arg.strip_prefix("--short=") {
                verify = true;
                short_len = Some(parse_short_len(value)?);
            } else if arg == "--short" {
                verify = true;
                short_len = Some(7);
            } else if arg == "--default" {
                i += 1;
                let value = args
                    .args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--default requires an argument"))?;
                default_rev = Some(value.clone());
            } else if let Some(value) = arg.strip_prefix("--default=") {
                default_rev = Some(value.to_owned());
            } else if arg == "--end-of-options" {
                end_of_options = true;
            } else if arg == "--branches" {
                if let Some(current) = discover_optional(None)? {
                    let matching = grit_lib::refs::list_refs(&current.git_dir, "refs/heads/")
                        .context("failed to list branch refs")?;
                    for (_, oid) in matching {
                        println!("{oid}");
                    }
                }
            } else if let Some(pattern) = arg.strip_prefix("--branches=") {
                if let Some(current) = discover_optional(None)? {
                    let full = format!("refs/heads/{pattern}");
                    let matching = grit_lib::refs::list_refs_glob(&current.git_dir, &full)
                        .context("failed to list branch refs")?;
                    for (_, oid) in matching {
                        println!("{oid}");
                    }
                }
            } else if arg == "--tags" {
                if let Some(current) = discover_optional(None)? {
                    let matching = grit_lib::refs::list_refs(&current.git_dir, "refs/tags/")
                        .context("failed to list tag refs")?;
                    for (_, oid) in matching {
                        println!("{oid}");
                    }
                }
            } else if let Some(pattern) = arg.strip_prefix("--tags=") {
                if let Some(current) = discover_optional(None)? {
                    let full = format!("refs/tags/{pattern}");
                    let matching = grit_lib::refs::list_refs_glob(&current.git_dir, &full)
                        .context("failed to list tag refs")?;
                    for (_, oid) in matching {
                        println!("{oid}");
                    }
                }
            } else if arg == "--remotes" {
                if let Some(current) = discover_optional(None)? {
                    let matching = grit_lib::refs::list_refs(&current.git_dir, "refs/remotes/")
                        .context("failed to list remote refs")?;
                    for (_, oid) in matching {
                        println!("{oid}");
                    }
                }
            } else if let Some(pattern) = arg.strip_prefix("--remotes=") {
                if let Some(current) = discover_optional(None)? {
                    let full = format!("refs/remotes/{pattern}");
                    let matching = grit_lib::refs::list_refs_glob(&current.git_dir, &full)
                        .context("failed to list remote refs")?;
                    for (_, oid) in matching {
                        println!("{oid}");
                    }
                }
            } else if arg == "--sq-quote" {
                sq_quote = true;
            } else {
                bail!("unsupported option: {arg}");
            }
            i += 1;
            continue;
        }
        if saw_path_separator {
            forced_paths.push(arg.clone());
        } else {
            revisions.push(arg.clone());
        }
        i += 1;
    }

    // --sq-quote: shell-quote all remaining args and exit
    if sq_quote {
        let mut out = String::new();
        for rev in &revisions {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&sq_quote_str(rev));
        }
        println!("{out}");
        return Ok(());
    }

    let repo = discover_optional(None)?;
    if show_is_inside_work_tree {
        let inside = repo
            .as_ref()
            .map(|current| is_inside_work_tree(current, &cwd))
            .unwrap_or(false);
        println!("{}", if inside { "true" } else { "false" });
    }
    if show_is_inside_git_dir {
        let inside = repo
            .as_ref()
            .map(|current| is_inside_git_dir(current, &cwd))
            .unwrap_or(false);
        println!("{}", if inside { "true" } else { "false" });
    }
    if show_is_bare {
        let bare = repo
            .as_ref()
            .map(|current| current.is_bare())
            .unwrap_or(false);
        println!("{}", if bare { "true" } else { "false" });
    }

    if show_toplevel {
        let Some(current) = repo.as_ref() else {
            bail!("not a git repository (or any of the parent directories)");
        };
        let Some(work_tree) = &current.work_tree else {
            bail!("this operation must be run in a work tree");
        };
        println!("{}", work_tree.display());
    }
    if show_prefix_flag {
        let Some(current) = repo.as_ref() else {
            bail!("not a git repository (or any of the parent directories)");
        };
        println!("{}", show_prefix(current, &cwd));
    }
    if show_git_dir {
        let Some(current) = repo.as_ref() else {
            bail!("not a git repository (or any of the parent directories)");
        };
        println!("{}", to_relative_path(&current.git_dir, &cwd));
    }

    if !verify && revisions.is_empty() && forced_paths.is_empty() {
        return Ok(());
    }

    let Some(current) = repo.as_ref() else {
        if quiet && verify {
            std::process::exit(1);
        }
        bail!("not a git repository (or any of the parent directories)");
    };

    if verify {
        if revisions.is_empty() {
            if let Some(default_name) = default_rev.as_deref() {
                revisions.push(default_name.to_owned());
            }
        }
        if revisions.len() != 1 {
            return fail_verify(quiet);
        }
        let oid = match resolve_revision(current, &revisions[0]) {
            Ok(oid) => oid,
            Err(_) => return fail_verify(quiet),
        };
        if let Some(len) = short_len {
            println!("{}", abbreviate_object_id(current, oid, len)?);
        } else {
            println!("{oid}");
        }
        return Ok(());
    }

    for rev in revisions {
        let rewritten = rewrite_tree_path_spec(&rev, prefix.as_deref());
        if let Ok(oid) = resolve_revision(current, &rewritten) {
            println!("{oid}");
            continue;
        }
        if let Some(path_prefix) = prefix.as_deref() {
            println!("{}", apply_prefix_for_forced_path(path_prefix, &rev));
            continue;
        }
        return Err(anyhow::anyhow!("bad revision '{rev}'"));
    }

    if saw_path_separator && !forced_paths.is_empty() {
        println!("--");
    }
    if let Some(path_prefix) = prefix.as_deref() {
        for path in forced_paths {
            println!("{}", apply_prefix_for_forced_path(path_prefix, &path));
        }
    } else {
        for path in forced_paths {
            println!("{path}");
        }
    }
    Ok(())
}

fn parse_short_len(raw: &str) -> Result<usize> {
    let parsed = raw
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("invalid --short length: {raw}"))?;
    Ok(parsed.clamp(4, 40))
}

fn fail_verify(quiet: bool) -> Result<()> {
    if quiet {
        std::process::exit(1);
    }
    bail!("Needed a single revision")
}

fn apply_prefix_for_forced_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        return path.to_owned();
    }
    format!("{prefix}{path}")
}

fn rewrite_tree_path_spec(spec: &str, prefix: Option<&str>) -> String {
    let Some((treeish, raw_path)) = spec.split_once(':') else {
        return spec.to_owned();
    };
    if treeish.is_empty() || raw_path.is_empty() {
        return spec.to_owned();
    }
    if !raw_path.starts_with("./") && !raw_path.starts_with("../") {
        return spec.to_owned();
    }

    let mut joined = String::new();
    if let Some(prefix) = prefix {
        joined.push_str(prefix);
    }
    joined.push_str(raw_path);
    let normalized = normalize_slash_path(&joined);
    format!("{treeish}:{normalized}")
}

/// Shell-quote a string using single quotes, matching git's sq_quote_buf.
fn sq_quote_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn normalize_slash_path(path: &str) -> String {
    let mut parts = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
