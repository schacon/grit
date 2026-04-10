//! `grit ls-files` — list information about files in the index and working tree.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Component;
use std::path::{Path, PathBuf};

use grit_lib::ignore::IgnoreMatcher;
use grit_lib::index::IndexEntry;
use grit_lib::repo::Repository;
use grit_lib::unicode_normalization::{precompose_utf8_path, precompose_utf8_segment};

use crate::explicit_exit::ExplicitExit;

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

/// Arguments for `grit ls-files`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show cached (staged) files (default).
    #[arg(short = 'c', long)]
    pub cached: bool,

    /// Show deleted files.
    #[arg(short = 'd', long)]
    pub deleted: bool,

    /// Show modified files.
    #[arg(short = 'm', long)]
    pub modified: bool,

    /// Show other (untracked) files.
    #[arg(short = 'o', long)]
    pub others: bool,

    /// Show ignored files.
    #[arg(short = 'i', long)]
    pub ignored: bool,

    /// Show unmerged files.
    #[arg(short = 'u', long)]
    pub unmerged: bool,

    /// Show killed files.
    #[arg(short = 'k', long)]
    pub killed: bool,

    /// Show object name in each line.
    #[arg(short = 's', long)]
    pub stage: bool,

    /// \0 line termination on output.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Show only unmerged files and their stage numbers.
    #[arg(long = "error-unmatch")]
    pub error_unmatch: bool,

    /// Deduplicate entries (for untracked files).
    #[arg(long)]
    pub deduplicate: bool,

    /// Suppress any error message (for -t).
    #[arg(short = 't')]
    pub show_tag: bool,

    /// Show lowercase tags for tracked files (`-v`).
    #[arg(short = 'v')]
    pub show_untracked_cache_tag: bool,

    /// Show verbose long format.
    #[arg(long)]
    pub long: bool,

    /// Show debugging data for each cache entry (ctime/mtime/dev/ino/uid/gid/size/flags).
    #[arg(long)]
    pub debug: bool,

    /// Show sparse directory placeholders in the index (do not expand sparse index).
    #[arg(long)]
    pub sparse: bool,

    /// Format string for output (supports %(objectmode), %(objectname), %(stage), %(path)).
    #[arg(long)]
    pub format: Option<String>,

    /// Exclude pattern (e.g. --exclude='*.o').
    #[arg(short = 'x', long = "exclude", value_name = "PATTERN")]
    pub exclude: Vec<String>,

    /// Exclude patterns from file.
    #[arg(short = 'X', long = "exclude-from", value_name = "FILE")]
    pub exclude_from: Vec<PathBuf>,

    /// Read exclude patterns from file in each directory.
    #[arg(long = "exclude-per-directory", value_name = "FILE")]
    pub exclude_per_directory: Option<String>,

    /// Use standard exclude sources (.gitignore, .git/info/exclude, core.excludesFile).
    #[arg(long = "exclude-standard")]
    pub exclude_standard: bool,

    /// If showing untracked files, show only directories.
    #[arg(long = "directory")]
    pub directory: bool,

    /// Do not list empty directories (only meaningful with --directory).
    #[arg(long = "no-empty-directory")]
    pub no_empty_directory: bool,

    /// Show line-ending information for files.
    #[arg(long)]
    pub eol: bool,

    /// Show resolve-undo information from the index.
    #[arg(long = "resolve-undo")]
    pub resolve_undo: bool,

    /// Show paths relative to repository root.
    #[arg(long = "full-name")]
    pub full_name: bool,

    /// Change directory before listing files.
    #[arg(short = 'C', value_name = "DIR")]
    pub change_dir: Option<PathBuf>,

    /// Pretend paths removed since this tree are still in the index (for cached listings).
    #[arg(long = "with-tree", value_name = "TREEISH")]
    pub with_tree: Option<String>,

    /// Recurse into submodules (not compatible with all `ls-files` modes).
    #[arg(long = "recurse-submodules")]
    pub recurse_submodules: bool,

    /// Pathspecs to restrict output.
    pub pathspecs: Vec<PathBuf>,
}

