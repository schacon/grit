//! `grit rev-parse` - pick out and massage revision parameters.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::error::Error as LibError;
use grit_lib::merge_base;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    abbreviate_object_id, abbreviate_ref_name, ambiguous_object_hint_lines, discover_optional,
    is_inside_git_dir, is_inside_work_tree, list_all_abbrev_matches, parse_peel_suffix,
    peel_to_commit_for_merge_base, resolve_revision, resolve_revision_for_range_end,
    resolve_revision_without_index_dwim, show_prefix, split_double_dot_range,
    split_triple_dot_range, symbolic_full_name, to_relative_path,
};
use std::env;

/// Arguments for `grit rev-parse`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `rev-parse` with argv as passed after the subcommand (preserves `--` for path separation).
///
/// Clap strips `--` from positional lists; `git rev-parse` relies on it, so the main binary
/// bypasses clap for this command and forwards raw args here.
pub fn run_with_raw_args(rest: &[String]) -> Result<()> {
    run(Args {
        args: rest.to_vec(),
    })
}

/// Run `grit rev-parse`.
pub fn run(args: Args) -> Result<()> {
    // Handle --parseopt mode: parse option spec from stdin, emit parsed args
    if args.args.first().map(|s| s.as_str()) == Some("--parseopt") {
        return run_parseopt(&args.args[1..]);
    }

    let cwd = env::current_dir().context("failed to read current directory")?;

    // Global modifier flags (these modify behavior but don't produce output themselves)
    let mut verify = false;
    let mut quiet = false;
    let mut sq_quote = false;
    let mut short_len: Option<usize> = None;
    let mut show_symbolic_full_name = false;
    let mut abbrev_ref = false;
    let mut prefix: Option<String> = None;
    let mut default_rev: Option<String> = None;
    let mut revs_only = false;
    let mut no_revs = false;
    let mut no_flags = false;
    let mut sq_output = false;

    // Collect ordered actions for sequential output
    // Each action captures the flag state at time of parsing
    #[derive(Debug)]
    enum Action {
        ShowIsInsideWorkTree,
        ShowIsInsideGitDir,
        ShowIsBare,
        ShowIsShallow,
        ShowToplevel,
        ShowPrefix,
        ShowCdup,
        ShowGitDir,
        ShowGitCommonDir,
        ShowAbsoluteGitDir,
        ShowRefFormat,
        ShowObjectFormat(String),
        GitPath(String),
        All,
        Branches(Option<String>),
        Tags(Option<String>),
        Remotes(Option<String>),
        Glob(String),
        Exclude(String),
        LocalEnvVars,
        ResolveGitDir(String),
        Revision(String, bool, bool), // (rev_spec, symbolic_full_name, strict_before_first_dd)
        ForcedPath(String),
        PathSeparator,
        Literal(String),
        Disambiguate(String),
    }

    let mut actions: Vec<Action> = Vec::new();
    let mut end_of_options = false;
    let mut saw_path_separator = false;
    let first_path_sep_dd = args.args.iter().position(|a| a == "--");

    // First pass: parse all arguments and build ordered action list
    let mut i = 0usize;
    while i < args.args.len() {
        let arg = &args.args[i];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            saw_path_separator = true;
            actions.push(Action::PathSeparator);
            i += 1;
            continue;
        }
        if end_of_options {
            if arg == "--" {
                saw_path_separator = true;
                actions.push(Action::PathSeparator);
                i += 1;
                continue;
            }
            if saw_path_separator {
                actions.push(Action::ForcedPath(arg.clone()));
            } else {
                let strict = first_path_sep_dd.is_some_and(|dd| i < dd);
                actions.push(Action::Revision(
                    arg.clone(),
                    show_symbolic_full_name,
                    strict,
                ));
            }
            i += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') {
            if arg == "--path-format=absolute" {
                // --path-format=absolute: output absolute paths; currently our default
                // for git-dir etc., so this is a no-op
                i += 1;
                continue;
            } else if arg == "--path-format=relative" {
                // Relative paths: no-op (we handle per command)
                i += 1;
                continue;
            } else if arg == "--verify" {
                verify = true;
            } else if arg == "--quiet" || arg == "-q" {
                quiet = true;
            } else if arg == "--is-inside-work-tree" {
                actions.push(Action::ShowIsInsideWorkTree);
            } else if arg == "--is-inside-git-dir" {
                actions.push(Action::ShowIsInsideGitDir);
            } else if arg == "--is-shallow-repository" {
                actions.push(Action::ShowIsShallow);
            } else if arg == "--is-bare-repository" {
                actions.push(Action::ShowIsBare);
            } else if arg == "--show-toplevel" {
                actions.push(Action::ShowToplevel);
            } else if arg == "--show-prefix" {
                actions.push(Action::ShowPrefix);
            } else if arg == "--show-cdup" {
                actions.push(Action::ShowCdup);
            } else if arg == "--symbolic-full-name" {
                show_symbolic_full_name = true;
            } else if arg == "--abbrev-ref" {
                abbrev_ref = true;
            } else if arg == "--git-dir" {
                actions.push(Action::ShowGitDir);
            } else if arg == "--git-common-dir" {
                actions.push(Action::ShowGitCommonDir);
            } else if arg == "--absolute-git-dir" {
                actions.push(Action::ShowAbsoluteGitDir);
            } else if arg == "--git-path" {
                i += 1;
                let path_arg = args
                    .args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--git-path requires an argument"))?;
                actions.push(Action::GitPath(path_arg.clone()));
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
                short_len = Some(parse_short_len(value)?);
            } else if arg == "--short" {
                // Default short length will be resolved later from core.abbrev
                short_len = Some(0);
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
                actions.push(Action::Literal("--end-of-options".to_owned()));
            } else if arg == "--branches" {
                actions.push(Action::Branches(None));
            } else if let Some(pattern) = arg.strip_prefix("--branches=") {
                actions.push(Action::Branches(Some(pattern.to_owned())));
            } else if arg == "--tags" {
                actions.push(Action::Tags(None));
            } else if let Some(pattern) = arg.strip_prefix("--tags=") {
                actions.push(Action::Tags(Some(pattern.to_owned())));
            } else if let Some(pattern) = arg.strip_prefix("--glob=") {
                actions.push(Action::Glob(normalize_glob_pattern(pattern)));
            } else if arg == "--glob" {
                i += 1;
                if let Some(pattern) = args.args.get(i) {
                    actions.push(Action::Glob(normalize_glob_pattern(pattern)));
                }
            } else if arg == "--remotes" {
                actions.push(Action::Remotes(None));
            } else if let Some(pattern) = arg.strip_prefix("--remotes=") {
                actions.push(Action::Remotes(Some(pattern.to_owned())));
            } else if arg == "--all" {
                actions.push(Action::All);
            } else if let Some(pattern) = arg.strip_prefix("--exclude=") {
                actions.push(Action::Exclude(pattern.to_owned()));
            } else if arg == "--exclude" {
                i += 1;
                if let Some(pattern) = args.args.get(i) {
                    actions.push(Action::Exclude(pattern.to_owned()));
                }
            } else if arg.starts_with("--exclude-hidden=") {
                // --exclude-hidden=fetch/receive: accepted but currently a no-op
                // (we don't have transfer.hideRefs support yet)
            } else if arg == "--show-ref-format" {
                actions.push(Action::ShowRefFormat);
            } else if let Some(mode) = arg.strip_prefix("--show-object-format=") {
                actions.push(Action::ShowObjectFormat(mode.to_owned()));
            } else if arg == "--show-object-format" {
                actions.push(Action::ShowObjectFormat("storage".to_owned()));
            } else if arg == "--sq-quote" {
                sq_quote = true;
            } else if arg == "--sq" {
                sq_output = true;
            } else if let Some(pfx) = arg.strip_prefix("--disambiguate=") {
                actions.push(Action::Disambiguate(pfx.to_owned()));
            } else if arg == "--disambiguate" {
                i += 1;
                let pfx = args
                    .args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--disambiguate requires a prefix argument"))?;
                actions.push(Action::Disambiguate(pfx.clone()));
            } else if arg == "--local-env-vars" {
                actions.push(Action::LocalEnvVars);
            } else if arg == "--resolve-git-dir" {
                i += 1;
                let path_arg = args
                    .args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--resolve-git-dir requires an argument"))?;
                actions.push(Action::ResolveGitDir(path_arg.clone()));
            } else if arg == "--revs-only" {
                revs_only = true;
            } else if arg == "--no-revs" {
                no_revs = true;
            } else if arg == "--no-flags" {
                no_flags = true;
            } else if no_flags {
                // In --no-flags mode, silently skip unknown flags
            } else if no_revs {
                // In --no-revs mode, output unknown flags as non-rev output
                println!("{arg}");
            } else {
                bail!("unsupported option: {arg}");
            }
            i += 1;
            continue;
        }
        if saw_path_separator {
            actions.push(Action::ForcedPath(arg.clone()));
        } else {
            let strict = first_path_sep_dd.is_some_and(|dd| i < dd);
            actions.push(Action::Revision(
                arg.clone(),
                show_symbolic_full_name,
                strict,
            ));
        }
        i += 1;
    }

    // --sq-quote: shell-quote all non-flag args and exit
    if sq_quote {
        let mut out = String::new();
        for action in &actions {
            if let Action::Revision(rev, _, _) = action {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&sq_quote_str(rev));
            }
        }
        println!("{out}");
        return Ok(());
    }

    // --verify mode: exactly one revision, output its OID
    if verify {
        let revisions: Vec<&str> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Revision(r, _, _) => Some(r.as_str()),
                _ => None,
            })
            .collect();
        let mut rev_list = revisions;
        if rev_list.is_empty() {
            if let Some(default_name) = default_rev.as_deref() {
                rev_list = vec![default_name];
            }
        }
        if rev_list.len() != 1 {
            return fail_verify(quiet, false);
        }
        let repo = discover_optional(None)?;
        let Some(current) = repo.as_ref() else {
            return fail_verify(quiet, false);
        };
        let spec = rev_list[0];
        if let Some((left, right)) = split_double_dot_range(spec) {
            let left_oid = match if left.is_empty() {
                resolve_revision_for_range_end(current, "HEAD")
            } else {
                resolve_revision_for_range_end(current, left)
            } {
                Ok(oid) => oid,
                Err(e) => return fail_verify_resolve(quiet, &e, Some(current)),
            };
            let right_oid = match if right.is_empty() {
                resolve_revision_for_range_end(current, "HEAD")
            } else {
                resolve_revision_for_range_end(current, right)
            } {
                Ok(oid) => oid,
                Err(e) => return fail_verify_resolve(quiet, &e, Some(current)),
            };
            if let Some(mut len) = short_len {
                if len == 0 {
                    use grit_lib::config::ConfigSet;
                    let config = ConfigSet::load(Some(&current.git_dir), false)
                        .unwrap_or_else(|_| ConfigSet::new());
                    len = config
                        .get("core.abbrev")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(7);
                }
                println!("{}", abbreviate_object_id(current, left_oid, len)?);
                println!("^{}", abbreviate_object_id(current, right_oid, len)?);
            } else {
                println!("{left_oid}");
                println!("^{right_oid}");
            }
            return Ok(());
        }
        if let Some((left, right)) = split_triple_dot_range(spec) {
            let left_tip = if left.is_empty() {
                resolve_revision_for_range_end(current, "HEAD")?
            } else {
                resolve_revision_for_range_end(current, left)?
            };
            let right_tip = if right.is_empty() {
                resolve_revision_for_range_end(current, "HEAD")?
            } else {
                resolve_revision_for_range_end(current, right)?
            };
            let left_commit = peel_to_commit_for_merge_base(current, left_tip)?;
            let right_commit = peel_to_commit_for_merge_base(current, right_tip)?;
            let bases =
                merge_base::merge_bases_first_vs_rest(current, left_commit, &[right_commit])?;
            let Some(mb) = bases.into_iter().next() else {
                return fail_verify(quiet, false);
            };
            if let Some(mut len) = short_len {
                if len == 0 {
                    use grit_lib::config::ConfigSet;
                    let config = ConfigSet::load(Some(&current.git_dir), false)
                        .unwrap_or_else(|_| ConfigSet::new());
                    len = config
                        .get("core.abbrev")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(7);
                }
                println!("{}", abbreviate_object_id(current, left_tip, len)?);
                println!("{}", abbreviate_object_id(current, right_tip, len)?);
                println!("^{}", abbreviate_object_id(current, mb, len)?);
            } else {
                println!("{left_tip}");
                println!("{right_tip}");
                println!("^{mb}");
            }
            return Ok(());
        }
        let oid = match resolve_revision(current, spec) {
            Ok(oid) => oid,
            Err(e) => return fail_verify_resolve(quiet, &e, Some(current)),
        };
        if let Some(mut len) = short_len {
            if len == 0 {
                use grit_lib::config::ConfigSet;
                let config = ConfigSet::load(Some(&current.git_dir), false)
                    .unwrap_or_else(|_| ConfigSet::new());
                len = config
                    .get("core.abbrev")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(7);
            }
            println!("{}", abbreviate_object_id(current, oid, len)?);
        } else {
            println!("{oid}");
        }
        return Ok(());
    }

    // Apply --default: if no Revision actions exist, inject the default
    if let Some(ref def) = default_rev {
        let has_revision = actions
            .iter()
            .any(|a| matches!(a, Action::Revision(_, _, _)));
        if !has_revision {
            actions.push(Action::Revision(
                def.clone(),
                show_symbolic_full_name,
                false,
            ));
        }
    }

    // Check if we have any actions at all
    let has_output_actions = actions.iter().any(|a| !matches!(a, Action::PathSeparator));
    if !has_output_actions {
        // Match git behavior: plain `git rev-parse` still requires repository
        // setup and should fail for invalid/missing gitdir state.
        let _ = Repository::discover(None)?;
        return Ok(());
    }

    let repo = discover_optional(None)?;

    // Resolve default --short length from core.abbrev config if not explicitly given
    if short_len == Some(0) {
        let default_abbrev = if let Some(ref r) = repo {
            use grit_lib::config::ConfigSet;
            let config =
                ConfigSet::load(Some(&r.git_dir), false).unwrap_or_else(|_| ConfigSet::new());
            config
                .get("core.abbrev")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(7)
        } else {
            7
        };
        short_len = Some(default_abbrev);
    }

    // Process actions in order
    let mut saw_path_sep_output = false;
    let mut exclude_patterns: Vec<String> = Vec::new();
    let _ = sq_output; // --sq accepted but output quoting deferred to callers
    let mut seen_ambiguous_revision = false;
    let mut deferred_fatal_stderr: Option<String> = None;
    for action in &actions {
        match action {
            Action::Literal(s) => {
                println!("{s}");
            }
            Action::Disambiguate(pfx) => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                let mut oids = list_all_abbrev_matches(current, pfx)?;
                oids.sort_by_key(|o| o.to_hex());
                oids.dedup();
                for oid in oids {
                    println!("{}", oid.to_hex());
                }
            }
            Action::ShowIsInsideWorkTree => {
                let inside = repo
                    .as_ref()
                    .map(|current| is_inside_work_tree(current, &cwd))
                    .unwrap_or(false);
                println!("{}", if inside { "true" } else { "false" });
            }
            Action::ShowIsInsideGitDir => {
                let inside = repo
                    .as_ref()
                    .map(|current| is_inside_git_dir(current, &cwd))
                    .unwrap_or(false);
                println!("{}", if inside { "true" } else { "false" });
            }
            Action::ShowIsShallow => {
                let is_shallow = repo
                    .as_ref()
                    .map(|r| r.git_dir.join("shallow").exists())
                    .unwrap_or(false);
                println!("{}", if is_shallow { "true" } else { "false" });
            }
            Action::ShowIsBare => {
                let bare = repo
                    .as_ref()
                    .map(|current| current.is_bare())
                    .unwrap_or(false);
                println!("{}", if bare { "true" } else { "false" });
            }
            Action::ShowToplevel => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                let Some(work_tree) = &current.work_tree else {
                    bail!("this operation must be run in a work tree");
                };
                println!("{}", work_tree.display());
            }
            Action::ShowPrefix => {
                let Some(current) = repo.as_ref() else {
                    eprintln!("error: not a git repository (or any of the parent directories)");
                    std::process::exit(128);
                };
                println!("{}", show_prefix(current, &cwd));
            }
            Action::ShowCdup => {
                let Some(current) = repo.as_ref() else {
                    eprintln!("error: not a git repository (or any of the parent directories)");
                    std::process::exit(128);
                };
                let pfx = show_prefix(current, &cwd);
                if pfx.is_empty() {
                    println!();
                } else {
                    let depth = pfx.trim_end_matches('/').matches('/').count() + 1;
                    let cdup: String = "../".repeat(depth);
                    println!("{cdup}");
                }
            }
            Action::ShowGitDir => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                let git_dir = current.git_dir.as_path();
                if cwd == git_dir {
                    println!(".");
                } else if cwd.starts_with(git_dir) {
                    // Inside the git directory (e.g. `.git/hooks`): Git prints an absolute path.
                    println!("{}", git_dir.display());
                } else if let Ok(rel) = git_dir.strip_prefix(&cwd) {
                    println!("{}", rel.display());
                } else {
                    println!("{}", to_relative_path(git_dir, &cwd));
                }
            }
            Action::ShowGitCommonDir => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                let common_git_dir =
                    refs::common_dir(&current.git_dir).unwrap_or_else(|| current.git_dir.clone());
                let common_c = common_git_dir
                    .canonicalize()
                    .unwrap_or_else(|_| common_git_dir.clone());
                println!("{}", common_c.display());
            }
            Action::ShowAbsoluteGitDir => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                println!("{}", current.git_dir.display());
            }
            Action::ShowRefFormat => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                let config_path = current.git_dir.join("config");
                let format = if let Ok(content) = std::fs::read_to_string(&config_path) {
                    let mut in_ext = false;
                    let mut found = String::from("files");
                    for line in content.lines() {
                        let t = line.trim();
                        if t.starts_with('[') {
                            in_ext = t.eq_ignore_ascii_case("[extensions]");
                            continue;
                        }
                        if in_ext {
                            if let Some((k, v)) = t.split_once('=') {
                                if k.trim().eq_ignore_ascii_case("refstorage") {
                                    found = v.trim().to_lowercase();
                                }
                            }
                        }
                    }
                    found
                } else {
                    "files".to_string()
                };
                println!("{format}");
            }
            Action::ShowObjectFormat(mode) => {
                let Some(current) = repo.as_ref() else {
                    bail!("not a git repository (or any of the parent directories)");
                };
                let (storage_fmt, compat_fmt) = read_object_format_from_config(&current.git_dir);
                match mode.as_str() {
                    "storage" | "input" | "output" => println!("{storage_fmt}"),
                    "compat" => {
                        if let Some(c) = compat_fmt {
                            println!("{c}");
                        } else {
                            println!();
                        }
                    }
                    other => {
                        bail!("unknown mode for --show-object-format: {other}");
                    }
                }
            }
            Action::GitPath(path_arg) => {
                if let Some(current) = repo.as_ref() {
                    // Use original path_arg for output, normalized for matching
                    let path_arg_for_match = {
                        let mut s = path_arg.clone();
                        while s.contains("//") {
                            s = s.replace("//", "/");
                        }
                        s = s.trim_start_matches('/').to_owned();
                        s
                    };
                    let path_arg_out = path_arg; // original for output
                    let path_arg = &path_arg_for_match; // normalized for matching

                    // Check GIT_COMMON_DIR: certain paths are relative to common dir
                    // Worktree-local paths (NOT common):
                    let is_worktree_local = {
                        let p = path_arg.as_str();
                        p == "HEAD"
                            || p == "index"
                            || p == "config.worktree"
                            || p == "MERGE_HEAD"
                            || p == "CHERRY_PICK_HEAD"
                            || p == "REVERT_HEAD"
                            || p == "BISECT_LOG"
                            || p == "BISECT_TERMS"
                            || p == "BISECT_EXPECTED_REV"
                            || p == "AUTO_MERGE"
                            || p == "SQUASH_MSG"
                            || p == "MERGE_MSG"
                            || p.starts_with("rebase-")
                            || p.starts_with("sequencer")
                            || p == "logs/HEAD"
                            || p.starts_with("logs/HEAD.")
                            || p.starts_with("logs/FETCH_HEAD")
                            || p == "refs/bisect"
                            || p.starts_with("refs/bisect/")
                            || p == "logs/refs/bisect"
                            || p.starts_with("logs/refs/bisect/")
                            || p == "info/sparse-checkout"
                    };
                    if let Ok(common_dir) = std::env::var("GIT_COMMON_DIR") {
                        if !is_worktree_local {
                            let common_prefixes = [
                                "objects",
                                "refs",
                                "packed-refs",
                                "info",
                                "config",
                                "ORIG_HEAD",
                                "FETCH_HEAD",
                                "logs",
                                "shallow",
                                "remotes",
                                "branches",
                                "hooks",
                                "common",
                            ];
                            let is_common = common_prefixes
                                .iter()
                                .any(|p| path_arg == p || path_arg.starts_with(&format!("{}/", p)));
                            if is_common {
                                println!("{}/{}", common_dir, path_arg_out);
                                continue;
                            }
                        }
                    }
                    // Check env var overrides
                    let env_override = if path_arg == "info/grafts" {
                        std::env::var("GIT_GRAFT_FILE").ok()
                    } else if path_arg == "index" {
                        std::env::var("GIT_INDEX_FILE").ok()
                    } else if path_arg == "objects" {
                        std::env::var("GIT_OBJECT_DIRECTORY").ok()
                    } else if let Some(remainder) = path_arg.strip_prefix("objects/") {
                        if let Ok(obj_dir) = std::env::var("GIT_OBJECT_DIRECTORY") {
                            Some(format!("{obj_dir}/{remainder}"))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(env_val) = env_override {
                        println!("{env_val}");
                        continue;
                    }
                    let resolved = if path_arg == "hooks" || path_arg.starts_with("hooks/") {
                        let config =
                            grit_lib::config::ConfigSet::load(Some(&current.git_dir), true)?;
                        if let Some(hooks_path) = config.get("core.hooksPath") {
                            let hooks_dir = std::path::Path::new(&hooks_path);
                            if path_arg == "hooks" {
                                hooks_dir.to_path_buf()
                            } else {
                                let remainder = &path_arg["hooks/".len()..];
                                hooks_dir.join(remainder)
                            }
                        } else {
                            current.git_dir.join(path_arg)
                        }
                    } else {
                        // Some paths are stored in the common dir (shared across worktrees)
                        let common_paths = [
                            "objects",
                            "refs",
                            "packed-refs",
                            "info",
                            "config",
                            "ORIG_HEAD",
                            "FETCH_HEAD",
                            "logs",
                            "shallow",
                        ];
                        let use_common = common_paths
                            .iter()
                            .any(|p| path_arg == *p || path_arg.starts_with(&format!("{}/", p)));
                        if use_common {
                            let common = refs::common_dir(&current.git_dir)
                                .unwrap_or_else(|| current.git_dir.clone());
                            common.join(path_arg)
                        } else {
                            current.git_dir.join(path_arg)
                        }
                    };
                    // Output relative path when possible (relative to cwd)
                    // Use original path_arg_out to preserve double slashes etc.
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let git_dir_rel = if let Ok(rel) = current.git_dir.strip_prefix(&cwd) {
                        rel.display().to_string()
                    } else {
                        // Compute relative path from cwd to git_dir
                        let git_abs = current
                            .git_dir
                            .canonicalize()
                            .unwrap_or_else(|_| current.git_dir.clone());
                        let cwd_abs = cwd.canonicalize().unwrap_or(cwd.clone());
                        let git_comps: Vec<_> = git_abs.components().collect();
                        let cwd_comps: Vec<_> = cwd_abs.components().collect();
                        let common = git_comps
                            .iter()
                            .zip(cwd_comps.iter())
                            .take_while(|(a, b)| a == b)
                            .count();
                        let up = cwd_comps.len() - common;
                        let mut result = std::path::PathBuf::new();
                        for _ in 0..up {
                            result.push("..");
                        }
                        for comp in &git_comps[common..] {
                            result.push(comp.as_os_str());
                        }
                        result.display().to_string()
                    };
                    // If the resolved path is under git_dir, use git_dir_rel + path_arg_out.
                    // When cwd is the bare repo root, `git_dir_rel` is empty; avoid a leading `/`.
                    let output = if resolved.starts_with(&current.git_dir) {
                        if git_dir_rel.is_empty() {
                            path_arg_out.clone()
                        } else {
                            format!("{git_dir_rel}/{path_arg_out}")
                        }
                    } else if let Ok(rel) = resolved.strip_prefix(&cwd) {
                        rel.display().to_string()
                    } else {
                        resolved.display().to_string()
                    };
                    println!("{output}");
                } else {
                    bail!("not a git repository");
                }
            }
            Action::Exclude(pattern) => {
                exclude_patterns.push(pattern.clone());
            }
            Action::All => {
                if let Some(current) = repo.as_ref() {
                    let matching = grit_lib::refs::list_refs(&current.git_dir, "refs/")
                        .context("failed to list refs")?;
                    for (refname, oid) in &matching {
                        if !is_excluded(refname, &exclude_patterns) {
                            println!("{oid}");
                        }
                    }
                    exclude_patterns.clear();
                }
            }
            Action::Branches(pattern) => {
                if let Some(current) = repo.as_ref() {
                    let matching = if let Some(pat) = pattern {
                        let full = normalize_ref_pattern("refs/heads/", pat);
                        grit_lib::refs::list_refs_glob(&current.git_dir, &full)
                            .context("failed to list branch refs")?
                    } else {
                        grit_lib::refs::list_refs(&current.git_dir, "refs/heads/")
                            .context("failed to list branch refs")?
                    };
                    for (refname, oid) in &matching {
                        if !is_excluded(refname, &exclude_patterns) {
                            println!("{oid}");
                        }
                    }
                    exclude_patterns.clear();
                }
            }
            Action::Tags(pattern) => {
                if let Some(current) = repo.as_ref() {
                    let matching = if let Some(pat) = pattern {
                        let full = normalize_ref_pattern("refs/tags/", pat);
                        grit_lib::refs::list_refs_glob(&current.git_dir, &full)
                            .context("failed to list tag refs")?
                    } else {
                        grit_lib::refs::list_refs(&current.git_dir, "refs/tags/")
                            .context("failed to list tag refs")?
                    };
                    for (refname, oid) in &matching {
                        if !is_excluded(refname, &exclude_patterns) {
                            println!("{oid}");
                        }
                    }
                    exclude_patterns.clear();
                }
            }
            Action::Remotes(pattern) => {
                if let Some(current) = repo.as_ref() {
                    let matching = if let Some(pat) = pattern {
                        let full = normalize_ref_pattern("refs/remotes/", pat);
                        grit_lib::refs::list_refs_glob(&current.git_dir, &full)
                            .context("failed to list remote refs")?
                    } else {
                        grit_lib::refs::list_refs(&current.git_dir, "refs/remotes/")
                            .context("failed to list remote refs")?
                    };
                    for (refname, oid) in &matching {
                        if !is_excluded(refname, &exclude_patterns) {
                            println!("{oid}");
                        }
                    }
                    exclude_patterns.clear();
                }
            }
            Action::Glob(full) => {
                if let Some(current) = repo.as_ref() {
                    let matching = grit_lib::refs::list_refs_glob(&current.git_dir, full)
                        .context("failed to list refs")?;
                    for (refname, oid) in &matching {
                        if !is_excluded(refname, &exclude_patterns) {
                            println!("{oid}");
                        }
                    }
                }
            }
            Action::LocalEnvVars => {
                for var in &[
                    "GIT_DIR",
                    "GIT_WORK_TREE",
                    "GIT_OBJECT_DIRECTORY",
                    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                    "GIT_INDEX_FILE",
                    "GIT_GRAFT_FILE",
                    "GIT_COMMON_DIR",
                ] {
                    println!("{var}");
                }
            }
            Action::ResolveGitDir(path_arg) => {
                let p = std::path::Path::new(path_arg);
                if p.is_dir() && p.join("HEAD").exists() {
                    let resolved = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                    println!("{}", resolved.display());
                } else if p.is_file() {
                    let content = std::fs::read_to_string(p)
                        .with_context(|| format!("cannot read '{}'", p.display()))?;
                    let mut found = false;
                    for line in content.lines() {
                        if let Some(rest) = line.strip_prefix("gitdir:") {
                            let rel = rest.trim();
                            let git_dir = if std::path::Path::new(rel).is_absolute() {
                                std::path::PathBuf::from(rel)
                            } else {
                                p.parent().unwrap_or(std::path::Path::new(".")).join(rel)
                            };
                            let resolved = git_dir.canonicalize().unwrap_or(git_dir);
                            println!("{}", resolved.display());
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        bail!("not a gitdir: {path_arg}");
                    }
                } else {
                    bail!("not a valid directory: {path_arg}");
                }
            }
            Action::Revision(rev, rev_symbolic_full_name, strict_before_first_dd) => {
                let Some(current) = repo.as_ref() else {
                    if quiet {
                        std::process::exit(1);
                    }
                    bail!("not a git repository (or any of the parent directories)");
                };
                if seen_ambiguous_revision {
                    println!("{rev}");
                    if rev.contains(':') && !rev.starts_with(':') {
                        deferred_fatal_stderr = Some(format!(
                            "fatal: {rev}: no such path in the working tree.\n\
Use 'git <command> -- <path>...' to specify paths that do not exist locally."
                        ));
                    }
                    continue;
                }
                // Use ONLY the per-action flag (not global, to support mixed usage)
                let use_symbolic = *rev_symbolic_full_name;

                if abbrev_ref {
                    // --abbrev-ref: resolve to symbolic name and abbreviate
                    if let Some(full) = symbolic_full_name(current, rev) {
                        println!("{}", abbreviate_ref_name(&full));
                        continue;
                    }
                    // Fall through to try resolving as OID and printing as-is
                }

                if use_symbolic {
                    if let Some(full) = symbolic_full_name(current, rev) {
                        println!("{full}");
                        continue;
                    }
                }

                let rewritten = rewrite_tree_path_spec(rev, prefix.as_deref());
                if let Some((left, right)) = split_triple_dot_range(&rewritten) {
                    if no_revs {
                        continue;
                    }
                    let left_tip = if left.is_empty() {
                        resolve_revision_for_range_end(current, "HEAD")?
                    } else {
                        resolve_revision_for_range_end(current, left)?
                    };
                    let right_tip = if right.is_empty() {
                        resolve_revision_for_range_end(current, "HEAD")?
                    } else {
                        resolve_revision_for_range_end(current, right)?
                    };
                    let left_commit = peel_to_commit_for_merge_base(current, left_tip)?;
                    let right_commit = peel_to_commit_for_merge_base(current, right_tip)?;
                    let bases = merge_base::merge_bases_first_vs_rest(
                        current,
                        left_commit,
                        &[right_commit],
                    )?;
                    let Some(mb) = bases.into_iter().next() else {
                        bail!("no merge base for '{rewritten}'");
                    };
                    if let Some(len) = short_len {
                        println!("{}", abbreviate_object_id(current, left_tip, len)?);
                        println!("{}", abbreviate_object_id(current, right_tip, len)?);
                        println!("^{}", abbreviate_object_id(current, mb, len)?);
                    } else {
                        println!("{left_tip}");
                        println!("{right_tip}");
                        println!("^{mb}");
                    }
                    continue;
                }
                if let Some((left, right)) = split_double_dot_range(&rewritten) {
                    if no_revs {
                        continue;
                    }
                    let left_oid = if left.is_empty() {
                        resolve_revision_for_range_end(current, "HEAD")?
                    } else {
                        resolve_revision_for_range_end(current, left)?
                    };
                    let right_oid = if right.is_empty() {
                        resolve_revision_for_range_end(current, "HEAD")?
                    } else {
                        resolve_revision_for_range_end(current, right)?
                    };
                    if left.is_empty() && right.is_empty() {
                        println!("..");
                    } else if let Some(len) = short_len {
                        println!("{}", abbreviate_object_id(current, left_oid, len)?);
                        println!("^{}", abbreviate_object_id(current, right_oid, len)?);
                    } else {
                        println!("{left_oid}");
                        println!("^{right_oid}");
                    }
                    continue;
                }
                if looks_like_shell_glob(&rewritten) {
                    if no_revs {
                        continue;
                    }
                    match resolve_revision_without_index_dwim(current, &rewritten) {
                        Ok(oid) => {
                            if let Some(len) = short_len {
                                println!("{}", abbreviate_object_id(current, oid, len)?);
                            } else {
                                println!("{oid}");
                            }
                        }
                        Err(_) => {
                            if revs_only {
                                continue;
                            }
                            println!("{rewritten}");
                        }
                    }
                    continue;
                }
                match resolve_revision_without_index_dwim(current, &rewritten) {
                    Ok(oid) => {
                        if no_revs {
                            // --no-revs: skip resolved revisions
                            continue;
                        }
                        if let Some(len) = short_len {
                            println!("{}", abbreviate_object_id(current, oid, len)?);
                        } else {
                            println!("{oid}");
                        }
                    }
                    Err(e) => {
                        if revs_only {
                            // --revs-only: silently skip unresolvable args
                            continue;
                        }
                        let msg = e.to_string();
                        if *strict_before_first_dd && !rev.contains(':') {
                            match &e {
                                LibError::Message(_) | LibError::ObjectNotFound(_) => {
                                    bail!("fatal: bad revision '{rev}'");
                                }
                                _ if msg.contains("ambiguous argument") => {
                                    bail!("fatal: bad revision '{rev}'");
                                }
                                _ => {}
                            }
                        }
                        if matches!(&e, LibError::Message(m) if m.contains("ambiguous argument")) {
                            // With `--short`, match Git: no stdout for the failed rev; exit via
                            // fail_verify after other actions (t9903 bare/orphan prompt).
                            if short_len.is_some() {
                                return fail_verify(quiet, false);
                            }
                            println!("{rev}");
                            seen_ambiguous_revision = true;
                            deferred_fatal_stderr = Some(msg);
                            continue;
                        }
                        let amb_prefix = parse_ambiguous_short_oid(&msg);
                        if let Some(ref pfx) = amb_prefix {
                            print_ambiguous_short_oid_error(current, rev, pfx)?;
                        }
                        if matches!(&e, LibError::Message(_) | LibError::InvalidRef(_)) {
                            return Err(e.into());
                        }
                        if msg.contains("ambiguous") {
                            return Err(anyhow::anyhow!("{msg}"));
                        }
                        // With `--short`, Git does not echo the unresolved spec to stdout; it fails
                        // with "Needed a single revision" (t9903 `__git_ps1` + bare/orphan repos).
                        if short_len.is_some() {
                            return fail_verify(quiet, false);
                        }
                        if no_revs || amb_prefix.is_some() {
                            if let Some(path_prefix) = prefix.as_deref() {
                                println!("{}", apply_prefix_for_forced_path(path_prefix, rev));
                            } else {
                                println!("{rev}");
                            }
                        } else {
                            bail!("fatal: bad revision '{rev}'");
                        }
                    }
                }
            }
            Action::PathSeparator => {
                println!("--");
                saw_path_sep_output = true;
            }
            Action::ForcedPath(path) => {
                if !saw_path_sep_output {
                    println!("--");
                    saw_path_sep_output = true;
                }
                if let Some(path_prefix) = prefix.as_deref() {
                    println!("{}", apply_prefix_for_forced_path(path_prefix, path));
                } else {
                    println!("{path}");
                }
            }
        }
    }
    if let Some(msg) = deferred_fatal_stderr {
        eprintln!("{msg}");
        std::process::exit(128);
    }
    Ok(())
}

