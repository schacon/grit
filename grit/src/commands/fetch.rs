//! `grit fetch` — download objects and refs from a local repository.
//!
//! Only the **local (file://)** transport is supported.  Reads the remote
//! URL from `remote.<name>.url` in the local config, opens the remote
//! repository, copies missing objects (loose + packs), and updates
//! remote-tracking refs under `refs/remotes/<remote>/`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::merge_base;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::resolve_head;
use grit_lib::state::HeadState;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Arguments for `grit fetch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download objects and refs from another repository")]
pub struct Args {
    /// Remote name or path to fetch from (defaults to "origin").
    #[arg(value_name = "REMOTE")]
    pub remote: Option<String>,

    /// Refspec(s) to fetch (e.g. "main", "main:refs/heads/from-one", or `tag <name>` pairs).
    ///
    /// Negative refspecs start with `^` and must not be parsed as flags.
    #[arg(
        value_name = "REFSPEC",
        num_args = 0..,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        allow_negative_numbers = true
    )]
    pub refspecs: Vec<String>,

    /// Fetch all configured remotes.
    #[arg(long)]
    pub all: bool,

    /// Fetch tags from the remote.
    #[arg(long)]
    pub tags: bool,

    /// Do not fetch tags.
    #[arg(long = "no-tags")]
    pub no_tags: bool,

    /// Remove remote-tracking refs that no longer exist on the remote.
    #[arg(long)]
    pub prune: bool,

    /// Remove local tags that no longer exist on the remote (implies --prune).
    #[arg(long)]
    pub prune_tags: bool,

    /// Deepen a shallow clone by N commits.
    #[arg(long, value_name = "N")]
    pub deepen: Option<usize>,

    /// Limit fetching to the specified number of commits from the tip.
    #[arg(long, value_name = "N")]
    pub depth: Option<usize>,

    /// Partial clone filter spec (accepted for compatibility).
    #[arg(long = "filter", value_name = "FILTER-SPEC")]
    pub filter: Option<String>,

    /// Deepen history of a shallow clone back to a date.
    #[arg(long, value_name = "DATE")]
    pub shallow_since: Option<String>,

    /// Deepen history of a shallow clone excluding a revision.
    #[arg(long, value_name = "REV")]
    pub shallow_exclude: Option<String>,

    /// Re-fetch all objects even if they already exist locally.
    #[arg(long)]
    pub refetch: bool,

    /// Write machine-readable fetch output to the given file.
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Be quiet — suppress informational output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Show detailed progress (Git: same as non-quiet for local transport).
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Number of parallel children for fetching (accepted but ignored).
    #[arg(short = 'j', long = "jobs", value_name = "N")]
    pub jobs: Option<usize>,

    /// Machine-readable porcelain output.
    #[arg(long)]
    pub porcelain: bool,

    /// Do not show forced updates.
    #[arg(long = "no-show-forced-updates")]
    pub no_show_forced_updates: bool,

    /// Show forced updates (default, overrides --no-show-forced-updates).
    #[arg(long = "show-forced-updates")]
    pub show_forced_updates: bool,

    /// Only negotiate, do not fetch objects.
    #[arg(long)]
    pub negotiate_only: bool,

    /// Allow updating the current branch head (normally refused).
    #[arg(long)]
    pub update_head_ok: bool,
    /// Rewrite positive refspec destinations under `refs/prefetch/` (Git maintenance prefetch).
    #[arg(long)]
    pub prefetch: bool,
    /// Update remote-tracking refs after fetch.
    #[arg(long = "update-refs")]
    pub update_refs: bool,

    /// Command to run on the remote side for pack transfer (protocol v0).
    #[arg(long = "upload-pack", value_name = "PATH")]
    pub upload_pack: Option<String>,

    /// Recurse into submodules and fetch each default remote.
    #[arg(long = "recurse-submodules", num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub recurse_submodules: Option<String>,

    /// Disable submodule recursion (overrides config).
    #[arg(long = "no-recurse-submodules")]
    pub no_recurse_submodules: bool,
}

pub fn run(mut args: Args) -> Result<()> {
    if args.negotiate_only {
        // Negotiate-only mode: just exit successfully without fetching.
        return Ok(());
    }

    // `git fetch tag <name>` is parsed as remote=`tag` unless we lift the magic keyword.
    if args.remote.as_deref() == Some("tag") {
        if args.refspecs.is_empty() {
            bail!("missing tag name after 'tag'");
        }
        let mut lifted = vec!["tag".to_string()];
        lifted.append(&mut args.refspecs);
        args.refspecs = lifted;
        args.remote = None;
    }

    crate::bundle_uri::clear_http_bundle_cache();
    crate::http_smart::clear_trace2_https_url_dedup();

    let git_dir = resolve_git_dir()?;
    let config = ConfigSet::load(Some(&git_dir), true)?;

    // Validate fetch.output config if set
    if let Some(val) = config.get("fetch.output") {
        match val.as_str() {
            "full" | "compact" => {}
            _ => bail!("invalid value for 'fetch.output': '{}'", val),
        }
    }

    let result = if args.all {
        let remotes = collect_remote_names(&config);
        if remotes.is_empty() {
            bail!("no remotes configured");
        }
        for name in &remotes {
            fetch_remote(&git_dir, &config, name, None, &args)?;
        }
        Ok(())
    } else {
        let remote_resolved = args
            .remote
            .clone()
            .unwrap_or_else(|| default_fetch_remote_name(&git_dir, &config));
        let remote_name = remote_resolved.as_str();
        // Remote config takes precedence over path-like names, even if the
        // remote name contains '/' or matches an existing directory.
        let url_key = format!("remote.{remote_name}.url");
        if config.get(&url_key).is_some() {
            fetch_remote(&git_dir, &config, &remote_name, None, &args)
        } else {
            let group_key = format!("remotes.{remote_name}");
            let group_lines = config.get_all(&group_key);
            if !group_lines.is_empty() {
                let mut seen = HashSet::<String>::new();
                let mut members = Vec::new();
                for line in &group_lines {
                    for m in line.split_whitespace() {
                        if seen.insert(m.to_string()) {
                            members.push(m.to_string());
                        }
                    }
                }
                for m in members {
                    fetch_remote(&git_dir, &config, &m, None, &args)?;
                }
                Ok(())
            } else if remote_name.starts_with('.')
                || remote_name.contains('/')
                || std::path::Path::new(&remote_name).is_dir()
            {
                fetch_remote(&git_dir, &config, &remote_name, Some(remote_name), &args)
            } else {
                fetch_remote(&git_dir, &config, &remote_name, None, &args)
            }
        }
    };

    if result.is_ok() && should_recurse_fetch_submodules(&config, &args) {
        super::submodule::recursive_fetch_submodules(true)?;
    }
    result
}

/// When `git fetch` is run with no remote argument, Git uses `branch.<current>.remote`
/// if set; otherwise `origin`.
/// Current branch name when `HEAD` is a symbolic ref to `refs/heads/<name>`.
fn current_branch_from_head(git_dir: &Path) -> Option<String> {
    let head_raw = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head_raw.trim();
    let target = head.strip_prefix("ref: ")?.trim();
    let branch = target.strip_prefix("refs/heads/")?;
    Some(branch.to_string())
}

/// FETCH_HEAD "for merge" when the current branch tracks this remote and the remote ref
/// matches `branch.<name>.merge` (Git `branch_merge_matches` / `add_merge_config`).
fn fetch_head_is_for_merge_with_branch(
    git_dir: &Path,
    config: &ConfigSet,
    remote_name: &str,
    remote_refname: &str,
) -> bool {
    let Some(branch) = current_branch_from_head(git_dir) else {
        return false;
    };
    let remote_key = format!("branch.{branch}.remote");
    let Some(cfg_remote) = config.get(&remote_key) else {
        return false;
    };
    if cfg_remote.trim() != remote_name {
        return false;
    }
    let merge_key = format!("branch.{branch}.merge");
    let Some(merge_ref) = config.get(&merge_key) else {
        return false;
    };
    merge_ref.trim() == remote_refname
}

/// When there is no `branch.*.merge` for HEAD, Git marks only the first ref from the first
/// non-pattern remote fetch refspec as for-merge (`get_ref_map` in fetch.c).
fn fetch_head_is_for_merge_first_refspec_only(
    refspecs: &[FetchRefspec],
    is_first_remote_head: bool,
) -> bool {
    if !is_first_remote_head {
        return false;
    }
    let Some(first) = refspecs.iter().find(|r| !r.negative && !r.src.is_empty()) else {
        return false;
    };
    !first.src.contains('*')
}

/// True when `HEAD` names `refs/heads/<b>`, `branch.<b>.remote` matches `remote_name`, and
/// `branch.<b>.merge` is set (Git `branch_has_merge_config` for default fetch).
fn branch_has_merge_config_for_remote(
    git_dir: &Path,
    config: &ConfigSet,
    remote_name: &str,
) -> bool {
    let Some(branch) = current_branch_from_head(git_dir) else {
        return false;
    };
    let remote_key = format!("branch.{branch}.remote");
    let Some(cfg_remote) = config.get(&remote_key) else {
        return false;
    };
    if cfg_remote.trim() != remote_name {
        return false;
    }
    let merge_key = format!("branch.{branch}.merge");
    config.get(&merge_key).is_some()
}

fn default_fetch_remote_name(git_dir: &Path, config: &ConfigSet) -> String {
    let Ok(head_raw) = fs::read_to_string(git_dir.join("HEAD")) else {
        return "origin".to_owned();
    };
    let head = head_raw.trim();
    let Some(rest) = head.strip_prefix("ref: ") else {
        return "origin".to_owned();
    };
    let target = rest.trim();
    let Some(branch) = target.strip_prefix("refs/heads/") else {
        return "origin".to_owned();
    };
    let remote_key = format!("branch.{branch}.remote");
    let Some(remote) = config.get(&remote_key) else {
        return "origin".to_owned();
    };
    let remote = remote.trim();
    if remote.is_empty() {
        return "origin".to_owned();
    }
    let url_key = format!("remote.{remote}.url");
    if config.get(&url_key).is_some() {
        remote.to_owned()
    } else {
        "origin".to_owned()
    }
}

fn should_recurse_fetch_submodules(config: &ConfigSet, args: &Args) -> bool {
    if args.no_recurse_submodules {
        return false;
    }
    if args.recurse_submodules.as_deref() == Some("no")
        || args.recurse_submodules.as_deref() == Some("false")
    {
        return false;
    }
    if args.recurse_submodules.is_some() {
        return true;
    }
    config
        .get("fetch.recursesubmodules")
        .or_else(|| config.get("fetch.recurseSubmodules"))
        .map(|v| {
            let l = v.to_ascii_lowercase();
            l == "true" || l == "yes" || l == "on" || l == "1"
        })
        .unwrap_or(false)
}

/// Build the upload-pack `want` list for a configured (non-CLI) fetch: only refs matched by
/// `refspecs`, optionally following annotated tags that point at each matched branch tip, and
/// omitting objects already present in the local ODB (so incremental fetches do not re-want old
/// tags — matches Git's t5503 expectations).
fn collect_wants_for_upload_pack(
    local_git_dir: &Path,
    remote_git_dir: &Path,
    advertised: &[(String, ObjectId)],
    refspecs: &[FetchRefspec],
    should_fetch_tags: bool,
    remote_name: &str,
) -> Result<Vec<ObjectId>> {
    let local_odb = Odb::new(&local_git_dir.join("objects"));
    let remote_odb = Odb::new(&remote_git_dir.join("objects"));
    let remote_repo = open_repo(remote_git_dir)?;
    let mut wants: Vec<ObjectId> = Vec::new();
    let local_tag_tips: HashSet<ObjectId> = refs::list_refs(local_git_dir, "refs/tags/")?
        .into_iter()
        .map(|(_, oid)| oid)
        .collect();

    let effective_refspecs: Vec<FetchRefspec> = if refspecs.is_empty() {
        vec![FetchRefspec {
            src: "refs/heads/*".to_owned(),
            dst: format!("refs/remotes/{remote_name}/*"),
            force: true,
            negative: false,
        }]
    } else {
        refspecs.to_vec()
    };

    for (refname, oid) in advertised {
        if !refname.starts_with("refs/heads/") {
            continue;
        }
        let Some(local_ref) = map_ref_through_refspecs(refname, &effective_refspecs) else {
            continue;
        };
        let tip_oid = refs::resolve_ref(remote_git_dir, refname)
            .ok()
            .unwrap_or(*oid);
        let local_tracking_oid = read_loose_ref_chain(local_git_dir, &local_ref)
            .or_else(|| refs::resolve_ref(local_git_dir, &local_ref).ok());
        if local_tracking_oid.as_ref() == Some(&tip_oid) {
            continue;
        }
        // Always request the branch tip OID when the remote-tracking ref lags, even if the object
        // already exists locally (e.g. via a prior pack). Upstream `fetch-pack` emits matching
        // `want` lines and tag-following depends on the correct tip (t5503-tagfollow).
        crate::fetch_transport::push_want_unique(&mut wants, tip_oid);
        if should_fetch_tags
            && remote_odb
                .read(&tip_oid)
                .map(|o| o.kind == ObjectKind::Commit)
                .unwrap_or(false)
        {
            let tag_refs = refs::list_refs(remote_git_dir, "refs/tags/")?;
            for (_tag_refname, tag_ref_oid) in tag_refs {
                if local_tag_tips.contains(&tag_ref_oid) {
                    continue;
                }
                if local_odb.exists(&tag_ref_oid) {
                    continue;
                }
                let Ok(tag_obj) = remote_odb.read(&tag_ref_oid) else {
                    continue;
                };
                if tag_obj.kind != ObjectKind::Tag {
                    continue;
                }
                let Ok(tag) = parse_tag(&tag_obj.data) else {
                    continue;
                };
                if !merge_base::is_ancestor(&remote_repo, tag.object, tip_oid).unwrap_or(false) {
                    continue;
                }
                if let Some(old_tip) = local_tracking_oid {
                    if tag.object == old_tip {
                        continue;
                    }
                    if merge_base::is_ancestor(&remote_repo, tag.object, old_tip).unwrap_or(false) {
                        continue;
                    }
                }
                crate::fetch_transport::push_want_unique(&mut wants, tag_ref_oid);
            }
        }
    }

    wants.dedup();
    Ok(wants)
}

/// Fetch from a single remote.
///
/// If `url_override` is Some, use it directly as the remote URL instead of
/// looking it up in config.  This supports path-based remotes like `../one`.
fn fetch_remote(
    git_dir: &Path,
    config: &ConfigSet,
    remote_name: &str,
    url_override: Option<&str>,
    args: &Args,
) -> Result<()> {
    let url_key = format!("remote.{remote_name}.url");
    let legacy_remote = if url_override.is_none() && config.get(&url_key).is_none() {
        read_git_remotes_file(git_dir, remote_name)
    } else {
        None
    };
    let branches_remote =
        if url_override.is_none() && config.get(&url_key).is_none() && legacy_remote.is_none() {
            read_git_branches_remote_file(git_dir, remote_name)
        } else {
            None
        };

    // Determine remote URL: CLI path, config, `.git/remotes/<name>`, or `.git/branches/<name>`.
    let url = if let Some(u) = url_override {
        u.to_owned()
    } else if let Some(u) = config.get(&url_key) {
        u
    } else if let Some(leg) = &legacy_remote {
        leg.url.clone()
    } else if let Some(br) = &branches_remote {
        br.url.clone()
    } else {
        bail!("remote '{remote_name}' not found; no such remote");
    };

    let is_ext_url = url.starts_with("ext::");
    if is_ext_url {
        crate::protocol::check_protocol_allowed("ext", Some(git_dir))?;
    }

    let is_http_url = !is_ext_url && (url.starts_with("http://") || url.starts_with("https://"));

    let mut remote_path = if is_ext_url {
        PathBuf::new()
    } else if is_http_url {
        let proto = if url.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        crate::protocol::check_protocol_allowed(proto, Some(git_dir))?;
        PathBuf::new()
    } else if url.starts_with("git://") {
        crate::protocol::check_protocol_allowed("git", Some(git_dir))?;
        let Some(p) = crate::fetch_transport::try_local_path_for_git_daemon_url(&url) else {
            bail!("git: could not resolve '{}' to a local repository", url);
        };
        p
    } else if crate::ssh_transport::is_configured_ssh_url(&url) {
        crate::protocol::check_protocol_allowed("ssh", Some(git_dir))?;
        let spec = crate::ssh_transport::parse_ssh_url(&url)?;
        let Some(gd) = crate::ssh_transport::try_local_git_dir(&spec) else {
            bail!(
                "ssh: could not resolve remote URL '{}' to a local repository",
                url
            );
        };
        gd
    } else {
        crate::protocol::check_protocol_allowed("file", Some(git_dir))?;
        // Strip file:// prefix if present.
        // For configured remotes, resolve relative paths from the repository root
        // (not the process CWD), matching Git's behavior for remote.<name>.url.
        if let Some(stripped) = url.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(&url)
        }
    };
    // Resolve relative paths from the repository root (not process CWD), for both
    // configured `remote.<name>.url` and path-based remotes (`git fetch ./server`).
    if !is_ext_url && !is_http_url && remote_path.is_relative() {
        let base = configured_remote_base(git_dir);
        remote_path = base.join(&remote_path);
        if url_override.is_none() && !remote_path.exists() {
            let mut trimmed = url.as_str();
            let mut stripped_any_parent = false;
            while let Some(rest) = trimmed.strip_prefix("../") {
                stripped_any_parent = true;
                trimmed = rest;
            }
            if stripped_any_parent {
                let fallback = base.join(trimmed);
                if fallback.exists() {
                    remote_path = fallback;
                }
            }
        }
    }

    // `git clone` from a bundle records the bundle path as `remote.origin.url`. A no-op `fetch`
    // must succeed (`t5605` bundle clone + fetch).
    if !is_ext_url && !is_http_url && remote_path_is_git_bundle_file(&remote_path) {
        return Ok(());
    }

    let remote_repo = if is_ext_url || is_http_url {
        None
    } else {
        Some(open_repo(&remote_path).with_context(|| {
            format!(
                "could not open remote repository at '{}'",
                remote_path.display()
            )
        })?)
    };

    if crate::ssh_transport::is_configured_ssh_url(&url) {
        if let Some(ref remote_repo) = remote_repo {
            if let Ok(spec) = crate::ssh_transport::parse_ssh_url(&url) {
                let _ = crate::ssh_transport::record_fake_ssh_line(
                    &spec.host,
                    "git-upload-pack",
                    &crate::ssh_transport::ssh_remote_repo_path_for_display(&remote_repo.git_dir),
                );
            }
        }
    }

    let display_url = resolve_fetch_display_url(git_dir, &url, url_override, remote_repo.as_ref())?;
    let effective_tracking_remote = if url_override.is_some() {
        remote_repo
            .as_ref()
            .and_then(|rr| find_remote_for_repository_url(config, git_dir, rr))
    } else {
        Some(remote_name.to_owned())
    };
    let tracking_remote_name = effective_tracking_remote
        .clone()
        .unwrap_or_else(|| "origin".to_owned());
    let merge_remote_key =
        effective_tracking_remote
            .as_deref()
            .unwrap_or(if url_override.is_some() {
                "origin"
            } else {
                remote_name
            });
    let _merge_specs = branch_merge_remote_specs(config, merge_remote_key);
    let _head_branch_short = head_short_branch(git_dir);

    let mut cli_refspecs_owned = expand_fetch_cli_tag_args(&args.refspecs)?;
    let fetch_key = format!("remote.{remote_name}.fetch");
    let mut configured_refspecs = collect_refspecs(config, &fetch_key);
    let had_configured_fetch = !configured_refspecs.is_empty();
    let user_passed_cli_refspecs = !args.refspecs.is_empty();

    if args.prefetch {
        if user_passed_cli_refspecs {
            let mut specs = parse_cli_fetch_refspecs(&cli_refspecs_owned);
            apply_prefetch_to_refspecs(&mut specs);
            cli_refspecs_owned = specs.iter().map(fetch_refspec_to_cli_string).collect();
        } else {
            apply_prefetch_to_refspecs(&mut configured_refspecs);
        }
    }

    let cli_refspecs: &[String] = &cli_refspecs_owned;

    let refspecs = if user_passed_cli_refspecs {
        Vec::new()
    } else if let Some(leg) = &legacy_remote {
        parse_legacy_pull_lines(&leg.pull_lines)?
    } else if let Some(br) = &branches_remote {
        if let Some(ref b) = br.default_branch {
            vec![FetchRefspec {
                src: format!("refs/heads/{b}"),
                dst: format!("refs/remotes/origin/{b}"),
                force: false,
                negative: false,
            }]
        } else {
            vec![FetchRefspec {
                src: "refs/heads/*".to_owned(),
                dst: "refs/remotes/origin/*".to_owned(),
                force: false,
                negative: false,
            }]
        }
    } else {
        configured_refspecs.clone()
    };
    let prefetch_left_no_positive = args.prefetch
        && cli_refspecs_owned.is_empty()
        && (user_passed_cli_refspecs || had_configured_fetch);
    let _use_default_remote_tracking = refspecs.is_empty() && !prefetch_left_no_positive;

    let implicit_path_fetch = url_override.is_some()
        && effective_tracking_remote.is_some()
        && cli_refspecs.is_empty()
        && refspecs.is_empty();

    let coalesced_remotes = if url_override.is_none()
        && legacy_remote.is_none()
        && branches_remote.is_none()
        && cli_refspecs.is_empty()
        && remote_repo.is_some()
    {
        remotes_sharing_repository_url(
            config,
            git_dir,
            remote_repo
                .as_ref()
                .expect("coalesced remotes need local repo"),
            remote_name,
        )
    } else {
        vec![remote_name.to_owned()]
    };
    let fetch_head_refspecs = refspecs.clone();

    let tagopt_remote = effective_tracking_remote.as_deref().unwrap_or(remote_name);
    let should_fetch_tags = if args.tags {
        true
    } else if args.no_tags {
        false
    } else if implicit_path_fetch {
        false
    } else {
        let tagopt_key = format!("remote.{tagopt_remote}.tagopt");
        match config.get(&tagopt_key).as_deref() {
            Some("--no-tags") => false,
            Some("--tags") => true,
            _ => true,
        }
    };

    let upload_pack_cmd = args.upload_pack.clone().or_else(|| {
        let key = format!("remote.{remote_name}.uploadpack");
        config.get(&key)
    });

    // Local (non-SSH) non-URL fetches use the upload-pack protocol like upstream Git. This keeps
    // `GIT_TRACE_PACKET` lines (`upload-pack< want …`) and tag-following wants aligned with
    // tests such as t5503-tagfollow. CLI refspecs are applied after the pack is received.
    let use_upload_pack_negotiation =
        !is_ext_url && !is_http_url && !crate::ssh_transport::is_configured_ssh_url(&url);

    let upload_pack_refspecs: &[String] = if prefetch_left_no_positive {
        &[]
    } else {
        cli_refspecs
    };

    let (remote_heads, remote_tags) = if is_ext_url {
        let (heads, tags, _, _) =
            crate::fetch_transport::with_packet_trace_identity("fetch", || {
                crate::ext_transport::fetch_via_ext_skipping(
                    git_dir,
                    &url,
                    "git-upload-pack",
                    upload_pack_refspecs,
                )
            })?;
        (heads, tags)
    } else if url.starts_with("git://") {
        crate::protocol::check_protocol_allowed("git", Some(git_dir))?;
        let (heads, tags, _, _) =
            crate::fetch_transport::with_packet_trace_identity("fetch", || {
                crate::fetch_transport::fetch_via_git_protocol_skipping(git_dir, &url, cli_refspecs)
            })?;
        (heads, tags)
    } else if is_http_url {
        let (heads, tags, _adv) =
            crate::http_smart::http_fetch_pack(git_dir, &url, upload_pack_refspecs)?;
        crate::bundle_uri::maybe_apply_bundle_uri_after_http_fetch(git_dir, &url, None)?;
        let heads: Vec<(String, ObjectId)> = heads.into_iter().map(|e| (e.name, e.oid)).collect();
        let tags: Vec<(String, ObjectId)> = tags.into_iter().map(|e| (e.name, e.oid)).collect();
        (heads, tags)
    } else if use_upload_pack_negotiation {
        crate::protocol::check_protocol_allowed("file", Some(git_dir))?;
        let remote_repo_upload = remote_repo
            .as_ref()
            .expect("upload-pack path requires local remote repository");
        let cli_owned = if prefetch_left_no_positive {
            Vec::new()
        } else {
            cli_refspecs_owned.clone()
        };
        let refspec_owned = refspecs.clone();
        let local_git = git_dir.to_path_buf();
        let remote_gd = remote_repo_upload.git_dir.clone();
        let remote_nm = remote_name.to_owned();
        let has_cli_refspecs = !cli_owned.is_empty();
        let compute_wants = move |adv: &[(String, ObjectId)]| -> Result<Vec<ObjectId>> {
            if !cli_owned.is_empty() {
                crate::fetch_transport::collect_wants_cli(&remote_gd, adv, &cli_owned)
            } else {
                collect_wants_for_upload_pack(
                    &local_git,
                    &remote_gd,
                    adv,
                    &refspec_owned,
                    should_fetch_tags,
                    &remote_nm,
                )
            }
        };
        let (heads, tags, _, _) = crate::fetch_transport::fetch_via_upload_pack_skipping(
            git_dir,
            &remote_path,
            upload_pack_cmd.as_deref(),
            compute_wants,
            has_cli_refspecs,
        )?;
        (heads, tags)
    } else {
        let remote_repo = remote_repo
            .as_ref()
            .expect("non-ext fetch has local remote repo");
        let heads = refs::list_refs(&remote_repo.git_dir, "refs/heads/")?;
        let tags = refs::list_refs(&remote_repo.git_dir, "refs/tags/")?;
        let object_copy_roots = if prefetch_left_no_positive {
            fetch_object_copy_roots(&remote_repo.git_dir, &[], &[], &heads, &tags)?
        } else if !cli_refspecs.is_empty() {
            fetch_object_copy_roots(&remote_repo.git_dir, cli_refspecs, &[], &heads, &tags)?
        } else if legacy_remote.is_some() || branches_remote.is_some() {
            fetch_object_copy_roots(&remote_repo.git_dir, &[], &refspecs, &heads, &tags)?
        } else {
            let coalesced =
                remotes_sharing_repository_url(config, git_dir, remote_repo, remote_name);
            let mut merged = Vec::new();
            for rn in &coalesced {
                let rs = collect_refspecs(config, &format!("remote.{rn}.fetch"));
                merged.extend(fetch_object_copy_roots(
                    &remote_repo.git_dir,
                    &[],
                    &rs,
                    &heads,
                    &tags,
                )?);
            }
            if !args.no_tags {
                merged.extend(tags.iter().map(|(_, o)| *o));
            }
            merged.sort_by_key(|o| o.to_hex());
            merged.dedup();
            merged
        };
        if args.refetch {
            copy_objects(&remote_repo.git_dir, git_dir, true)
                .context("copying objects from remote")?;
        } else {
            copy_reachable_objects(&remote_repo.git_dir, git_dir, &object_copy_roots)
                .context("copying reachable objects from remote")?;
        }
        check_connectivity(git_dir, &object_copy_roots)?;
        (heads, tags)
    };

    let tip_oids: Vec<ObjectId> = remote_heads
        .iter()
        .chain(remote_tags.iter())
        .map(|(_, oid)| *oid)
        .collect();
    crate::trace_packet::trace_fetch_tip_availability(&git_dir.join("objects"), &tip_oids);

    // Handle --depth / --deepen: write shallow graft info
    let effective_depth = args.depth.or(args.deepen);
    if let Some(depth) = effective_depth {
        if let Some(ref remote_repo) = remote_repo {
            write_shallow_info(git_dir, &remote_heads, remote_repo, depth)?;
        } else if is_http_url {
            let local_repo = Repository::open(git_dir, None)
                .context("open repository for shallow metadata after HTTP fetch")?;
            write_shallow_info(git_dir, &remote_heads, &local_repo, depth)?;
        }
    }

    // Prune namespace: URL/path remotes with explicit refspecs update refs outside
    // refs/remotes/<name>/; prune must cover those destinations (Git behavior).
    let is_url_remote = url_override.is_some();
    let prune_namespace =
        (args.prune || args.prune_tags) && is_url_remote && user_passed_cli_refspecs;
    let _dst_prefix = if prune_namespace {
        longest_common_ref_prefix_from_cli_positive(cli_refspecs)
            .unwrap_or_else(|| "refs/".to_string())
    } else {
        format!("refs/remotes/{remote_name}/")
    };

    // Track which remote-tracking refs we updated (for prune)
    let mut updated_refs: Vec<String> = Vec::new();
    let mut has_updates = false;

    let remote_symbolic_head_branch = remote_repo
        .as_ref()
        .and_then(|r| remote_symbolic_head_branch(&r.git_dir));
    let _remote_default_branch_fetch = remote_repo
        .as_ref()
        .and_then(|r| remote_default_branch_for_fetch_merge(&r.git_dir));

    // Collect FETCH_HEAD entries
    let mut fetch_head_entries: Vec<String> = Vec::new();

    // Command-line refspecs (including OID sources like `git fetch origin $B:refs/heads/main`)
    // must update local refs after upload-pack negotiation as well as after the SSH/copy-object
    // path. `collect_wants_cli` only drives the pack request; ref writes happen here.
    if user_passed_cli_refspecs && !prefetch_left_no_positive && !is_ext_url && !is_http_url {
        let remote_repo = remote_repo
            .as_ref()
            .expect("CLI refspec fetch requires a local remote repository");
        // Collect negative refspecs first (^pattern)
        let negative_patterns: Vec<&str> = cli_refspecs
            .iter()
            .filter_map(|s| s.strip_prefix('^'))
            .collect();

        // Validate negative refspecs: they must be ref patterns, not OIDs
        for pat in &negative_patterns {
            let clean = pat.strip_prefix("refs/").unwrap_or(pat);
            if clean.chars().all(|c| c.is_ascii_hexdigit()) && clean.len() >= 7 {
                bail!("negative refspecs do not support object ids: ^{pat}");
            }
        }

        let is_excluded = |refname: &str| -> bool {
            negative_patterns
                .iter()
                .any(|pat| ref_excluded_by_negative_pattern(pat, refname))
        };

        // Pre-check: detect conflicting CLI refspec mappings
        {
            let mut dst_to_src: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let remote_all_refs = refs::list_refs(&remote_repo.git_dir, "refs/")?;
            for spec in cli_refspecs {
                if spec.starts_with('^') {
                    continue;
                }
                let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
                let (src, dst) = if let Some(idx) = spec_clean.find(':') {
                    (
                        spec_clean[..idx].to_owned(),
                        spec_clean[idx + 1..].to_owned(),
                    )
                } else {
                    continue;
                };
                if dst.is_empty() {
                    continue;
                }
                if src.contains('*') {
                    for (refname, _) in &remote_all_refs {
                        if is_excluded(refname) {
                            continue;
                        }
                        if let Some(matched) = match_glob_pattern(&src, refname) {
                            let local_ref = dst.replacen('*', matched, 1);
                            if let Some(prev_src) = dst_to_src.get(&local_ref) {
                                if prev_src != refname {
                                    {
                                        eprintln!(
                                            "fatal: Cannot fetch both {} and {} to {}",
                                            prev_src, refname, local_ref
                                        );
                                        std::process::exit(128);
                                    }
                                }
                            } else {
                                dst_to_src.insert(local_ref, refname.to_string());
                            }
                        }
                    }
                } else {
                    let remote_ref = if src.starts_with("refs/") {
                        src.clone()
                    } else {
                        format!("refs/heads/{src}")
                    };
                    let local_ref = normalize_fetch_refspec_dst(&dst);
                    if let Some(prev_src) = dst_to_src.get(&local_ref) {
                        if prev_src != &remote_ref {
                            {
                                eprintln!(
                                    "fatal: Cannot fetch both {} and {} to {}",
                                    prev_src, remote_ref, local_ref
                                );
                                std::process::exit(128);
                            }
                        }
                    } else {
                        dst_to_src.insert(local_ref, remote_ref);
                    }
                }
            }
        }

        // Process command-line refspecs directly.
        for spec in cli_refspecs {
            // Skip negative refspecs (already collected above)
            if spec.starts_with('^') {
                continue;
            }
            // Check for force prefix '+'
            let (force, spec_clean) = if spec.starts_with('+') {
                (true, &spec[1..])
            } else {
                (false, spec.as_str())
            };
            let (src, dst) = if let Some(idx) = spec_clean.find(':') {
                (
                    spec_clean[..idx].to_owned(),
                    spec_clean[idx + 1..].to_owned(),
                )
            } else {
                (spec_clean.to_owned(), String::new())
            };

            // Handle glob refspecs (e.g. refs/remotes/*:refs/remotes/*)
            if src.contains('*') {
                let remote_all_refs = refs::list_refs(&remote_repo.git_dir, "refs/")?;
                for (refname, remote_oid) in &remote_all_refs {
                    if is_excluded(refname) {
                        continue;
                    }
                    if let Some(matched) = match_glob_pattern(&src, refname) {
                        let local_ref = dst.replacen('*', matched, 1);
                        let old_oid = read_ref_oid(git_dir, &local_ref);
                        if old_oid.as_ref() == Some(remote_oid) {
                            continue;
                        }

                        if !has_updates && !args.quiet {
                            eprintln!("From {display_url}");
                            has_updates = true;
                        }

                        // Refuse to update a ref that's checked out in a worktree (unless
                        // `--update-head-ok`, matching Git's fetch safety valve).
                        if local_ref.starts_with("refs/heads/") && !args.update_head_ok {
                            if let Some(wt_path) = is_branch_in_worktree(git_dir, &local_ref) {
                                bail!(
                                    "refusing to fetch into branch '{}' checked out at '{}'",
                                    local_ref,
                                    wt_path
                                );
                            }
                        }
                        refs::write_ref(git_dir, &local_ref, remote_oid)
                            .with_context(|| format!("updating ref {local_ref}"))?;

                        if !args.quiet {
                            let short = local_ref
                                .strip_prefix("refs/heads/")
                                .or_else(|| local_ref.strip_prefix("refs/tags/"))
                                .unwrap_or(&local_ref);
                            let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
                            match old_oid {
                                None => eprintln!(" * [new branch]      {branch:<17} -> {short}"),
                                Some(old) => eprintln!(
                                    "   {}..{}  {branch:<17} -> {short}",
                                    &old.to_string()[..7],
                                    &remote_oid.to_string()[..7],
                                ),
                            }
                        }

                        // Build FETCH_HEAD entry
                        let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
                        fetch_head_entries.push(fetch_head_branch_line(
                            remote_oid,
                            branch,
                            &display_url,
                            false,
                        ));
                    }
                }
                // Also copy symbolic refs for the matched pattern
                copy_symrefs(&remote_repo.git_dir, git_dir, &src, &dst)?;
                continue;
            }

            // Resolve source: full OID, ref name, or short branch/tag (match `collect_wants_cli`).
            let remote_oid = if let Ok(oid) = ObjectId::from_hex(src.as_str()) {
                oid
            } else {
                let remote_ref = if src.starts_with("refs/") {
                    src.clone()
                } else {
                    format!("refs/heads/{src}")
                };
                match refs::resolve_ref(&remote_repo.git_dir, &remote_ref) {
                    Ok(oid) => oid,
                    Err(_) if !src.starts_with("refs/") => {
                        let tag_ref = format!("refs/tags/{src}");
                        refs::resolve_ref(&remote_repo.git_dir, &tag_ref)
                            .with_context(|| format!("couldn't find remote ref '{}'", src))?
                    }
                    Err(e) => {
                        bail!("couldn't find remote ref '{src}': {e}");
                    }
                }
            };

            let branch_label = if ObjectId::from_hex(src.as_str()).is_ok() {
                src.as_str()
            } else if let Some(rest) = src.strip_prefix("refs/heads/") {
                rest
            } else if let Some(rest) = src.strip_prefix("refs/tags/") {
                rest
            } else {
                src.as_str()
            };
            fetch_head_entries.push(fetch_head_branch_line(
                &remote_oid,
                branch_label,
                &display_url,
                dst.is_empty(),
            ));

            // If a destination is specified, write the ref there
            if !dst.is_empty() {
                let local_ref = normalize_fetch_refspec_dst(&dst);

                let old_oid = read_ref_oid(git_dir, &local_ref);

                // Check fast-forward: reject non-ff updates unless forced
                if let Some(ref old) = old_oid {
                    if old != &remote_oid && !force {
                        let is_ff = merge_base::is_ancestor(&remote_repo, *old, remote_oid)
                            .unwrap_or(false);
                        if !is_ff {
                            eprintln!(" ! [rejected]        {src} -> {dst} (non-fast-forward)");
                            bail!("cannot fast-forward ref '{local_ref}'");
                        }
                    }
                }

                if old_oid.as_ref() != Some(&remote_oid) {
                    if !has_updates && !args.quiet {
                        eprintln!("From {display_url}");
                        has_updates = true;
                    }

                    // Check if branch is checked out in a worktree before updating.
                    if local_ref.starts_with("refs/heads/") && !args.update_head_ok {
                        if let Some(wt_path) = is_branch_in_worktree(git_dir, &local_ref) {
                            bail!(
                                "refusing to fetch into branch '{}' checked out at '{}'",
                                local_ref,
                                wt_path
                            );
                        }
                    }
                    refs::write_ref(git_dir, &local_ref, &remote_oid)
                        .with_context(|| format!("updating ref {local_ref}"))?;

                    if !args.quiet {
                        let short = local_ref
                            .strip_prefix("refs/heads/")
                            .or_else(|| local_ref.strip_prefix("refs/tags/"))
                            .unwrap_or(&local_ref);
                        match old_oid {
                            None => {
                                eprintln!(" * [new branch]      {branch_label:<17} -> {short}");
                            }
                            Some(old) => {
                                eprintln!(
                                    "   {}..{}  {branch_label:<17} -> {short}",
                                    &old.to_string()[..7],
                                    &remote_oid.to_string()[..7],
                                );
                            }
                        }
                    }
                }
            }
        }

        // Emit warnings when CLI refspec destinations conflict with configured tracking
        let configured_refspecs = collect_refspecs(config, &fetch_key);
        if !configured_refspecs.is_empty() {
            for spec in cli_refspecs {
                if spec.starts_with('^') {
                    continue;
                }
                let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
                let (src, dst) = if let Some(idx) = spec_clean.find(':') {
                    (
                        spec_clean[..idx].to_owned(),
                        spec_clean[idx + 1..].to_owned(),
                    )
                } else {
                    continue;
                };
                if dst.is_empty() || src.contains('*') {
                    continue;
                }
                let remote_ref = if src.starts_with("refs/") {
                    src.clone()
                } else {
                    format!("refs/heads/{src}")
                };
                let local_ref = normalize_fetch_refspec_dst(&dst);
                // Check what the configured refspec would map this destination to
                if let Some(usual_src) = reverse_map_refspec(&local_ref, &configured_refspecs) {
                    if usual_src != remote_ref {
                        eprintln!(
                            "warning: {} usually tracks {}, not {}",
                            local_ref, usual_src, remote_ref
                        );
                    }
                }
            }
        }
    } else {
        let mut union_refspecs: Vec<FetchRefspec> = Vec::new();
        for rn in &coalesced_remotes {
            let key = format!("remote.{rn}.fetch");
            let mut rs = collect_refspecs(config, &key);
            if rs.is_empty() {
                rs = default_fetch_refspecs(rn);
            }
            union_refspecs.extend(rs);
        }

        if !union_refspecs.is_empty() {
            let mut dst_to_src: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (refname, _) in &remote_heads {
                if let Some(local_ref) = map_ref_through_refspecs(refname, &union_refspecs) {
                    if let Some(prev_src) = dst_to_src.get(&local_ref) {
                        if prev_src != refname {
                            eprintln!(
                                "fatal: Cannot fetch both {} and {} to {}",
                                prev_src, refname, local_ref
                            );
                            std::process::exit(128);
                        }
                    } else {
                        dst_to_src.insert(local_ref, refname.to_string());
                    }
                }
            }
        }

        // Standard path: update remote-tracking refs from remote heads
        let has_merge_cfg = branch_has_merge_config_for_remote(git_dir, config, remote_name);

        let remote_for_refs = remote_repo
            .as_ref()
            .expect("configured fetch requires local remote repository");

        for (idx, (refname, advertised_oid)) in remote_heads.iter().enumerate() {
            let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            let Some(local_ref) = map_ref_through_refspecs(refname, &union_refspecs) else {
                continue;
            };
            let remote_oid = refs::resolve_ref(&remote_for_refs.git_dir, refname)
                .ok()
                .unwrap_or(*advertised_oid);
            updated_refs.push(local_ref.clone());

            let for_merge = if has_merge_cfg {
                fetch_head_is_for_merge_with_branch(git_dir, config, remote_name, refname)
            } else if refspecs.is_empty() {
                idx == 0
            } else {
                fetch_head_is_for_merge_first_refspec_only(&refspecs, idx == 0)
            };
            fetch_head_entries.push(fetch_head_branch_line(
                &remote_oid,
                branch,
                &display_url,
                for_merge,
            ));

            let old_oid = read_ref_oid(git_dir, &local_ref);
            if old_oid.as_ref() == Some(&remote_oid) {
                continue;
            }

            if !has_updates && !args.quiet {
                eprintln!("From {display_url}");
                has_updates = true;
            }

            refs::write_ref(git_dir, &local_ref, &remote_oid)
                .with_context(|| format!("updating ref {local_ref}"))?;
            let _ = append_fetch_reflog(
                git_dir,
                &local_ref,
                old_oid.as_ref(),
                &remote_oid,
                &url,
                branch,
            );

            let tracking_print = local_ref
                .strip_prefix("refs/remotes/")
                .and_then(|s| s.find('/').map(|i| &s[..i]))
                .unwrap_or("origin");

            if args.porcelain {
                let zero = "0".repeat(40);
                let old_hex = old_oid
                    .as_ref()
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| zero.clone());
                let flag = if old_oid.is_none() { "*" } else { " " };
                println!("{flag} {old_hex} {remote_oid} {local_ref}");
            } else if !args.quiet {
                print_update(&old_oid, &remote_oid, branch, tracking_print);
            }
        }

        if implicit_path_fetch {
            if let Some(rb) = remote_symbolic_head_branch.as_deref() {
                if let Some(oid) = remote_heads
                    .iter()
                    .find(|(r, _)| r == &format!("refs/heads/{rb}"))
                    .map(|(_, o)| *o)
                {
                    fetch_head_entries.push(fetch_head_bare_url_line(&oid, &display_url));
                }
            }
        }
    }

    if should_fetch_tags {
        for (refname, remote_oid) in &remote_tags {
            let old_oid = read_ref_oid(git_dir, refname);
            if old_oid.as_ref() == Some(remote_oid) {
                continue;
            }

            if !has_updates && !args.quiet {
                eprintln!("From {display_url}");
                has_updates = true;
            }

            refs::write_ref(git_dir, refname, remote_oid)
                .with_context(|| format!("updating tag {refname}"))?;
            let _ = append_fetch_reflog(git_dir, refname, old_oid.as_ref(), remote_oid, &url, "");

            if !args.quiet {
                let tag_name = refname.strip_prefix("refs/tags/").unwrap_or(refname);
                if let Some(old) = old_oid {
                    eprintln!(
                        "   {}..{}  {tag_name:<17} -> {tag_name}",
                        &old.to_string()[..7],
                        &remote_oid.to_string()[..7],
                    );
                } else {
                    eprintln!(" * [new tag]         {tag_name:<17} -> {tag_name}");
                }
            }
        }
    }

    if user_passed_cli_refspecs && !prefetch_left_no_positive && !is_ext_url {
        // Tag refs updated via explicit CLI refspecs already emitted branch-style FETCH_HEAD lines;
        // replace those with Git-shaped `tag 'name'` lines.
        fetch_head_entries.retain(|line| !line.contains("refs/tags/"));
        let remote_repo = remote_repo
            .as_ref()
            .expect("CLI refspec fetch requires a local remote repository");
        for spec in cli_refspecs {
            if spec.starts_with('^') {
                continue;
            }
            let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
            let src = spec_clean
                .split_once(':')
                .map(|(a, _)| a)
                .unwrap_or(spec_clean);
            if src.contains('*') || !src.starts_with("refs/tags/") {
                continue;
            }
            let tag_name = src.strip_prefix("refs/tags/").unwrap_or(src);
            let remote_oid = refs::resolve_ref(&remote_repo.git_dir, src)
                .with_context(|| format!("couldn't find remote ref '{tag_name}'"))?;
            fetch_head_entries.push(fetch_head_tag_line(
                &remote_oid,
                tag_name,
                &display_url,
                true,
            ));
        }
    } else if should_fetch_tags && (!implicit_path_fetch || args.tags) {
        for (refname, remote_oid) in &remote_tags {
            let tag_name = refname.strip_prefix("refs/tags/").unwrap_or(refname);
            fetch_head_entries.push(fetch_head_tag_line(
                remote_oid,
                tag_name,
                &display_url,
                false,
            ));
        }
    }

    // Prune tags that no longer exist on the remote
    if args.prune_tags {
        let local_tags = refs::list_refs(git_dir, "refs/tags/")?;
        for (local_tag_ref, _oid) in &local_tags {
            let exists_on_remote = remote_tags.iter().any(|(r, _)| r == local_tag_ref);
            if !exists_on_remote {
                if !has_updates && !args.quiet {
                    eprintln!("From {display_url}");
                    has_updates = true;
                }
                refs::delete_ref(git_dir, local_tag_ref)
                    .with_context(|| format!("pruning tag {local_tag_ref}"))?;
                if !args.quiet {
                    let tag_name = local_tag_ref
                        .strip_prefix("refs/tags/")
                        .unwrap_or(local_tag_ref);
                    eprintln!(" - [deleted]         (none)     -> {tag_name}");
                }
            }
        }
    }

    // Prune stale remote-tracking refs
    if args.prune || args.prune_tags {
        if !has_updates && !args.quiet {
            let mut will_prune = false;
            for rn in &coalesced_remotes {
                let prefix = format!("refs/remotes/{rn}/");
                let existing = refs::list_refs(git_dir, &prefix)?;
                if existing.iter().any(|(r, _)| !updated_refs.contains(r)) {
                    will_prune = true;
                    break;
                }
            }
            if will_prune {
                eprintln!("From {display_url}");
            }
        }
        for rn in &coalesced_remotes {
            let prefix = format!("refs/remotes/{rn}/");
            prune_stale_refs(git_dir, &prefix, &updated_refs, rn, args.quiet)?;
        }
    }

    // Update `refs/remotes/<remote>/HEAD` to match the remote's default branch (Git `set_head`).
    let follow = parse_follow_remote_head(config, remote_name);
    let do_set_head =
        cli_refspecs.is_empty() && !refspecs.is_empty() && follow.mode != FollowRemoteHead::Never;
    if do_set_head {
        if follow.mode != FollowRemoteHead::Never {
            trace_ls_refs_head_prefix();
        }
        if let Some(default_branch) = remote_symbolic_head_branch.as_deref() {
            let head_source = format!("refs/heads/{default_branch}");
            let mapped_default = if refspecs.is_empty() {
                Some(format!("refs/remotes/{remote_name}/{default_branch}"))
            } else {
                map_ref_through_refspecs(&head_source, &refspecs)
            };
            if let Some(mapped_default_ref) = mapped_default {
                if refs::resolve_ref(git_dir, &mapped_default_ref).is_ok() {
                    let remote_head_ref = format!("refs/remotes/{remote_name}/HEAD");
                    let previous = read_remote_head_previous(git_dir, remote_name);
                    let head_missing = matches!(previous, RemoteHeadPrevious::Missing);
                    let should_write = match follow.mode {
                        FollowRemoteHead::Never => false,
                        FollowRemoteHead::Create | FollowRemoteHead::Warn => head_missing,
                        FollowRemoteHead::Always => true,
                    };
                    if should_write {
                        refs::write_symbolic_ref(git_dir, &remote_head_ref, &mapped_default_ref)
                            .with_context(|| format!("updating symbolic ref {remote_head_ref}"))?;
                        updated_refs.push(remote_head_ref);
                    }
                    maybe_warn_follow_remote_head(
                        &follow,
                        remote_name,
                        default_branch,
                        previous,
                        args.quiet,
                    );
                }
            }
        }
    }
    if !fetch_head_entries.is_empty() {
        sort_fetch_head_lines(&mut fetch_head_entries, &fetch_head_refspecs);
        let fetch_head_path = git_dir.join("FETCH_HEAD");
        let content = fetch_head_entries.join("\n") + "\n";
        fs::write(&fetch_head_path, content).context("writing FETCH_HEAD")?;
    }

    if args.filter.as_deref() == Some("blob:none") {
        apply_blob_none_filter(git_dir, &remote_heads).context("applying blob:none filter")?;
    }

    // Write machine-readable output if --output is given
    if let Some(ref output_path) = args.output {
        let mut lines = Vec::new();
        let out_prefix = format!("refs/remotes/{tracking_remote_name}/");
        for (refname, remote_oid) in &remote_heads {
            let branch = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            let local_ref = format!("{out_prefix}{branch}");
            let old_oid = read_ref_oid(git_dir, &local_ref);
            let old_hex = old_oid
                .map(|o| o.to_string())
                .unwrap_or_else(|| "0".repeat(40));
            let flag = if old_oid.is_none() {
                "*"
            } else if old_oid.as_ref() == Some(remote_oid) {
                "="
            } else {
                " "
            };
            lines.push(format!("{flag} {} {} {local_ref}", old_hex, remote_oid,));
        }
        let content = lines.join("\n") + "\n";
        fs::write(output_path, content).context("writing --output file")?;
    }

    Ok(())
}

fn apply_blob_none_filter(git_dir: &Path, remote_heads: &[(String, ObjectId)]) -> Result<()> {
    let patterns = load_sparse_patterns(git_dir)?;
    let odb = grit_lib::odb::Odb::new(&git_dir.join("objects"));
    let mut seen_trees = HashSet::new();
    let mut all_blobs = HashSet::new();
    let mut keep_blobs = HashSet::new();

    for (_, commit_oid) in remote_heads {
        let commit_obj = match odb.read(commit_oid) {
            Ok(obj) => obj,
            Err(_) => continue,
        };
        if commit_obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = match parse_commit(&commit_obj.data) {
            Ok(c) => c,
            Err(_) => continue,
        };
        collect_blob_sets_for_tree(
            &odb,
            commit.tree,
            "",
            &patterns,
            &mut seen_trees,
            &mut all_blobs,
            &mut keep_blobs,
        )?;
    }

    for oid in all_blobs.drain() {
        if keep_blobs.contains(&oid) {
            continue;
        }
        let hex = oid.to_hex();
        if hex.len() < 3 {
            continue;
        }
        let loose_path = git_dir
            .join("objects")
            .join(&hex[..2])
            .join(&hex[2..hex.len()]);
        if loose_path.exists() {
            let _ = fs::remove_file(loose_path);
        }
    }

    Ok(())
}

fn collect_blob_sets_for_tree(
    odb: &grit_lib::odb::Odb,
    tree_oid: ObjectId,
    prefix: &str,
    patterns: &[String],
    seen_trees: &mut HashSet<ObjectId>,
    all_blobs: &mut HashSet<ObjectId>,
    keep_blobs: &mut HashSet<ObjectId>,
) -> Result<()> {
    if !seen_trees.insert(tree_oid) {
        return Ok(());
    }

    let tree_obj = match odb.read(&tree_oid) {
        Ok(obj) => obj,
        Err(_) => return Ok(()),
    };
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(());
    }

    let entries = parse_tree(&tree_obj.data)?;
    for entry in entries {
        if entry.mode == 0o160000 {
            continue;
        }
        let name = String::from_utf8_lossy(&entry.name);
        let rel_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if (entry.mode & 0o170000) == 0o040000 {
            collect_blob_sets_for_tree(
                odb, entry.oid, &rel_path, patterns, seen_trees, all_blobs, keep_blobs,
            )?;
            continue;
        }
        let blob_obj = match odb.read(&entry.oid) {
            Ok(obj) => obj,
            Err(_) => continue,
        };
        if blob_obj.kind != ObjectKind::Blob {
            continue;
        }
        all_blobs.insert(entry.oid);
        if sparse_path_is_included(patterns, &rel_path) {
            keep_blobs.insert(entry.oid);
        }
    }
    Ok(())
}

fn load_sparse_patterns(git_dir: &Path) -> Result<Vec<String>> {
    let sparse_path = git_dir.join("info").join("sparse-checkout");
    let content = match fs::read_to_string(&sparse_path) {
        Ok(content) => content,
        Err(_) => return Ok(Vec::new()),
    };
    let patterns = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_owned())
        .collect::<Vec<_>>();
    Ok(patterns)
}

fn sparse_path_is_included(patterns: &[String], path: &str) -> bool {
    if patterns.is_empty() {
        return false;
    }

    let mut include = false;
    for raw in patterns {
        let pattern = raw.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }
        let (exclude, pat) = if let Some(rest) = pattern.strip_prefix('!') {
            (true, rest)
        } else {
            (false, pattern)
        };
        if sparse_pattern_matches(pat, path) {
            include = !exclude;
        }
    }
    include
}

fn sparse_pattern_matches(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim();
    if pat.is_empty() {
        return false;
    }

    let anchored = pat.starts_with('/');
    let pat = pat.trim_start_matches('/');

    if let Some(dir) = pat.strip_suffix('/') {
        if anchored {
            return path == dir || path.starts_with(&format!("{dir}/"));
        }
        return path == dir
            || path.starts_with(&format!("{dir}/"))
            || path.split('/').any(|component| component == dir);
    }

    if anchored {
        return sparse_glob_match(pat.as_bytes(), path.as_bytes());
    }
    sparse_glob_match(pat.as_bytes(), path.as_bytes())
        || path
            .rsplit('/')
            .next()
            .is_some_and(|base| sparse_glob_match(pat.as_bytes(), base.as_bytes()))
}

fn sparse_glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut pi, mut ti) = (0, 0);
    let (mut star_p, mut star_t) = (usize::MAX, 0);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Print a ref update line (to stderr, matching git).
fn print_update(old_oid: &Option<ObjectId>, new_oid: &ObjectId, branch: &str, remote_name: &str) {
    let tracking = format!("{remote_name}/{branch}");
    match old_oid {
        None => {
            eprintln!(" * [new branch]      {branch:<17} -> {tracking}");
        }
        Some(old) => {
            eprintln!(
                "   {}..{}  {branch:<17} -> {tracking}",
                &old.to_string()[..7],
                &new_oid.to_string()[..7],
            );
        }
    }
}

/// Symref target of the remote's `HEAD` (may differ from the configured default branch).
fn remote_symbolic_head_branch(remote_git_dir: &Path) -> Option<String> {
    let head_path = remote_git_dir.join("HEAD");
    let content = fs::read_to_string(&head_path).ok()?;
    let content = content.trim();
    let refname = content.strip_prefix("ref: refs/heads/")?;
    Some(refname.to_string())
}

/// Branch used for "default" FETCH_HEAD for-merge when the current branch has no `branch.*.merge`:
/// `init.defaultBranch` when present and valid, else [`remote_symbolic_head_branch`].
fn remote_default_branch_for_fetch_merge(remote_git_dir: &Path) -> Option<String> {
    let default_branch_cfg =
        ConfigSet::read_early_config(Some(remote_git_dir), "init.defaultBranch")
            .ok()
            .and_then(|v| v.last().cloned())
            .or_else(|| {
                ConfigSet::read_early_config(Some(remote_git_dir), "init.defaultbranch")
                    .ok()
                    .and_then(|v| v.last().cloned())
            });
    if let Some(db) = default_branch_cfg {
        let db = db.trim();
        if !db.is_empty() && refs::resolve_ref(remote_git_dir, &format!("refs/heads/{db}")).is_ok()
        {
            return Some(db.to_owned());
        }
    }
    remote_symbolic_head_branch(remote_git_dir)
}

/// Read a ref to get its OID, returning None if it doesn't exist.
fn read_ref_oid(git_dir: &Path, refname: &str) -> Option<ObjectId> {
    refs::resolve_ref(git_dir, refname).ok()
}

/// Resolve a ref by reading only loose files under `git_dir` (symbolic chain).
///
/// `refs::resolve_ref` can still fail in some harness states even when the ref file exists;
/// upload-pack want computation must match on-disk refs (t5503-tagfollow).
fn read_loose_ref_chain(git_dir: &Path, refname: &str) -> Option<ObjectId> {
    let mut name = refname.to_string();
    for _ in 0..20 {
        let path = git_dir.join(&name);
        let content = fs::read_to_string(&path).ok()?;
        let line = content.trim_end_matches(['\n', '\r']);
        if let Some(target) = line.strip_prefix("ref: ") {
            name = target.trim().to_string();
            continue;
        }
        if line.len() == 40 && line.chars().all(|c| c.is_ascii_hexdigit()) {
            return line.parse().ok();
        }
        return None;
    }
    None
}

fn zero_oid() -> ObjectId {
    ObjectId::from_hex("0000000000000000000000000000000000000000").expect("null oid")
}

fn fetch_reflog_identity(git_dir: &Path) -> String {
    let config = ConfigSet::load(Some(git_dir), true).ok();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_NAME").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_EMAIL").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}

/// Append a reflog line for a ref updated by fetch (remote-tracking branches and tags).
fn append_fetch_reflog(
    git_dir: &Path,
    refname: &str,
    old_oid: Option<&ObjectId>,
    new_oid: &ObjectId,
    remote_url: &str,
    branch: &str,
) -> anyhow::Result<()> {
    let old = old_oid.cloned().unwrap_or_else(zero_oid);
    let message = if branch.is_empty() {
        format!("fetch --append --prune {remote_url}")
    } else {
        format!("fetch --append --prune {remote_url} branch '{branch}' of {remote_url}")
    };
    let ident = fetch_reflog_identity(git_dir);
    refs::append_reflog(git_dir, refname, &old, new_oid, &ident, &message, true)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// OIDs whose object closure should be copied for this fetch (non-refetch local transport).
///
/// When the user passes explicit refspecs, only those sources are roots; otherwise configured
/// refspecs filter which remote refs participate; if there are no configured refspecs, all
/// remote heads and tags are roots (Git's default refspec set).
fn fetch_object_copy_roots(
    remote_git_dir: &Path,
    cli_refspecs: &[String],
    refspecs: &[FetchRefspec],
    heads: &[(String, ObjectId)],
    tags: &[(String, ObjectId)],
) -> Result<Vec<ObjectId>> {
    let mut roots = Vec::new();

    if !cli_refspecs.is_empty() {
        let negative_patterns: Vec<&str> = cli_refspecs
            .iter()
            .filter_map(|s| s.strip_prefix('^'))
            .collect();
        let is_excluded = |refname: &str| -> bool {
            for pat in &negative_patterns {
                let full_pat = if pat.starts_with("refs/") {
                    pat.to_string()
                } else {
                    format!("refs/heads/{pat}")
                };
                if match_glob_pattern(&full_pat, refname).is_some() || full_pat == refname {
                    return true;
                }
            }
            false
        };

        for spec in cli_refspecs {
            if spec.starts_with('^') {
                continue;
            }
            let spec_clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
            let src = spec_clean
                .split_once(':')
                .map(|(a, _)| a)
                .unwrap_or(spec_clean);
            if src.contains('*') {
                let remote_all_refs = refs::list_refs(remote_git_dir, "refs/")?;
                for (refname, oid) in &remote_all_refs {
                    if is_excluded(refname) {
                        continue;
                    }
                    if match_glob_pattern(src, refname).is_some() {
                        roots.push(*oid);
                    }
                }
                continue;
            }
            let remote_ref = resolve_remote_ref_for_fetch_src(remote_git_dir, src)?;
            let oid = refs::resolve_ref(remote_git_dir, &remote_ref)
                .with_context(|| format!("couldn't find remote ref '{src}'"))?;
            roots.push(oid);
        }
    } else if refspecs.is_empty() {
        roots.extend(heads.iter().map(|(_, o)| *o));
        roots.extend(tags.iter().map(|(_, o)| *o));
    } else {
        for (refname, oid) in heads.iter().chain(tags.iter()) {
            let mut excluded = false;
            for rs in refspecs {
                if !rs.negative {
                    continue;
                }
                let pat = &rs.src;
                if match_glob_pattern(pat, refname).is_some() || pat == refname {
                    excluded = true;
                    break;
                }
            }
            if excluded {
                continue;
            }
            if map_ref_through_refspecs(refname, refspecs).is_some() {
                roots.push(*oid);
            }
        }
    }

    roots.sort_by_key(|o| o.to_hex());
    roots.dedup();
    Ok(roots)
}

/// Resolve a short or full remote ref name for fetch (CLI refspec source side).
fn resolve_remote_ref_for_fetch_src(remote_git_dir: &Path, src: &str) -> Result<String> {
    if src.starts_with("refs/") {
        return Ok(src.to_owned());
    }
    let heads_ref = format!("refs/heads/{src}");
    if refs::resolve_ref(remote_git_dir, &heads_ref).is_ok() {
        return Ok(heads_ref);
    }
    let tags_ref = format!("refs/tags/{src}");
    if refs::resolve_ref(remote_git_dir, &tags_ref).is_ok() {
        return Ok(tags_ref);
    }
    Ok(heads_ref)
}

/// Copy all objects (loose + packs) from remote to local.
/// If `refetch` is true, re-copy objects even if they already exist locally.
/// Copy objects from a remote git dir to local git dir (public for pull).
pub fn copy_objects_for_pull(src_git_dir: &Path, dst_git_dir: &Path) -> Result<()> {
    copy_objects(src_git_dir, dst_git_dir, false)
}

/// Copy objects reachable from `roots` from `src_git_dir` into `dst_git_dir` (loose only).
///
/// Used for local `fetch` so the destination does not receive unrelated objects from the
/// source object database (matching Git's behavior and keeping negotiation tests faithful).
fn copy_reachable_objects(
    src_git_dir: &Path,
    dst_git_dir: &Path,
    roots: &[ObjectId],
) -> Result<()> {
    let src_odb = Odb::new(&src_git_dir.join("objects"));
    let dst_odb = Odb::new(&dst_git_dir.join("objects"));
    let mut stack: Vec<ObjectId> = roots.to_vec();
    let mut seen = HashSet::new();

    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = src_odb.read(&oid).with_context(|| {
            format!("missing object {} while copying from remote", oid.to_hex())
        })?;
        if !dst_odb.exists(&oid) {
            dst_odb
                .write(obj.kind, &obj.data)
                .with_context(|| format!("write object {}", oid.to_hex()))?;
        }
        match obj.kind {
            ObjectKind::Commit => {
                let c = parse_commit(&obj.data)?;
                stack.push(c.tree);
                stack.extend_from_slice(&c.parents);
            }
            ObjectKind::Tree => {
                for e in parse_tree(&obj.data)? {
                    stack.push(e.oid);
                }
            }
            ObjectKind::Tag => {
                stack.push(parse_tag(&obj.data)?.object);
            }
            ObjectKind::Blob => {}
        }
    }
    Ok(())
}

fn copy_objects(src_git_dir: &Path, dst_git_dir: &Path, refetch: bool) -> Result<()> {
    let src_objects = src_git_dir.join("objects");
    let dst_objects = dst_git_dir.join("objects");

    // Copy loose objects (fan-out directories: 00..ff)
    if src_objects.is_dir() {
        for entry in fs::read_dir(&src_objects)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Skip info/ and pack/ — handled separately
            if name_str == "info" || name_str == "pack" {
                continue;
            }

            // Only process 2-character hex fan-out dirs
            if !entry.file_type()?.is_dir() || name_str.len() != 2 {
                continue;
            }

            let dst_dir = dst_objects.join(&*name);
            for inner in fs::read_dir(entry.path())? {
                let inner = inner?;
                if inner.file_type()?.is_file() {
                    let dst_file = dst_dir.join(inner.file_name());
                    if refetch || !dst_file.exists() {
                        fs::create_dir_all(&dst_dir)?;
                        if refetch {
                            // Force copy when refetching
                            fs::copy(inner.path(), &dst_file)?;
                        } else if fs::hard_link(inner.path(), &dst_file).is_err() {
                            fs::copy(inner.path(), &dst_file)?;
                        }
                    }
                }
            }
        }
    }

    // Copy pack files
    let src_pack = src_objects.join("pack");
    let dst_pack = dst_objects.join("pack");
    if src_pack.is_dir() {
        fs::create_dir_all(&dst_pack)?;
        for entry in fs::read_dir(&src_pack)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let dst_file = dst_pack.join(entry.file_name());
                if refetch || !dst_file.exists() {
                    if refetch {
                        fs::copy(entry.path(), &dst_file)?;
                    } else if fs::hard_link(entry.path(), &dst_file).is_err() {
                        fs::copy(entry.path(), &dst_file)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Verify that all objects reachable from the given OIDs exist in the local ODB.
/// This is used after copying objects from a remote to detect incomplete transfers.
fn check_connectivity(git_dir: &Path, tip_oids: &[ObjectId]) -> Result<()> {
    use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
    use grit_lib::odb::Odb;
    use std::collections::HashSet;

    let odb = Odb::new(&git_dir.join("objects"));
    let mut seen = HashSet::new();
    let mut stack: Vec<ObjectId> = tip_oids.to_vec();

    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = odb
            .read(&oid)
            .with_context(|| "remote did not send all necessary objects".to_string())?;
        match obj.kind {
            ObjectKind::Commit => {
                if let Ok(commit) = parse_commit(&obj.data) {
                    stack.push(commit.tree);
                    for parent in &commit.parents {
                        stack.push(*parent);
                    }
                }
            }
            ObjectKind::Tree => {
                if let Ok(entries) = parse_tree(&obj.data) {
                    for entry in entries {
                        // Skip gitlink (submodule) entries
                        if entry.mode == 0o160000 {
                            continue;
                        }
                        stack.push(entry.oid);
                    }
                }
            }
            ObjectKind::Blob | ObjectKind::Tag => {
                // Blobs and tags are leaf objects, no further traversal needed
            }
        }
    }
    Ok(())
}

/// Remove remote-tracking refs that no longer exist on the remote.
fn prune_stale_refs(
    git_dir: &Path,
    prefix: &str,
    current_refs: &[String],
    remote_name: &str,
    quiet: bool,
) -> Result<()> {
    let existing = refs::list_refs(git_dir, prefix)?;
    for (refname, _oid) in &existing {
        if !current_refs.contains(refname) {
            refs::delete_ref(git_dir, refname).with_context(|| format!("pruning {refname}"))?;
            if !quiet {
                // Show short name: "origin/branch" instead of "refs/remotes/origin/branch"
                let short = refname.strip_prefix("refs/remotes/").unwrap_or(refname);
                let branch = short
                    .strip_prefix(&format!("{remote_name}/"))
                    .unwrap_or(short);
                eprintln!(" - [deleted]         (none)     -> {remote_name}/{branch}");
            }
        }
    }
    Ok(())
}

/// Write shallow graft information when --depth / --deepen is used.
///
/// For local transport we approximate shallowness by listing the commit(s) at
/// the boundary depth and recording them in `$GIT_DIR/shallow`.
fn write_shallow_info(
    git_dir: &Path,
    remote_heads: &[(String, ObjectId)],
    remote_repo: &Repository,
    depth: usize,
) -> Result<()> {
    use grit_lib::objects::{parse_commit, ObjectKind};
    use grit_lib::odb::Odb;

    let shallow_path = git_dir.join("shallow");
    // Collect existing shallow commits
    let mut shallow_set: std::collections::HashSet<String> = if shallow_path.exists() {
        fs::read_to_string(&shallow_path)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let odb = Odb::new(&remote_repo.git_dir.join("objects"));

    // For each remote head, walk `depth` commits and mark the boundary
    for (_refname, tip_oid) in remote_heads {
        let mut oid = *tip_oid;
        for _ in 0..depth.saturating_sub(1) {
            match odb.read(&oid) {
                Ok(obj) if obj.kind == ObjectKind::Commit => match parse_commit(&obj.data) {
                    Ok(c) => {
                        if c.parents.is_empty() {
                            break;
                        }
                        oid = c.parents[0];
                    }
                    Err(_) => break,
                },
                _ => break,
            }
        }
        shallow_set.insert(oid.to_string());
    }

    let mut entries: Vec<&str> = shallow_set.iter().map(|s| s.as_str()).collect();
    entries.sort();
    let content = entries.join("\n") + "\n";
    fs::write(&shallow_path, content).context("writing shallow file")?;
    Ok(())
}

/// `remote.<name>.followRemoteHEAD` (subset matching Git's `remote.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FollowRemoteHead {
    Never,
    Create,
    Warn,
    Always,
}

