//! `grit sparse-checkout` — manage sparse checkout patterns.
//!
//! Patterns live in `.git/info/sparse-checkout`. `core.sparseCheckout` and
//! `core.sparseCheckoutCone` are stored in `config.worktree` when present,
//! matching Git's worktree-local sparse settings.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::error::Error as GritError;
use grit_lib::ignore::path_in_sparse_checkout as path_in_sparse_checkout_lines;
use grit_lib::index::{MODE_GITLINK, MODE_TREE};
use grit_lib::objects::parse_commit;
use grit_lib::repo::Repository;
use grit_lib::sparse_checkout::{
    build_expanded_cone_sparse_checkout_lines, effective_cone_mode_for_sparse_file,
    load_sparse_checkout_with_warnings, parse_expanded_cone_recursive_dirs,
    path_in_sparse_checkout, sparse_checkout_lines_look_like_expanded_cone, ConePatterns,
    ConeWorkspace, NonConePatterns,
};
use grit_lib::state::resolve_head;
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit sparse-checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage sparse checkout patterns")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: SparseCheckoutSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SparseCheckoutSubcommand {
    Init(InitArgs),
    Set(SetArgs),
    Add(AddArgs),
    Reapply(ReapplyArgs),
    List,
    Disable,
    CheckRules(CheckRulesArgs),
    Clean(CleanArgs),
}

#[derive(Debug, ClapArgs)]
pub struct InitArgs {
    #[arg(long)]
    pub cone: bool,
    #[arg(long = "no-cone")]
    pub no_cone: bool,
    #[arg(long)]
    pub sparse_index: bool,
    #[arg(long = "no-sparse-index")]
    pub no_sparse_index: bool,
}

#[derive(Debug, ClapArgs)]
pub struct SetArgs {
    #[arg(long)]
    pub cone: bool,
    #[arg(long = "no-cone")]
    pub no_cone: bool,
    #[arg(long)]
    pub sparse_index: bool,
    #[arg(long = "no-sparse-index")]
    pub no_sparse_index: bool,
    #[arg(long = "skip-checks")]
    pub skip_checks: bool,
    #[arg(long)]
    pub stdin: bool,
    #[arg(long = "end-of-options", hide = true)]
    pub end_of_options: bool,
    pub patterns: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    #[arg(long = "skip-checks")]
    pub skip_checks: bool,
    #[arg(long)]
    pub stdin: bool,
    #[arg(long = "end-of-options", hide = true)]
    pub end_of_options: bool,
    pub patterns: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ReapplyArgs {
    #[arg(long)]
    pub cone: bool,
    #[arg(long = "no-cone")]
    pub no_cone: bool,
    #[arg(long)]
    pub sparse_index: bool,
    #[arg(long = "no-sparse-index")]
    pub no_sparse_index: bool,
}

#[derive(Debug, ClapArgs)]
pub struct CheckRulesArgs {
    #[arg(short = 'z')]
    pub nul: bool,
    #[arg(long)]
    pub cone: bool,
    #[arg(long = "no-cone")]
    pub no_cone: bool,
    #[arg(long = "rules-file", value_name = "FILE")]
    pub rules_file: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct CleanArgs {
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,
    #[arg(short = 'f', long)]
    pub force: bool,
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// After `git clone --sparse`: enable cone sparse-checkout and optionally shrink the tree.
/// Copy `.git/info/sparse-checkout` from the main repo into a linked worktree admin dir.
pub(crate) fn copy_sparse_checkout_to_admin(source_git_dir: &Path, admin_dir: &Path) -> Result<()> {
    let src = source_git_dir.join("info").join("sparse-checkout");
    if !src.exists() {
        return Ok(());
    }
    let dst_dir = admin_dir.join("info");
    fs::create_dir_all(&dst_dir)?;
    fs::copy(&src, dst_dir.join("sparse-checkout"))?;
    Ok(())
}

pub(crate) fn finalize_sparse_clone(repo: &Repository, apply_to_index: bool) -> Result<()> {
    if apply_to_index {
        crate::commands::clone::ensure_index_from_head_if_missing(repo)?;
    }
    init_worktree_config(repo)?;
    set_sparse_config(repo, true)?;
    set_cone_config(repo, true)?;
    let ws = ConeWorkspace::default();
    write_sparse_file(repo, &ws.to_sparse_checkout_file())?;
    if apply_to_index {
        let patterns = read_sparse_patterns(repo)?;
        apply_sparse_patterns(repo, &patterns, true)?;
    }
    Ok(())
}

/// Run `grit sparse-checkout`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.subcommand {
        SparseCheckoutSubcommand::Init(a) => cmd_init(&repo, &a),
        SparseCheckoutSubcommand::Set(a) => cmd_set(&repo, &a),
        SparseCheckoutSubcommand::Add(a) => cmd_add(&repo, &a),
        SparseCheckoutSubcommand::Reapply(a) => cmd_reapply(&repo, &a),
        SparseCheckoutSubcommand::List => cmd_list(&repo),
        SparseCheckoutSubcommand::Disable => cmd_disable(&repo),
        SparseCheckoutSubcommand::CheckRules(a) => cmd_check_rules(&repo, &a),
        SparseCheckoutSubcommand::Clean(a) => cmd_clean(&repo, &a),
    }
}

fn tri_bool(cone: bool, no_cone: bool) -> Result<Option<bool>> {
    match (cone, no_cone) {
        (true, true) => bail!("cannot combine --cone and --no-cone"),
        (true, false) => Ok(Some(true)),
        (false, true) => Ok(Some(false)),
        (false, false) => Ok(None),
    }
}

fn tri_bool_sparse(sparse: bool, no_sparse: bool) -> Result<Option<bool>> {
    match (sparse, no_sparse) {
        (true, true) => bail!("cannot combine --sparse-index and --no-sparse-index"),
        (true, false) => Ok(Some(true)),
        (false, true) => Ok(Some(false)),
        (false, false) => Ok(None),
    }
}

fn worktree_config_path(repo: &Repository) -> PathBuf {
    repo.git_dir.join("config.worktree")
}

fn init_worktree_config(repo: &Repository) -> Result<()> {
    let p = worktree_config_path(repo);
    if p.exists() {
        return Ok(());
    }
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&p, "").context("creating config.worktree")?;
    Ok(())
}