fn parse_short_len(raw: &str) -> Result<usize> {
    let parsed = raw
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("invalid --short length: {raw}"))?;
    Ok(parsed.clamp(4, 40))
}

fn fail_verify(quiet: bool, is_reflog_selector: bool) -> Result<()> {
    if quiet {
        std::process::exit(1);
    }
    if is_reflog_selector {
        // Match git behavior for invalid reflog selectors when not quiet.
        bail!("log for '<ref>' has no entries")
    } else {
        bail!("Needed a single revision")
    }
}

fn fail_verify_resolve(
    quiet: bool,
    err: &LibError,
    repo: Option<&grit_lib::repo::Repository>,
) -> Result<()> {
    if quiet {
        std::process::exit(1);
    }
    let msg = err.to_string();
    if msg.contains("only has") && msg.contains("entries") {
        bail!("{msg}");
    }
    if let (LibError::ObjectNotFound(spec), Some(r)) = (err, repo) {
        if spec.contains("-g") && spec.matches('-').count() >= 2 {
            if let Ok(oid) = resolve_revision(r, spec) {
                println!("{oid}");
                return Ok(());
            }
        }
    }
    if matches!(err, LibError::InvalidRef(_) | LibError::Message(_)) {
        bail!("{msg}");
    }
    fail_verify(quiet, false)
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
    // Without `--prefix`, `./` and `../` are resolved by the library relative to cwd; do not
    // normalize here (stripping `./` would wrongly turn `HEAD:./file` into `HEAD:file`).
    let Some(prefix) = prefix else {
        return spec.to_owned();
    };

    let mut joined = String::new();
    joined.push_str(prefix);
    joined.push_str(raw_path);
    let normalized = normalize_slash_path(&joined);
    format!("{treeish}:{normalized}")
}