/// Parsed `remote.<name>.followRemoteHEAD` plus optional `warn-if-not-<branch>` suffix.
struct FollowRemoteHeadConfig {
    mode: FollowRemoteHead,
    /// When `mode == Warn` and this is `Some`, suppress the warning if the remote HEAD branch
    /// equals this name (Git's `warn-if-not-<branch>`).
    no_warn_branch: Option<String>,
}

fn parse_follow_remote_head(config: &ConfigSet, remote_name: &str) -> FollowRemoteHeadConfig {
    let key = format!("remote.{remote_name}.followRemoteHEAD");
    let key_alt = format!("remote.{remote_name}.followremotehead");
    let raw = config
        .get(&key)
        .or_else(|| config.get(&key_alt))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if raw.is_empty() {
        return FollowRemoteHeadConfig {
            mode: FollowRemoteHead::Create,
            no_warn_branch: None,
        };
    }
    let lower = raw.to_ascii_lowercase();
    if lower == "never" {
        return FollowRemoteHeadConfig {
            mode: FollowRemoteHead::Never,
            no_warn_branch: None,
        };
    }
    if lower == "create" {
        return FollowRemoteHeadConfig {
            mode: FollowRemoteHead::Create,
            no_warn_branch: None,
        };
    }
    if lower == "warn" {
        return FollowRemoteHeadConfig {
            mode: FollowRemoteHead::Warn,
            no_warn_branch: None,
        };
    }
    if lower == "always" {
        return FollowRemoteHeadConfig {
            mode: FollowRemoteHead::Always,
            no_warn_branch: None,
        };
    }
    let prefix = "warn-if-not-";
    if lower.starts_with(prefix) {
        let branch = raw[prefix.len()..].to_string();
        return FollowRemoteHeadConfig {
            mode: FollowRemoteHead::Warn,
            no_warn_branch: if branch.is_empty() {
                None
            } else {
                Some(branch)
            },
        };
    }
    FollowRemoteHeadConfig {
        mode: FollowRemoteHead::Create,
        no_warn_branch: None,
    }
}