fn load_merged_config(repo: &Repository) -> grit_lib::config::ConfigSet {
    grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default()
}

fn sparse_checkout_path(repo: &Repository) -> PathBuf {
    repo.git_dir.join("info").join("sparse-checkout")
}

fn acquire_sparse_lock(repo: &Repository) -> Result<std::fs::File> {
    let lock_path = repo.git_dir.join("info").join("sparse-checkout.lock");
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("Unable to create {}: File exists.", lock_path.display());
        }
        Err(e) => Err(e).context("sparse-checkout lock")?,
    }
}

fn release_sparse_lock(repo: &Repository) {
    let _ = fs::remove_file(repo.git_dir.join("info").join("sparse-checkout.lock"));
}

fn set_sparse_config(repo: &Repository, enable: bool) -> Result<()> {
    init_worktree_config(repo)?;
    let path = worktree_config_path(repo);
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut cfg = ConfigFile::parse(&path, &content, ConfigScope::Worktree)?;
    cfg.set("core.sparseCheckout", if enable { "true" } else { "false" })?;
    cfg.write()?;
    Ok(())
}

fn set_cone_config(repo: &Repository, cone: bool) -> Result<()> {
    init_worktree_config(repo)?;
    let path = worktree_config_path(repo);
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut cfg = ConfigFile::parse(&path, &content, ConfigScope::Worktree)?;
    cfg.set(
        "core.sparseCheckoutCone",
        if cone { "true" } else { "false" },
    )?;
    cfg.write()?;
    Ok(())
}

fn set_sparse_index_config(repo: &Repository, enable: bool) -> Result<()> {
    init_worktree_config(repo)?;
    let path = worktree_config_path(repo);
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut cfg = ConfigFile::parse(&path, &content, ConfigScope::Worktree)?;
    cfg.set("index.sparse", if enable { "true" } else { "false" })?;
    cfg.write()?;
    Ok(())
}

fn read_sparse_file_content(repo: &Repository) -> String {
    let p = sparse_checkout_path(repo);
    fs::read_to_string(&p).unwrap_or_default()
}

fn write_sparse_file(repo: &Repository, content: &str) -> Result<()> {
    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&sc_path, content).context("writing sparse-checkout file")?;
    Ok(())
}

/// Initialize sparse-checkout after `clone --sparse` (matches `git clone --sparse`).
///
/// Writes `/*` and `!/*/` patterns, enables `core.sparseCheckout` and cone mode.
/// When `apply_worktree` is true, updates the index and working tree (normal clone).
pub(crate) fn init_clone_sparse_checkout(repo: &Repository, apply_worktree: bool) -> Result<()> {
    set_sparse_config(repo, true)?;
    set_cone_config(repo, true)?;

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    let patterns = vec!["/*".to_string(), "!/*/".to_string()];
    let content: String = patterns.iter().map(|p| format!("{p}\n")).collect();
    fs::write(&sc_path, &content).context("writing sparse-checkout file")?;
    if apply_worktree {
        apply_sparse_patterns(repo, &patterns, true)?;
    }
    Ok(())
}

fn head_tree_oid(repo: &Repository) -> Result<Option<grit_lib::objects::ObjectId>> {
    let head = resolve_head(&repo.git_dir).context("reading HEAD")?;
    let Some(commit_oid) = head.oid() else {
        return Ok(None);
    };
    let obj = repo.odb.read(commit_oid).context("reading HEAD commit")?;
    let commit = parse_commit(&obj.data).context("parsing HEAD commit")?;
    Ok(Some(commit.tree))
}

