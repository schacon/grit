//! `grit add` — add file contents to the index.
//!
//! Stages files from the working tree into the index so they will be
//! included in the next commit.

use anyhow::{anyhow, bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::attributes::{parse_gitattributes_file_content, validate_rules_for_add};
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{self, ConversionConfig, GitAttributes};
use grit_lib::error::Error;
use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::{entry_from_metadata, normalize_mode, Index, IndexEntry};
#[allow(unused_imports)]
use grit_lib::objects::ObjectId;
use grit_lib::objects::ObjectKind;
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::unicode_normalization::{precompose_utf8_path, precompose_utf8_segment};
use grit_lib::wildmatch::wildmatch;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn resolved_env_index_path(repo: &Repository) -> PathBuf {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            p
        } else if let Ok(cwd) = std::env::current_dir() {
            cwd.join(p)
        } else {
            p
        }
    } else {
        repo.index_path()
    }
}

/// Arguments for `grit add`.
#[derive(Debug, ClapArgs)]
#[command(about = "Add file contents to the index")]
pub struct Args {
    /// Files to add. Use '.' to add everything.
    #[arg(value_name = "PATHSPEC", num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true)]
    pub pathspec: Vec<String>,

    /// Update tracked files (don't add new files).
    #[arg(short = 'u', long = "update")]
    pub update: bool,

    /// Add, modify, and remove index entries to match the working tree.
    #[arg(short = 'A', long = "all", alias = "no-ignore-removal")]
    pub all: bool,

    /// Only update already-tracked files, don't add new ones.
    #[arg(long = "no-all", alias = "ignore-removal")]
    pub no_all: bool,

    /// Record only the intent to add a path (placeholder entry).
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Dry run — show what would be added.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Be verbose.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Allow adding otherwise ignored files.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Interactive patch mode.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Interactive add mode.
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Edit the diff vs. the index before staging.
    #[arg(short = 'e', long = "edit")]
    pub edit: bool,

    /// Override the file mode for the added files (+x or -x).
    #[arg(long = "chmod")]
    pub chmod: Option<String>,

    /// Renormalize tracked files (apply clean/smudge filters).
    #[arg(long = "renormalize")]
    pub renormalize: bool,

    /// Refresh stat info in the index without changing content.
    #[arg(long = "refresh")]
    pub refresh: bool,

    /// Continue adding files when some cannot be added.
    #[arg(long = "ignore-errors")]
    pub ignore_errors: bool,

    /// Suppress warning for non-existent pathspecs (with --refresh).
    #[arg(long = "ignore-missing")]
    pub ignore_missing: bool,

    /// Suppress warning for adding an embedded repository.
    #[arg(long = "no-warn-embedded-repo")]
    pub no_warn_embedded_repo: bool,

    /// Read pathspecs from a file (one per line, or NUL-separated with --pathspec-file-nul).
    #[arg(long = "pathspec-from-file", value_name = "FILE")]
    pub pathspec_from_file: Option<PathBuf>,

    /// NUL-terminated pathspec input (requires --pathspec-from-file).
    #[arg(long = "pathspec-file-nul")]
    pub pathspec_file_nul: bool,
}

/// Flags for [`stage_file`] shared by `git add` and `git commit <paths>`.
pub(crate) struct StageFileContext<'a> {
    pub dry_run: bool,
    pub verbose: bool,
    pub intent_to_add: bool,
    pub chmod: Option<&'a str>,
}

impl StageFileContext<'_> {
    /// Staging as performed by `git commit` pathspecs (no dry-run, no chmod, no intent-to-add).
    pub fn for_commit() -> Self {
        Self {
            dry_run: false,
            verbose: false,
            intent_to_add: false,
            chmod: None,
        }
    }
}

impl<'a> From<&'a Args> for StageFileContext<'a> {
    fn from(a: &'a Args) -> Self {
        Self {
            dry_run: a.dry_run,
            verbose: a.verbose,
            intent_to_add: a.intent_to_add,
            chmod: a.chmod.as_deref(),
        }
    }
}

