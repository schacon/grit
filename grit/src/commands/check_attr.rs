//! `grit check-attr` — display gitattributes information.
//!
//! Argument parsing follows Git `builtin/check-attr.c` manually: clap cannot preserve a lone `--`
//! after `--stdin`, which changes whether trailing tokens are attributes or pathspecs.

use anyhow::{bail, Context, Result};
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

struct CheckAttrOptions {
    all: bool,
    stdin_mode: bool,
    cached: bool,
    source: Option<String>,
    nul: bool,
    positionals: Vec<String>,
}

fn parse_check_attr_argv(rest: &[String]) -> Result<CheckAttrOptions> {
    let mut all = false;
    let mut stdin_mode = false;
    let mut cached = false;
    let mut source: Option<String> = None;
    let mut nul = false;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 0usize;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "-a" | "--all" => {
                all = true;
                i += 1;
            }
            "--stdin" => {
                stdin_mode = true;
                i += 1;
            }
            "--cached" => {
                cached = true;
                i += 1;
            }
            "-Z" => {
                nul = true;
                i += 1;
            }
            "--source" => {
                i += 1;
                let Some(v) = rest.get(i) else {
                    bail!("a value is required for '--source <TREEISH>' but none was supplied");
                };
                source = Some(v.clone());
                i += 1;
            }
            s if s.starts_with("--source=") => {
                source = Some(s["--source=".len()..].to_string());
                i += 1;
            }
            s if s.starts_with('-') && s != "-" && s != "--" => {
                bail!("unknown option '{s}'");
            }
            _ => {
                positionals.push(rest[i].clone());
                i += 1;
            }
        }
    }

    Ok(CheckAttrOptions {
        all,
        stdin_mode,
        cached,
        source,
        nul,
        positionals,
    })
}

/// Split positionals like Git `cmd_check_attr` after `parse_options`.
fn split_attrs_paths(
    all: bool,
    stdin_mode: bool,
    positionals: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let doubledash = positionals.iter().position(|a| a == "--");

    if all {
        if doubledash.is_some_and(|dd| dd >= 1) {
            bail!("usage: Attributes and --all both specified");
        }
        let paths = if let Some(dd) = doubledash {
            positionals[dd + 1..].to_vec()
        } else {
            positionals.to_vec()
        };
        return Ok((Vec::new(), paths));
    }

    if doubledash == Some(0) {
        bail!("usage: missing attribute name");
    }

    let (attrs, paths) = if let Some(dd) = doubledash {
        let attrs = positionals[..dd].to_vec();
        let paths = positionals[dd + 1..].to_vec();
        (attrs, paths)
    } else if stdin_mode {
        (positionals.to_vec(), Vec::new())
    } else {
        let mut attrs = positionals.to_vec();
        let paths = if attrs.len() >= 2 {
            attrs.split_off(1)
        } else {
            Vec::new()
        };
        (attrs, paths)
    };

    if attrs.is_empty() {
        bail!("usage: missing attribute name");
    }
    for a in &attrs {
        if a.is_empty() {
            bail!("usage: empty attribute name");
        }
    }
    if !stdin_mode && paths.is_empty() {
        bail!("usage: missing pathspec");
    }

    Ok((attrs, paths))
}

fn load_parsed_for_run(
    repo: &Repository,
    source: Option<&str>,
    cached: bool,
) -> Result<ParsedGitAttributes> {
    let (treeish, ignore_bad_tree) = resolve_attr_treeish(repo, source)?;

    if let Some(spec) = treeish.filter(|s| !s.is_empty()) {
        match resolve_tree_oid(repo, &spec) {
            Ok(oid) => {
                return load_gitattributes_from_tree(&repo.odb, &oid)
                    .context("load tree attributes");
            }
            Err(_) if ignore_bad_tree => {}
            Err(_) => {
                bail!("fatal: bad --attr-source or GIT_ATTR_SOURCE");
            }
        }
    }

    if cached {
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

fn write_line(out: &mut dyn Write, path_out: &str, attr: &str, val: &str, nul: bool) -> Result<()> {
    if nul {
        write!(out, "{path_out}\0{attr}\0{val}\0")?;
    } else {
        writeln!(out, "{path_out}: {attr}: {val}")?;
    }
    Ok(())
}

/// Run `check-attr` from argv after the subcommand (matches Git `cmd_check_attr` argv).
pub fn run_from_argv(rest: &[String]) -> Result<()> {
    let opts = parse_check_attr_argv(rest)?;
    let (attrs, mut paths) = split_attrs_paths(opts.all, opts.stdin_mode, &opts.positionals)?;
    if opts.stdin_mode && !paths.is_empty() {
        bail!("usage: pathspec with --stdin");
    }

    if opts.stdin_mode {
        let mut stdin = io::stdin().lock();
        let mut line = String::new();
        while stdin.read_line(&mut line)? > 0 {
            let p = line.trim_end_matches(['\r', '\n']);
            if !p.is_empty() {
                paths.push(p.to_string());
            }
            line.clear();
        }
        if attrs.is_empty() && !opts.all {
            bail!("usage: missing attribute name");
        }
        if paths.is_empty() {
            bail!("usage: missing pathspec");
        }
        for a in &attrs {
            if a.is_empty() {
                bail!("usage: empty attribute name");
            }
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    let parsed = load_parsed_for_run(&repo, opts.source.as_deref(), opts.cached)?;

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

        if opts.all {
            let mut names: Vec<String> = map.keys().cloned().collect();
            names.sort();
            for name in names {
                if let Some(v) = map.get(&name) {
                    let disp = v.display();
                    if disp != "unspecified" {
                        write_line(&mut out, &path_out, &name, disp, opts.nul)?;
                    }
                }
            }
            continue;
        }

        for a in &attrs {
            if a == "builtin_objectmode" {
                let mode = if opts.cached {
                    index_cached
                        .as_ref()
                        .and_then(|i| builtin_objectmode_index(i, &rel))
                } else {
                    builtin_objectmode_worktree(&repo, &rel)
                };
                let val = mode.unwrap_or_else(|| "unspecified".to_string());
                write_line(&mut out, &path_out, a, &val, opts.nul)?;
                continue;
            }
            let val = match map.get(a) {
                Some(v) => v.display().to_string(),
                None => "unspecified".to_string(),
            };
            write_line(&mut out, &path_out, a, &val, opts.nul)?;
        }
    }

    Ok(())
}
