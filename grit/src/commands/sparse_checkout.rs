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
        fs::write(&sc_path, "/*\n").context("writing sparse-checkout file")?;
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

    let mut content = String::new();
    for pat in &patterns {
        content.push_str(pat);
        content.push('\n');
    }
    fs::write(&sc_path, &content).context("writing sparse-checkout file")?;

    apply_sparse_patterns(repo, &patterns)?;
    crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
        repo, &patterns, cone,
    )?;
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

    let mut patterns = read_sparse_patterns(repo)?;
    if !args.skip_checks && cone {
        validate_cone_patterns(repo, &args.patterns)?;
    }
    for pat in &args.patterns {
        if !patterns.iter().any(|p| p == pat) {
            patterns.push(pat.clone());
        }
    }

    let sc_path = sparse_checkout_path(repo);
    if let Some(parent) = sc_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content: String = patterns.iter().map(|p| format!("{p}\n")).collect();
    fs::write(&sc_path, &content)?;
    apply_sparse_patterns(repo, &patterns)?;
    crate::commands::promisor_hydrate::hydrate_sparse_patterns_after_sparse_checkout_update(
        repo, &patterns, cone,
    )?;
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
            bail!(
                "'{}' is not a directory; to treat it as a directory anyway, rerun with --skip-checks",
                p
            );
        }
        let with_slash = format!("{p}/");
        if let Some(ce) = index.get(with_slash.as_bytes(), 0) {
            if ce.is_sparse_directory_placeholder() {
                continue;
            }
            bail!(
                "'{}' is not a directory; to treat it as a directory anyway, rerun with --skip-checks",
                p
            );
        }
        // No exact index entry: allowed (Git treats as missing / directory prefix only).
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

/// Apply sparse checkout patterns: remove files from the working tree that
/// don't match any pattern, and set skip-worktree bit in the index.
fn apply_sparse_patterns(repo: &Repository, patterns: &[String]) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bare repository cannot use sparse checkout"))?;
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let cone_mode = config
        .get("core.sparseCheckoutCone")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);
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
                cone_mode,
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
    Ok(())
}

/// Whether `path` is included in the sparse checkout for the given patterns.
///
/// Used by `grit backfill --sparse` to mirror Git's path-walk sparse filtering.
pub(crate) fn path_matches_sparse_patterns(
    path: &str,
    patterns: &[String],
    cone_mode: bool,
) -> bool {
    if cone_mode {
        if !path.contains('/') {
            return true;
        }

        for pattern in patterns {
            let prefix = pattern.trim_end_matches('/');
            if path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/') {
                return true;
            }
            if path == prefix {
                return true;
            }
        }
        return false;
    }

    let mut included = false;
    for raw_pattern in patterns {
        let pattern = raw_pattern.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }

        let (negated, core_pattern) = if let Some(rest) = pattern.strip_prefix('!') {
            (true, rest)
        } else {
            (false, pattern)
        };
        let normalized = core_pattern.strip_prefix('/').unwrap_or(core_pattern);
        if normalized.is_empty() {
            continue;
        }

        let matches = if let Some(prefix) = normalized.strip_suffix('/') {
            path == prefix || path.starts_with(&format!("{prefix}/"))
        } else if let Some(tail) = normalized.strip_prefix("**/") {
            if tail.is_empty() {
                true
            } else if crate::pathspec::pathspec_matches(tail, path) {
                true
            } else {
                path.match_indices('/')
                    .any(|(i, _)| crate::pathspec::pathspec_matches(tail, &path[i + 1..]))
            }
        } else {
            crate::pathspec::pathspec_matches(normalized, path)
        };

        if matches {
            included = !negated;
        }
    }

    included
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
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            writeln!(out, "{trimmed}")?;
        }
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
        apply_sparse_patterns(repo, &patterns)?;
    }
    Ok(())
}