fn trace_ls_refs_head_prefix() {
    crate::trace_packet::trace_packet_line(b"fetch> ref-prefix HEAD");
}

/// Previous `refs/remotes/<remote>/HEAD` state for `followRemoteHEAD` warnings.
enum RemoteHeadPrevious {
    Symref(String),
    DetachedOid(ObjectId),
    Missing,
}

fn read_remote_head_previous(git_dir: &Path, remote_name: &str) -> RemoteHeadPrevious {
    let head_ref = format!("refs/remotes/{remote_name}/HEAD");
    let sym = refs::read_symbolic_ref(git_dir, &head_ref).ok().flatten();
    if let Some(target) = sym {
        let prefix = format!("refs/remotes/{remote_name}/");
        let short = target
            .strip_prefix(&prefix)
            .map(|s| s.to_string())
            .unwrap_or(target);
        return RemoteHeadPrevious::Symref(short);
    }
    match read_ref_oid(git_dir, &head_ref) {
        Some(oid) => RemoteHeadPrevious::DetachedOid(oid),
        None => RemoteHeadPrevious::Missing,
    }
}

fn maybe_warn_follow_remote_head(
    follow: &FollowRemoteHeadConfig,
    remote_name: &str,
    remote_default_branch: &str,
    previous: RemoteHeadPrevious,
    quiet: bool,
) {
    if quiet || follow.mode != FollowRemoteHead::Warn {
        return;
    }
    if let Some(ref skip) = follow.no_warn_branch {
        if skip == remote_default_branch {
            return;
        }
    }
    let mut stdout = std::io::stdout();
    match previous {
        RemoteHeadPrevious::Symref(prev_short) if prev_short != remote_default_branch => {
            let _ = writeln!(
                stdout,
                "'HEAD' at '{remote_name}' is '{remote_default_branch}', but we have '{prev_short}' locally."
            );
        }
        RemoteHeadPrevious::DetachedOid(oid) => {
            let _ = writeln!(
                stdout,
                "'HEAD' at '{remote_name}' is '{remote_default_branch}', but we have a detached HEAD pointing to '{}' locally.",
                oid.to_hex()
            );
        }
        _ => {}
    }
}