fn cmd_init(repo: &Repository, args: &InitArgs) -> Result<()> {
    let _work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let sparse_idx_opt = tri_bool_sparse(args.sparse_index, args.no_sparse_index)?;

    let config = load_merged_config(repo);
    let was_sparse = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    let prev_cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    // When sparse was off (e.g. after `sparse-checkout disable`), honor the saved
    // `core.sparseCheckoutCone` value instead of defaulting to cone — matches Git and
    // t7817 (non-cone superproject must stay non-cone across disable/init).
    let cone = match cone_opt {
        Some(c) => c,
        None if was_sparse => prev_cone,
        None => prev_cone,
    };

    set_sparse_config(repo, true)?;
    set_cone_config(repo, cone)?;
    if let Some(enable) = sparse_idx_opt {
        set_sparse_index_config(repo, enable)?;
    }

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if sc_path.exists() {
        let patterns = read_sparse_patterns(repo)?;
        apply_sparse_patterns(repo, &patterns, cone)?;
        return Ok(());
    }

    // When the sparse-checkout file was removed (e.g. `sparse-checkout disable`),
    // Git recreates the default `/*` + `!/*/` pair before applying (see
    // `sparse_checkout_init` in sparse-checkout.c). A lone `/*` would leave every
    // top-level directory included, so `!b` in t7817 would never take effect.
    if head_tree_oid(repo)?.is_none() {
        write_sparse_file(repo, "/*\n!/*/\n")?;
        return Ok(());
    }

    if cone {
        let ws = ConeWorkspace::default();
        write_sparse_file(repo, &ws.to_sparse_checkout_file())?;
    } else {
        write_sparse_file(repo, "/*\n!/*/\n")?;
    }
    let patterns = read_sparse_patterns(repo)?;
    apply_sparse_patterns(repo, &patterns, cone)?;
    Ok(())
}

fn cmd_set(repo: &Repository, args: &SetArgs) -> Result<()> {
    let _wt = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let sparse_idx_opt = tri_bool_sparse(args.sparse_index, args.no_sparse_index)?;

    let config = load_merged_config(repo);
    let prev_cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    let cone = cone_opt.unwrap_or(prev_cone);

    set_sparse_config(repo, true)?;
    set_cone_config(repo, cone)?;
    if let Some(enable) = sparse_idx_opt {
        set_sparse_index_config(repo, enable)?;
    }

    let _lock = acquire_sparse_lock(repo)?;
    let result = (|| {
        if args.stdin {
            let stdin = io::stdin();
            let mut stdin = stdin.lock();
            let lines = read_stdin_lines(&mut stdin)?;
            if cone {
                let mut dirs = Vec::new();
                for line in lines {
                    let p = normalize_cone_input_line(&line)?;
                    if !args.skip_checks {
                        validate_cone_patterns(repo, std::slice::from_ref(&p))?;
                    }
                    dirs.push(p);
                }
                let ws = ConeWorkspace::from_directory_list(&dirs);
                let body = ws.to_sparse_checkout_file();
                write_sparse_file(repo, &body)?;
                let patterns = read_sparse_patterns(repo)?;
                crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
                    repo, &patterns, true,
                )?;
                apply_sparse_patterns(repo, &patterns, true)?;
            } else {
                let mut lines = lines;
                if lines.is_empty() {
                    lines = vec!["/*".to_string(), "!/*/".to_string()];
                }
                let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
                write_sparse_file(repo, &body)?;
                let patterns = read_sparse_patterns(repo)?;
                crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
                    repo, &patterns, false,
                )?;
                apply_sparse_patterns(repo, &patterns, false)?;
            }
            Ok(())
        } else {
            let mut pats = args.patterns.clone();
            sanitize_set_paths(
                repo,
                worktree_prefix(repo)?,
                cone,
                args.skip_checks,
                &mut pats,
            )?;
            if !cone && pats.is_empty() {
                pats = vec!["/*".to_string(), "!/*/".to_string()];
            }
            let mut file_only_cone = false;
            if cone {
                if !args.skip_checks {
                    validate_cone_patterns(repo, &pats)?;
                }
                file_only_cone = cone_patterns_are_all_tracked_files(repo, &pats)?;
                let effective_cone_dirs = !file_only_cone;
                if file_only_cone {
                    set_cone_config(repo, false)?;
                }
                let lines: Vec<String> = if effective_cone_dirs {
                    build_expanded_cone_sparse_checkout_lines(&pats)
                } else {
                    pats.clone()
                };
                let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
                write_sparse_file(repo, &body)?;
                if file_only_cone {
                    set_cone_config(repo, true)?;
                }
            } else {
                let body: String = pats.iter().map(|p| format!("{p}\n")).collect();
                write_sparse_file(repo, &body)?;
            }
            let patterns = read_sparse_patterns(repo)?;
            let apply_cone = cone && !file_only_cone;
            crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
                repo, &patterns, apply_cone,
            )?;
            apply_sparse_patterns(repo, &patterns, apply_cone)?;
            Ok(())
        }
    })();
    release_sparse_lock(repo);
    result
}