/// Run the `add` command.
pub fn run(mut args: Args) -> Result<()> {
    if args.pathspec_file_nul && args.pathspec_from_file.is_none() {
        bail!("the option '--pathspec-file-nul' requires '--pathspec-from-file'");
    }
    if let Some(ref file) = args.pathspec_from_file {
        if !args.pathspec.is_empty() {
            bail!("'--pathspec-from-file' and pathspec arguments cannot be used together");
        }
        if args.interactive || args.patch {
            bail!(
                "options '--pathspec-from-file' and '--interactive/--patch' cannot be used together"
            );
        }
        if args.edit {
            bail!("options '--pathspec-from-file' and '--edit' cannot be used together");
        }
        let path = file.as_os_str();
        let data = if path == "-" {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .context("reading pathspecs from stdin")?;
            buf
        } else {
            fs::read(file)
                .with_context(|| format!("cannot read pathspec file '{}'", file.display()))?
        };
        args.pathspec =
            crate::pathspec::parse_pathspecs_from_source(&data, args.pathspec_file_nul)?;
    }

    // --dry-run is incompatible with interactive modes
    if args.dry_run && (args.interactive || args.patch) {
        bail!("options '--dry-run' and '--interactive'/'--patch' cannot be used together");
    }

    // Stubs for unsupported interactive modes — accept the flags
    // gracefully so scripts that pass them don't hard-fail.
    if args.patch || args.interactive || args.edit {
        // Real git would enter interactive mode; we just warn and succeed.
        let mode_name = if args.patch {
            "-p/--patch"
        } else if args.interactive {
            "-i/--interactive"
        } else {
            "-e/--edit"
        };
        eprintln!(
            "warning: {} mode is not yet implemented; doing nothing",
            mode_name
        );
        return Ok(());
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let core_filemode = config
        .get_bool("core.filemode")
        .and_then(|r| r.ok())
        .unwrap_or(true);
    let precompose_unicode =
        grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir));

    let index_path = resolved_env_index_path(&repo);
    let idx_exists = index_path.exists();
    let mut index = if idx_exists {
        repo.load_index_at(&index_path)?
    } else {
        Index::new_from_config(&config)
    };

    let odb = &repo.odb;

    // Resolve the current working directory relative to the worktree
    let cwd = std::env::current_dir()?;
    let prefix = crate::pathspec::pathdiff(&cwd, work_tree);
    die_if_in_unpopulated_submodule(&index, prefix.as_deref());

    // Validate empty string pathspecs
    for ps in &args.pathspec {
        if ps.is_empty() {
            bail!("invalid path ''");
        }
    }

    // "git add" with no pathspecs and no flags: give advice
    if args.pathspec.is_empty()
        && !args.all
        && !args.update
        && !args.refresh
        && args.chmod.is_none()
    {
        eprintln!("Nothing specified, nothing added.");
        eprintln!("hint: Maybe you wanted to say 'git add .'?");
        eprintln!(
            "hint: Disable this message with \"git config set advice.addEmptyPathspec false\""
        );
        return Ok(());
    }

    // --refresh mode
    if args.refresh {
        return run_refresh(&repo, &mut index, work_tree, prefix.as_deref(), &args);
    }

    // --chmod with no pathspecs: do nothing (don't error, just return)
    if args.chmod.is_some() && args.pathspec.is_empty() {
        if !args.dry_run {
            write_index_or_lock_err(&repo, &mut index, &index_path)?;
        }
        return Ok(());
    }

    // Build ignore matcher if needed (not needed with --force)
    let mut ignore_matcher = if !args.force {
        Some(IgnoreMatcher::from_repository(&repo)?)
    } else {
        None
    };

    let conv = ConversionConfig::from_config(&config);
    let attrs = crlf::load_gitattributes(work_tree);

    let add_cfg = AddConfig {
        core_filemode,
        precompose_unicode,
        ignore_errors: args.ignore_errors
            || config
                .get_bool("add.ignore-errors")
                .and_then(|r| r.ok())
                .unwrap_or(false),
        conv,
        attrs,
        config: config.clone(),
    };

    // --renormalize: re-apply clean conversion to tracked files
    if args.renormalize {
        return run_renormalize(
            &repo,
            odb,
            &mut index,
            work_tree,
            prefix.as_deref(),
            &args,
            &add_cfg,
        );
    }

    let is_root_pathspec = args.pathspec.iter().any(|p| p == ":/");
    if args.all || args.pathspec.iter().any(|p| p == ".") || is_root_pathspec {
        let effective_prefix = if is_root_pathspec {
            None
        } else {
            prefix.as_deref()
        };
        if args.all
            && !args.pathspec.is_empty()
            && !is_root_pathspec
            && !args.pathspec.iter().any(|p| p == ".")
        {
            add_all_for_pathspecs(
                odb,
                &mut index,
                work_tree,
                prefix.as_deref(),
                &args.pathspec,
                &args,
                &repo,
                &mut ignore_matcher,
                &add_cfg,
            )?;
        } else {
            add_all(
                odb,
                &mut index,
                work_tree,
                effective_prefix,
                &args,
                &repo,
                &mut ignore_matcher,
                &add_cfg,
            )?;
        }
    } else if args.update {
        update_tracked(
            odb,
            &mut index,
            work_tree,
            prefix.as_deref(),
            &args,
            &repo,
            &add_cfg,
        )?;
    } else {
        let mut had_errors = false;
        let mut had_ignored = false;
        for pathspec in &args.pathspec {
            let resolved =
                crate::pathspec::resolve_pathspec(pathspec, work_tree, prefix.as_deref());
            // Expand glob patterns (e.g. "file?.t", "*.c") against the working tree.
            let expanded = expand_glob_pathspec(&resolved, work_tree, add_cfg.precompose_unicode);
            for resolved in &expanded {
                match add_path(
                    odb,
                    &mut index,
                    work_tree,
                    resolved,
                    &args,
                    &repo,
                    &mut ignore_matcher,
                    &add_cfg,
                ) {
                    Ok(()) => {}
                    Err(AddPathError::Ignored(msg)) => {
                        eprintln!("{msg}");
                        had_ignored = true;
                        had_errors = true;
                    }
                    Err(AddPathError::IoError(e)) => {
                        if add_cfg.ignore_errors {
                            eprintln!("warning: {e}");
                            had_errors = true;
                        } else {
                            if is_unwritable_odb_error(&e) {
                                eprintln!(
                                    "error: insufficient permission for adding an object to repository database .git/objects"
                                );
                                eprintln!("error: {}: failed to insert into database", resolved);
                                eprintln!("error: unable to index file '{}'", resolved);
                                eprintln!("fatal: updating files failed");
                                std::process::exit(1);
                            }
                            return Err(e);
                        }
                    }
                    Err(AddPathError::Other(e)) => {
                        if add_cfg.ignore_errors {
                            eprintln!("warning: {e}");
                            had_errors = true;
                        } else {
                            return Err(e);
                        }
                    }
                }
            } // end expanded loop
        }

        if had_ignored {
            if !args.dry_run {
                write_index_or_lock_err(&repo, &mut index, &index_path)?;
            }
            bail!("some ignored files could not be added");
        }
        if had_errors {
            if !args.dry_run {
                write_index_or_lock_err(&repo, &mut index, &index_path)?;
            }
            if !add_cfg.ignore_errors {
                bail!("adding files failed");
            } else {
                // With --ignore-errors, still exit non-zero if there were errors
                std::process::exit(1);
            }
        }
    }

    if !args.dry_run {
        write_index_or_lock_err(&repo, &mut index, &index_path)?;
    }

    Ok(())
}

pub(crate) struct AddConfig {
    pub core_filemode: bool,
    pub precompose_unicode: bool,
    pub ignore_errors: bool,
    pub conv: ConversionConfig,
    pub attrs: GitAttributes,
    pub config: ConfigSet,
}

/// Stage pathspecs the same way `git commit <paths>` does (recursive dirs, CRLF clean, etc.).
pub(crate) fn stage_pathspecs_for_commit(
    repo: &Repository,
    work_tree: &Path,
    pathspecs: &[String],
    add_cfg: &AddConfig,
) -> Result<HashSet<Vec<u8>>> {
    let index_path = resolved_env_index_path(repo);
    let mut index = match repo.load_index_at(&index_path) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| work_tree.to_path_buf());
    let prefix = crate::pathspec::pathdiff(&cwd, work_tree);

    let mut ignore_matcher = Some(IgnoreMatcher::from_repository(repo)?);
    let odb = &repo.odb;
    let ctx = StageFileContext::for_commit();

    let mut matched_paths = HashSet::new();

    for spec in pathspecs {
        let resolved = crate::pathspec::resolve_pathspec(spec, work_tree, prefix.as_deref());

        if !crate::pathspec::has_glob_chars(&resolved) {
            let abs_path = work_tree.join(&resolved);
            let meta = match fs::symlink_metadata(&abs_path) {
                Ok(m) => m,
                Err(_) => {
                    index.remove(resolved.as_bytes());
                    matched_paths.insert(resolved.as_bytes().to_vec());
                    continue;
                }
            };

            let is_real_dir = !meta.file_type().is_symlink() && meta.file_type().is_dir();
            if is_real_dir {
                if is_nested_embedded_git_repo(&abs_path, repo) {
                    stage_gitlink(
                        odb, &mut index, work_tree, &resolved, &abs_path, false, false, false,
                    )?;
                    matched_paths.insert(resolved.as_bytes().to_vec());
                    continue;
                }
                let rels = collect_paths_for_stage_from_directory(
                    &abs_path,
                    work_tree,
                    repo,
                    &mut ignore_matcher,
                    false,
                    add_cfg.precompose_unicode,
                )?;
                for (rel, file_abs) in rels {
                    stage_file(
                        odb, &mut index, work_tree, &rel, &file_abs, repo, &ctx, add_cfg,
                    )?;
                    matched_paths.insert(rel.as_bytes().to_vec());
                }
                continue;
            }

            stage_file(
                odb, &mut index, work_tree, &resolved, &abs_path, repo, &ctx, add_cfg,
            )?;
            matched_paths.insert(resolved.as_bytes().to_vec());
            continue;
        }

        let (dir_prefix, pattern) = if let Some(slash_pos) = resolved.rfind('/') {
            (&resolved[..slash_pos], &resolved[slash_pos + 1..])
        } else {
            ("", resolved.as_str())
        };

        let search_dir = if dir_prefix.is_empty() {
            work_tree.to_path_buf()
        } else {
            work_tree.join(dir_prefix)
        };

        let mut spec_matched = false;
        let mut matched_rels: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let raw_name = file_name.to_string_lossy();
                let name_str = if add_cfg.precompose_unicode {
                    precompose_utf8_segment(raw_name.as_ref()).into_owned()
                } else {
                    raw_name.into_owned()
                };
                if name_str == ".git" {
                    continue;
                }
                if !wildmatch(pattern.as_bytes(), name_str.as_bytes(), 0) {
                    continue;
                }
                let rel = if dir_prefix.is_empty() {
                    name_str.clone()
                } else {
                    format!("{dir_prefix}/{name_str}")
                };
                matched_rels.push(rel);
            }
        }
        if pattern.contains('[') && fs::symlink_metadata(search_dir.join(pattern)).is_ok() {
            let rel = if dir_prefix.is_empty() {
                pattern.to_string()
            } else {
                format!("{dir_prefix}/{pattern}")
            };
            if !matched_rels.contains(&rel) {
                matched_rels.push(rel);
            }
        }

        for rel in matched_rels {
            let abs_path = work_tree.join(&rel);
            if fs::symlink_metadata(&abs_path).is_ok() {
                stage_file(
                    odb, &mut index, work_tree, &rel, &abs_path, repo, &ctx, add_cfg,
                )?;
                spec_matched = true;
                matched_paths.insert(rel.as_bytes().to_vec());
            }
        }

        if !spec_matched {
            bail!("pathspec '{spec}' did not match any file(s) known to git");
        }
    }

    repo.write_index_at(&index_path, &mut index)?;
    Ok(matched_paths)
}