/// Normalize the right-hand side of a fetch refspec from config (Git allows `remotes/foo/bar`
/// without the leading `refs/`).
fn normalize_fetch_refspec_dst(dst: &str) -> String {
    let d = dst.trim();
    if d.starts_with("refs/") {
        return d.to_owned();
    }
    if d.starts_with("remotes/") {
        return format!("refs/{d}");
    }
    if d.starts_with("tags/") {
        return format!("refs/{d}");
    }
    format!("refs/heads/{d}")
}
/// A parsed refspec (e.g. "+refs/heads/*:refs/remotes/origin/*").
#[derive(Clone)]
struct FetchRefspec {
    /// Source pattern (remote side), e.g. "refs/heads/*".
    src: String,
    /// Destination pattern (local side), e.g. "refs/remotes/origin/*".
    dst: String,
    /// Whether this is a force refspec (leading '+').
    #[allow(dead_code)]
    force: bool,
    /// Whether this is a negative (exclusion) refspec (leading '^').
    negative: bool,
}

/// Returns true if `refname` is excluded by a negative refspec pattern (without the leading `^`).
///
/// Patterns that start with `refs/` are matched against `refname` as written (glob or exact).
/// Unqualified patterns are matched only against `refname` itself — Git does **not** prepend
/// `refs/heads/` to the pattern (see t5582 "does not expand prefix").
fn ref_excluded_by_negative_pattern(pattern: &str, refname: &str) -> bool {
    match_glob_pattern(pattern, refname).is_some() || pattern == refname
}