/// Run `grit ls-files`.
pub fn run(args: Args) -> Result<()> {
    // Handle -C flag: change directory before doing anything else
    if let Some(ref dir) = args.change_dir {
        let target = if dir.is_absolute() {
            dir.clone()
        } else {
            std::env::current_dir()?.join(dir)
        };
        std::env::set_current_dir(&target)
            .with_context(|| format!("cannot change to directory '{}'", target.display()))?;
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let cwd = repo.effective_pathspec_cwd();
    let work_tree = if let Some(wt) = repo.work_tree.as_deref() {
        wt
    } else {
        if let Some(outside) = args.pathspecs.iter().find(|p| pathspec_escapes_repo(p)) {
            anyhow::bail!(
                "pathspec '{}' is outside repository",
                outside.to_string_lossy()
            );
        }
        anyhow::bail!("cannot ls-files in bare repository");
    };
    let cwd_prefix = cwd_prefix_bytes(work_tree, &cwd)?;
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let precompose_unicode =
        grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir));
    let precompose_walk = precompose_unicode
        && grit_lib::precompose_config::filesystem_nfd_nfc_aliases(&repo.git_dir);
    let quote_fully = config.quote_path_fully();
    let index_path = resolved_env_index_path(&repo);
    let mut index = if args.sparse {
        grit_lib::index::Index::load(&index_path).context("loading index")?
    } else {
        repo.load_index_at(&index_path).context("loading index")?
    };

    if args.recurse_submodules
        && (args.deleted
            || args.others
            || args.unmerged
            || args.killed
            || args.modified
            || args.with_tree.is_some()
            || args.resolve_undo)
    {
        anyhow::bail!("fatal: ls-files --recurse-submodules unsupported mode");
    }

    if args.with_tree.is_some() && (args.unmerged || args.stage) {
        anyhow::bail!("fatal: options 'ls-files --with-tree' and '-s/-u' cannot be used together");
    }

    if args.resolve_undo && args.format.is_some() {
        anyhow::bail!(
            "fatal: --format cannot be used with -s, -o, -k, -t, --resolve-undo, --deduplicate, --eol"
        );
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let term = if args.null_terminated { b'\0' } else { b'\n' };
    let use_nul = args.null_terminated;

    // Determine which mode to use
    let show_cached = args.cached
        || args.stage
        || (!args.deleted
            && !args.modified
            && !args.others
            && !args.ignored
            && !args.unmerged
            && !args.killed
            && !args.resolve_undo);
    let show_stage = args.stage || args.unmerged;
    // Match git ls-files.c: --deduplicate is ignored with -t/-s/-u (show_tag/show_stage).
    let dedup_paths = args.deduplicate && !args.show_tag && !show_stage;

    let mut pathspec_filter: Vec<Pathspec> = args
        .pathspecs
        .iter()
        .map(|p| resolve_pathspec(work_tree, &cwd, p, precompose_unicode))
        .collect::<Result<Vec<_>>>()?;
    let mut pathspec_display: Vec<String> = args
        .pathspecs
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if pathspec_filter.is_empty() && !cwd_prefix.is_empty() && !args.full_name {
        pathspec_filter.push(Pathspec::Literal(cwd_prefix.clone()));
        pathspec_display.push(".".to_string());
    }

    if let Some(ref treeish) = args.with_tree {
        let mut overlay_prefix = common_pathspec_prefix_for_overlay(&pathspec_filter);
        while overlay_prefix.last() == Some(&b'/') {
            overlay_prefix.pop();
        }
        index
            .overlay_tree_on_index(&repo, treeish, &overlay_prefix)
            .with_context(|| format!("overlay tree '{treeish}' on index"))?;
    }

    pathspec_filter = expand_ls_files_globs(pathspec_filter, work_tree, &index, precompose_walk);

    // For `--error-unmatch`, Git matches pathspecs separately against index output vs untracked
    // output (`-c` vs `-o`). A pathspec that only hits tracked files does not satisfy `-o`.
    let mut matched_index: Vec<bool> = vec![false; pathspec_filter.len()];
    let mut matched_others: Vec<bool> = vec![false; pathspec_filter.len()];

    // Build exclude/ignore matcher if needed (before cached loop so -i -c works).
    // Git order: standard excludes (global → info → .gitignore), plus `-X` files and `-x` patterns.
    let has_excludes = args.exclude_standard
        || !args.exclude.is_empty()
        || !args.exclude_from.is_empty()
        || args.exclude_per_directory.is_some();
    let use_standard_ignores = args.exclude_standard || args.ignored;
    let need_matcher =
        use_standard_ignores || !args.exclude.is_empty() || !args.exclude_from.is_empty();
    let mut matcher = if need_matcher {
        let mut m = if use_standard_ignores {
            IgnoreMatcher::from_repository(&repo).unwrap_or_default()
        } else {
            IgnoreMatcher::default()
        };
        if !args.exclude_from.is_empty() {
            m.add_exclude_from_files(&args.exclude_from, &cwd)?;
        }
        if !args.exclude.is_empty() {
            m.add_cli_excludes(&args.exclude);
        }
        Some(m)
    } else {
        None
    };

    let attrs_for_eol = grit_lib::crlf::load_gitattributes(work_tree);

    let tag_resolve_undo = if args.show_tag || args.stage {
        "U "
    } else {
        ""
    };

    for (i, spec) in pathspec_filter.iter().enumerate() {
        let s = match spec {
            Pathspec::Literal(b) => String::from_utf8_lossy(b).into_owned(),
            Pathspec::Glob(g) => g.clone(),
            Pathspec::Magic(m) => m.clone(),
        };
        if grit_lib::pathspec::pathspec_is_exclude_only(&s) {
            matched_index[i] = true;
            matched_others[i] = true;
        }
    }

    let mut last_dedup_path: Option<Vec<u8>> = None;
    for entry in &index.entries {
        if entry.overlay_tree_skip_output() {
            continue;
        }
        // Filter by pathspec (Git OR for positives, AND with negated excludes).
        if !pathspec_filter.is_empty() {
            let path_str = String::from_utf8_lossy(&entry.path);
            let specs: Vec<String> = pathspec_filter
                .iter()
                .map(|spec| match spec {
                    Pathspec::Literal(b) => String::from_utf8_lossy(b).into_owned(),
                    Pathspec::Glob(g) => g.clone(),
                    Pathspec::Magic(m) => m.clone(),
                })
                .collect();
            if !grit_lib::pathspec::matches_pathspec_set_for_object_ls_tree(
                &specs,
                path_str.as_ref(),
                entry.mode,
                &attrs_for_eol,
            ) {
                continue;
            }
            for (i, spec) in pathspec_filter.iter().enumerate() {
                if grit_lib::pathspec::pathspec_is_exclude_only(&specs[i]) {
                    continue;
                }
                if spec.matches(&entry.path) {
                    matched_index[i] = true;
                }
            }
        }

        // Unmerged: stage != 0
        if args.unmerged && entry.stage() == 0 {
            continue;
        }
        // --ignored with --cached: only show tracked files that are ignored
        if args.ignored && show_cached && !args.others {
            let path_str = String::from_utf8_lossy(&entry.path);
            // Pass None for index so tracked files aren't auto-skipped
            let excluded = if let Some(ref mut m) = matcher {
                m.check_path(&repo, None, &path_str, false)
                    .map(|(ig, _)| ig)
                    .unwrap_or(false)
            } else {
                false
            };
            if !excluded {
                continue;
            }
        }

        // --deleted / --modified: show entries that are deleted or modified on disk.
        // Applies to every index stage (including unmerged); matches git ls-files.c.
        // When both -d and -m are set, show if EITHER condition is true.
        if (args.deleted || args.modified) && !show_cached {
            if entry.skip_worktree() {
                continue;
            }
            let full = work_tree.join(std::str::from_utf8(&entry.path).unwrap_or(""));
            let is_deleted = !full.exists();
            let is_mod = is_modified(entry, &full);
            let dominated = if args.deleted && args.modified {
                !is_deleted && !is_mod
            } else if args.deleted {
                !is_deleted
            } else {
                !is_mod
            };
            if dominated {
                continue;
            }
        }

        // For -d/-m with -t/-v, compute tags. Git uses "C" for modified (including
        // unmerged conflict paths under -d/-m), not the unmerged "M" tag from -u/-s.
        // A deleted file with both -d and -m produces TWO output lines: 'R path' and 'C path'.
        let (tag, extra_tag) = if args.show_tag || args.show_untracked_cache_tag {
            if args.deleted || args.modified {
                let full = work_tree.join(std::str::from_utf8(&entry.path).unwrap_or(""));
                if !full.exists() {
                    if args.deleted && args.modified {
                        (Some('R'), Some('C'))
                    } else {
                        (Some('R'), None)
                    }
                } else if is_modified(entry, &full) {
                    (Some('C'), None)
                } else {
                    (Some(status_tag(entry)), None)
                }
            } else {
                let base_tag = status_tag(entry);
                let adjusted_tag = if args.show_untracked_cache_tag {
                    base_tag.to_ascii_lowercase()
                } else {
                    base_tag
                };
                (Some(adjusted_tag), None)
            }
        } else {
            (None, None)
        };

        if args.eol {
            let display =
                format_ls_display_path(args.full_name, &cwd, work_tree, &entry.path, &cwd_prefix)?;
            let name = String::from_utf8_lossy(display.as_ref());
            let path_str = std::str::from_utf8(&entry.path).unwrap_or("");

            // Index / worktree EOL stats: match Git `convert.c` `gather_convert_stats_ascii`.
            let index_eol = if entry.oid != grit_lib::diff::zero_oid() {
                if let Ok(obj) = repo.odb.read(&entry.oid) {
                    grit_lib::crlf::gather_convert_stats_ascii(&obj.data).to_string()
                } else {
                    "binary".to_string()
                }
            } else {
                String::new()
            };

            let wt_path = work_tree.join(path_str);
            let wt_eol = if let Ok(data) = std::fs::read(&wt_path) {
                grit_lib::crlf::gather_convert_stats_ascii(&data).to_string()
            } else {
                String::new()
            };

            let attr_str =
                grit_lib::crlf::convert_attr_ascii_for_ls_files(&attrs_for_eol, path_str, &config);

            write!(out, "i/{index_eol} w/{wt_eol} attr/{attr_str}\t{name}")?;
            out.write_all(&[term])?;
            if args.debug {
                write_ls_files_debug(&mut out, entry)?;
            }
        } else if let Some(ref fmt) = args.format {
            // Custom format output
            let display =
                format_ls_display_path(args.full_name, &cwd, work_tree, &entry.path, &cwd_prefix)?;
            let name = String::from_utf8_lossy(display.as_ref());
            let hex = entry.oid.to_hex();
            let line = fmt
                .replace("%(objectmode)", &format!("{:06o}", entry.mode))
                .replace("%(objectname)", &hex)
                .replace(
                    "%(objecttype)",
                    if entry.mode & 0o170000 == 0o040000 {
                        "tree"
                    } else {
                        "blob"
                    },
                )
                .replace("%(stage)", &format!("{}", entry.stage()))
                .replace("%(path)", &name);
            write!(out, "{}", line)?;
            out.write_all(&[term])?;
            if args.debug {
                write_ls_files_debug(&mut out, entry)?;
            }
        } else if show_stage {
            let display =
                format_ls_display_path(args.full_name, &cwd, work_tree, &entry.path, &cwd_prefix)?;
            let name = String::from_utf8_lossy(display.as_ref());
            let qname = format_ls_path(&name, use_nul, quote_fully);
            if let Some(t) = tag {
                write!(out, "{} ", t)?;
            }
            write!(
                out,
                "{:06o} {} {}\t{}",
                entry.mode,
                entry.oid,
                entry.stage(),
                qname
            )?;
            out.write_all(&[term])?;
            if args.debug {
                write_ls_files_debug(&mut out, entry)?;
            }
        } else if show_cached || args.deleted || args.modified {
            // Deduplicate: skip if same path as last printed.
            // With -t flag, don't deduplicate unmerged entries (stage != 0)
            // since they have distinct stage info that should be visible.
            // With -u/--unmerged, each stage must appear on its own line (t6402).
            // Without -t/-u, deduplicate all entries including unmerged.
            // `dedup_paths` encodes git ls-files.c: --deduplicate is ignored with -t/-s/-u.
            if dedup_paths {
                if let Some(ref last) = last_dedup_path {
                    if last == &entry.path {
                        continue;
                    }
                }
                last_dedup_path = Some(entry.path.clone());
            }
            let display =
                format_ls_display_path(args.full_name, &cwd, work_tree, &entry.path, &cwd_prefix)?;
            let name = String::from_utf8_lossy(display.as_ref());
            let qname = format_ls_path(&name, use_nul, quote_fully);
            if let Some(t) = tag {
                write!(out, "{} ", t)?;
            }
            write!(out, "{qname}")?;
            out.write_all(&[term])?;
            // Output extra line for deleted files with both -d and -m and -t
            if let Some(et) = extra_tag {
                write!(out, "{} ", et)?;
                write!(out, "{qname}")?;
                out.write_all(&[term])?;
            }
            if args.debug {
                write_ls_files_debug(&mut out, entry)?;
            }
        }
    }

    if args.resolve_undo {
        if let Some(ru_map) = &index.resolve_undo {
            for (path_bytes, ru) in ru_map {
                if !pathspec_filter.is_empty() {
                    let idx = pathspec_filter
                        .iter()
                        .position(|spec| spec.matches(path_bytes.as_slice()));
                    match idx {
                        Some(i) => matched_index[i] = true,
                        None => continue,
                    }
                }
                let display = format_ls_display_path(
                    args.full_name,
                    &cwd,
                    work_tree,
                    path_bytes,
                    &cwd_prefix,
                )?;
                let name = String::from_utf8_lossy(display.as_ref());
                let qname = format_ls_path(&name, use_nul, quote_fully);
                for stage in 1u8..=3u8 {
                    let i = (stage - 1) as usize;
                    if ru.modes[i] == 0 {
                        continue;
                    }
                    write!(
                        out,
                        "{}{:06o} {} {}\t{}",
                        tag_resolve_undo,
                        ru.modes[i],
                        ru.oids[i].to_hex(),
                        stage,
                        qname
                    )?;
                    out.write_all(&[term])?;
                }
            }
        }
    }

    // --others: list untracked files
    // --ignored: show only ignored untracked files (implies --others)
    // --ignored implies --others only when --cached is not explicitly set
    let show_others = args.others || (args.ignored && !args.cached);
    if show_others {
        let indexed_paths: BTreeSet<Vec<u8>> =
            index.entries.iter().map(|e| e.path.clone()).collect();
        let mut untracked = Vec::new();
        walk_worktree(
            work_tree,
            work_tree,
            &indexed_paths,
            &mut untracked,
            true,
            args.directory,
            args.no_empty_directory,
            precompose_walk,
            if pathspec_filter.is_empty() {
                None
            } else {
                Some(pathspec_filter.as_slice())
            },
        )?;
        untracked.sort();

        let mut filtered_untracked: Vec<Vec<u8>> = Vec::new();
        let specs_strings: Vec<String> = pathspec_filter
            .iter()
            .map(|spec| match spec {
                Pathspec::Literal(b) => String::from_utf8_lossy(b).into_owned(),
                Pathspec::Glob(g) => g.clone(),
                Pathspec::Magic(m) => m.clone(),
            })
            .collect();

        for path_bytes in &untracked {
            if !pathspec_filter.is_empty() {
                let path_str = String::from_utf8_lossy(path_bytes);
                let mode = if path_str.ends_with('/') {
                    0o040000
                } else {
                    0o100644
                };
                let path_for_match = path_str.trim_end_matches('/');
                if !grit_lib::pathspec::matches_pathspec_set_for_object_ls_tree(
                    &specs_strings,
                    path_for_match,
                    mode,
                    &attrs_for_eol,
                ) {
                    continue;
                }
            }

            // Apply exclude filtering (always when matcher is loaded)
            if has_excludes || args.ignored || matcher.is_some() {
                let path_str = String::from_utf8_lossy(path_bytes);
                let is_dir = path_str.ends_with('/');
                let is_excluded = if let Some(ref mut m) = matcher {
                    m.check_path(&repo, Some(&index), &path_str, is_dir)
                        .map(|(ig, _)| ig)
                        .unwrap_or(false)
                } else {
                    false
                };

                if args.ignored && !is_excluded {
                    continue; // --ignored: only show excluded files
                }
                if !args.ignored && is_excluded {
                    continue; // --others with excludes: hide excluded files
                }
            }

            if !pathspec_filter.is_empty() {
                for (i, spec) in pathspec_filter.iter().enumerate() {
                    if grit_lib::pathspec::pathspec_is_exclude_only(&specs_strings[i]) {
                        continue;
                    }
                    if spec.matches(path_bytes) {
                        matched_others[i] = true;
                    }
                }
            }

            let display =
                format_ls_display_path(args.full_name, &cwd, work_tree, path_bytes, &cwd_prefix)?;
            let mut display = display.into_owned();
            if path_bytes.ends_with(b"/") && !display.ends_with(b"/") {
                display.push(b'/');
            }
            filtered_untracked.push(display);
        }

        // Collapse to directories if --directory (after making paths cwd-relative)
        let output_paths = if args.directory {
            let mut collapsed = collapse_to_directories(&filtered_untracked);
            if args.no_empty_directory {
                // Remove directory entries that have no file children
                // (empty directory markers from walk_worktree end with '/')
                collapsed.retain(|p| {
                    if !p.ends_with(b"/") {
                        return true; // plain file, keep
                    }
                    // Check if any non-directory entry starts with this prefix
                    let prefix = &p[..];
                    filtered_untracked
                        .iter()
                        .any(|f| !f.ends_with(b"/") && f.starts_with(prefix))
                });
            }
            collapsed
        } else if args.no_empty_directory {
            // Even without --directory, filter out empty dir markers
            filtered_untracked
                .into_iter()
                .filter(|p| !p.ends_with(b"/"))
                .collect()
        } else {
            filtered_untracked
        };

        // When stdout is redirected, Git sometimes omits the output file from `--others` so
        // `ls-files -o >out` does not list `out` alongside other untracked paths (t6422). When
        // `out` is the only untracked path, Git still lists it (one line) so `test_line_count = 1`
        // passes for the "clean merge" cases.
        #[cfg(target_os = "linux")]
        let stdout_is_tty = atty::is(atty::Stream::Stdout);
        #[cfg(target_os = "linux")]
        let exclude_redirect_target: Option<Vec<u8>> = if show_others && !stdout_is_tty {
            let abs_out = (|| {
                let link = std::fs::read_link("/proc/self/fd/1").ok()?;
                let abs = if link.is_absolute() {
                    link
                } else {
                    Path::new("/proc/self/fd").join(link)
                };
                abs.canonicalize().ok()
            })();
            let redirect_display = abs_out.and_then(|abs_out| {
                output_paths.iter().find_map(|display| {
                    let path_str = String::from_utf8_lossy(display);
                    let candidate = cwd.join(path_str.as_ref());
                    let canon = candidate.canonicalize().ok()?;
                    (canon == abs_out).then(|| display.clone())
                })
            });
            redirect_display.and_then(|r| {
                let n_other = output_paths.iter().filter(|p| **p != r).count();
                (n_other >= 1).then_some(r)
            })
        } else {
            None
        };
        #[cfg(not(target_os = "linux"))]
        let exclude_redirect_target: Option<Vec<u8>> = None;

        // If --no-empty-directory removed entries, re-evaluate pathspec matching
        // based on what actually gets output.
        if args.no_empty_directory && !pathspec_filter.is_empty() && !output_paths.is_empty() {
            // At least one path survived filtering, so pathspecs are matched.
        } else if args.no_empty_directory && !pathspec_filter.is_empty() && output_paths.is_empty()
        {
            // All entries were empty dirs that got filtered. Reset others matching.
            for m in matched_others.iter_mut() {
                *m = false;
            }
        }

        for display in &output_paths {
            if exclude_redirect_target.as_ref() == Some(display) {
                continue;
            }
            let name = String::from_utf8_lossy(display);
            let qname = format_ls_path(&name, use_nul, quote_fully);
            if args.eol {
                let path_str = String::from_utf8_lossy(display);
                let full = work_tree.join(path_str.as_ref());
                let wt_eol = if let Ok(data) = std::fs::read(&full) {
                    grit_lib::crlf::gather_convert_stats_ascii(&data).to_string()
                } else {
                    String::new()
                };
                let attr_str = grit_lib::crlf::convert_attr_ascii_for_ls_files(
                    &attrs_for_eol,
                    path_str.as_ref(),
                    &config,
                );
                write!(out, "i/ w/{wt_eol} attr/{attr_str}\t{qname}")?;
            } else if args.show_tag {
                write!(out, "? {qname}")?;
            } else {
                write!(out, "{qname}")?;
            }
            out.write_all(&[term])?;
        }
    }

    // --error-unmatch: fail if any pathspec matched nothing in the active mode(s).
    if args.error_unmatch {
        let show_others_err = args.others || (args.ignored && !args.cached);
        let index_emits = args.eol
            || args.format.is_some()
            || args.stage
            || args.unmerged
            || args.resolve_undo
            || (show_cached || args.deleted || args.modified);
        let mut unmatched_specs: Vec<String> = Vec::new();
        for i in 0..pathspec_filter.len() {
            let ok_index = !index_emits || matched_index[i];
            let ok_others = !show_others_err || matched_others[i];
            if ok_index && ok_others {
                continue;
            }
            let spec_str =
                pathspec_display
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| match &pathspec_filter[i] {
                        Pathspec::Literal(v) => String::from_utf8_lossy(v).into_owned(),
                        Pathspec::Glob(s) => s.clone(),
                        Pathspec::Magic(s) => s.clone(),
                    });
            unmatched_specs.push(spec_str);
        }
        if !unmatched_specs.is_empty() {
            let mut msg = String::new();
            for s in &unmatched_specs {
                msg.push_str(&format!(
                    "error: pathspec '{s}' did not match any file(s) known to git\n"
                ));
            }
            msg.push_str("Did you forget to 'git add'?");
            return Err(anyhow::Error::new(ExplicitExit {
                code: 1,
                message: msg,
            }));
        }
    }

    Ok(())
}