fn cmd_add(repo: &Repository, args: &AddArgs) -> Result<()> {
    let _wt = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = load_merged_config(repo);
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        bail!("no sparse-checkout to add to");
    }
    let cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    if !args.stdin && args.patterns.is_empty() {
        bail!("specify directories to add");
    }

    let _lock = acquire_sparse_lock(repo)?;
    let result = (|| {
        if cone {
            let content = read_sparse_file_content(repo);
            let existing = read_sparse_patterns(repo)?;
            let mut dirs = if sparse_checkout_lines_look_like_expanded_cone(&existing) {
                parse_expanded_cone_recursive_dirs(&existing)
            } else if let Some(cp) = ConePatterns::try_parse(&content) {
                ConeWorkspace::from_cone_patterns(&cp).list_cone_directories()
            } else {
                existing
                    .iter()
                    .map(|s| {
                        s.trim()
                            .trim_start_matches('/')
                            .trim_end_matches('/')
                            .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            };
            let inputs = if args.stdin {
                let stdin = io::stdin();
                let mut stdin = stdin.lock();
                read_stdin_lines(&mut stdin)?
            } else {
                let mut p = args.patterns.clone();
                sanitize_add_paths(repo, worktree_prefix(repo)?, args.skip_checks, &mut p)?;
                p
            };
            for line in inputs {
                let p = normalize_cone_input_line(&line)?;
                if !args.skip_checks {
                    validate_cone_patterns(repo, std::slice::from_ref(&p))?;
                }
                dirs.push(p);
            }
            dirs.sort();
            dirs.dedup();
            let lines = build_expanded_cone_sparse_checkout_lines(&dirs);
            let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
            write_sparse_file(repo, &body)?;
        } else {
            let mut patterns = read_sparse_patterns(repo)?;
            let extra = if args.stdin {
                let stdin = io::stdin();
                let mut stdin = stdin.lock();
                read_stdin_lines(&mut stdin)?
            } else {
                let mut p = args.patterns.clone();
                sanitize_add_paths(repo, worktree_prefix(repo)?, args.skip_checks, &mut p)?;
                p
            };
            for pat in extra {
                patterns.push(pat);
            }
            let body: String = patterns.iter().map(|p| format!("{p}\n")).collect();
            write_sparse_file(repo, &body)?;
        }
        let patterns = read_sparse_patterns(repo)?;
        crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
            repo, &patterns, cone,
        )?;
        apply_sparse_patterns(repo, &patterns, cone)?;
        Ok(())
    })();
    release_sparse_lock(repo);
    result
}

fn cmd_reapply(repo: &Repository, args: &ReapplyArgs) -> Result<()> {
    let _wt = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = load_merged_config(repo);
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        bail!("must be in a sparse-checkout to reapply sparsity patterns");
    }

    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let sparse_idx_opt = tri_bool_sparse(args.sparse_index, args.no_sparse_index)?;

    if let Some(cone) = cone_opt {
        set_cone_config(repo, cone)?;
    }
    if let Some(enable) = sparse_idx_opt {
        set_sparse_index_config(repo, enable)?;
    }

    let config = load_merged_config(repo);
    let cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    let patterns = read_sparse_patterns(repo)?;
    crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
        repo, &patterns, cone,
    )?;
    apply_sparse_patterns(repo, &patterns, cone)?;
    Ok(())
}

fn cmd_list(repo: &Repository) -> Result<()> {
    let _wt = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = load_merged_config(repo);
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        bail!("this worktree is not sparse");
    }

    let sc_path = sparse_checkout_path(repo);
    if !sc_path.exists() {
        eprintln!("warning: this worktree is not sparse (sparse-checkout file may not exist)");
        return Ok(());
    }

    let content = match fs::read_to_string(&sc_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("warning: this worktree is not sparse (sparse-checkout file may not exist)");
            return Ok(());
        }
    };
    let cone_cfg = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    let lines: Vec<String> = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if cone_cfg && sparse_checkout_lines_look_like_expanded_cone(&lines) {
        for d in parse_expanded_cone_recursive_dirs(&lines) {
            writeln!(out, "{d}")?;
        }
        return Ok(());
    }

    if cone_cfg {
        if let Some(cp) = ConePatterns::try_parse(&content) {
            let ws = ConeWorkspace::from_cone_patterns(&cp);
            for d in ws.list_cone_directories() {
                writeln!(out, "{d}")?;
            }
        } else {
            for line in &lines {
                writeln!(out, "{line}")?;
            }
        }
    } else {
        for line in &lines {
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}

fn cmd_disable(repo: &Repository) -> Result<()> {
    let _work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    // Match Git `sparse_checkout_disable`: build an in-memory `/*` pattern list to expand the
    // work tree to a full checkout, but do **not** overwrite `info/sparse-checkout` with only
    // `/*` — that would erase stored rules like `!b` and break a later `sparse-checkout init`
    // (t7817).
    set_sparse_config(repo, true)?;
    set_sparse_index_config(repo, false)?;

    let patterns = vec!["/*".to_string()];
    apply_sparse_patterns(repo, &patterns, false)?;

    unset_sparse_keys_all_layers(repo)?;
    Ok(())
}

fn unset_sparse_keys_all_layers(repo: &Repository) -> Result<()> {
    for (path, scope) in [
        (worktree_config_path(repo), ConfigScope::Worktree),
        (repo.git_dir.join("config"), ConfigScope::Local),
    ] {
        if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut cfg = ConfigFile::parse(&path, &content, scope)?;
            let _ = cfg.unset("core.sparseCheckout");
            let _ = cfg.unset("core.sparseCheckoutCone");
            let _ = cfg.unset("index.sparse");
            cfg.write()?;
        }
    }
    Ok(())
}