fn ref_excluded_by_fetch_refspecs(refname: &str, refspecs: &[FetchRefspec]) -> bool {
    refspecs
        .iter()
        .any(|rs| rs.negative && ref_excluded_by_negative_pattern(&rs.src, refname))
}

/// Rewrite positive fetch refspec destinations under `refs/prefetch/`, matching Git's
/// `fetch --prefetch` behavior.
fn apply_prefetch_to_refspecs(specs: &mut Vec<FetchRefspec>) {
    const PREFETCH_NS: &str = "refs/prefetch/";
    let mut i = 0usize;
    while i < specs.len() {
        if specs[i].negative {
            i += 1;
            continue;
        }
        let src = specs[i].src.as_str();
        let dst = specs[i].dst.as_str();
        let remove = dst.is_empty() || src.starts_with("refs/tags/");
        if remove {
            specs.remove(i);
            continue;
        }
        let mut new_dst = String::from(PREFETCH_NS);
        if let Some(rest) = dst.strip_prefix("refs/") {
            new_dst.push_str(rest);
        } else {
            new_dst.push_str(dst);
        }
        specs[i].dst = new_dst;
        specs[i].force = true;
        i += 1;
    }
}

/// Parse command-line fetch refspec strings into [`FetchRefspec`] entries (for `--prefetch`).
fn parse_cli_fetch_refspecs(cli: &[String]) -> Vec<FetchRefspec> {
    let mut out = Vec::new();
    for spec in cli {
        if let Some(pat) = spec.strip_prefix('^') {
            out.push(FetchRefspec {
                src: pat.to_owned(),
                dst: String::new(),
                force: false,
                negative: true,
            });
            continue;
        }
        let (force, rest) = if let Some(s) = spec.strip_prefix('+') {
            (true, s)
        } else {
            (false, spec.as_str())
        };
        if let Some(colon) = rest.find(':') {
            out.push(FetchRefspec {
                src: rest[..colon].to_owned(),
                dst: rest[colon + 1..].to_owned(),
                force,
                negative: false,
            });
        } else {
            out.push(FetchRefspec {
                src: rest.to_owned(),
                dst: rest.to_owned(),
                force,
                negative: false,
            });
        }
    }
    out
}

