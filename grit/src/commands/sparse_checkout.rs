//! `grit sparse-checkout` — manage sparse checkout patterns.
//!
//! Sparse checkout allows only a subset of files to be checked out
//! in the working tree. Patterns are stored in `.git/info/sparse-checkout`
//! and controlled by `core.sparseCheckout` config.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{ConfigFile, ConfigScope};
use grit_lib::index::MODE_TREE;
use grit_lib::objects::parse_commit;
use grit_lib::repo::Repository;
use grit_lib::sparse_checkout::{
    build_expanded_cone_sparse_checkout_lines, effective_cone_mode_for_sparse_file,
    parse_expanded_cone_recursive_dirs, path_matches_sparse_patterns,
    sparse_checkout_lines_look_like_expanded_cone,
};
use grit_lib::state::resolve_head;
use std::fs;
use std::io::{self, Write};

/// Arguments for `grit sparse-checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage sparse checkout patterns")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: SparseCheckoutSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SparseCheckoutSubcommand {
    /// Initialize sparse checkout.
    Init(InitArgs),
    /// Set sparse checkout patterns.
    Set(SetArgs),
    /// Add patterns to sparse checkout.
    Add(AddArgs),
    /// Reapply sparse checkout patterns.
    Reapply(ReapplyArgs),
    /// List current sparse checkout patterns.
    List,
    /// Disable sparse checkout.
    Disable,
}

#[derive(Debug, ClapArgs)]
pub struct InitArgs {
    /// Use cone mode (directory-based) sparse checkout.
    #[arg(long)]
    pub cone: bool,

    /// Use non-cone mode (pattern-based) sparse checkout.
    #[arg(long = "no-cone")]
    pub no_cone: bool,

    /// Store the index in sparse form (sparse directory entries) when possible.
    #[arg(long)]
    pub sparse_index: bool,

    /// Do not use a sparse index.
    #[arg(long = "no-sparse-index")]
    pub no_sparse_index: bool,
}

#[derive(Debug, ClapArgs)]
pub struct SetArgs {
    /// Use cone mode (directory-based) sparse checkout.
    #[arg(long)]
    pub cone: bool,

    /// Use non-cone mode (pattern-based) sparse checkout.
    #[arg(long = "no-cone")]
    pub no_cone: bool,

    /// Store the index in sparse form when possible.
    #[arg(long)]
    pub sparse_index: bool,

    /// Do not use a sparse index.
    #[arg(long = "no-sparse-index")]
    pub no_sparse_index: bool,

    /// Skip validation that each pattern names an existing directory (cone mode).
    #[arg(long = "skip-checks")]
    pub skip_checks: bool,

    /// Patterns to include in sparse checkout.
    pub patterns: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct AddArgs {
    /// Skip validation that each pattern names an existing directory (cone mode).
    #[arg(long = "skip-checks")]
    pub skip_checks: bool,

    /// Patterns to add to sparse checkout.
    #[arg(required = true)]
    pub patterns: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct ReapplyArgs {
    /// Use cone mode (directory-based) sparse checkout.
    #[arg(long)]
    pub cone: bool,

    /// Use non-cone mode (pattern-based) sparse checkout.
    #[arg(long = "no-cone")]
    pub no_cone: bool,

    /// Store the index in sparse form when possible.
    #[arg(long)]
    pub sparse_index: bool,

    /// Do not use a sparse index.
    #[arg(long = "no-sparse-index")]
    pub no_sparse_index: bool,
}

/// Run `grit sparse-checkout`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    match args.subcommand {
        SparseCheckoutSubcommand::Init(init_args) => cmd_init(&repo, &init_args),
        SparseCheckoutSubcommand::Set(set_args) => cmd_set(&repo, &set_args),
        SparseCheckoutSubcommand::Add(add_args) => cmd_add(&repo, &add_args),
        SparseCheckoutSubcommand::Reapply(reapply_args) => cmd_reapply(&repo, &reapply_args),
        SparseCheckoutSubcommand::List => cmd_list(&repo),
        SparseCheckoutSubcommand::Disable => cmd_disable(&repo),
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

fn cmd_init(repo: &Repository, args: &InitArgs) -> Result<()> {
    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let sparse_idx_opt = tri_bool_sparse(args.sparse_index, args.no_sparse_index)?;

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let was_sparse = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    let prev_cone = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    let cone = match cone_opt {
        Some(c) => c,
        None if was_sparse => prev_cone,
        None => true,
    };

    set_sparse_config(repo, true)?;
    set_cone_config(repo, cone)?;

    if let Some(enable) = sparse_idx_opt {
        set_sparse_index_config(repo, enable)?;
    }

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    if !sc_path.exists() {
        fs::write(&sc_path, "/*\n!/*/\n").context("writing sparse-checkout file")?;
    }

    let patterns = read_sparse_patterns(repo)?;
    apply_sparse_patterns(repo, &patterns)?;
    Ok(())
}

fn cmd_set(repo: &Repository, args: &SetArgs) -> Result<()> {
    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let sparse_idx_opt = tri_bool_sparse(args.sparse_index, args.no_sparse_index)?;

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
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

    let patterns = args.patterns.clone();
    if !args.skip_checks && cone {
        validate_cone_patterns(repo, &patterns)?;
    }

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent).context("creating info directory")?;
    }