fn parse_ambiguous_short_oid(message: &str) -> Option<String> {
    let trimmed = message.trim();
    if let Some(rest) = trimmed.strip_prefix("invalid ref: short object ID ") {
        return rest
            .strip_suffix(" is ambiguous")
            .map(std::borrow::ToOwned::to_owned);
    }
    if let Some(rest) = trimmed.strip_prefix("short object ID ") {
        return rest
            .strip_suffix(" is ambiguous")
            .map(std::borrow::ToOwned::to_owned);
    }
    None
}

fn print_ambiguous_short_oid_error(
    repo: &grit_lib::repo::Repository,
    rev: &str,
    short_prefix: &str,
) -> Result<()> {
    let candidates = list_all_abbrev_matches(repo, short_prefix)?;
    if candidates.is_empty() {
        return Err(anyhow::anyhow!(
            "invalid ref: short object ID {} is ambiguous",
            short_prefix
        ));
    }

    let mut typed_count = 0usize;
    let mut bad_oids: Vec<String> = Vec::new();
    for oid in &candidates {
        let oid_hex = oid.to_hex();
        match repo.odb.read(oid) {
            Ok(_) => typed_count += 1,
            Err(_) => bad_oids.push(oid_hex),
        }
    }

    eprintln!("error: short object ID {} is ambiguous", short_prefix);

    if typed_count == 0 {
        eprintln!("fatal: invalid object type");
        std::process::exit(128);
    }

    if !bad_oids.is_empty() {
        for oid_hex in &bad_oids {
            eprintln!("error: inflate: data stream error (incorrect header check)");
            eprintln!("error: unable to unpack {} header", oid_hex);
            eprintln!("error: inflate: data stream error (incorrect header check)");
            eprintln!("error: unable to unpack {} header", oid_hex);
        }
    }

    let peel_filter = parse_peel_suffix(rev).1;
    eprintln!("hint: The candidates are:");
    for line in ambiguous_object_hint_lines(repo, short_prefix, peel_filter)? {
        eprintln!("{line}");
    }

    eprintln!(
        "fatal: ambiguous argument '{}': unknown revision or path not in the working tree.",
        rev
    );
    eprintln!("Use '--' to separate paths from revisions, like this:");
    eprintln!("'git <command> [<revision>...] -- [<file>...]'");
    std::process::exit(128);
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

fn looks_like_shell_glob(spec: &str) -> bool {
    let mut it = spec.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            let _ = it.next();
            continue;
        }
        if matches!(c, '*' | '?' | '[') {
            return true;
        }
    }
    false
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