#[allow(dead_code)]
enum AddPathError {
    Ignored(String),
    IoError(anyhow::Error),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for AddPathError {
    fn from(e: anyhow::Error) -> Self {
        AddPathError::Other(e)
    }
}

/// Run --refresh: update stat info in the index.
fn run_refresh(
    repo: &Repository,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
) -> Result<()> {
    if args.pathspec.is_empty() {
        // Refresh all entries
        for ie in &mut index.entries {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            if let Some(p) = prefix {
                if !path_str.starts_with(p) {
                    continue;
                }
            }
            let abs_path = work_tree.join(&path_str);
            if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                // Update stat fields but keep oid/mode
                ie.ctime_sec = meta.ctime() as u32;
                ie.ctime_nsec = meta.ctime_nsec() as u32;
                ie.mtime_sec = meta.mtime() as u32;
                ie.mtime_nsec = meta.mtime_nsec() as u32;
                ie.dev = meta.dev() as u32;
                ie.ino = meta.ino() as u32;
                ie.uid = meta.uid();
                ie.gid = meta.gid();
                ie.size = meta.len() as u32;
            }
        }
    } else {
        for pathspec in &args.pathspec {
            let resolved = crate::pathspec::resolve_pathspec(pathspec, work_tree, prefix);
            let found = index.entries.iter_mut().any(|ie| {
                let path_str = String::from_utf8_lossy(&ie.path);
                if path_str == resolved || path_str.starts_with(&format!("{resolved}/")) {
                    let abs_path = work_tree.join(path_str.as_ref());
                    if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                        ie.ctime_sec = meta.ctime() as u32;
                        ie.ctime_nsec = meta.ctime_nsec() as u32;
                        ie.mtime_sec = meta.mtime() as u32;
                        ie.mtime_nsec = meta.mtime_nsec() as u32;
                        ie.dev = meta.dev() as u32;
                        ie.ino = meta.ino() as u32;
                        ie.uid = meta.uid();
                        ie.gid = meta.gid();
                        ie.size = meta.len() as u32;
                    }
                    true
                } else {
                    false
                }
            });
            if !found && !args.ignore_missing {
                eprintln!(
                    "fatal: pathspec '{}' did not match any file(s) known to git",
                    pathspec
                );
                std::process::exit(128);
            }
        }
    }

    if !args.dry_run {
        write_index_or_lock_err(repo, index, &resolved_env_index_path(repo))?;
    }

    Ok(())
}

/// Re-apply clean conversion (CRLF normalization) to tracked files.
fn run_renormalize(
    repo: &Repository,
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    _prefix: Option<&str>,
    args: &Args,
    add_cfg: &AddConfig,
) -> Result<()> {
    // Reload gitattributes (may have been updated)
    let attrs = crlf::load_gitattributes(work_tree);

    // Collect paths to renormalize based on pathspecs
    let entries: Vec<(Vec<u8>, ObjectId, u32)> = index
        .entries
        .iter()
        .filter(|ie| {
            if ie.stage() != 0 {
                return false;
            }
            if args.pathspec.is_empty() {
                return true;
            }
            let path_str = String::from_utf8_lossy(&ie.path);
            args.pathspec.iter().any(|ps| {
                // Simple glob/prefix match
                let ps_clean = ps.trim_end_matches('*').trim_end_matches('/');
                path_str.starts_with(ps_clean) || glob_matches_simple(ps, &path_str)
            })
        })
        .map(|ie| (ie.path.clone(), ie.oid, ie.mode))
        .collect();

    for (path, oid, _mode) in entries {
        let rel_path = String::from_utf8_lossy(&path).to_string();
        let file_attrs = crlf::get_file_attrs(&attrs, &rel_path, false, &add_cfg.config);

        // Read current blob content
        let obj = odb.read(&oid).context("reading blob for renormalize")?;
        if obj.kind != ObjectKind::Blob {
            continue;
        }

        // Apply clean conversion
        let converted = match crlf::convert_to_git(&obj.data, &rel_path, &add_cfg.conv, &file_attrs)
        {
            Ok(c) => c,
            Err(_) => continue,
        };

        // If content changed, write new blob and update index
        if converted != obj.data {
            let new_oid = odb.write(ObjectKind::Blob, &converted)?;
            if let Some(entry) = index.get_mut(path.as_slice(), 0) {
                entry.oid = new_oid;
            }
        }
    }

    if !args.dry_run {
        let index_path = resolved_env_index_path(repo);
        write_index_or_lock_err(repo, index, &index_path)?;
    }

    Ok(())
}

fn glob_matches_simple(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text == pattern || text.starts_with(&format!("{pattern}/"));
    }
    // Simple glob: *.txt
    if let Some(suffix) = pattern.strip_prefix('*') {
        return text.ends_with(suffix);
    }
    text == pattern
}

/// Add all files under the working tree (or a prefix) to the index.
fn add_all(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> Result<()> {
    let scan_root = match prefix {
        Some(p) if !p.is_empty() => work_tree.join(p),
        _ => work_tree.to_path_buf(),
    };

    let mut paths: Vec<(String, PathBuf)> = Vec::new();
    walk_directory(
        &scan_root,
        work_tree,
        &mut paths,
        repo,
        ignore_matcher,
        args.force,
        add_cfg.precompose_unicode,
    )?;

    // Build a set of worktree paths for fast deletion detection
    let worktree_paths: std::collections::HashSet<&str> =
        paths.iter().map(|(r, _)| r.as_str()).collect();

    for (rel_path, abs_path) in &paths {
        if let Err(e) = stage_file(
            odb,
            index,
            work_tree,
            rel_path,
            abs_path,
            repo,
            &StageFileContext::from(args),
            add_cfg,
        ) {
            if add_cfg.ignore_errors {
                eprintln!("warning: {e}");
            } else {
                return Err(e);
            }
        }
    }

    // Handle deletions: index entries whose files are not in the worktree scan
    let prefix_bytes = prefix.map(|p| p.as_bytes());
    let removed: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|ie| {
            if let Some(pb) = prefix_bytes {
                if !index_path_under_prefix(&ie.path, pb) {
                    return false;
                }
            }
            // In sparse-checkout mode, entries outside the sparse view are
            // marked skip-worktree and may legitimately be absent from the
            // working tree. Do not treat those as deletions for `git add .`.
            if ie.skip_worktree() {
                return false;
            }
            let path_str = std::str::from_utf8(&ie.path).unwrap_or("");
            !worktree_paths.contains(path_str)
        })
        .map(|ie| ie.path.clone())
        .collect();

    for path in removed {
        if args.verbose {
            let path_str = String::from_utf8_lossy(&path);
            eprintln!("remove '{path_str}'");
        }
        if !args.dry_run {
            index.remove(&path);
        }
    }

    Ok(())
}