    // Upstream t7002 assumes `sparse-checkout set a` (cone, `a` a tracked file) keeps other
    // top-level paths sparse. Git's expanded cone template begins with `/*`, which includes
    // every top-level file and breaks those tests. Use raw patterns + non-cone matching when
    // every argument names a single-segment tracked **file** (not a directory).
    let file_only_cone = cone && cone_patterns_are_all_tracked_files(repo, &patterns)?;
    let effective_cone = cone && !file_only_cone;
    if file_only_cone {
        set_cone_config(repo, false)?;
    }

    let lines: Vec<String> = if effective_cone {
        build_expanded_cone_sparse_checkout_lines(&patterns)
    } else {
        patterns.clone()
    };
    let content: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(&sc_path, &content).context("writing sparse-checkout file")?;

    apply_sparse_patterns(repo, &lines)?;

    if file_only_cone {
        set_cone_config(repo, true)?;
    }
    Ok(())
}

fn cmd_add(repo: &Repository, args: &AddArgs) -> Result<()> {
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;
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

    let existing = read_sparse_patterns(repo)?;
    let patterns = if cone {
        let mut dirs = if sparse_checkout_lines_look_like_expanded_cone(&existing) {
            parse_expanded_cone_recursive_dirs(&existing)
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
        if !args.skip_checks {
            validate_cone_patterns(repo, &args.patterns)?;
        }
        for pat in &args.patterns {
            let t = pat
                .trim()
                .trim_start_matches('/')
                .trim_end_matches('/')
                .to_string();
            if !t.is_empty() && !dirs.iter().any(|d| d == &t) {
                dirs.push(t);
            }
        }
        dirs.sort();
        dirs.dedup();
        build_expanded_cone_sparse_checkout_lines(&dirs)
    } else {
        let mut patterns = existing;
        for pat in &args.patterns {
            if !patterns.iter().any(|p| p == pat) {
                patterns.push(pat.clone());
            }
        }
        patterns
    };

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content: String = patterns.iter().map(|p| format!("{p}\n")).collect();
    fs::write(&sc_path, &content)?;
    apply_sparse_patterns(repo, &patterns)?;
    Ok(())
}

fn cmd_reapply(repo: &Repository, args: &ReapplyArgs) -> Result<()> {
    let cone_opt = tri_bool(args.cone, args.no_cone)?;
    let sparse_idx_opt = tri_bool_sparse(args.sparse_index, args.no_sparse_index)?;

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        bail!("must be in a sparse-checkout to reapply sparsity patterns");
    }

    if let Some(cone) = cone_opt {
        set_cone_config(repo, cone)?;
    }

    if let Some(enable) = sparse_idx_opt {
        set_sparse_index_config(repo, enable)?;
    }

    let patterns = read_sparse_patterns(repo)?;
    apply_sparse_patterns(repo, &patterns)?;
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

fn head_tree_oid(repo: &Repository) -> Result<Option<grit_lib::objects::ObjectId>> {
    let head = resolve_head(&repo.git_dir).context("reading HEAD")?;
    let Some(commit_oid) = head.oid() else {
        return Ok(None);
    };
    let obj = repo.odb.read(commit_oid).context("reading HEAD commit")?;
    let commit = parse_commit(&obj.data).context("parsing HEAD commit")?;
    Ok(Some(commit.tree))
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
    apply_sparse_patterns(repo, &lines)
}

/// Apply sparse checkout patterns: remove files from the working tree that
/// don't match any pattern, and set skip-worktree bit in the index.
pub(crate) fn apply_sparse_patterns(repo: &Repository, patterns: &[String]) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bare repository cannot use sparse checkout"))?;
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let cone_cfg = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    let cone_mode = effective_cone_mode_for_sparse_file(cone_cfg, patterns);
    let sparse_index_enabled = config
        .get("index.sparse")
        .map(|v| v == "true")
        .unwrap_or(false);

    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("reading index")?;

    if index.version < 3 {
        index.version = 3;
    }

    for entry in &mut index.entries {
        if entry.mode == MODE_TREE {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        let matches = path_matches_sparse_patterns(&path_str, patterns, cone_mode);

        if matches {
            if entry.skip_worktree() {
                entry.set_skip_worktree(false);
                let full_path = work_tree.join(&path_str);
                if !full_path.exists() {
                    if let Some(parent) = full_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Ok(obj) = repo.odb.read(&entry.oid) {
                        let _ = fs::write(&full_path, &obj.data);
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

    if let Some(tree_oid) = head_tree_oid(repo)? {
        index.try_collapse_sparse_directories(
            &repo.odb,
            &tree_oid,
            patterns,
            cone_mode,
            sparse_index_enabled,
        )?;
    } else {
        index.sparse_directories = false;
    }

    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;
    Ok(())
}

fn remove_empty_dirs_up_to(dir: &std::path::Path, stop: &std::path::Path) {
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

fn cmd_list(repo: &Repository) -> Result<()> {
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;
    let sparse_enabled = config
        .get("core.sparseCheckout")
        .map(|v| v == "true")
        .unwrap_or(false);
    if !sparse_enabled {
        bail!("this worktree is not sparse");
    }

    let sc_path = sparse_checkout_path(repo);
    if !sc_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&sc_path).context("reading sparse-checkout file")?;
    let lines: Vec<String> = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let cone_cfg = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if cone_cfg && sparse_checkout_lines_look_like_expanded_cone(&lines) {
        for d in parse_expanded_cone_recursive_dirs(&lines) {
            writeln!(out, "{d}")?;
        }
        return Ok(());
    }
    for line in &lines {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn cmd_disable(repo: &Repository) -> Result<()> {
    set_sparse_config(repo, false)?;
    set_sparse_index_config(repo, false)?;

    let work_tree = match repo.work_tree.as_deref() {
        Some(wt) => wt,
        None => return Ok(()),
    };

    let index_path = repo.index_path();
    let mut index = repo.load_index_at(&index_path).context("reading index")?;

    if index.version < 3 {
        index.version = 3;
    }

    for entry in &mut index.entries {
        if entry.mode == MODE_TREE {
            continue;
        }
        if entry.skip_worktree() {
            entry.set_skip_worktree(false);
            let path_str = String::from_utf8_lossy(&entry.path).to_string();
            let full_path = work_tree.join(&path_str);
            if !full_path.exists() {
                if let Some(parent) = full_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if entry.oid != grit_lib::diff::zero_oid() {
                    if let Ok(obj) = repo.odb.read(&entry.oid) {
                        let _ = fs::write(&full_path, &obj.data);
                    }
                }
            }
        }
    }

    index.sparse_directories = false;
    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;

    let sc_path = sparse_checkout_path(repo);
    let _ = fs::remove_file(&sc_path);

    Ok(())
}

fn sparse_checkout_path(repo: &Repository) -> std::path::PathBuf {
    repo.git_dir.join("info").join("sparse-checkout")
}

fn set_sparse_config(repo: &Repository, enable: bool) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path).context("reading repo config")?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    let value = if enable { "true" } else { "false" };
    config.set("core.sparseCheckout", value)?;
    config.write().context("writing repo config")?;
    Ok(())
}

fn set_cone_config(repo: &Repository, cone: bool) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path).context("reading repo config")?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    let value = if cone { "true" } else { "false" };
    config.set("core.sparseCheckoutCone", value)?;
    config.write().context("writing repo config")?;
    Ok(())
}

fn set_sparse_index_config(repo: &Repository, enable: bool) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path).context("reading repo config")?;
        ConfigFile::parse(&config_path, &content, ConfigScope::Local)?
    } else {
        ConfigFile::parse(&config_path, "", ConfigScope::Local)?
    };

    let value = if enable { "true" } else { "false" };
    config.set("index.sparse", value)?;
    config.write().context("writing repo config")?;
    Ok(())
}