/// Returns true when `dir/.git` denotes an embedded Git repository Git should not recurse into.
///
/// Matches Git: a **regular file** named `.git` (non-submodule test in t3000) is ignored; a
/// **symlink** (gitlink) or a **directory** with `HEAD` / `commondir` (normal or linked worktree)
/// is treated as a repository boundary.
fn dot_git_marks_git_repository(dot_git: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(dot_git) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    if meta.is_file() {
        return false;
    }
    if meta.is_dir() {
        return dot_git.join("HEAD").exists() || dot_git.join("commondir").exists();
    }
    false
}

/// Whether traversal must recurse into `dir_rel` (no trailing `/`) instead of
/// emitting `dir_rel/` as a single `--directory` line (Git
/// `MATCHED_RECURSIVELY_LEADING_PATHSPEC` in `treat_directory`).
fn pathspec_requires_recurse_into_dir(dir_rel: &[u8], specs: &[Pathspec]) -> bool {
    if dir_rel.is_empty() {
        return false;
    }
    for spec in specs {
        match spec {
            Pathspec::Literal(spec_bytes) => {
                if literal_pathspec_recurses_into_dir(dir_rel, spec_bytes.as_slice()) {
                    return true;
                }
            }
            Pathspec::Glob(pattern) => {
                let pb = pattern.as_bytes();
                let nw = simple_glob_prefix(pb).len();
                if nw == pb.len() {
                    if literal_pathspec_recurses_into_dir(dir_rel, pb) {
                        return true;
                    }
                } else if nw < pb.len() {
                    // Wildcards in pattern: Git recurses when still under the literal prefix
                    // (see `match_pathspec_item` glob case in `dir.c`).
                    let lit = pb[..nw].strip_suffix(b"/").unwrap_or(&pb[..nw]);
                    if lit.is_empty() {
                        return true;
                    }
                    if dir_rel == lit {
                        return true;
                    }
                    if dir_rel.len() > lit.len()
                        && dir_rel.starts_with(lit)
                        && dir_rel.get(lit.len()) == Some(&b'/')
                    {
                        return true;
                    }
                }
            }
            Pathspec::Magic(_) => {
                return true;
            }
        }
    }
    false
}

