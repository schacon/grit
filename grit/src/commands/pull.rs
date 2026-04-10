//! `grit pull` — fetch from a remote and integrate changes.
//!
//! Equivalent to running `grit fetch` followed by `grit merge` (or
//! `grit rebase` with `--rebase`).  Only local transports are supported.

use crate::protocol_wire;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::merge_base::is_ancestor;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `grit pull`.
#[derive(Debug, ClapArgs)]
#[command(about = "Fetch from and integrate with another repository")]
pub struct Args {
    /// Repository (remote name or path) to pull from; defaults from branch config.
    #[arg(value_name = "REPOSITORY")]
    pub remote: Option<String>,

    /// Refs to fetch and merge (multiple refs merge via octopus / multi-head rules).
    #[arg(value_name = "REFSPEC")]
    pub refspecs: Vec<String>,

    /// Rebase instead of merge. Use `--rebase=merges` for the merges strategy; a bare `--rebase`
    /// must not consume the remote name (`git pull --rebase origin` matches C Git).
    #[arg(
        long = "rebase",
        short = 'r',
        num_args = 0..=1,
        default_missing_value = "true",
        require_equals = true
    )]
    pub rebase: Option<String>,

    /// Only allow fast-forward merges.
    #[arg(long = "ff-only")]
    pub ff_only: bool,

    /// Do not allow fast-forward (always create merge commit).
    #[arg(long = "no-ff")]
    pub no_ff: bool,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Merge strategy to use (may be given multiple times).
    #[arg(short = 's', long = "strategy", action = clap::ArgAction::Append)]
    pub strategy: Vec<String>,

    /// Strategy option.
    #[arg(short = 'X', long = "strategy-option")]
    pub strategy_option: Vec<String>,

    /// Disable rebase (use merge, the default).
    #[arg(long = "no-rebase")]
    pub no_rebase: bool,

    /// Allow fast-forward (default).
    #[arg(long = "ff")]
    pub ff: bool,

    /// Include one-line descriptions from commit messages in the merge commit.
    /// Optionally limit the number of entries.
    #[arg(long = "log", num_args = 0..=1, default_missing_value = "0", require_equals = true)]
    pub log: Option<String>,

    /// Do not include one-line descriptions.
    #[arg(long = "no-log")]
    pub no_log: bool,

    /// After merge/rebase, run `submodule update --init --recursive`.
    #[arg(long = "recurse-submodules", num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub recurse_submodules: Option<String>,

    /// Do not recurse into submodules after pull.
    #[arg(long = "no-recurse-submodules")]
    pub no_recurse_submodules: bool,
}

fn rebase_cli_value_is_valid(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "false" | "no" | "off" | "0" | "true" | "yes" | "on" | "1" | "merges" | "interactive"
    )
}

/// If clap consumed the repository token as `--rebase`'s optional value (`pull --rebase . c1` →
/// rebase=".", remote=c1), restore Git's argv layout: remote=`.`, refspecs start with `c1`.
fn normalize_pull_positionals(mut args: Args) -> Args {
    if let Some(ref r) = args.rebase {
        if !rebase_cli_value_is_valid(r) {
            let bad = r.clone();
            args.rebase = Some("true".to_owned());
            if let Some(old_remote) = args.remote.take() {
                args.refspecs.insert(0, old_remote);
            }
            args.remote = Some(bad);
        }
    }
    args
}