fn fetch_refspec_to_cli_string(rs: &FetchRefspec) -> String {
    if rs.negative {
        return format!("^{}", rs.src);
    }
    let mut s = String::new();
    if rs.force {
        s.push('+');
    }
    s.push_str(&rs.src);
    s.push(':');
    s.push_str(&rs.dst);
    s
}

/// Longest directory prefix shared by all ref names (must end on `/`), for pruning after a
/// URL-remote fetch with explicit refspecs.
fn longest_common_ref_prefix(refs: &[String]) -> Option<String> {
    if refs.is_empty() {
        return None;
    }
    let first = refs[0].as_bytes();
    let mut len = first.len();
    for r in refs.iter().skip(1) {
        let b = r.as_bytes();
        let max = len.min(b.len());
        let mut common = 0usize;
        while common < max && first[common] == b[common] {
            common += 1;
        }
        len = len.min(common);
    }
    let prefix = std::str::from_utf8(&first[..len]).ok()?;
    let cut = prefix.rfind('/')?;
    if cut == 0 {
        return None;
    }
    Some(prefix[..=cut].to_string())
}

fn normalize_fetch_dst(dst: &str) -> String {
    if dst.starts_with("refs/") {
        dst.to_owned()
    } else {
        format!("refs/heads/{dst}")
    }
}