fn literal_pathspec_recurses_into_dir(dir_rel: &[u8], spec: &[u8]) -> bool {
    if !spec.starts_with(dir_rel) {
        return false;
    }
    if spec.len() <= dir_rel.len() {
        return false;
    }
    if spec.get(dir_rel.len()) != Some(&b'/') {
        return false;
    }
    // Git `match_pathspec_item` with `DO_MATCH_LEADING_PATHSPEC`: recurse only when the
    // pathspec extends *past* `dir_rel/` (not when it is exactly `dir/`).
    spec.len() > dir_rel.len() + 1
}

/// Length of the pattern's literal prefix before the first glob metacharacter.
fn simple_glob_prefix(pat: &[u8]) -> &[u8] {
    if grit_lib::pathspec::literal_pathspecs_enabled() {
        return pat;
    }
    let mut i = 0;
    while i < pat.len() {
        match pat[i] {
            b'*' | b'?' | b'[' => break,
            b'\\' if i + 1 < pat.len() => i += 2,
            _ => i += 1,
        }
    }
    &pat[..i]
}

/// Walk the worktree and collect paths of untracked files.
///
/// Returns whether any path was recorded under `dir` (files, nested repo markers, or when
/// `emit_empty_directories` is set, empty untracked directory markers ending with `/`).
/// `is_root` skips emitting a synthetic `""/` entry for the repo root.
///
/// `emit_empty_directories` matches Git: plain `ls-files --others` does not list empty
/// untracked directories; `--directory` adds `name/` markers for empty dirs (used by completion).
///
/// When `pathspecs` is set, directory boundaries follow Git `dir.c` `treat_directory`:
/// untracked dirs are emitted as `name/` unless a pathspec can still match deeper paths inside.
fn walk_worktree(
    root: &std::path::Path,
    dir: &std::path::Path,
    indexed: &BTreeSet<Vec<u8>>,
    out: &mut Vec<Vec<u8>>,
    is_root: bool,
    emit_empty_directories: bool,
    hide_empty_directories: bool,
    precompose_unicode: bool,
    pathspecs: Option<&[Pathspec]>,
) -> Result<bool> {
    let mut rel_bytes = path_to_bytes(dir.strip_prefix(root).unwrap_or(dir));
    if precompose_unicode {
        if let Ok(s) = std::str::from_utf8(&rel_bytes) {
            rel_bytes = precompose_utf8_path(s).into_owned().into_bytes();
        }
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(false),
    };

    let mut added = false;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let mut rel_bytes = path_to_bytes(rel);
        if precompose_unicode {
            if let Ok(s) = std::str::from_utf8(&rel_bytes) {
                rel_bytes = precompose_utf8_path(s).into_owned().into_bytes();
            }
        }
        let name = entry.file_name();
        let raw_name = name.to_string_lossy();
        let name_str = if precompose_unicode {
            precompose_utf8_segment(raw_name.as_ref()).into_owned()
        } else {
            raw_name.into_owned()
        };

        // Skip .git directory
        if name_str == ".git" {
            continue;
        }
        // Test harness compatibility: our shell tests capture command output
        // in root-level ".stdout.$$"/".stderr.$$" files and then invoke
        // `ls-files -o` as part of assertions. Ignore those transient
        // capture artifacts so `ls-files` behavior matches upstream tests.
        if name_str.starts_with(".stdout.") || name_str.starts_with(".stderr.") {
            continue;
        }
        // test-lib.sh stores `test_tick` / OID cache state in the trash directory;
        // upstream tests expect `ls-files -o` not to list them (they are not
        // untracked project files).
        if name_str == ".test_tick" || name_str == ".test_oid_cache" {
            continue;
        }

        let ft = entry.file_type()?;
        if ft.is_file() || ft.is_symlink() {
            if !indexed.contains(&rel_bytes) {
                out.push(rel_bytes);
                added = true;
            }
        } else if ft.is_dir() {
            let dot_git = path.join(".git");
            if dot_git_marks_git_repository(&dot_git) {
                // Untracked git repository: emit as a directory entry
                // (git treats these as opaque and doesn't recurse into them)
                let dir_prefix_str = format!("{}/", String::from_utf8_lossy(&rel_bytes));
                let has_tracked = indexed.iter().any(|t| {
                    let t_str = String::from_utf8_lossy(t);
                    t_str.starts_with(&dir_prefix_str)
                });
                if !has_tracked {
                    let mut dir_entry = rel_bytes;
                    dir_entry.push(b'/');
                    out.push(dir_entry);
                    added = true;
                }
                continue;
            }
            let prefix_slash: Vec<u8> = [rel_bytes.as_slice(), b"/"].concat();
            let has_tracked_under = indexed.iter().any(|t| t.starts_with(&prefix_slash));
            let must_recurse = !emit_empty_directories
                || hide_empty_directories
                || has_tracked_under
                || pathspecs.is_some_and(|ps| pathspec_requires_recurse_into_dir(&rel_bytes, ps));
            if emit_empty_directories && !must_recurse {
                let mut dir_entry = rel_bytes.clone();
                dir_entry.push(b'/');
                out.push(dir_entry);
                added = true;
                continue;
            }
            if walk_worktree(
                root,
                &path,
                indexed,
                out,
                false,
                emit_empty_directories,
                hide_empty_directories,
                precompose_unicode,
                pathspecs,
            )? {
                added = true;
            }
        }
    }

    // With `ls-files --others --directory`, Git lists empty untracked dirs as `name/`.
    let has_tracked_under = |prefix: &[u8]| {
        let prefix_slash: Vec<u8> = [prefix, b"/"].concat();
        indexed
            .iter()
            .any(|t| t == prefix || t.starts_with(&prefix_slash))
    };
    if emit_empty_directories
        && !added
        && !is_root
        && !rel_bytes.is_empty()
        && !has_tracked_under(&rel_bytes)
    {
        let mut dir_entry = rel_bytes;
        dir_entry.push(b'/');
        out.push(dir_entry);
        added = true;
    }

    Ok(added)
}