pub fn run(args: Args) -> Result<()> {
    let args = normalize_pull_positionals(args);
    let repo = Repository::discover(None).context("not a git repository")?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    let head = resolve_head(&repo.git_dir)?;
    let current_branch = head.branch_name().map(|s| s.to_owned());

    let remote_name_owned: String = if let Some(ref r) = args.remote {
        r.clone()
    } else if let Some(ref branch) = current_branch {
        config
            .get(&format!("branch.{branch}.remote"))
            .unwrap_or_else(|| "origin".to_owned())
    } else {
        "origin".to_owned()
    };
    let remote_name = remote_name_owned.as_str();

    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let is_local_dot = remote_name == ".";
    let local_remote_path = if is_local_dot {
        None
    } else {
        resolve_local_remote_path(work_tree, remote_name, &config)?
    };

    let merge_branch = if let Some(first) = args.refspecs.first() {
        first.clone()
    } else if let Some(ref branch) = current_branch {
        if let Some(merge_ref) = config.get(&format!("branch.{branch}.merge")) {
            merge_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(&merge_ref)
                .to_owned()
        } else {
            branch.clone()
        }
    } else {
        // Detached HEAD (common in submodules): use the remote's default branch (remote HEAD).
        let Some(path) = local_remote_path.as_ref() else {
            bail!("no tracking branch configured and no branch specified");
        };
        let remote_repo = open_repository_at_path(path)?;
        let head_sym = refs::read_symbolic_ref(&remote_repo.git_dir, "HEAD")?;
        let Some(sym) = head_sym else {
            bail!("no tracking branch configured and no branch specified");
        };
        sym.strip_prefix("refs/heads/").unwrap_or(&sym).to_owned()
    };

    let client_proto = protocol_wire::effective_client_protocol_version();

    let fetch_recurse = if args.no_recurse_submodules {
        None
    } else if config
        .get("fetch.recursesubmodules")
        .or_else(|| config.get("fetch.recurseSubmodules"))
        .map(|v| {
            let l = v.to_ascii_lowercase();
            l == "true" || l == "yes" || l == "on" || l == "1"
        })
        .unwrap_or(false)
    {
        Some("true".to_owned())
    } else {
        None
    };

    if let Some(remote_path) = local_remote_path {
        // protocol.version ≥ 1: use `grit fetch` so packet traces match upstream (t5700).
        if client_proto == 0 {
            let remote_repo = open_repository_at_path(&remote_path)?;
            super::fetch::copy_objects_for_pull(&remote_repo.git_dir, &repo.git_dir)?;

            let lines = if args.refspecs.is_empty() {
                let remote_oid = if let Ok(oid) =
                    refs::resolve_ref(&remote_repo.git_dir, &format!("refs/heads/{merge_branch}"))
                {
                    oid
                } else if let Ok(oid) = refs::resolve_ref(&remote_repo.git_dir, "HEAD") {
                    oid
                } else {
                    bail!("bad revision '{merge_branch}': could not resolve in remote");
                };
                let tracking_ref = format!("refs/remotes/{remote_name}/{merge_branch}");
                refs::write_ref(&repo.git_dir, &tracking_ref, &remote_oid)
                    .with_context(|| format!("update remote-tracking ref {tracking_ref}"))?;
                if let Ok(Some(sym)) = refs::read_symbolic_ref(&remote_repo.git_dir, "HEAD") {
                    let short = sym.strip_prefix("refs/heads/").unwrap_or(&sym);
                    if short == merge_branch.as_str() {
                        let remote_head_ref = format!("refs/remotes/{remote_name}/HEAD");
                        let _ = refs::write_symbolic_ref(
                            &repo.git_dir,
                            &remote_head_ref,
                            &tracking_ref,
                        );
                    }
                }
                vec![format!(
                    "{}\t\tbranch 'refs/heads/{merge_branch}' of .\n",
                    remote_oid.to_hex()
                )]
            } else {
                let mut out = Vec::new();
                for spec in &args.refspecs {
                    let oid = grit_lib::rev_parse::resolve_revision(&remote_repo, spec)
                        .with_context(|| format!("bad revision '{spec}'"))?;
                    out.push(format!(
                        "{}\t\tbranch 'refs/heads/{spec}' of .\n",
                        oid.to_hex()
                    ));
                }
                out
            };
            fs::write(repo.git_dir.join("FETCH_HEAD"), lines.concat())?;

            let res = do_merge_or_rebase_after_fetch(&args, &config, &repo, &head);
            if res.is_ok() {
                maybe_update_submodules_after_pull(&args, &config)?;
            }
            return res;
        }
    }

    if is_local_dot {
        let lines = if args.refspecs.is_empty() {
            let remote_oid = resolve_revision(&repo, &merge_branch)
                .with_context(|| format!("bad revision '{merge_branch}'"))?;
            vec![format!(
                "{}\t\tbranch 'refs/heads/{merge_branch}' of .\n",
                remote_oid.to_hex()
            )]
        } else {
            let mut out = Vec::new();
            for spec in &args.refspecs {
                let oid = resolve_revision(&repo, spec)
                    .with_context(|| format!("bad revision '{spec}'"))?;
                out.push(format!(
                    "{}\t\tbranch 'refs/heads/{spec}' of .\n",
                    oid.to_hex()
                ));
            }
            out
        };
        fs::write(repo.git_dir.join("FETCH_HEAD"), lines.concat())?;

        let res = do_merge_or_rebase_after_fetch(&args, &config, &repo, &head);
        if res.is_ok() {
            maybe_update_submodules_after_pull(&args, &config)?;
        }
        return res;
    }

    let fetch_args = super::fetch::Args {
        remote: Some(remote_name.to_owned()),
        refspecs: args.refspecs.clone(),
        filter: None,
        all: false,
        no_all: false,
        multiple: false,
        tags: false,
        no_tags: false,
        prune: false,
        prune_tags: false,
        deepen: None,
        depth: None,
        shallow_since: None,
        shallow_exclude: None,
        unshallow: false,
        refetch: false,
        output: None,
        quiet: args.quiet,
        jobs: None,
        append: false,
        no_write_fetch_head: false,
        no_auto_maintenance: false,
        no_auto_gc: false,
        end_of_options: false,
        parallel_fetch_collect_fetch_head: false,
        parallel_fetch_order: None,
        porcelain: false,
        no_show_forced_updates: false,
        show_forced_updates: false,
        negotiate_only: false,
        update_head_ok: false,
        prefetch: false,
        verbose: 0,
        update_refs: false,
        upload_pack: None,
        dry_run: false,
        recurse_submodules: fetch_recurse,
        no_recurse_submodules: args.no_recurse_submodules,
        recurse_submodules_default: None,
        submodule_prefix: None,
    };
    super::fetch::run(fetch_args)?;

    let res = do_merge_or_rebase_after_fetch(&args, &config, &repo, &head);
    if res.is_ok() {
        maybe_update_submodules_after_pull(&args, &config)?;
    }
    res
}