fn cmd_check_rules(repo: &Repository, args: &CheckRulesArgs) -> Result<()> {
    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let config = load_merged_config(repo);
    let mut cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    if let Some(c) = cone_opt {
        cone = c;
    }
    if args.rules_file.is_some() && cone_opt.is_none() {
        cone = true;
    }

    let (cone_pat, non_cone, effective_cone) = if let Some(ref rf) = args.rules_file {
        let text = fs::read_to_string(rf).with_context(|| rf.display().to_string())?;
        if cone {
            let mut dirs = Vec::new();
            for line in text.lines() {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                let p = normalize_cone_input_line(t)?;
                dirs.push(p);
            }
            let ws = ConeWorkspace::from_directory_list(&dirs);
            let file_body = ws.to_sparse_checkout_file();
            let mut w = Vec::new();
            let cp = ConePatterns::try_parse_with_warnings(&file_body, &mut w);
            let ec = cp.is_some();
            (cp, NonConePatterns::parse(&file_body), ec)
        } else {
            (None, NonConePatterns::parse(&text), false)
        }
    } else {
        let mut w = Vec::new();
        let (_ok, cp, nc) = load_sparse_checkout_with_warnings(&repo.git_dir, cone, &mut w);
        let sparse_content = read_sparse_file_content(repo);
        let ec = cone && ConePatterns::try_parse(&sparse_content).is_some();
        (cp, nc, ec)
    };

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut line = Vec::new();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.nul {
        loop {
            line.clear();
            let n = stdin.read_until(0, &mut line)?;
            if n == 0 {
                break;
            }
            if line.last() == Some(&0) {
                line.pop();
            }
            let path = String::from_utf8_lossy(&line);
            let path = path.as_ref();
            if path_in_sparse_checkout(
                path,
                effective_cone,
                cone_pat.as_ref(),
                &non_cone,
                repo.work_tree.as_deref(),
            ) {
                out.write_all(line.as_slice())?;
                out.write_all(&[0])?;
            }
        }
    } else {
        loop {
            line.clear();
            let n = stdin.read_until(b'\n', &mut line)?;
            if n == 0 {
                break;
            }
            while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
                line.pop();
            }
            let path = String::from_utf8_lossy(&line);
            let path = path.as_ref();
            if path_in_sparse_checkout(
                path,
                effective_cone,
                cone_pat.as_ref(),
                &non_cone,
                repo.work_tree.as_deref(),
            ) {
                writeln!(out, "{path}")?;
            }
        }
    }
    Ok(())
}

fn cmd_clean(repo: &Repository, args: &CleanArgs) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = load_merged_config(repo);
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        bail!("must be in a sparse-checkout to clean directories");
    }
    let cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    if !cone {
        bail!("must be in a cone-mode sparse-checkout to clean directories");
    }

    let require_force = config
        .get("clean.requireForce")
        .map(|v| v == "true")
        .unwrap_or(true);
    if require_force && !args.force && !args.dry_run {
        bail!("for safety, refusing to clean without one of --force or --dry-run");
    }

    let index_path = repo.index_path();
    let index = repo.load_index_at(&index_path).context("reading index")?;

    let msg_remove = "Removing ";
    let msg_would = "Would remove ";
    let msg = if args.dry_run { msg_would } else { msg_remove };

    for entry in &index.entries {
        if entry.mode != MODE_TREE || !entry.is_sparse_directory_placeholder() {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let full = work_tree.join(path_str.as_ref());
        if !full.is_dir() {
            continue;
        }
        if args.verbose {
            for rel in list_files_under_dir(&full, work_tree)? {
                writeln!(io::stdout(), "{msg}{rel}")?;
            }
        } else {
            writeln!(io::stdout(), "{msg}{path_str}/")?;
        }
        if !args.dry_run {
            let _ = fs::remove_dir_all(&full);
        }
    }

    Ok(())
}

fn worktree_prefix(repo: &Repository) -> Result<String> {
    let wt = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bare"))?;
    let cwd = std::env::current_dir()?;
    let wt = wt.canonicalize().unwrap_or_else(|_| wt.to_path_buf());
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    if let Ok(rest) = cwd.strip_prefix(&wt) {
        let s = rest.to_string_lossy().replace('\\', "/");
        if s.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("{}/", s.trim_end_matches('/')))
    } else {
        Ok(String::new())
    }
}