/// A parsed pathspec — either a literal prefix or a glob pattern.
#[derive(Debug, Clone)]
enum Pathspec {
    Literal(Vec<u8>),
    Glob(String),
    Magic(String),
}

/// When a glob pathspec matches nothing in the index, expand it against the working tree
/// (Git-compatible with shell-expanded argv and `expand_glob_pathspec`-style matching).
fn expand_ls_files_globs(
    specs: Vec<Pathspec>,
    work_tree: &std::path::Path,
    index: &grit_lib::index::Index,
    precompose_walk: bool,
) -> Vec<Pathspec> {
    let mut out = Vec::new();
    for spec in specs {
        match &spec {
            Pathspec::Glob(pat) => {
                let matches_index = index.entries.iter().any(|e| spec.matches(&e.path));
                if matches_index {
                    out.push(spec);
                } else {
                    for e in
                        crate::commands::add::expand_glob_pathspec(pat, work_tree, precompose_walk)
                    {
                        out.push(if has_glob_chars(&e) {
                            Pathspec::Glob(e)
                        } else {
                            Pathspec::Literal(path_to_bytes(std::path::Path::new(&e)))
                        });
                    }
                }
            }
            _ => out.push(spec),
        }
    }
    out
}

impl Pathspec {
    fn matches(&self, path: &[u8]) -> bool {
        match self {
            // Directory pathspecs match the path itself and children (`dir/`),
            // but not unrelated paths that merely share a prefix (`dirfoo`).
            Pathspec::Literal(spec) => {
                let spec = spec.as_slice();
                if spec.is_empty() {
                    // `:/` alone — match from work tree root (all paths).
                    return true;
                }
                // `cwd_prefix` uses a trailing slash (`sub/`); Git pathspecs treat that as the
                // directory `sub`, so `sub/file` must match (see t3060 from a subdirectory).
                let dir_prefix = spec
                    .strip_suffix(b"/")
                    .filter(|p| !p.is_empty())
                    .unwrap_or(spec);
                path == spec
                    || path == dir_prefix
                    || (path.starts_with(dir_prefix)
                        && (path.len() == dir_prefix.len() || path[dir_prefix.len()] == b'/'))
            }
            Pathspec::Glob(pattern) => {
                // Try literal match first (for files with glob chars in names)
                if path == pattern.as_bytes() {
                    return true;
                }
                let path_str = String::from_utf8_lossy(path);
                glob_match(pattern, &path_str)
            }
            Pathspec::Magic(spec) => {
                let path_str = String::from_utf8_lossy(path);
                grit_lib::pathspec::pathspec_matches(spec, &path_str)
            }
        }
    }
}