/// Local ref destinations from positive CLI refspecs (used for `--prune` namespace).
fn longest_common_ref_prefix_from_cli_positive(cli: &[String]) -> Option<String> {
    let mut locals = Vec::new();
    for spec in cli {
        if spec.starts_with('^') {
            continue;
        }
        let clean = spec.strip_prefix('+').unwrap_or(spec.as_str());
        let Some(colon) = clean.find(':') else {
            continue;
        };
        let (src, dst) = (&clean[..colon], &clean[colon + 1..]);
        if dst.is_empty() {
            continue;
        }
        if src.contains('*') {
            let base = dst.split_once('*').map(|(p, _)| p).unwrap_or(dst);
            locals.push(normalize_fetch_dst(base));
        } else {
            locals.push(normalize_fetch_dst(dst));
        }
    }
    longest_common_ref_prefix(&locals)
}

/// When `remote.<name>.fetch` is unset, Git uses `refs/heads/*:refs/remotes/<name>/*`.
fn default_fetch_refspecs(remote_name: &str) -> Vec<FetchRefspec> {
    vec![FetchRefspec {
        src: "refs/heads/*".to_owned(),
        dst: format!("refs/remotes/{remote_name}/*"),
        force: false,
        negative: false,
    }]
}

/// Collect all fetch refspecs from a config key (may be multi-valued).
fn collect_refspecs(config: &ConfigSet, key: &str) -> Vec<FetchRefspec> {
    let mut result = Vec::new();
    for entry in config.entries() {
        if entry.key == key {
            if let Some(ref val) = entry.value {
                let val = val.trim();
                // Check for negative refspec (^pattern)
                if let Some(pattern) = val.strip_prefix('^') {
                    result.push(FetchRefspec {
                        src: pattern.to_owned(),
                        dst: String::new(),
                        force: false,
                        negative: true,
                    });
                    continue;
                }
                let (force, val) = if let Some(stripped) = val.strip_prefix('+') {
                    (true, stripped)
                } else {
                    (false, val)
                };
                if let Some(colon) = val.find(':') {
                    let dst_raw = val[colon + 1..].trim().to_owned();
                    let mut src = val[..colon].trim().to_owned();
                    if !src.starts_with('^') && !src.contains('*') && !src.starts_with("refs/") {
                        src = format!("refs/heads/{src}");
                    }
                    result.push(FetchRefspec {
                        src,
                        dst: normalize_fetch_refspec_dst(&dst_raw),
                        force,
                        negative: false,
                    });
                }
            }
        }
    }
    result
}

/// Map a remote ref through fetch refspecs, returning the local ref and the index of the first
/// matching positive refspec (config order). Used to order FETCH_HEAD like Git (t5515).
fn map_ref_through_refspecs_ex(
    remote_ref: &str,
    refspecs: &[FetchRefspec],
) -> Option<(String, usize)> {
    for rs in refspecs {
        if rs.negative
            && (match_glob_pattern(&rs.src, remote_ref).is_some() || rs.src == remote_ref)
        {
            return None;
        }
    }
    for (idx, rs) in refspecs.iter().enumerate() {
        if rs.negative {
            continue;
        }
        if let Some(mapped) = match_refspec_pattern(&rs.src, &rs.dst, remote_ref) {
            return Some((mapped, idx));
        }
    }
    None
}

/// Map a remote ref through fetch refspecs.
///
/// For a refspec like `refs/heads/*:refs/remotes/origin/*`, if the remote ref
/// is `refs/heads/main`, the result is `refs/remotes/origin/main`.
/// Returns None if no refspec matches.
fn map_ref_through_refspecs(remote_ref: &str, refspecs: &[FetchRefspec]) -> Option<String> {
    map_ref_through_refspecs_ex(remote_ref, refspecs).map(|(m, _)| m)
}

/// Reverse-map a local ref through configured refspecs to find
/// the remote ref that would normally map to it.
fn reverse_map_refspec(local_ref: &str, refspecs: &[FetchRefspec]) -> Option<String> {
    for rs in refspecs {
        if rs.negative || rs.dst.is_empty() {
            continue;
        }
        // Try to reverse the dst pattern to find what src would produce local_ref
        if let Some(star_pos) = rs.dst.find('*') {
            let prefix = &rs.dst[..star_pos];
            let suffix = &rs.dst[star_pos + 1..];
            if local_ref.starts_with(prefix) && local_ref.ends_with(suffix) {
                let matched = &local_ref[prefix.len()..local_ref.len() - suffix.len()];
                let remote_ref = rs.src.replacen('*', matched, 1);
                return Some(remote_ref);
            }
        } else if rs.dst == local_ref {
            return Some(rs.src.clone());
        }
    }
    None
}

/// Match a single refspec pattern. Both src and dst may contain a single '*'.
fn match_refspec_pattern(src_pattern: &str, dst_pattern: &str, refname: &str) -> Option<String> {
    if let Some(star_pos) = src_pattern.find('*') {
        let prefix = &src_pattern[..star_pos];
        let suffix = &src_pattern[star_pos + 1..];
        if refname.starts_with(prefix) && refname.ends_with(suffix) {
            let matched = &refname[prefix.len()..refname.len() - suffix.len()];
            let result = dst_pattern.replacen('*', matched, 1);
            return Some(result);
        }
    } else if src_pattern == refname {
        // Exact match (no wildcard)
        return Some(dst_pattern.to_owned());
    }
    None
}

/// Match a glob pattern against a ref name, returning the matched wildcard portion.
fn match_glob_pattern<'a>(pattern: &str, refname: &'a str) -> Option<&'a str> {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        if refname.starts_with(prefix)
            && refname.ends_with(suffix)
            && refname.len() >= prefix.len() + suffix.len()
        {
            Some(&refname[prefix.len()..refname.len() - suffix.len()])
        } else {
            None
        }
    } else if pattern == refname {
        Some(refname)
    } else {
        None
    }
}

/// Copy symbolic refs that match a glob pattern from remote to local.
fn copy_symrefs(
    remote_git_dir: &Path,
    local_git_dir: &Path,
    src_pattern: &str,
    dst_pattern: &str,
) -> Result<()> {
    // Walk the remote refs directory for symbolic refs
    let refs_dir = remote_git_dir.join("refs");
    if !refs_dir.is_dir() {
        return Ok(());
    }
    for_each_ref_file(&refs_dir, "refs", &mut |refname, path| {
        if let Some(matched) = match_glob_pattern(src_pattern, &refname) {
            let content = fs::read_to_string(path)?;
            let content = content.trim();
            if let Some(target) = content.strip_prefix("ref: ") {
                // It's a symbolic ref — write it locally
                let local_ref = dst_pattern.replacen('*', matched, 1);
                let local_path =
                    local_git_dir.join(local_ref.replace('/', std::path::MAIN_SEPARATOR_STR));
                if let Some(parent) = local_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&local_path, format!("ref: {target}\n"))?;
            }
        }
        Ok(())
    })?;
    Ok(())
}

fn for_each_ref_file(
    dir: &Path,
    prefix: &str,
    cb: &mut dyn FnMut(String, &Path) -> Result<()>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let refname = format!("{prefix}/{name_str}");
        if entry.file_type()?.is_dir() {
            for_each_ref_file(&entry.path(), &refname, cb)?;
        } else {
            cb(refname, &entry.path())?;
        }
    }
    Ok(())
}