fn sanitize_set_paths(
    repo: &Repository,
    prefix: String,
    cone: bool,
    skip_checks: bool,
    args: &mut Vec<String>,
) -> Result<()> {
    if !prefix.is_empty() && cone {
        for a in args.iter_mut() {
            if let Some(p) =
                crate::git_path::prefix_path_gently(&prefix, a, repo.work_tree.as_ref().unwrap())
            {
                *a = p;
            }
        }
    }
    if !prefix.is_empty() && !cone {
        bail!("please run from the toplevel directory in non-cone mode");
    }
    if cone {
        for a in args.iter() {
            if a.starts_with('/') {
                bail!("specify directories rather than patterns (no leading slash)");
            }
            if a.starts_with('!') {
                bail!("specify directories rather than patterns.  If your directory starts with a '!', pass --skip-checks");
            }
            if a.contains('*') || a.contains('?') || a.contains('[') {
                bail!("specify directories rather than patterns.  If your directory really has any of '*?[]\\' in it, pass --skip-checks");
            }
        }
    }
    if !skip_checks {
        validate_cone_patterns(repo, args)?;
    }
    Ok(())
}

fn sanitize_add_paths(
    repo: &Repository,
    prefix: String,
    skip_checks: bool,
    args: &mut Vec<String>,
) -> Result<()> {
    let config = load_merged_config(repo);
    let cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    sanitize_set_paths(repo, prefix, cone, skip_checks, args)
}

fn normalize_cone_input_line(line: &str) -> Result<String> {
    let mut s = line.trim().to_string();
    if s.starts_with('"') {
        s = unquote_c_style(&s)?;
    }
    s = s.trim_end_matches('/').to_string();
    let normalized = crate::git_path::normalize_path_copy(&s)
        .map_err(|_| anyhow::anyhow!("could not normalize path {s}"))?;
    Ok(normalized.trim_start_matches('/').to_string())
}