/// True when `path` (index UTF-8 path) is exactly `prefix` or under `prefix/` (path component boundary).
fn index_path_under_prefix(path: &[u8], prefix: &[u8]) -> bool {
    if path == prefix {
        return true;
    }
    path.len() > prefix.len() && path.starts_with(prefix) && path[prefix.len()] == b'/'
}

fn path_matches_any_resolved_spec(path: &str, specs: &[String]) -> bool {
    specs
        .iter()
        .any(|s| path == s.as_str() || path.starts_with(&format!("{s}/")))
}

/// `git add -A <pathspec>...` — stage updates only under the given pathspecs and record deletions
/// there (not the whole tree). Matches Git when path arguments are present with `-A`.
fn add_all_for_pathspecs(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    cwd_prefix: Option<&str>,
    pathspecs: &[String],
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> Result<()> {
    let mut resolved_specs: Vec<String> = Vec::new();

    for ps in pathspecs {
        let resolved = crate::pathspec::resolve_pathspec(ps, work_tree, cwd_prefix);
        let expanded = expand_glob_pathspec(&resolved, work_tree, add_cfg.precompose_unicode);
        for r in expanded {
            resolved_specs.push(r);
        }
    }

    resolved_specs.sort();
    resolved_specs.dedup();

    let mut had_ignored = false;
    let mut had_errors = false;
    let mut any_staged = false;
    for r in &resolved_specs {
        match add_path(
            odb,
            index,
            work_tree,
            r,
            args,
            repo,
            ignore_matcher,
            add_cfg,
        ) {
            Ok(()) => {
                any_staged = true;
            }
            Err(AddPathError::Ignored(msg)) => {
                eprintln!("{msg}");
                had_ignored = true;
                had_errors = true;
            }
            Err(AddPathError::IoError(e)) => {
                if add_cfg.ignore_errors {
                    eprintln!("warning: {e}");
                    had_errors = true;
                } else {
                    return Err(e);
                }
            }
            Err(AddPathError::Other(e)) => {
                let s = e.to_string();
                if pathspecs.len() > 1
                    && (s.contains("did not match any files")
                        || s.contains("did not match any file"))
                {
                    continue;
                }
                if add_cfg.ignore_errors {
                    eprintln!("warning: {e}");
                    had_errors = true;
                } else {
                    return Err(e);
                }
            }
        }
    }

    if !any_staged && !had_ignored && !had_errors {
        bail!(
            "pathspec '{}' did not match any file(s) known to git",
            pathspecs.join(" ")
        );
    }

    if had_ignored {
        bail!("some ignored files could not be added");
    }
    if had_errors && !add_cfg.ignore_errors {
        bail!("adding files failed");
    }

    let to_remove: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|ie| {
            if ie.stage() != 0 {
                return false;
            }
            if ie.skip_worktree() {
                return false;
            }
            let path_str = std::str::from_utf8(&ie.path).unwrap_or("");
            if !path_matches_any_resolved_spec(path_str, &resolved_specs) {
                return false;
            }
            fs::symlink_metadata(work_tree.join(path_str)).is_err()
        })
        .map(|ie| ie.path.clone())
        .collect();

    for path in to_remove {
        if args.verbose {
            let path_str = String::from_utf8_lossy(&path);
            eprintln!("remove '{path_str}'");
        }
        if !args.dry_run {
            index.remove(&path);
        }
    }

    Ok(())
}

/// Update only already-tracked files.
fn update_tracked(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    prefix: Option<&str>,
    args: &Args,
    repo: &Repository,
    add_cfg: &AddConfig,
) -> Result<()> {
    // If explicit pathspecs given with -u, validate that each matches a tracked file.
    let explicit_pathspecs = !args.pathspec.is_empty();
    if explicit_pathspecs {
        let pfx = prefix.unwrap_or("");
        for spec in &args.pathspec {
            // Build the full path as it would appear in the index
            let full_spec = if pfx.is_empty() || spec.starts_with('/') {
                spec.clone()
            } else {
                format!("{pfx}/{spec}")
            };
            let matches_tracked = spec == "."
                || spec.is_empty()
                || index.entries.iter().any(|ie| {
                    let p = String::from_utf8_lossy(&ie.path);
                    p == full_spec.as_str()
                        || p.starts_with(&format!("{full_spec}/"))
                        || p == spec.as_str()
                        || p.starts_with(&format!("{spec}/"))
                });
            if !matches_tracked {
                eprintln!("error: pathspec '{spec}' did not match any file(s) known to git");
                std::process::exit(128);
            }
        }
    }

    let tracked: Vec<(Vec<u8>, String)> = index
        .entries
        .iter()
        .filter(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path);
            // Apply prefix filter ONLY when explicit pathspecs are given.
            // Without explicit pathspecs, git add -u updates ALL tracked files from root.
            let prefix_ok = if explicit_pathspecs {
                prefix.map(|p| path_str.starts_with(p)).unwrap_or(true)
            } else {
                true // update everything from root
            };
            // Apply explicit pathspec filter
            let pathspec_ok = if explicit_pathspecs {
                let pfx2 = prefix.unwrap_or("");
                args.pathspec.iter().any(|spec| {
                    let full = if pfx2.is_empty() {
                        spec.clone()
                    } else {
                        format!("{pfx2}/{spec}")
                    };
                    spec == "."
                        || path_str == full.as_str()
                        || path_str.starts_with(&format!("{full}/"))
                        || path_str == spec.as_str()
                        || path_str.starts_with(&format!("{spec}/"))
                })
            } else {
                true
            };
            prefix_ok && pathspec_ok
        })
        .map(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            (ie.path.clone(), path_str)
        })
        .collect();

    for (raw_path, path_str) in &tracked {
        let abs_path = work_tree.join(path_str);
        // Use symlink_metadata so a symlink whose target was removed still counts as
        // present (`exists()` follows the link and returns false).
        if let Ok(meta) = fs::symlink_metadata(&abs_path) {
            // A tracked blob replaced by a plain directory: `git add -u` only drops the index entry
            // so a later `git add` can record the directory (matches git). Embedded repos still stage
            // as gitlinks via `stage_file`.
            let is_plain_directory = meta.is_dir() && !meta.file_type().is_symlink();
            let is_embedded_repo = abs_path.join(".git").exists();
            if is_plain_directory && !is_embedded_repo {
                if args.verbose || args.dry_run {
                    println!("remove '{path_str}'");
                }
                if !args.dry_run {
                    index.remove(raw_path);
                }
            } else if args.dry_run {
                // For dry-run, hash without writing to ODB
                if let Ok(data) = std::fs::read(&abs_path) {
                    let oid = grit_lib::odb::Odb::hash_object_data(
                        grit_lib::objects::ObjectKind::Blob,
                        &data,
                    );
                    let current = index.get(raw_path, 0);
                    if current.map(|e| e.oid != oid).unwrap_or(false) {
                        println!("add '{path_str}'");
                    }
                }
            } else {
                stage_file(
                    odb,
                    index,
                    work_tree,
                    path_str,
                    &abs_path,
                    repo,
                    &StageFileContext::from(args),
                    add_cfg,
                )?;
            }
        } else {
            if args.verbose || args.dry_run {
                println!("remove '{path_str}'");
            }
            if !args.dry_run {
                index.remove(raw_path);
            }
        }
    }

    Ok(())
}