fn maybe_update_submodules_after_pull(args: &Args, config: &ConfigSet) -> Result<()> {
    if args.no_recurse_submodules {
        return Ok(());
    }
    let fetch_wants_recurse = config
        .get("fetch.recursesubmodules")
        .or_else(|| config.get("fetch.recurseSubmodules"))
        .map(|v| {
            let l = v.to_ascii_lowercase();
            l == "true" || l == "yes" || l == "on" || l == "1"
        })
        .unwrap_or(false);
    let cli = args.recurse_submodules.as_deref();
    let explicit_on = matches!(
        cli,
        Some("true") | Some("yes") | Some("on") | Some("1") | Some("")
    );
    let explicit_off = matches!(cli, Some("no") | Some("false") | Some("off") | Some("0"));
    let config_recurse = config
        .get("submodule.recurse")
        .map(|v| {
            let l = v.to_ascii_lowercase();
            l == "true" || l == "yes" || l == "on" || l == "1"
        })
        .unwrap_or(false);
    let should_update = if explicit_off {
        false
    } else if explicit_on {
        true
    } else if fetch_wants_recurse {
        false
    } else {
        config_recurse
    };
    if should_update {
        super::submodule::update_after_superproject_merge(true, true)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RebaseTri {
    False,
    True,
    Unset,
}

fn parse_rebase_value(key: &str, value: &str) -> Result<RebaseTri> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    match lower.as_str() {
        "false" | "no" | "off" | "0" => Ok(RebaseTri::False),
        "true" | "yes" | "on" | "1" | "merges" | "interactive" => Ok(RebaseTri::True),
        _ => bail!("invalid value for '{key}': '{value}'"),
    }
}

fn config_pull_rebase(
    config: &ConfigSet,
    current_branch: Option<&str>,
) -> Result<(RebaseTri, bool)> {
    if let Some(b) = current_branch {
        let key = format!("branch.{b}.rebase");
        if let Some(v) = config.get(&key) {
            return Ok((parse_rebase_value(&key, &v)?, false));
        }
    }
    if let Some(v) = config.get("pull.rebase") {
        return Ok((parse_rebase_value("pull.rebase", &v)?, false));
    }
    // When `pull.rebase` is not configured, behave like merge (Git ≤2.26 and tests such as
    // t7102-reset that run `git pull` without advising on divergent branches).
    Ok((RebaseTri::False, false))
}

fn pull_ff_from_config(config: &ConfigSet) -> Result<Option<(bool, bool, bool)>> {
    let Some(val) = config.get("pull.ff") else {
        return Ok(None);
    };
    let lower = val.to_ascii_lowercase();
    match lower.as_str() {
        "true" | "yes" | "on" | "1" => Ok(Some((true, false, false))),
        "false" | "no" | "off" | "0" => Ok(Some((false, true, false))),
        "only" => Ok(Some((false, false, true))),
        _ => bail!("invalid value for 'pull.ff': '{val}'"),
    }
}

fn merge_heads_from_fetch_head(repo: &Repository) -> Result<Vec<grit_lib::objects::ObjectId>> {
    use grit_lib::objects::ObjectId;
    let path = repo.git_dir.join("FETCH_HEAD");
    let content = fs::read_to_string(&path).with_context(|| "could not read FETCH_HEAD")?;
    let mut out = Vec::new();
    for hex in grit_lib::fetch_head::merge_object_ids_hex(&content) {
        out.push(ObjectId::from_hex(&hex)?);
    }
    if out.is_empty() {
        bail!("FETCH_HEAD: no merge candidates");
    }
    Ok(out)
}

fn pull_can_fast_forward(
    repo: &Repository,
    head_oid: grit_lib::objects::ObjectId,
    merge_heads: &[grit_lib::objects::ObjectId],
) -> Result<bool> {
    if merge_heads.len() != 1 {
        return Ok(false);
    }
    Ok(is_ancestor(repo, head_oid, merge_heads[0])?)
}

fn pull_already_up_to_date(
    repo: &Repository,
    head_oid: grit_lib::objects::ObjectId,
    merge_heads: &[grit_lib::objects::ObjectId],
) -> Result<bool> {
    for h in merge_heads {
        if !is_ancestor(repo, *h, head_oid)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn advice_color_prefix(config: &ConfigSet) -> &'static str {
    let use_color = config
        .get("color.advice")
        .or_else(|| config.get("color.ui"))
        .map(|v| {
            let l = v.to_ascii_lowercase();
            l == "true" || l == "always" || l == "auto"
        })
        .unwrap_or(false);
    if use_color {
        "\x1b[33m"
    } else {
        ""
    }
}

fn show_advice_pull_non_ff(config: &ConfigSet) {
    let p = advice_color_prefix(config);
    eprintln!("{p}hint: You have divergent branches and need to specify how to reconcile them.");
    eprintln!("{p}hint: You can do so by running one of the following commands sometime before");
    eprintln!("{p}hint: your next pull:");
    eprintln!("{p}hint:");
    eprintln!("{p}hint:   git config pull.rebase false  # merge");
    eprintln!("{p}hint:   git config pull.rebase true   # rebase");
    eprintln!("{p}hint:   git config pull.ff only       # fast-forward only");
    eprintln!("{p}hint:");
    eprintln!(
        "{p}hint: You can replace \"git config\" with \"git config --global\" to set a default"
    );
    eprintln!("{p}hint: preference for all repositories. You can also pass --rebase, --no-rebase,");
    eprintln!("{p}hint: or --ff-only on the command line to override the configured default per");
    eprintln!("{p}hint: invocation.");
}

fn do_merge_or_rebase_after_fetch(
    args: &Args,
    config: &ConfigSet,
    repo: &Repository,
    head: &grit_lib::state::HeadState,
) -> Result<()> {
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => {
            if merge_heads_from_fetch_head(repo)?.len() > 1 {
                bail!("Cannot merge multiple branches into empty head.");
            }
            let merge_args =
                build_pull_merge_args(args, true, false, false, vec!["FETCH_HEAD".to_owned()])?;
            return super::merge::run(merge_args);
        }
    };

    let merge_heads = merge_heads_from_fetch_head(repo)?;
    if merge_heads.is_empty() {
        bail!("no merge candidates in FETCH_HEAD");
    }

    let (mut opt_rebase, rebase_unspecified) = if args.no_rebase {
        (RebaseTri::False, false)
    } else if let Some(ref s) = args.rebase {
        (parse_rebase_value("--rebase", s)?, false)
    } else {
        config_pull_rebase(config, head.branch_name())?
    };

    let mut ff = args.ff;
    let mut no_ff = args.no_ff;
    let mut ff_only = args.ff_only;
    let cli_touched_ff = ff || no_ff || ff_only;
    let pull_ff_in_config = config.get("pull.ff").is_some();

    if !cli_touched_ff {
        if let Some((f, n, o)) = pull_ff_from_config(config)? {
            ff = f;
            no_ff = n;
            ff_only = o;
        }
    }
    // git pull.c: explicit `--rebase` on the CLI overrides `pull.ff=only` from config (not when
    // both rebase and ff=only come from config).
    if args.rebase.is_some() && !cli_touched_ff && pull_ff_in_config {
        if let Some((_, _, only)) = pull_ff_from_config(config)? {
            if only {
                ff_only = false;
                ff = true;
                no_ff = false;
            }
        }
    }
    // `--no-rebase` overrides `pull.ff=only` from config (t7601: merge when branches diverge).
    if args.no_rebase && ff_only && !args.ff_only {
        ff_only = false;
    }

    if merge_heads.len() > 1 {
        if opt_rebase == RebaseTri::True {
            bail!("Cannot rebase onto multiple branches.");
        }
        if ff_only {
            bail!("Cannot fast-forward to multiple branches.");
        }
    }

    let can_ff = pull_can_fast_forward(repo, head_oid, &merge_heads)?;
    let divergent = !can_ff && !pull_already_up_to_date(repo, head_oid, &merge_heads)?;

    if ff_only {
        if divergent {
            bail!("Not possible to fast-forward, aborting.");
        }
        opt_rebase = RebaseTri::False;
    }

    // Match git pull.c: advise only when pull.rebase was left unspecified by config, branches
    // diverge, and pull.ff was not configured (Git's `!opt_ff` after reading pull.ff).
    if rebase_unspecified && divergent && !pull_ff_in_config && !cli_touched_ff && !ff_only {
        show_advice_pull_non_ff(config);
        bail!("Need to specify how to reconcile divergent branches.");
    }

    if opt_rebase == RebaseTri::True {
        if can_ff {
            let merge_args =
                build_pull_merge_args(args, false, false, true, vec!["FETCH_HEAD".to_owned()])?;
            return super::merge::run(merge_args);
        }
        let upstream = super::merge::read_fetch_head_merge_oids(repo)?
            .into_iter()
            .next()
            .context("FETCH_HEAD merge oid")?;
        let rebase_args = super::rebase::Args {
            upstream_explicit: true,
            upstream: Some(upstream),
            onto: None,
            root: false,
            interactive: false,
            r#continue: false,
            abort: false,
            skip: false,
            exec: None,
            merge: false,
            apply: false,
            rebase_merges: false,
            no_rebase_merges: false,
            no_ff: false,
            keep_base: 0,
            fork_point: false,
            no_fork_point: false,
            reapply_cherry_picks: false,
            no_reapply_cherry_picks: false,
            verbose: false,
            update_refs: false,
            no_update_refs: false,
            empty: None,
            strategy: None,
            strategy_option: vec![],
            branch: None,
            stat: false,
            no_stat: false,
            context_lines: None,
            whitespace: None,
            autostash: false,
            no_autostash: false,
            quit: false,
            autosquash: false,
            no_autosquash: false,
            keep_empty: false,
            ignore_whitespace: false,
            committer_date_is_author_date: false,
            reset_author_date: false,
        };
        return super::rebase::run(rebase_args);
    }

    let merge_args =
        build_pull_merge_args(args, ff, no_ff, ff_only, vec!["FETCH_HEAD".to_owned()])?;
    super::merge::run(merge_args)
}

fn build_pull_merge_args(
    args: &Args,
    ff: bool,
    no_ff: bool,
    ff_only: bool,
    commits: Vec<String>,
) -> Result<super::merge::Args> {
    Ok(super::merge::Args {
        commits,
        message: None,
        ff_only,
        no_ff,
        no_commit: false,
        squash: false,
        abort: false,
        continue_merge: false,
        strategy: args.strategy.clone(),
        strategy_option: args.strategy_option.clone(),
        quiet: args.quiet,
        progress: false,
        no_progress: false,
        no_edit: true,
        edit: false,
        signoff: false,
        no_signoff: false,
        stat: false,
        no_stat: false,
        log: args.log.as_ref().map(|v| {
            let n = v.parse::<usize>().unwrap_or(0);
            if n == 0 {
                20
            } else {
                n
            }
        }),
        no_log: args.no_log,
        compact_summary: false,
        summary: false,
        ff,
        commit: false,
        no_squash: false,
        quit: false,
        autostash: false,
        allow_unrelated_histories: false,
        cleanup: None,
        file: None,
        rerere_autoupdate: false,
        no_rerere_autoupdate: false,
    })
}

fn remote_token_looks_like_path(token: &str) -> bool {
    token == "." || token.contains('/') || token.starts_with("file://")
}

fn resolve_local_remote_path(
    work_tree: &Path,
    remote_name: &str,
    config: &ConfigSet,
) -> Result<Option<PathBuf>> {
    if remote_token_looks_like_path(remote_name) {
        let raw = remote_name.strip_prefix("file://").unwrap_or(remote_name);
        let p = Path::new(raw);
        let resolved = if p.is_absolute() {
            p.to_path_buf()
        } else {
            work_tree.join(p)
        };
        return Ok(Some(resolved));
    }
    let Some(url) = config.get(&format!("remote.{remote_name}.url")) else {
        return Ok(None);
    };
    if !remote_token_looks_like_path(&url) {
        return Ok(None);
    }
    let raw = url.strip_prefix("file://").unwrap_or(&url);
    let p = Path::new(raw);
    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else {
        work_tree.join(p)
    };
    Ok(Some(resolved))
}

fn open_repository_at_path(remote_path: &Path) -> Result<Repository> {
    if let Ok(r) = Repository::open(remote_path, None) {
        return Ok(r);
    }
    let git_dir = remote_path.join(".git");
    Repository::open(&git_dir, Some(remote_path)).with_context(|| {
        format!(
            "could not open remote repository at '{}'",
            remote_path.display()
        )
    })
}