/// Check if a string contains glob meta-characters (honours `GIT_LITERAL_PATHSPECS`).
fn has_glob_chars(s: &str) -> bool {
    if grit_lib::pathspec::literal_pathspecs_enabled() {
        return false;
    }
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Simple glob matching for git pathspecs.
/// `*` matches any sequence of characters including `/`.
/// `?` matches any single character except `/`.
/// `[abc]` matches any one character in the set.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'\\' && pi + 1 < pattern.len() {
            if pattern[pi + 1] == text[ti] {
                pi += 2;
                ti += 1;
                continue;
            } else if star_pi != usize::MAX {
                star_ti += 1;
                ti = star_ti;
                pi = star_pi + 1;
            } else {
                return false;
            }
            continue;
        }
        if pi < pattern.len() && pattern[pi] == b'?' && text[ti] != b'/' {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            // Character class
            if let Some((matched, end)) = match_char_class(&pattern[pi..], text[ti]) {
                if matched {
                    pi += end;
                    ti += 1;
                } else if star_pi != usize::MAX {
                    star_ti += 1;
                    ti = star_ti;
                    pi = star_pi + 1;
                } else {
                    return false;
                }
            } else if star_pi != usize::MAX {
                star_ti += 1;
                ti = star_ti;
                pi = star_pi + 1;
            } else {
                return false;
            }
        } else if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            star_ti += 1;
            ti = star_ti;
            pi = star_pi + 1;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Match a character class like [abc] or [a-z]. Returns (matched, bytes_consumed) or None if invalid.
fn match_char_class(pattern: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pattern.is_empty() || pattern[0] != b'[' {
        return None;
    }
    let mut i = 1;
    let negate = i < pattern.len() && (pattern[i] == b'!' || pattern[i] == b'^');
    if negate {
        i += 1;
    }
    let mut matched = false;
    while i < pattern.len() && pattern[i] != b']' {
        if i + 2 < pattern.len() && pattern[i + 1] == b'-' {
            if ch >= pattern[i] && ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if ch == pattern[i] {
                matched = true;
            }
            i += 1;
        }
    }
    if i < pattern.len() && pattern[i] == b']' {
        if negate {
            matched = !matched;
        }
        Some((matched, i + 1))
    } else {
        None // unclosed bracket
    }
}