/// Resolve `rel` to the spelling that exists on disk when NFC/NFD differ (Linux + precompose).
fn resolve_add_path_on_disk(
    work_tree: &Path,
    rel: &str,
    precompose_unicode: bool,
) -> (PathBuf, String) {
    let abs = work_tree.join(rel);
    if fs::symlink_metadata(&abs).is_ok() {
        return (abs, rel.to_owned());
    }
    if !precompose_unicode {
        return (abs, rel.to_owned());
    }
    let p = Path::new(rel);
    let want_leaf = precompose_utf8_path(
        p.file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
            .as_ref(),
    )
    .into_owned();
    if want_leaf.is_empty() {
        return (abs, rel.to_owned());
    }
    let parent_rel = p.parent().filter(|x| !x.as_os_str().is_empty());
    let parent_abs = parent_rel
        .map(|pr| work_tree.join(pr))
        .unwrap_or_else(|| work_tree.to_path_buf());
    if let Ok(rd) = fs::read_dir(&parent_abs) {
        for ent in rd.flatten() {
            let n = ent.file_name().to_string_lossy().into_owned();
            if precompose_utf8_path(&n).as_ref() == want_leaf.as_str() {
                let new_rel = match parent_rel {
                    Some(pr) => format!("{}/{}", pr.to_string_lossy(), n),
                    None => n,
                };
                return (work_tree.join(&new_rel), new_rel);
            }
        }
    }
    (abs, rel.to_owned())
}