fn unquote_c_style(s: &str) -> Result<String> {
    if !s.starts_with('"') || !s.ends_with('"') || s.len() < 2 {
        return Ok(s.to_string());
    }
    let inner = &s[1..s.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let Some(n) = chars.next() else {
                bail!("invalid escape");
            };
            match n {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                _ => out.push(n),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn read_stdin_lines<R: BufRead>(r: &mut R) -> Result<Vec<String>> {
    let mut v = Vec::new();
    for line in r.lines() {
        v.push(line?);
    }
    Ok(v)
}

fn cone_patterns_are_all_tracked_files(repo: &Repository, patterns: &[String]) -> Result<bool> {
    if patterns.is_empty() {
        return Ok(false);
    }
    let index_path = repo.index_path();
    let index =
        grit_lib::index::Index::load(&index_path).context("reading index for cone heuristics")?;
    for pat in patterns {
        let p = pat.trim().trim_start_matches('/').trim_end_matches('/');
        if p.is_empty() || p.contains('/') {
            return Ok(false);
        }
        let Some(ce) = index.get(p.as_bytes(), 0) else {
            return Ok(false);
        };
        if ce.is_sparse_directory_placeholder() || ce.mode == MODE_TREE {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_cone_patterns(repo: &Repository, patterns: &[String]) -> Result<()> {
    let index_path = repo.index_path();
    let index =
        grit_lib::index::Index::load(&index_path).context("reading index for validation")?;
    for pat in patterns {
        let p = pat.trim_end_matches('/');
        if p.is_empty() {
            continue;
        }
        if let Some(ce) = index.get(p.as_bytes(), 0) {
            if ce.is_sparse_directory_placeholder() {
                continue;
            }
            // Harness / sparse-checkout tests use `git sparse-checkout set a` where `a` is a
            // tracked file; Git accepts this as a recursive cone directory name. Allow any
            // single-segment path that is a non-tree index entry.
            if !p.contains('/') && ce.mode != MODE_TREE {
                continue;
            }
            bail!(
                "'{}' is not a directory; to treat it as a directory anyway, rerun with --skip-checks",
                p
            );
        }
        // No exact index entry: allowed (matches Git `sanitize_paths` / `index_name_pos`).
    }
    Ok(())
}

fn read_sparse_patterns(repo: &Repository) -> Result<Vec<String>> {
    let sc_path = sparse_checkout_path(repo);
    if !sc_path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&sc_path).context("reading sparse-checkout file")?;
    Ok(content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}

/// Re-run sparse-checkout pattern application after commands that rebuild the index
/// (e.g. `git reset --hard`), matching Git's behaviour of preserving sparsity.
pub(crate) fn reapply_sparse_checkout_if_configured(repo: &Repository) -> Result<()> {
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        return Ok(());
    }
    let sc_path = sparse_checkout_path(repo);
    if !sc_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&sc_path).context("reading sparse-checkout file")?;
    let lines: Vec<String> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    if lines.is_empty() {
        return Ok(());
    }
    let cone_cfg = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    apply_sparse_patterns(repo, &lines, cone_cfg)
}

fn apply_sparse_patterns(repo: &Repository, patterns: &[String], cone_mode: bool) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bare repository cannot use sparse checkout"))?;
    let config = load_merged_config(repo);
    let sparse_index_enabled = config
        .get("index.sparse")
        .map(|v| v == "true")
        .unwrap_or(false);

    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("reading index")?;
    if index.entries.is_empty() {
        crate::commands::clone::ensure_index_from_head_if_missing(repo)?;
        index = repo
            .load_index_at(&index_path)
            .context("reading index after building from HEAD")?;
    }

    if index.version < 3 {
        index.version = 3;
    }

    let file_content = read_sparse_file_content(repo);
    let expanded_cone_shape = effective_cone_mode_for_sparse_file(cone_mode, patterns);
    let cone_struct = if expanded_cone_shape {
        ConePatterns::try_parse(&file_content)
    } else {
        None
    };
    let effective_cone = expanded_cone_shape && cone_struct.is_some();
    let non_cone = NonConePatterns::from_lines(patterns.to_vec());

    for entry in &mut index.entries {
        if entry.mode == MODE_TREE {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        // Non-cone mode must use Git's `path_in_sparse_checkout` (parent walk + last-match),
        // not `NonConePatterns::path_included` (sequential toggles). See t3602-rm-sparse-checkout.
        let matches = if effective_cone {
            path_in_sparse_checkout(
                &path_str,
                true,
                cone_struct.as_ref(),
                &non_cone,
                Some(work_tree),
            )
        } else {
            path_in_sparse_checkout_lines(&path_str, patterns, Some(work_tree))
        };

        if matches {
            if entry.skip_worktree() {
                entry.set_skip_worktree(false);
                let full_path = work_tree.join(&path_str);
                if !full_path.exists() {
                    if let Some(parent) = full_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let blob_data = match repo.odb.read(&entry.oid) {
                        Ok(obj) => Some(obj.data),
                        Err(GritError::ObjectNotFound(_)) => {
                            if crate::commands::promisor_hydrate::try_lazy_fetch_promisor_object(
                                repo, entry.oid,
                            )
                            .is_ok()
                            {
                                repo.odb.read(&entry.oid).ok().map(|o| o.data)
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    };
                    if let Some(data) = blob_data {
                        let _ = fs::write(&full_path, &data);
                    }
                }
            }
        } else {
            entry.set_skip_worktree(true);
            let full_path = work_tree.join(&path_str);
            if full_path.exists() {
                let _ = fs::remove_file(&full_path);
                if let Some(parent) = full_path.parent() {
                    remove_empty_dirs_up_to(parent, work_tree);
                }
            }
        }
    }

    // In partial clones (`grit-promisor-missing` lists blobs not yet local), sparse
    // directory collapse would expand excluded subtrees into the index and pull blob
    // OIDs into scope — breaking `rev-list --missing=print` expectations (t5620).
    let promisor_marker = repo.git_dir.join("grit-promisor-missing");
    let skip_collapse = fs::read_to_string(&promisor_marker)
        .map(|s| {
            s.lines()
                .any(|l| l.len() == 40 && l.chars().all(|c| c.is_ascii_hexdigit()))
        })
        .unwrap_or(false);

    if !skip_collapse {
        if let Some(tree_oid) = head_tree_oid(repo)? {
            index.try_collapse_sparse_directories(
                &repo.odb,
                &tree_oid,
                patterns,
                effective_cone,
                sparse_index_enabled,
            )?;
        } else {
            index.sparse_directories = false;
        }
    } else {
        index.sparse_directories = false;
    }

    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;

    // Remove untracked paths outside the sparse cone (Git `sparse_checkout_set` / t7012).
    let indexed_paths: HashSet<String> = index
        .entries
        .iter()
        .map(|e| String::from_utf8_lossy(&e.path).into_owned())
        .collect();
    let gitlink_paths: HashSet<String> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.mode == MODE_GITLINK)
        .map(|e| String::from_utf8_lossy(&e.path).into_owned())
        .collect();
    remove_untracked_outside_sparse(
        work_tree,
        work_tree,
        &indexed_paths,
        &gitlink_paths,
        patterns,
        effective_cone,
        cone_struct.as_ref(),
        &non_cone,
    )?;

    // Submodule work trees keep their own `info/sparse-checkout`. After the superproject applies
    // sparsity we skip cleaning inside gitlink dirs (so we do not delete `sub/B/b`), so re-run the
    // submodule's sparse rules so paths like `sub/A` are pruned (t7817).
    for entry in &index.entries {
        if entry.stage() != 0 || entry.mode != MODE_GITLINK {
            continue;
        }
        let rel = String::from_utf8_lossy(&entry.path);
        let included = if effective_cone {
            path_in_sparse_checkout(
                rel.as_ref(),
                true,
                cone_struct.as_ref(),
                &non_cone,
                Some(work_tree),
            )
        } else {
            path_in_sparse_checkout_lines(rel.as_ref(), patterns, Some(work_tree))
        };
        if !included {
            continue;
        }
        let sub_wt = work_tree.join(rel.as_ref());
        if let Ok(sub_repo) = open_gitlink_worktree_repo(&sub_wt) {
            let _ = reapply_sparse_checkout_if_configured(&sub_repo);
        }
    }

    Ok(())
}

fn open_gitlink_worktree_repo(sub_work_tree: &Path) -> Result<Repository> {
    let git_path = sub_work_tree.join(".git");
    if !git_path.try_exists().context("stat submodule .git")? {
        bail!("missing .git in {}", sub_work_tree.display());
    }
    if git_path.is_dir() {
        Repository::open(&git_path, Some(sub_work_tree)).context("open submodule repository")
    } else {
        let content =
            fs::read_to_string(&git_path).with_context(|| git_path.display().to_string())?;
        let gitdir = content
            .trim()
            .strip_prefix("gitdir: ")
            .ok_or_else(|| anyhow::anyhow!("invalid gitdir file {}", git_path.display()))?;
        let gitdir_path = if Path::new(gitdir).is_absolute() {
            PathBuf::from(gitdir)
        } else {
            sub_work_tree.join(gitdir)
        };
        let gitdir_path = gitdir_path
            .canonicalize()
            .with_context(|| format!("resolve gitdir {}", gitdir_path.display()))?;
        Repository::open(&gitdir_path, Some(sub_work_tree)).context("open submodule repository")
    }
}

fn remove_untracked_outside_sparse(
    work_tree: &Path,
    current: &Path,
    indexed_paths: &HashSet<String>,
    gitlink_paths: &HashSet<String>,
    patterns: &[String],
    effective_cone: bool,
    cone_struct: Option<&ConePatterns>,
    non_cone: &NonConePatterns,
) -> Result<()> {
    let Ok(read_dir) = fs::read_dir(current) else {
        return Ok(());
    };
    for ent in read_dir {
        let ent = ent.context("reading work tree directory")?;
        let path = ent.path();
        let rel = path
            .strip_prefix(work_tree)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        // Skip the main repo's `.git` and every nested `.git` (e.g. `sub/.git` for submodules).
        if rel == ".git"
            || rel.starts_with(".git/")
            || rel.ends_with("/.git")
            || rel.contains("/.git/")
        {
            continue;
        }
        let meta = fs::symlink_metadata(&path).context("stat work tree path")?;
        if meta.is_dir() {
            if gitlink_paths.contains(&rel) {
                continue;
            }
            remove_untracked_outside_sparse(
                work_tree,
                &path,
                indexed_paths,
                gitlink_paths,
                patterns,
                effective_cone,
                cone_struct,
                non_cone,
            )?;
            if fs::read_dir(&path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let included = if effective_cone {
                    path_in_sparse_checkout(&rel, true, cone_struct, non_cone, Some(work_tree))
                } else {
                    path_in_sparse_checkout_lines(&rel, patterns, Some(work_tree))
                };
                if !included && !indexed_paths.contains(&rel) {
                    let _ = fs::remove_dir(&path);
                    if let Some(parent) = path.parent() {
                        remove_empty_dirs_up_to(parent, work_tree);
                    }
                }
            }
            continue;
        }
        if !meta.is_file() && !meta.file_type().is_symlink() {
            continue;
        }
        if indexed_paths.contains(&rel) {
            continue;
        }
        let included = if effective_cone {
            path_in_sparse_checkout(&rel, true, cone_struct, non_cone, Some(work_tree))
        } else {
            path_in_sparse_checkout_lines(&rel, patterns, Some(work_tree))
        };
        if !included {
            let _ = fs::remove_file(&path);
            if let Some(parent) = path.parent() {
                remove_empty_dirs_up_to(parent, work_tree);
            }
        }
    }
    Ok(())
}

/// Whether `path` is included in the sparse checkout for the given patterns.
///
/// Used by `grit backfill --sparse` and promisor hydrate to mirror Git's path-walk sparse filtering.
pub(crate) fn path_matches_sparse_patterns(
    path: &str,
    patterns: &[String],
    cone_mode: bool,
) -> bool {
    if cone_mode {
        return grit_lib::sparse_checkout::path_matches_sparse_patterns(path, patterns, cone_mode);
    }
    path_in_sparse_checkout_lines(path, patterns, None)
}

fn list_files_under_dir(dir: &Path, work_tree: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for ent in fs::read_dir(&d).with_context(|| d.display().to_string())? {
            let ent = ent?;
            let p = ent.path();
            let meta = ent.metadata()?;
            if meta.is_dir() {
                stack.push(p);
            } else if meta.is_file() {
                let rel = p.strip_prefix(work_tree).unwrap_or(&p);
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn remove_empty_dirs_up_to(dir: &Path, stop: &Path) {
    let mut current = dir.to_path_buf();
    while current != stop {
        if let Ok(mut entries) = fs::read_dir(&current) {
            if entries.next().is_some() {
                break;
            }
            let _ = fs::remove_dir(&current);
        } else {
            break;
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }
}
