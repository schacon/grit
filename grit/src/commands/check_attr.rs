//! `grit check-attr` — display gitattributes information.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::attributes::{
    builtin_objectmode_index, builtin_objectmode_worktree, collect_attrs_for_path,
    load_gitattributes_bare, load_gitattributes_from_index, load_gitattributes_from_tree,
    load_gitattributes_stack, normalize_rel_path, path_relative_to_worktree,
    quote_path_for_check_attr, resolve_attr_treeish, resolve_tree_oid, ParsedGitAttributes,
};
use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit check-attr`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display gitattributes information")]
pub struct Args {
    /// Report all attributes set for each file.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Read paths from stdin (one per line).
    #[arg(long = "stdin")]
    pub stdin: bool,

    /// Use .gitattributes from the index only.
    #[arg(long = "cached")]
    pub cached: bool,

    /// Read attributes from the given tree-ish.
    #[arg(long = "source", value_name = "TREEISH")]
    pub source: Option<String>,

    /// Use NUL as delimiter with --stdin / output.
    #[arg(short = 'Z')]
    pub nul: bool,

    /// Attribute names and pathnames (after `--`).
    #[arg(required = true, allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

fn parse_attrs_paths(args: &Args) -> (Vec<String>, Vec<String>) {
    if args.all {
        let mut paths = Vec::new();
        for a in &args.args {
            if a == "--" {
                continue;
            }
            paths.push(a.clone());
        }
        return (Vec::new(), paths);
    }
    let mut attrs = Vec::new();
    let mut paths = Vec::new();
    let mut after = false;
    for a in &args.args {
        if a == "--" {
            after = true;
            continue;
        }
        if after {
            paths.push(a.clone());
        } else {
            attrs.push(a.clone());
        }
    }
    if !after && attrs.len() > 1 {
        paths.push(attrs.pop().unwrap_or_default());
    } else if !after && attrs.len() == 1 && paths.is_empty() {
        paths.push(attrs.remove(0));
    }
    (attrs, paths)
}

fn validate_cli(attrs: &[String], paths: &[String], args: &Args) -> Result<()> {
    if args.stdin {
        if attrs.is_empty() && !args.all {
            bail!("usage: missing attribute name");
        }
        if !paths.is_empty() {
            bail!("usage: pathspec with --stdin");
        }
        return Ok(());
    }
    if attrs.is_empty() && !args.all {
        bail!("usage: missing attribute name");
    }
    if paths.is_empty() {
        bail!("usage: missing pathspec");
    }
    for a in attrs {
        if a.is_empty() {
            bail!("usage: empty attribute name");
        }
    }
    Ok(())
}

fn load_parsed_for_run(repo: &Repository, args: &Args) -> Result<ParsedGitAttributes> {
    let treeish = resolve_attr_treeish(repo, args.source.as_deref())?;

    if let Some(spec) = treeish.filter(|s| !s.is_empty()) {
        let oid = resolve_tree_oid(repo, &spec)
            .map_err(|_| anyhow::anyhow!("fatal: bad --attr-source or GIT_ATTR_SOURCE"))?;
        return load_gitattributes_from_tree(&repo.odb, &oid).context("load tree attributes");
    }

    if args.cached {
        let index_path = std::env::var("GIT_INDEX_FILE")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.index_path());
        let index = repo.load_index_at(&index_path).context("read index")?;
        let wt = repo.work_tree.as_deref().unwrap_or_else(|| Path::new("."));
        return load_gitattributes_from_index(&index, &repo.odb, wt).context("index attributes");
    }

    if repo.work_tree.is_none() {
        return load_gitattributes_bare(repo).context("bare attributes");
    }

    let wt = repo.work_tree.as_ref().unwrap();
    load_gitattributes_stack(repo, wt).context("work tree attributes")
}

/// Run the `check-attr` command.
pub fn run(args: Args) -> Result<()> {
    let (attrs, mut paths) = parse_attrs_paths(&args);
    validate_cli(&attrs, &paths, &args)?;

    if args.stdin {
        let mut stdin = io::stdin().lock();
        let mut line = String::new();
        while stdin.read_line(&mut line)? > 0 {
            let p = line.trim_end_matches(['\r', '\n']);
            if !p.is_empty() {
                paths.push(p.to_string());
            }
            line.clear();
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    let parsed = load_parsed_for_run(&repo, &args)?;

    for w in &parsed.warnings {
        eprintln!("{w}");
    }

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let ignore_case = config
        .get("core.ignorecase")
        .is_some_and(|v| v == "true" || v == "1" || v == "yes");

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let index_path = std::env::var("GIT_INDEX_FILE")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.index_path());
    let index_cached = repo.load_index_at(&index_path).ok();

    for raw_path in &paths {
        let rel = if repo.work_tree.is_some() {
            path_relative_to_worktree(&repo, raw_path)
                .unwrap_or_else(|_| normalize_rel_path(raw_path))
        } else {
            normalize_rel_path(raw_path)
        };
        let rel = normalize_rel_path(&rel);
        let path_out = quote_path_for_check_attr(raw_path);

        let map = collect_attrs_for_path(&parsed.rules, &parsed.macros, &rel, ignore_case);

        if args.all {
            let mut names: Vec<String> = map.keys().cloned().collect();
            names.sort();
            for name in names {
                if let Some(v) = map.get(&name) {
                    let disp = v.display();
                    if disp != "unspecified" {
                        write_line(&mut out, &path_out, &name, disp, args.nul)?;
                    }
                }
            }
            continue;
        }

        for a in &attrs {
            if a == "builtin_objectmode" {
                let mode = if args.cached {
                    index_cached
                        .as_ref()
                        .and_then(|i| builtin_objectmode_index(i, &rel))
                } else {
                    builtin_objectmode_worktree(&repo, &rel)
                };
                let val = mode.unwrap_or_else(|| "unspecified".to_string());
                write_line(&mut out, &path_out, a, &val, args.nul)?;
                continue;
            }
            let val = match map.get(a) {
                Some(v) => v.display().to_string(),
                None => "unspecified".to_string(),
            };
            write_line(&mut out, &path_out, a, &val, args.nul)?;
        }
    }

    Ok(())
}

fn write_line(out: &mut dyn Write, path_out: &str, attr: &str, val: &str, nul: bool) -> Result<()> {
    if nul {
        write!(out, "{path_out}\0{attr}\0{val}\0")?;
    } else {
        writeln!(out, "{path_out}: {attr}: {val}")?;
    }
    Ok(())
}