/// Add a single pathspec (which may be a file or directory).
fn add_path(
    odb: &Odb,
    index: &mut Index,
    work_tree: &Path,
    path: &str,
    args: &Args,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    add_cfg: &AddConfig,
) -> std::result::Result<(), AddPathError> {
    let (abs_path, path_on_disk) =
        resolve_add_path_on_disk(work_tree, path, add_cfg.precompose_unicode);
    let path = path_on_disk.as_str();

    // Refuse to add a path inside a registered submodule (gitlink).
    // Only reject when a *proper* parent directory is a gitlink;
    // adding the submodule entry itself (e.g. `git add embed`) is fine.
    {
        let components: Vec<&str> = path.split('/').collect();
        let mut prefix = String::new();
        for &component in &components[..components.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if let Some(ie) = index.get(prefix.as_bytes(), 0) {
                if ie.mode == 0o160000 {
                    eprintln!("fatal: Pathspec '{}' is in submodule '{}'", path, prefix);
                    std::process::exit(128);
                }
            }
        }
    }

    // Refuse to add a path that traverses through a symbolic link.
    if check_symlink_in_path(work_tree, Path::new(path)).is_some() {
        return Err(AddPathError::Other(anyhow::anyhow!(
            "'{}' is beyond a symbolic link",
            path
        )));
    }

    // Use symlink_metadata to detect dangling symlinks (exists() follows symlinks)
    if fs::symlink_metadata(&abs_path).is_err() {
        let path_bytes = path.as_bytes();
        // Check if it's an index entry that needs to be removed
        if index.get(path_bytes, 0).is_some() {
            if !args.dry_run {
                index.remove(path_bytes);
            }
            if args.verbose {
                eprintln!("remove '{path}'");
            }
            return Ok(());
        }
        // Check unmerged entries (stages 1, 2, 3)
        let has_unmerged = (1..=3).any(|stage| index.get(path_bytes, stage).is_some());
        if has_unmerged {
            // Can't resolve a conflict if file doesn't exist
            return Err(AddPathError::Other(anyhow::anyhow!(
                "pathspec '{}' did not match any files",
                path
            )));
        }
        return Err(AddPathError::Other(anyhow::anyhow!(
            "pathspec '{}' did not match any files",
            path
        )));
    }

    // Use symlink_metadata so symlinks to directories are staged as
    // symlinks, not traversed.
    let is_real_dir = fs::symlink_metadata(&abs_path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if is_real_dir {
        // Check if the directory itself is ignored (reject unless -f)
        if !args.force {
            if let Some(ref mut matcher) = ignore_matcher {
                // Check the directory path (with trailing slash for dir matching)
                let dir_path_slash = format!("{path}/");
                let (is_ignored, _) = matcher
                    .check_path(repo, Some(&*index), path, true)
                    .or_else(|_| matcher.check_path(repo, Some(&*index), &dir_path_slash, true))
                    .unwrap_or((false, None));
                if is_ignored {
                    return Err(AddPathError::Ignored(format!(
                        "The following paths are ignored by one of your .gitignore files:\n\
                         {path}\n\
                         Use -f if you really want to add them."
                    )));
                }
            }
        }

        // Nested repository (subdirectory with its own .git, not the superproject root).
        if is_nested_embedded_git_repo(&abs_path, repo) {
            return stage_gitlink(
                odb,
                index,
                work_tree,
                path,
                &abs_path,
                args.dry_run,
                args.verbose,
                args.no_warn_embedded_repo,
            )
            .map_err(AddPathError::IoError);
        }

        let mut paths: Vec<(String, PathBuf)> = Vec::new();
        walk_directory(
            &abs_path,
            work_tree,
            &mut paths,
            repo,
            ignore_matcher,
            args.force,
            add_cfg.precompose_unicode,
        )?;
        for (rel_path, file_abs) in &paths {
            if let Err(e) = stage_file(
                odb,
                index,
                work_tree,
                rel_path,
                file_abs,
                repo,
                &StageFileContext::from(args),
                add_cfg,
            ) {
                if add_cfg.ignore_errors {
                    eprintln!("warning: {e}");
                } else {
                    return Err(AddPathError::IoError(e));
                }
            }
        }
    } else {
        // Allow adding ignored files when resolving merge conflicts (unmerged entries).
        let path_bytes = path.as_bytes();
        let has_unmerged = (1..=3).any(|stage| index.get(path_bytes, stage).is_some());

        // Check ignore patterns for explicitly named files (like real git),
        // but skip the check if the file has unmerged entries (conflict resolution).
        if !has_unmerged {
            if let Some(ref mut matcher) = ignore_matcher {
                let (is_ignored, _match_info) = matcher
                    .check_path(repo, Some(&*index), path, false)
                    .map_err(|e| AddPathError::Other(e.into()))?;
                if is_ignored {
                    return Err(AddPathError::Ignored(format!(
                        "The following paths are ignored by one of your .gitignore files:\n\
                         {path}\n\
                         Use -f if you really want to add them."
                    )));
                }
            }
        }
        stage_file(
            odb,
            index,
            work_tree,
            path,
            &abs_path,
            repo,
            &StageFileContext::from(args),
            add_cfg,
        )
        .map_err(AddPathError::IoError)?;
    }

    Ok(())
}

/// Resolve the Git directory for an embedded repository work tree.
///
/// Supports a `.git` directory or a `.git` file with `gitdir:` (submodule-style layout).
fn embedded_repository_git_dir(worktree: &Path) -> Result<PathBuf> {
    let dot_git = worktree.join(".git");
    let meta = fs::symlink_metadata(&dot_git)
        .with_context(|| format!("cannot stat .git in embedded repo {}", worktree.display()))?;
    if meta.file_type().is_dir() {
        return Ok(dot_git);
    }
    let content = fs::read_to_string(&dot_git)
        .with_context(|| format!("cannot read .git file in {}", worktree.display()))?;
    let line = content.lines().next().unwrap_or("").trim();
    let rest = line.strip_prefix("gitdir:").map(str::trim).ok_or_else(|| {
        anyhow!(
            "invalid .git file in {} (expected gitdir:)",
            worktree.display()
        )
    })?;
    let p = Path::new(rest);
    Ok(if p.is_absolute() {
        p.to_path_buf()
    } else {
        worktree.join(p)
    })
}

/// Stage an embedded repository as a gitlink (mode 160000) in the index.
///
/// Reads the HEAD of the embedded repo to get the commit OID, and warns
/// (unless `--no-warn-embedded-repo` is set) that a bare `git add` of an
/// embedded repo is probably a mistake.
fn stage_gitlink(
    _odb: &Odb,
    index: &mut Index,
    _work_tree: &Path,
    rel_path: &str,
    abs_path: &Path,
    dry_run: bool,
    verbose: bool,
    no_warn_embedded_repo: bool,
) -> Result<()> {
    let git_dir = embedded_repository_git_dir(abs_path)?;
    let embedded_head_path = git_dir.join("HEAD");
    let head_content = fs::read_to_string(&embedded_head_path)
        .with_context(|| format!("cannot read HEAD of embedded repo '{}'", rel_path))?;
    let head_trimmed = head_content.trim();

    // Resolve HEAD: prefer `refs::resolve_ref` (packed-refs, worktrees). If `HEAD` is a stale
    // symref to `refs/heads/master` while only `main` exists (common with
    // `GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME`), fall back like Git's `resolve_gitlink_ref`.
    let oid = if head_trimmed.starts_with("ref: ") {
        match refs::resolve_ref(&git_dir, "HEAD") {
            Ok(o) => o,
            Err(_) => {
                let mut found = None;
                for branch in ["main", "master"] {
                    let p = git_dir.join("refs/heads").join(branch);
                    if let Ok(s) = fs::read_to_string(&p) {
                        if let Ok(o) = ObjectId::from_hex(s.trim()) {
                            found = Some(o);
                            break;
                        }
                    }
                }
                found
                    .ok_or_else(|| anyhow!("cannot resolve HEAD in embedded repo '{}'", rel_path))?
            }
        }
    } else {
        ObjectId::from_hex(head_trimmed)
            .with_context(|| format!("invalid HEAD OID in embedded repo '{}'", rel_path))?
    };

    // Check whether this entry is already tracked as a gitlink in the index
    let already_tracked = index
        .get(rel_path.as_bytes(), 0)
        .map(|e| e.mode == 0o160000)
        .unwrap_or(false);

    // Warn about embedded repository unless suppressed
    if !no_warn_embedded_repo && !already_tracked {
        eprintln!("warning: adding embedded git repository: {}", rel_path);
        eprintln!("hint: You've added another git repository inside your current repository.");
        eprintln!("hint: Clones of the outer repository will not contain the contents of");
        eprintln!("hint: the embedded repository and will not know how to obtain it.");
        eprintln!("hint: If you meant to add a submodule, use:");
        eprintln!("hint: ");
        eprintln!("hint: \tgit submodule add <url> {}", rel_path);
        eprintln!("hint: ");
        eprintln!("hint: If you added this path by mistake, you can remove it from the");
        eprintln!("hint: index with:");
        eprintln!("hint: ");
        eprintln!("hint: \tgit rm --cached {}", rel_path);
        eprintln!("hint: ");
        eprintln!("hint: See \"git help submodule\" for more information.");
    }

    if dry_run {
        println!("add '{}'", rel_path);
        return Ok(());
    }

    remove_obstructing_parent_file_entries(index, rel_path);
    index.remove_descendants_under_path(rel_path);

    let meta = fs::metadata(abs_path)?;
    let entry = IndexEntry {
        ctime_sec: meta.ctime() as u32,
        ctime_nsec: meta.ctime_nsec() as u32,
        mtime_sec: meta.mtime() as u32,
        mtime_nsec: meta.mtime_nsec() as u32,
        dev: meta.dev() as u32,
        ino: meta.ino() as u32,
        mode: 0o160000, // gitlink mode
        uid: meta.uid(),
        gid: meta.gid(),
        size: 0,
        oid,
        flags: rel_path.len().min(0xFFF) as u16,
        flags_extended: None,
        path: rel_path.as_bytes().to_vec(),
    };
    index.add_or_replace(entry);

    if verbose {
        println!("add '{}'", rel_path);
    }

    Ok(())
}

fn path_is_symlink(abs_path: &Path) -> bool {
    fs::symlink_metadata(abs_path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// True when `abs_path/.git` is a **nested** repository, not this repo's own `.git` at the work tree root.
fn is_nested_embedded_git_repo(abs_path: &Path, repo: &Repository) -> bool {
    let embedded = abs_path.join(".git");
    if !embedded.exists() {
        return false;
    }
    let Ok(emb) = fs::canonicalize(&embedded) else {
        return true;
    };
    let Ok(super_git) = fs::canonicalize(&repo.git_dir) else {
        return true;
    };
    emb != super_git
}

fn remove_obstructing_parent_file_entries(index: &mut Index, rel_path: &str) {
    for (i, ch) in rel_path.char_indices() {
        if ch != '/' {
            continue;
        }
        let prefix = &rel_path[..i];
        let prefix_bytes = prefix.as_bytes();
        if let Some(e) = index.get(prefix_bytes, 0) {
            let is_tree = e.mode & 0o170000 == 0o040000;
            if !is_tree {
                index.remove(prefix_bytes);
            }
        }
    }
}

/// Stage a single file into the index.
pub(crate) fn stage_file(
    odb: &Odb,
    index: &mut Index,
    _work_tree: &Path,
    rel_path: &str,
    abs_path: &Path,
    repo: &Repository,
    ctx: &StageFileContext<'_>,
    add_cfg: &AddConfig,
) -> Result<()> {
    if ctx.dry_run {
        if ctx.chmod.is_some() {
            // Don't actually stage, just check if the file exists
            return Ok(());
        }
        println!("add '{rel_path}'");
        return Ok(());
    }

    remove_obstructing_parent_file_entries(index, rel_path);
    index.remove_descendants_under_path(rel_path);

    if rel_path.ends_with(".gitattributes") && !path_is_symlink(abs_path) {
        let content = fs::read_to_string(abs_path).unwrap_or_default();
        let parsed = parse_gitattributes_file_content(&content, rel_path);
        if let Err(msg) = validate_rules_for_add(&parsed.rules, rel_path) {
            eprintln!("{msg}");
            // Do not stage invalid gitattributes; match Git behavior (exit 0, message on stderr).
            return Ok(());
        }
    }

    let meta = fs::symlink_metadata(abs_path)?;

    // Submodule / embedded repo roots appear as directories with `.git`; `walk_directory` records
    // the directory path without recursing, so we must stage them as gitlinks here.
    if meta.is_dir()
        && !meta.file_type().is_symlink()
        && is_nested_embedded_git_repo(abs_path, repo)
    {
        return stage_gitlink(
            odb,
            index,
            _work_tree,
            rel_path,
            abs_path,
            ctx.dry_run,
            ctx.verbose,
            false,
        );
    }

    if ctx.intent_to_add {
        // Don't clobber existing entries — only add the intent marker if not already staged
        if index.get(rel_path.as_bytes(), 0).is_some() {
            return Ok(());
        }
        let mode = if meta.file_type().is_symlink() {
            0o120000
        } else if add_cfg.core_filemode {
            normalize_mode(meta.mode())
        } else {
            0o100644 // When core.filemode=false, default to regular
        };
        let empty_oid = odb
            .write(ObjectKind::Blob, b"")
            .with_context(|| format!("writing empty blob for intent-to-add '{rel_path}'"))?;
        let mut entry = IndexEntry {
            ctime_sec: meta.ctime() as u32,
            ctime_nsec: meta.ctime_nsec() as u32,
            mtime_sec: meta.mtime() as u32,
            mtime_nsec: meta.mtime_nsec() as u32,
            dev: meta.dev() as u32,
            ino: meta.ino() as u32,
            mode,
            uid: meta.uid(),
            gid: meta.gid(),
            size: 0,
            oid: empty_oid,
            flags: rel_path.len().min(0xFFF) as u16,
            flags_extended: None,
            path: rel_path.as_bytes().to_vec(),
        };
        entry.set_intent_to_add(true);
        index.add_or_replace(entry);
        if ctx.verbose {
            println!("add '{rel_path}'");
        }
        return Ok(());
    }

    // Determine mode
    let is_symlink = meta.file_type().is_symlink();
    let mode = if is_symlink {
        0o120000
    } else if add_cfg.core_filemode {
        normalize_mode(meta.mode())
    } else {
        // core.filemode=false: preserve existing mode from index if any,
        // otherwise default to 100644
        // Check for unmerged entries: prefer higher stages for mode
        let existing_mode = index
            .get(rel_path.as_bytes(), 0)
            .or_else(|| index.get(rel_path.as_bytes(), 2))
            .or_else(|| index.get(rel_path.as_bytes(), 1))
            .map(|e| e.mode);
        existing_mode.unwrap_or(0o100644)
    };

    // Handle --chmod flag
    let final_mode = if let Some(chmod_val) = ctx.chmod {
        if is_symlink {
            let display_path = rel_path;
            eprintln!("warning: cannot chmod {} '{}'", chmod_val, display_path);
            return Err(anyhow::anyhow!(
                "cannot chmod {} '{}'",
                chmod_val,
                display_path
            ));
        }
        match chmod_val {
            "+x" => 0o100755,
            "-x" => 0o100644,
            other => bail!("unrecognized --chmod value: {}", other),
        }
    } else {
        mode
    };

    // Do not skip based on stat cache alone: mtime/ctime can match across different contents
    // (common in tests and on fast filesystems), which would leave the index stale (t7601).
    // Also do not skip based on stat alone: two different blobs can share size/mtime (e.g. "0\n"
    // vs "1\n"), which breaks `git add -u` after small single-digit edits (t3415-rebase-autosquash).

    // Read file content and hash it
    let data = if is_symlink {
        let target = fs::read_link(abs_path)?;
        target.to_string_lossy().into_owned().into_bytes()
    } else {
        let raw = fs::read(abs_path)?;
        // Apply CRLF / clean-filter conversion
        let file_attrs = crlf::get_file_attrs(&add_cfg.attrs, rel_path, false, &add_cfg.config);
        // Apply working-tree-encoding conversion (e.g. UTF-16 → UTF-8)
        let raw = if let Some(ref encoding) = file_attrs.working_tree_encoding {
            convert_from_working_tree_encoding(&raw, encoding).with_context(|| {
                format!(
                    "failed to convert '{}' from encoding '{}'",
                    rel_path, encoding
                )
            })?
        } else {
            raw
        };
        let prior_blob: Option<Vec<u8>> = index
            .get(rel_path.as_bytes(), 0)
            .filter(|e| e.oid != ObjectId::zero())
            .and_then(|e| odb.read(&e.oid).ok())
            .map(|o| o.data);
        let opts = crlf::ConvertToGitOpts {
            index_blob: prior_blob.as_deref(),
            renormalize: false,
            check_safecrlf: true,
        };
        match crlf::convert_to_git_with_opts(&raw, rel_path, &add_cfg.conv, &file_attrs, opts) {
            Ok(converted) => converted,
            Err(msg) => bail!("{msg}"),
        }
    };

    let oid = odb
        .write(ObjectKind::Blob, &data)
        .map_err(anyhow::Error::from)?;
    let mut entry = entry_from_metadata(&meta, rel_path.as_bytes(), oid, final_mode);
    entry.mode = final_mode; // Ensure mode override sticks
    entry.set_assume_unchanged(false);
    entry.set_skip_worktree(false);
    // Use stage_file which also clears conflict stages (1, 2, 3) for the same
    // path — this is how `git add` resolves merge/cherry-pick conflicts.
    index.stage_file(entry);

    if ctx.verbose {
        println!("add '{rel_path}'");
    }

    Ok(())
}

fn is_unwritable_odb_error(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            if io_err.kind() == std::io::ErrorKind::PermissionDenied {
                return true;
            }
        }
        if let Some(grit_err) = cause.downcast_ref::<grit_lib::error::Error>() {
            if let grit_lib::error::Error::Io(io_err) = grit_err {
                if io_err.kind() == std::io::ErrorKind::PermissionDenied {
                    return true;
                }
            }
        }
    }
    err.to_string().contains("Permission denied")
}

fn is_unwritable_lock_error(err: &grit_lib::error::Error) -> bool {
    matches!(
        err,
        grit_lib::error::Error::Io(io_err) if io_err.kind() == std::io::ErrorKind::AlreadyExists
    )
}

fn write_index_or_lock_err(repo: &Repository, index: &mut Index, index_path: &Path) -> Result<()> {
    repo.write_index_at(index_path, index).map_err(|e| {
        if is_unwritable_lock_error(&e) {
            let mut msg = format!(
                "Unable to create '{}': File exists.",
                index_path.with_extension("lock").display()
            );

            if let Some(pid_msg) = lockfile_pid_diagnostic(index_path) {
                msg.push_str("\n\n");
                msg.push_str(&pid_msg);
            } else {
                msg.push_str("\n\n");
                msg.push_str(
                    "Another git process seems to be running in this repository, or the lock file may be stale",
                );
            }

            anyhow!(msg)
        } else {
            anyhow!(e)
        }
    })
}

fn lockfile_pid_diagnostic(index_path: &Path) -> Option<String> {
    let pid_path = index_path.with_file_name("index~pid.lock");
    let pid_text = fs::read_to_string(&pid_path).ok()?;
    let pid = parse_pid_file(&pid_text)?;

    if is_pid_running(pid) {
        Some(format!(
            "Lock may be held by process {pid}; if no git process is running, the lock file may be stale (PIDs can be reused)"
        ))
    } else {
        Some(format!(
            "Lock was held by process {pid}, which is no longer running; the lock file appears to be stale"
        ))
    }
}

fn parse_pid_file(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    let pid_str = trimmed.strip_prefix("pid ")?;
    pid_str.trim().parse::<u32>().ok()
}

fn is_pid_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let proc_path = Path::new("/proc").join(pid.to_string());
        proc_path.exists()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Collect `(index path, absolute path)` pairs under `dir` for staging.
pub(crate) fn collect_paths_for_stage_from_directory(
    dir: &Path,
    work_tree: &Path,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    force: bool,
    precompose_unicode: bool,
) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    walk_directory(
        dir,
        work_tree,
        &mut out,
        repo,
        ignore_matcher,
        force,
        precompose_unicode,
    )?;
    Ok(out)
}