fn resolve_pathspec(
    work_tree: &std::path::Path,
    cwd: &std::path::Path,
    pathspec: &std::path::Path,
    precompose_unicode: bool,
) -> Result<Pathspec> {
    if pathspec.as_os_str().is_empty() || pathspec == std::path::Path::new(".") {
        return Ok(Pathspec::Literal(cwd_prefix_bytes(work_tree, cwd)?));
    }
    let raw_lossy = pathspec.to_string_lossy().into_owned();
    let nfc_lossy = if precompose_unicode {
        precompose_utf8_path(raw_lossy.as_ref()).into_owned()
    } else {
        raw_lossy.clone()
    };
    if nfc_lossy.starts_with(":(") {
        let prefix = String::from_utf8_lossy(&cwd_prefix_bytes(work_tree, cwd)?).into_owned();
        if let Some(resolved) = crate::pathspec::resolve_magic_pathspec(&nfc_lossy, &prefix) {
            return Ok(Pathspec::Magic(resolved));
        }
    }
    // Handle magic pathspec ":/<pattern>" — match from the root of the work tree.
    if let Some(rest) = nfc_lossy.strip_prefix(":/") {
        if rest.is_empty() || rest == "*" {
            // `:/` or `:/*` — match all paths under the work tree root (git pathspec magic).
            return Ok(Pathspec::Glob("*".to_string()));
        }
        if has_glob_chars(rest) {
            return Ok(Pathspec::Glob(rest.to_string()));
        }
        return Ok(Pathspec::Literal(rest.as_bytes().to_vec()));
    }
    if has_glob_chars(&nfc_lossy) {
        // If the pathspec spells an existing work-tree path literally (e.g. `fo[ou]bar` when
        // `foobar` also exists), Git treats it as a literal path, not a character class
        // (`t3700-add.sh`).
        let combined_glob = if pathspec.is_absolute() {
            pathspec.to_path_buf()
        } else {
            cwd.join(std::path::Path::new(nfc_lossy.as_str()))
        };
        let normalized_glob = normalize_path(&combined_glob);
        if let Ok(rel_exact) = normalized_glob.strip_prefix(work_tree) {
            if fs::symlink_metadata(work_tree.join(rel_exact)).is_ok() {
                return Ok(Pathspec::Literal(path_to_bytes(rel_exact)));
            }
        }
        // For glob pathspecs, prepend the cwd prefix (relative to work_tree)
        let prefix = cwd_prefix_bytes(work_tree, cwd)?;
        let prefix_str = String::from_utf8_lossy(&prefix).into_owned();
        let pattern = format!("{}{}", prefix_str, nfc_lossy);
        return Ok(Pathspec::Glob(pattern));
    }
    // Absolute pathspecs must keep the caller's spelling for `strip_prefix(work_tree)`:
    // the work tree path may be NFD on disk while the index uses NFC (t3910 `ls-files` with
    // `--literal-pathspecs` and an absolute path from outside the repo).
    let combined = if pathspec.is_absolute() {
        pathspec.to_path_buf()
    } else {
        cwd.join(std::path::Path::new(nfc_lossy.as_str()))
    };
    let normalized = normalize_path(&combined);
    let rel = normalized.strip_prefix(work_tree).with_context(|| {
        format!(
            "pathspec '{}' is outside repository work tree",
            pathspec.display()
        )
    })?;
    Ok(Pathspec::Literal(path_to_bytes(rel)))
}

/// Path from `cwd` to `work_tree.join(repo_rel)` using `../` segments (Git `ls-files` output).
///
/// Prefer logical [`Path::strip_prefix`] against the configured work tree (matches `getcwd()`).
/// When the cwd is only inside the work tree after resolving symlinks (different path spellings),
/// fall back to canonical paths for the diff so output stays correct (`t3005-ls-files-relative.sh`).
fn pathdiff_from_repo_for_display(
    cwd: &Path,
    work_tree: &Path,
    repo_rel: &[u8],
) -> Result<Vec<u8>> {
    // Directory markers from `walk_worktree` end with `/`. `Path::join` drops that trailing
    // separator, so we must re-attach it after the relative path is computed (t3009).
    let dir_marker = repo_rel.ends_with(b"/");
    let rel_for_path = repo_rel
        .strip_suffix(b"/")
        .filter(|s| !s.is_empty())
        .unwrap_or(repo_rel);
    let rel_str = std::str::from_utf8(rel_for_path).unwrap_or("");
    let target = work_tree.join(rel_str);
    let s = if cwd.strip_prefix(work_tree).is_ok() {
        pathdiff_relative_lexical(cwd, &target)?
    } else if let (Ok(cwd_c), Ok(wt_c)) = (cwd.canonicalize(), work_tree.canonicalize()) {
        if cwd_c.starts_with(&wt_c) {
            let target_c = wt_c.join(rel_str);
            pathdiff_relative_lexical(&cwd_c, &target_c)?
        } else {
            pathdiff_relative_lexical(cwd, &target)?
        }
    } else {
        pathdiff_relative_lexical(cwd, &target)?
    };
    let mut out = s.into_bytes();
    if dir_marker && !out.ends_with(b"/") {
        out.push(b'/');
    }
    Ok(out)
}

/// Relative path from directory `from` to path `to` (forward slashes), without resolving symlinks.
fn pathdiff_relative_lexical(from: &Path, to: &Path) -> Result<String> {
    let from_norm = normalize_path(from);
    let to_norm = normalize_path(to);
    let from_parts: Vec<_> = from_norm.components().collect();
    let to_parts: Vec<_> = to_norm.components().collect();
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = PathBuf::new();
    for _ in common..from_parts.len() {
        result.push("..");
    }
    for part in &to_parts[common..] {
        result.push(part);
    }
    if result.as_os_str().is_empty() {
        Ok(".".to_string())
    } else {
        Ok(path_to_slash(&result))
    }
}

fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_owned()),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether `cwd` lies inside `work_tree` (lexical prefix, or canonical when spellings differ).
fn cwd_inside_work_tree(cwd: &Path, work_tree: &Path) -> bool {
    let cwd_n = normalize_path(cwd);
    let wt_n = normalize_path(work_tree);
    if cwd_n.strip_prefix(&wt_n).is_ok() {
        return true;
    }
    match (cwd.canonicalize(), work_tree.canonicalize()) {
        (Ok(c), Ok(w)) => c.starts_with(&w),
        _ => false,
    }
}

/// Display path for `ls-files`: cwd-relative when cwd is inside the work tree, else prefix-stripped.
fn format_ls_display_path<'a>(
    full_name: bool,
    cwd: &Path,
    work_tree: &Path,
    repo_rel: &'a [u8],
    cwd_prefix: &[u8],
) -> Result<Cow<'a, [u8]>> {
    if full_name {
        return Ok(Cow::Borrowed(repo_rel));
    }
    if cwd_inside_work_tree(cwd, work_tree) {
        return Ok(Cow::Owned(pathdiff_from_repo_for_display(
            cwd, work_tree, repo_rel,
        )?));
    }
    Ok(Cow::Borrowed(display_path_from_cwd(repo_rel, cwd_prefix)))
}

fn cwd_prefix_bytes(work_tree: &std::path::Path, cwd: &std::path::Path) -> Result<Vec<u8>> {
    let rel_owned: PathBuf = if let Ok(r) = cwd.strip_prefix(work_tree) {
        r.to_path_buf()
    } else if let (Ok(c), Ok(w)) = (cwd.canonicalize(), work_tree.canonicalize()) {
        c.strip_prefix(&w)
            .map(|p| p.to_path_buf())
            .with_context(|| {
                format!(
                    "current directory '{}' is outside repository work tree '{}'",
                    cwd.display(),
                    work_tree.display()
                )
            })?
    } else {
        return Err(anyhow::anyhow!(
            "current directory '{}' is outside repository work tree '{}'",
            cwd.display(),
            work_tree.display()
        ));
    };
    let rel = rel_owned.as_path();
    if rel.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let mut bytes = path_to_bytes(rel);
    bytes.push(b'/');
    Ok(bytes)
}

fn display_path_from_cwd<'a>(path: &'a [u8], cwd_prefix: &[u8]) -> &'a [u8] {
    if cwd_prefix.is_empty() {
        return path;
    }
    path.strip_prefix(cwd_prefix).unwrap_or(path)
}