/// Collect all configured remote names.
fn collect_remote_names(config: &ConfigSet) -> Vec<String> {
    let mut names = Vec::new();
    for entry in config.entries() {
        let parts: Vec<&str> = entry.key.splitn(3, '.').collect();
        if parts.len() == 3 && parts[0] == "remote" && parts[2] == "url" {
            let name = parts[1].to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// True when `path` is a regular file starting with the v2 git bundle header.
fn remote_path_is_git_bundle_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if let Ok(mut f) = fs::File::open(path) {
        let mut buf = [0u8; 20];
        if let Ok(n) = std::io::Read::read(&mut f, &mut buf) {
            return buf[..n].starts_with(b"# v2 git bundle");
        }
    }
    false
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

/// Pre-`remote.*` layout: `.git/remotes/<name>` with `URL:` and `Pull:` lines.
struct LegacyRemotesFile {
    url: String,
    pull_lines: Vec<String>,
}

fn read_git_remotes_file(git_dir: &Path, remote_name: &str) -> Option<LegacyRemotesFile> {
    let path = git_dir.join("remotes").join(remote_name);
    let content = fs::read_to_string(path).ok()?;
    let mut url = None;
    let mut pull_lines = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("URL:") {
            url = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("Pull:") {
            pull_lines.push(rest.trim().to_owned());
        }
    }
    let url = url?;
    Some(LegacyRemotesFile { url, pull_lines })
}

/// `.git/branches/<name>` — single line `url` or `url#branch`.
struct GitBranchesRemote {
    url: String,
    default_branch: Option<String>,
}

fn read_git_branches_remote_file(git_dir: &Path, name: &str) -> Option<GitBranchesRemote> {
    let path = git_dir.join("branches").join(name);
    let raw = fs::read_to_string(path).ok()?;
    let line = raw.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let (url_part, branch) = if let Some((u, b)) = line.split_once('#') {
        (u.trim().to_owned(), Some(b.trim().to_owned()))
    } else {
        (line.to_owned(), None)
    };
    if url_part.is_empty() {
        return None;
    }
    Some(GitBranchesRemote {
        url: url_part,
        default_branch: branch,
    })
}

fn parse_legacy_pull_lines(lines: &[String]) -> Result<Vec<FetchRefspec>> {
    let mut out = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (force, rest) = if let Some(s) = line.strip_prefix('+') {
            (true, s.trim())
        } else {
            (false, line)
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let mut src = rest[..colon].trim().to_owned();
        if !src.contains('*') && !src.starts_with("refs/") {
            src = format!("refs/heads/{src}");
        }
        out.push(FetchRefspec {
            src,
            dst: normalize_fetch_refspec_dst(rest[colon + 1..].trim()),
            force,
            negative: false,
        });
    }
    if out.is_empty() {
        bail!("legacy remote file has no Pull: refspecs");
    }
    Ok(out)
}

/// Expand `tag <name>` pairs in fetch arguments into explicit refspecs.
fn expand_fetch_cli_tag_args(specs: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < specs.len() {
        if specs[i] == "tag" {
            let Some(name) = specs.get(i + 1) else {
                bail!("missing tag name after 'tag'");
            };
            out.push(format!("refs/tags/{name}:refs/tags/{name}"));
            i += 2;
        } else {
            out.push(specs[i].clone());
            i += 1;
        }
    }
    Ok(out)
}

fn normalize_fetch_url_display(s: &str) -> String {
    let t = s.trim_end_matches('/');
    if t.is_empty() {
        "/".to_owned()
    } else {
        t.to_owned()
    }
}

fn canonical_repo_path(p: &Path) -> Result<PathBuf> {
    let p = if p.is_symlink() {
        fs::read_link(p).unwrap_or_else(|_| p.to_path_buf())
    } else {
        p.to_path_buf()
    };
    p.canonicalize().or_else(|_| Ok(p))
}

/// Display URL for FETCH_HEAD (`../` relative to the local repo root when fetching by path).
fn resolve_fetch_display_url(
    git_dir: &Path,
    raw_url: &str,
    url_override: Option<&str>,
    remote_repo: Option<&Repository>,
) -> Result<String> {
    let base = configured_remote_base(git_dir);
    if let Some(remote_repo) = remote_repo {
        if !crate::ssh_transport::is_configured_ssh_url(raw_url) {
            if let Ok(canon_remote) = canonical_repo_path(&remote_repo.git_dir) {
                let mut sb = String::new();
                let base_s = base.to_string_lossy();
                let remote_s = canon_remote.to_string_lossy();
                if let Some(rel) = crate::git_path::relative_path(&remote_s, &base_s, &mut sb) {
                    let mut s = normalize_fetch_url_display(rel);
                    if let Some(prefix) = s.strip_suffix("/.git") {
                        s = normalize_fetch_url_display(prefix);
                    }
                    if s == ".." {
                        return Ok("../".to_owned());
                    }
                    return Ok(s);
                }
            }
        }
    }
    if url_override.is_some() {
        let trimmed = raw_url.trim_end_matches('/');
        let u = Path::new(trimmed);
        if u.is_relative() {
            let joined = base.join(u);
            if let Ok(canon) = canonical_repo_path(&joined) {
                let mut sb = String::new();
                let base_s = base.to_string_lossy();
                let canon_s = canon.to_string_lossy();
                if let Some(rel) = crate::git_path::relative_path(&canon_s, &base_s, &mut sb) {
                    let mut s = normalize_fetch_url_display(rel);
                    if let Some(prefix) = s.strip_suffix("/.git") {
                        s = normalize_fetch_url_display(prefix);
                    }
                    return Ok(s);
                }
            }
        }
        let mut s = normalize_fetch_url_display(raw_url);
        if let Some(prefix) = s.strip_suffix("/.git") {
            s = normalize_fetch_url_display(prefix);
        }
        return Ok(s);
    }
    Ok(normalize_fetch_url_display(raw_url))
}

fn remotes_match_same_repository(git_dir: &Path, remote_repo: &Repository, url_str: &str) -> bool {
    let mut candidate = if let Some(s) = url_str.strip_prefix("file://") {
        PathBuf::from(s)
    } else {
        PathBuf::from(url_str)
    };
    if candidate.is_relative() {
        candidate = configured_remote_base(git_dir).join(candidate);
    }
    let Ok(can_a) = canonical_repo_path(&candidate) else {
        return false;
    };
    let Ok(can_b) = canonical_repo_path(&remote_repo.git_dir) else {
        return false;
    };
    can_a == can_b
}

/// If the fetch URL points at the same repository as a configured remote, return that name.
fn find_remote_for_repository_url(
    config: &ConfigSet,
    git_dir: &Path,
    remote_repo: &Repository,
) -> Option<String> {
    let mut names = collect_remote_names(config);
    names.sort();
    for name in names {
        let key = format!("remote.{name}.url");
        let Some(u) = config.get(&key) else {
            continue;
        };
        if remotes_match_same_repository(git_dir, remote_repo, &u) {
            return Some(name);
        }
    }
    None
}

/// All configured remotes whose `remote.<name>.url` resolves to the same repository as `remote_repo`
/// (Git fetches every matching refspec in one object transfer; t5515 clone + extra remotes).
fn remotes_sharing_repository_url(
    config: &ConfigSet,
    git_dir: &Path,
    remote_repo: &Repository,
    primary: &str,
) -> Vec<String> {
    let mut out: Vec<String> = collect_remote_names(config)
        .into_iter()
        .filter(|n| {
            config
                .get(&format!("remote.{n}.url"))
                .map(|u| remotes_match_same_repository(git_dir, remote_repo, &u))
                .unwrap_or(false)
        })
        .collect();
    if !out.iter().any(|n| n == primary) {
        out.push(primary.to_owned());
    }
    out.sort();
    out.dedup();
    out
}

/// Local branches that list `remote_name` as their upstream, mapped to remote branch short names
/// from `branch.<name>.merge` (e.g. `refs/heads/three` → `three`).
fn branch_merge_remote_specs(
    config: &ConfigSet,
    remote_name: &str,
) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for entry in config.entries() {
        let key = entry.key.as_str();
        let Some(rest) = key.strip_prefix("branch.") else {
            continue;
        };
        let Some(local_branch) = rest.strip_suffix(".merge") else {
            continue;
        };
        if local_branch.is_empty() {
            continue;
        }
        let rkey = format!("branch.{local_branch}.remote");
        if config.get(&rkey).as_deref() != Some(remote_name) {
            continue;
        }
        let Some(m) = entry.value.as_deref() else {
            continue;
        };
        let rb = m.strip_prefix("refs/heads/").unwrap_or(m).to_owned();
        out.entry(local_branch.to_owned()).or_default().push(rb);
    }
    out
}

fn head_short_branch(git_dir: &Path) -> Option<String> {
    match resolve_head(git_dir).ok()? {
        HeadState::Branch { short_name, .. } => Some(short_name),
        _ => None,
    }
}

/// Rank for FETCH_HEAD ordering: lower = for-merge (printed first after sort).
fn fetch_head_branch_line(
    oid: &ObjectId,
    branch: &str,
    display_url: &str,
    for_merge: bool,
) -> String {
    let sep = if for_merge {
        "\t\t"
    } else {
        "\tnot-for-merge\t"
    };
    format!("{oid}{sep}branch '{branch}' of {display_url}")
}

fn fetch_head_tag_line(
    oid: &ObjectId,
    tag_name: &str,
    display_url: &str,
    for_merge: bool,
) -> String {
    let sep = if for_merge {
        "\t\t"
    } else {
        "\tnot-for-merge\t"
    };
    format!("{oid}{sep}tag '{tag_name}' of {display_url}")
}

fn fetch_head_bare_url_line(oid: &ObjectId, display_url: &str) -> String {
    format!("{oid}\t\t{display_url}")
}

fn fetch_head_line_has_not_for_merge(line: &str) -> bool {
    line.contains("not-for-merge")
}

fn fetch_head_parse_name(line: &str) -> Option<(bool, String)> {
    if let Some(idx) = line.find("branch '") {
        let rest = &line[idx + "branch '".len()..];
        let end = rest.find('\'')?;
        return Some((false, rest[..end].to_owned()));
    }
    if let Some(idx) = line.find("tag '") {
        let rest = &line[idx + "tag '".len()..];
        let end = rest.find('\'')?;
        return Some((true, rest[..end].to_owned()));
    }
    None
}

fn branch_refspec_index(branch: &str, refspecs: &[FetchRefspec]) -> usize {
    if refspecs.is_empty() {
        return 0;
    }
    let remote_ref = format!("refs/heads/{branch}");
    for (i, rs) in refspecs.iter().enumerate() {
        if rs.negative {
            continue;
        }
        if match_refspec_pattern(&rs.src, &rs.dst, &remote_ref).is_some() {
            return i;
        }
    }
    usize::MAX
}

/// Order FETCH_HEAD lines like Git (t5515): for-merge before not-for-merge; branches before tags;
/// then configured refspec order, then name.
fn sort_fetch_head_lines(lines: &mut [String], refspecs: &[FetchRefspec]) {
    lines.sort_by(|a, b| {
        let a_n = fetch_head_line_has_not_for_merge(a);
        let b_n = fetch_head_line_has_not_for_merge(b);
        a_n.cmp(&b_n).then_with(|| {
            let pa = fetch_head_parse_name(a);
            let pb = fetch_head_parse_name(b);
            match (pa, pb) {
                (Some((ta, na)), Some((tb, nb))) => {
                    let sa = if ta { 1 } else { 0 };
                    let sb = if tb { 1 } else { 0 };
                    sa.cmp(&sb).then_with(|| {
                        if !ta && !tb {
                            let ia = branch_refspec_index(&na, refspecs);
                            let ib = branch_refspec_index(&nb, refspecs);
                            ia.cmp(&ib).then_with(|| na.cmp(&nb))
                        } else {
                            na.cmp(&nb)
                        }
                    })
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.cmp(b),
            }
        })
    });
}

fn fetch_branch_merge_rank(
    remote_branch: &str,
    head_short: Option<&str>,
    merge_map: &HashMap<String, Vec<String>>,
) -> u32 {
    let Some(h) = head_short else {
        return 999;
    };
    let Some(sources) = merge_map.get(h) else {
        return 999;
    };
    if sources.len() >= 2 {
        if sources.iter().any(|b| b == remote_branch) {
            return 0;
        }
        return 999;
    }
    if sources.len() == 1 && sources[0] == remote_branch {
        return 0;
    }
    999
}

fn configured_remote_base(git_dir: &Path) -> PathBuf {
    if git_dir.file_name().is_some_and(|name| name == ".git") {
        git_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| git_dir.to_path_buf())
    } else {
        git_dir.to_path_buf()
    }
}

/// Resolve the git directory from CWD.
fn resolve_git_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("GIT_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let mut cur = cwd.as_path();
    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() {
            return Ok(dot_git);
        }
        if dot_git.is_file() {
            if let Ok(content) = fs::read_to_string(&dot_git) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("gitdir:") {
                        let path = rest.trim();
                        let resolved = if Path::new(path).is_absolute() {
                            PathBuf::from(path)
                        } else {
                            cur.join(path)
                        };
                        return Ok(resolved);
                    }
                }
            }
        }
        // Check if this is a bare repo
        if cur.join("objects").is_dir() && cur.join("HEAD").is_file() {
            return Ok(cur.to_path_buf());
        }
        cur = match cur.parent() {
            Some(p) => p,
            None => bail!("not a git repository (or any of the parent directories): .git"),
        };
    }
}

/// Check if a branch ref is checked out in any worktree, return the worktree path.
fn is_branch_in_worktree(git_dir: &std::path::Path, branch_ref: &str) -> Option<String> {
    let common = grit_lib::refs::common_dir(git_dir).unwrap_or_else(|| git_dir.to_path_buf());
    // Check main worktree
    if let Ok(head) = grit_lib::state::resolve_head(&common) {
        if let grit_lib::state::HeadState::Branch { ref refname, .. } = head {
            if refname == branch_ref {
                return Some(common.parent().unwrap_or(&common).display().to_string());
            }
        }
    }
    // Check linked worktrees
    let wt_dir = common.join("worktrees");
    if wt_dir.is_dir() {
        for entry in std::fs::read_dir(&wt_dir).into_iter().flatten().flatten() {
            let admin = entry.path();
            if !admin.is_dir() {
                continue;
            }
            let head_file = admin.join("HEAD");
            if let Ok(content) = std::fs::read_to_string(&head_file) {
                if let Some(refname) = content.trim().strip_prefix("ref: ") {
                    if refname.trim() == branch_ref {
                        let gitdir_file = admin.join("gitdir");
                        let path = if let Ok(raw) = std::fs::read_to_string(&gitdir_file) {
                            let p = std::path::Path::new(raw.trim());
                            p.parent().unwrap_or(p).display().to_string()
                        } else {
                            entry.file_name().to_string_lossy().to_string()
                        };
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}