/// Recursively walk a directory, collecting index-relative paths and their on-disk paths.
fn walk_directory(
    dir: &Path,
    work_tree: &Path,
    out: &mut Vec<(String, PathBuf)>,
    repo: &Repository,
    ignore_matcher: &mut Option<IgnoreMatcher>,
    force: bool,
    precompose_unicode: bool,
) -> Result<()> {
    let entries = fs::read_dir(dir)?;
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str == ".git" {
            continue;
        }

        let rel_fs = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        let rel_index = if precompose_unicode {
            grit_lib::unicode_normalization::precompose_utf8_path(&rel_fs).into_owned()
        } else {
            rel_fs.clone()
        };

        // Use symlink_metadata to detect symlinks *before* following them.
        // A symlink to a directory should be stored as a symlink blob,
        // not traversed into.
        let ft = match fs::symlink_metadata(&path) {
            Ok(m) => m.file_type(),
            Err(_) => continue,
        };
        let is_symlink = ft.is_symlink();
        let is_dir = !is_symlink && ft.is_dir();

        // Check if ignored
        if !force {
            if let Some(matcher) = ignore_matcher.as_mut() {
                if let Ok((ignored, _)) = matcher.check_path(repo, None, &rel_index, is_dir) {
                    if ignored {
                        continue;
                    }
                }
            }
        }

        if is_dir {
            // Nested repository (submodule or embedded repo): record the directory itself so
            // `git add .` does not treat an existing gitlink as deleted (the inner `.git` is
            // skipped below and would otherwise hide the whole tree from the scan).
            if path.join(".git").exists() {
                out.push((rel_index, path));
                continue;
            }
            walk_directory(
                &path,
                work_tree,
                out,
                repo,
                ignore_matcher,
                force,
                precompose_unicode,
            )?;
        } else {
            out.push((rel_index, path));
        }
    }

    Ok(())
}