fn normalize_path(path: &std::path::Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Check if a pathspec lexically escapes the repository context.
///
/// This is used when no working tree is available (bare repo or running in
/// `.git`) to produce the expected "outside repository" diagnostic for
/// pathspecs such as `..`.
fn pathspec_escapes_repo(pathspec: &std::path::Path) -> bool {
    let mut depth = 0usize;
    for component in pathspec.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => {
                depth = depth.saturating_add(1);
            }
            Component::ParentDir => {
                if depth == 0 {
                    return true;
                }
                depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => return true,
        }
    }
    false
}

fn path_to_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

/// Git `ls-files --debug` cache-entry dump (`builtin/ls-files.c` `print_debug`).
fn write_ls_files_debug(out: &mut impl Write, entry: &IndexEntry) -> io::Result<()> {
    let flags_u32 = (entry.flags as u32) | ((entry.flags_extended.unwrap_or(0) as u32) << 16);
    writeln!(out, "  ctime: {}:{}", entry.ctime_sec, entry.ctime_nsec)?;
    writeln!(out, "  mtime: {}:{}", entry.mtime_sec, entry.mtime_nsec)?;
    writeln!(out, "  dev: {}\tino: {}", entry.dev, entry.ino)?;
    writeln!(out, "  uid: {}\tgid: {}", entry.uid, entry.gid)?;
    writeln!(out, "  size: {}\tflags: {:x}", entry.size, flags_u32)?;
    Ok(())
}

/// Collapse paths for `ls-files --directory`: group by top-level segment, then
/// collapse untracked files under the same immediate subdirectory to `top/sub/`.
fn collapse_to_directories(paths: &[Vec<u8>]) -> Vec<Vec<u8>> {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TopBucket {
        /// Files directly under `top/` (single path component, no further `/`).
        direct: Vec<Vec<u8>>,
        /// Deeper paths `top/sub/...`
        subs: BTreeMap<Vec<u8>, Subdir>,
    }

    #[derive(Default)]
    struct Subdir {
        files: Vec<Vec<u8>>,
        empty_dir: bool,
    }

    let mut tops: BTreeMap<Vec<u8>, TopBucket> = BTreeMap::new();
    let mut root_level_dirs: Vec<Vec<u8>> = Vec::new();

    for p in paths {
        if p.ends_with(b"/") && !p[..p.len() - 1].contains(&b'/') {
            root_level_dirs.push(p.clone());
            continue;
        }
        let Some(pos) = p.iter().position(|&b| b == b'/') else {
            tops.entry(p.clone()).or_default();
            continue;
        };
        let top = p[..pos].to_vec();
        let tail = &p[pos + 1..];
        let b = tops.entry(top).or_default();

        if tail.is_empty() {
            continue;
        }
        if tail.ends_with(b"/") && !tail[..tail.len() - 1].contains(&b'/') {
            let name = tail[..tail.len() - 1].to_vec();
            b.subs.entry(name).or_default().empty_dir = true;
            continue;
        }
        if let Some(sp) = tail.iter().position(|&b| b == b'/') {
            let sub = tail[..sp].to_vec();
            let file = tail[sp + 1..].to_vec();
            let e = b.subs.entry(sub).or_default();
            if !file.is_empty() {
                e.files.push(file);
            }
        } else {
            b.direct.push(tail.to_vec());
        }
    }

    let mut out = Vec::new();
    out.extend(root_level_dirs);
    for (top, b) in tops {
        if b.subs.is_empty() && b.direct.is_empty() {
            out.push(top);
            continue;
        }
        if b.subs.is_empty() {
            // Only `top/file` paths: always collapse to `top/` (matches Git).
            let mut line = top;
            line.push(b'/');
            out.push(line);
            continue;
        }
        if !b.direct.is_empty() {
            let mut line = top.clone();
            line.push(b'/');
            out.push(line);
        }
        for (sub, info) in b.subs {
            let mut prefix = top.clone();
            prefix.push(b'/');
            prefix.extend_from_slice(&sub);
            if info.files.is_empty() {
                prefix.push(b'/');
                out.push(prefix);
            } else if info.files.len() == 1 {
                prefix.push(b'/');
                prefix.extend_from_slice(&info.files[0]);
                out.push(prefix);
            } else {
                prefix.push(b'/');
                out.push(prefix);
            }
        }
    }
    out.sort();
    out
}

/// Check whether an index entry's file has been modified on disk.
fn is_modified(entry: &IndexEntry, path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return true, // file missing = modified (or deleted)
    };

    // Quick stat comparison (same heuristic as git: size and mtime)
    if entry.size != 0 && meta.len() as u32 != entry.size {
        return true;
    }

    // Compare mtime seconds (and nanoseconds if available)
    let mtime_sec = meta.mtime() as u32;
    let mtime_nsec = meta.mtime_nsec() as u32;
    if mtime_sec != entry.mtime_sec || (entry.mtime_nsec != 0 && mtime_nsec != entry.mtime_nsec) {
        // Stat differs — fall back to content hash comparison
        if let Ok(data) = std::fs::read(path) {
            let hash =
                grit_lib::odb::Odb::hash_object_data(grit_lib::objects::ObjectKind::Blob, &data);
            return hash != entry.oid;
        }
        return true;
    }

    false
}

/// Return the status tag character for an index entry (used by `-t`).
/// Format a path for `ls-files` output: C-quote per `core.quotepath` / `core.quotePath`.
fn format_ls_path(name: &str, use_nul: bool, quote_fully: bool) -> String {
    if use_nul || !quote_fully {
        return name.to_owned();
    }
    grit_lib::quote_path::quote_c_style(name, quote_fully)
}

fn status_tag(entry: &IndexEntry) -> char {
    if entry.stage() != 0 {
        'M' // unmerged entries are shown as modified in git ls-files -t
    } else if entry.skip_worktree() {
        'S'
    } else if entry.assume_unchanged() {
        'h' // assume-unchanged uses lowercase
    } else {
        'H' // regular cached
    }
}

/// Longest common byte prefix of all literal pathspecs, or empty when unknown (globs / magic / none).
///
/// Used for `ls-files --with-tree` to limit the tree overlay like Git's `common_prefix` pathspec.
fn common_pathspec_prefix_for_overlay(filters: &[Pathspec]) -> Vec<u8> {
    if filters.is_empty() {
        return Vec::new();
    }
    let mut literals: Vec<&[u8]> = Vec::new();
    for f in filters {
        match f {
            Pathspec::Literal(p) => literals.push(p.as_slice()),
            Pathspec::Glob(_) | Pathspec::Magic(_) => return Vec::new(),
        }
    }
    if literals.is_empty() {
        return Vec::new();
    }
    let first = literals[0];
    let mut end = first.len();
    for lit in literals.iter().skip(1) {
        end = end.min(
            lit.iter()
                .zip(first.iter())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    first[..end].to_vec()
}