/// Run `rev-parse --parseopt` mode.
///
/// Reads an option specification from stdin, then parses the arguments
/// that follow `--` against that spec and outputs normalized options.
fn run_parseopt(extra_args: &[String]) -> Result<()> {
    super::rev_parse_parseopt::run_parseopt(extra_args)
}

/// Shell-escape a string for single-quote context.
/// Read `extensions.objectformat` and `extensions.compatobjectformat` from `config`.
///
/// Returns `(storage, compat)` where `storage` defaults to `sha1` when unset, matching Git.
fn read_object_format_from_config(git_dir: &std::path::Path) -> (String, Option<String>) {
    let config_path = git_dir.join("config");
    let Ok(content) = std::fs::read_to_string(&config_path) else {
        return ("sha1".to_owned(), None);
    };
    let mut in_extensions = false;
    let mut object_format: Option<String> = None;
    let mut compat: Option<String> = None;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_extensions = t.eq_ignore_ascii_case("[extensions]");
            continue;
        }
        if !in_extensions {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim().to_lowercase();
        if key.eq_ignore_ascii_case("objectformat") {
            object_format = Some(val);
        } else if key.eq_ignore_ascii_case("compatobjectformat") {
            compat = Some(val);
        }
    }
    (object_format.unwrap_or_else(|| "sha1".to_owned()), compat)
}

fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Check whether a ref name matches any of the exclude patterns.
fn is_excluded(refname: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        let full_pat = if pat.contains('*') || pat.contains('?') || pat.contains('[') {
            pat.clone()
        } else {
            // Treat non-glob patterns as exact ref suffixes
            pat.clone()
        };
        // Try matching as a glob pattern against the full refname
        if grit_lib::refs::ref_matches_glob(refname, &full_pat) {
            return true;
        }
    }
    false
}

/// Normalize a --glob pattern: prepend refs/ if needed, append /* if no glob chars.
fn normalize_glob_pattern(pattern: &str) -> String {
    let full = if pattern.starts_with("refs/") {
        pattern.to_owned()
    } else {
        format!("refs/{pattern}")
    };
    ensure_glob_suffix(&full)
}

/// Normalize a ref-category pattern (for --branches=, --tags=, --remotes=).
/// The `prefix` is e.g. `refs/heads/`, and `pattern` is the user-supplied
/// portion. If the pattern has no glob characters, append `/*` so it matches
/// everything under that prefix path.
fn normalize_ref_pattern(prefix: &str, pattern: &str) -> String {
    let full = format!("{prefix}{pattern}");
    ensure_glob_suffix(&full)
}

/// If the given pattern has no glob characters, treat it as a prefix and
/// append `/*` (or just `*` if it ends with `/`).
fn ensure_glob_suffix(pattern: &str) -> String {
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        return pattern.to_owned();
    }
    if pattern.ends_with('/') {
        format!("{pattern}*")
    } else {
        format!("{pattern}/*")
    }
}