/// Exit when running inside an unpopulated submodule worktree path.
///
/// Git discovers the superproject when invoked from an unpopulated submodule
/// directory. In that case, commands like `git -C sub add .` must fail with an
/// "in unpopulated submodule" fatal message instead of silently operating on
/// the superproject index.
fn die_if_in_unpopulated_submodule(index: &Index, prefix: Option<&str>) {
    let Some(prefix) = prefix else {
        return;
    };
    if prefix.is_empty() {
        return;
    }

    let prefix_bytes = prefix.as_bytes();
    for entry in &index.entries {
        if entry.mode != 0o160000 {
            continue;
        }
        let ce = entry.path.as_slice();
        let is_exact = prefix_bytes == ce;
        let is_inside = prefix_bytes.len() > ce.len()
            && prefix_bytes.starts_with(ce)
            && prefix_bytes[ce.len()] == b'/';
        if is_exact || is_inside {
            eprintln!(
                "fatal: in unpopulated submodule '{}'",
                String::from_utf8_lossy(ce)
            );
            std::process::exit(128);
        }
    }
}

/// Walk the parent components of `rel_path` (relative to `work_tree`) and
/// return `Some(prefix)` if any of them is a symbolic link.  Only *parent*
/// components are checked — the final path component itself may be a symlink.
fn check_symlink_in_path(work_tree: &Path, rel_path: &Path) -> Option<PathBuf> {
    let mut accumulated = PathBuf::new();
    let components: Vec<_> = rel_path.components().collect();
    // Check all components except the last one (the file itself).
    for component in components.iter().take(components.len().saturating_sub(1)) {
        accumulated.push(component);
        let abs = work_tree.join(&accumulated);
        if let Ok(meta) = fs::symlink_metadata(&abs) {
            if meta.file_type().is_symlink() {
                return Some(accumulated);
            }
        }
    }
    None
}

/// Expand a pathspec containing glob characters against the working tree.
///
/// If the pathspec does not contain glob characters, returns it unchanged.
/// Otherwise, matches it against files/dirs in the working tree directory.
pub(crate) fn expand_glob_pathspec(
    pathspec: &str,
    work_tree: &Path,
    precompose_unicode: bool,
) -> Vec<String> {
    if !crate::pathspec::has_glob_chars(pathspec) {
        return vec![pathspec.to_owned()];
    }

    // Split into directory prefix and glob pattern.
    // e.g. "dir/file?.t" -> dir_prefix="dir", pattern="file?.t"
    let (dir_prefix, pattern) = if let Some(slash_pos) = pathspec.rfind('/') {
        (&pathspec[..slash_pos], &pathspec[slash_pos + 1..])
    } else {
        ("", pathspec)
    };

    let search_dir = if dir_prefix.is_empty() {
        work_tree.to_owned()
    } else {
        work_tree.join(dir_prefix)
    };

    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(&search_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let raw = name.to_string_lossy();
            let raw_owned = raw.into_owned();
            let name_for_match = if precompose_unicode {
                precompose_utf8_segment(raw_owned.as_ref()).into_owned()
            } else {
                raw_owned.clone()
            };
            if name_for_match == ".git" {
                continue;
            }
            if wildmatch(pattern.as_bytes(), name_for_match.as_bytes(), 0) {
                // Use the filesystem spelling for `rel` so `open()` works on Linux when the index
                // stores NFC but `readdir` returns NFD (t3910 `git add *` on long filenames).
                let fs_name = raw_owned;
                let rel = if dir_prefix.is_empty() {
                    fs_name
                } else {
                    format!("{dir_prefix}/{fs_name}")
                };
                matches.push(rel);
            }
        }
    }

    // Git pathspec: a bracket pattern like `[abc]` matches one character class member *and*
    // the literal filename `[abc]` when present (wildmatch alone does not match that literal).
    if pattern.contains('[') && fs::symlink_metadata(search_dir.join(pattern)).is_ok() {
        let rel = if dir_prefix.is_empty() {
            pattern.to_string()
        } else {
            format!("{dir_prefix}/{pattern}")
        };
        if !matches.contains(&rel) {
            matches.push(rel);
        }
    }

    if matches.is_empty() {
        // No matches — return original pathspec so add_path gives a proper error.
        vec![pathspec.to_owned()]
    } else {
        matches.sort();
        matches
    }
}

/// Convert file content from a working-tree encoding to UTF-8.
pub(crate) fn convert_from_working_tree_encoding(data: &[u8], encoding: &str) -> Result<Vec<u8>> {
    let enc_upper = encoding.to_uppercase().replace('-', "");
    match enc_upper.as_str() {
        "UTF16" | "UTF16LE" | "UTF16BE" => {
            // Detect BOM and decode accordingly
            let text = if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xFE {
                // UTF-16 LE with BOM
                let u16_data: Vec<u16> = data[2..]
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 LE: {e}"))?
            } else if data.len() >= 2 && data[0] == 0xFE && data[1] == 0xFF {
                // UTF-16 BE with BOM
                let u16_data: Vec<u16> = data[2..]
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 BE: {e}"))?
            } else if enc_upper == "UTF16BE" {
                let u16_data: Vec<u16> = data
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 BE: {e}"))?
            } else {
                // Default: try LE
                let u16_data: Vec<u16> = data
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_data)
                    .map_err(|e| anyhow::anyhow!("invalid UTF-16 LE: {e}"))?
            };
            Ok(text.into_bytes())
        }
        "UTF8" | "UTF8BOM" => {
            // Already UTF-8, just strip BOM if present
            if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
                Ok(data[3..].to_vec())
            } else {
                Ok(data.to_vec())
            }
        }
        _ => {
            // Unknown encoding — fail
            anyhow::bail!("unsupported working-tree-encoding: {encoding}");
        }
    }
}
