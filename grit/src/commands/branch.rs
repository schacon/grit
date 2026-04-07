//! `grit branch` — list, create, or delete branches.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{resolve_revision, symbolic_full_name};
use grit_lib::state::{resolve_head, HeadState};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Arguments for `grit branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "List, create, or delete branches")]
pub struct Args {
    /// Branch name to create (or pattern to list).
    #[arg()]
    pub name: Option<String>,

    /// Start point for new branch (commit, branch, or tag).
    #[arg()]
    pub start_point: Option<String>,

    /// Delete a branch.
    #[arg(short = 'd', long = "delete")]
    pub delete: bool,

    /// Force delete a branch (even if not merged).
    #[arg(short = 'D')]
    pub force_delete: bool,

    /// Move/rename a branch.
    #[arg(short = 'm', long = "move")]
    pub rename: bool,

    /// Force move/rename.
    #[arg(short = 'M')]
    pub force_rename: bool,

    /// Copy a branch.
    #[arg(short = 'c', long = "copy")]
    pub copy: bool,

    /// List branches (default when no name given).
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// List remote-tracking branches.
    #[arg(short = 'r', long = "remotes")]
    pub remotes: bool,

    /// List both local and remote branches.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Show verbose info (commit subject). Use twice (-vv) for tracking info.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Show branches containing this commit.
    #[arg(long = "contains")]
    pub contains: Option<String>,

    /// Show branches not containing this commit.
    #[arg(long = "no-contains")]
    pub no_contains: Option<String>,

    /// Show branches merged into this commit (default: HEAD).
    #[arg(long = "merged", num_args = 0..=1, default_missing_value = "")]
    pub merged: Option<String>,

    /// Show branches not merged into this commit (default: HEAD).
    #[arg(long = "no-merged", num_args = 0..=1, default_missing_value = "")]
    pub no_merged: Option<String>,

    /// Force creation (overwrite existing branch).
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Set up tracking.
    #[arg(short = 't', long = "track", require_equals = true, num_args = 0..=1, default_missing_value = "direct")]
    pub track: Option<String>,

    /// Do not set up tracking.
    #[arg(long = "no-track")]
    pub no_track: bool,

    /// Show the current branch name.
    #[arg(long = "show-current")]
    pub show_current: bool,

    /// Set upstream tracking branch (e.g. origin/main).
    #[arg(short = 'u', long = "set-upstream-to")]
    pub set_upstream_to: Option<String>,

    /// Remove upstream tracking configuration.
    #[arg(long = "unset-upstream")]
    pub unset_upstream: bool,

    /// Sort branches by key: refname, committerdate, -committerdate.
    #[arg(long = "sort")]
    pub sort: Option<String>,

    /// Cancel sort keys (reset to default).
    #[arg(long = "no-sort")]
    pub no_sort: bool,

    /// Custom format string (for-each-ref style atoms).
    #[arg(long = "format")]
    pub format: Option<String>,

    /// Create the branch's reflog.
    #[arg(long = "create-reflog")]
    pub create_reflog: bool,

    /// Force copy.
    #[arg(short = 'C')]
    pub force_copy: bool,

    /// Display branches in columns.
    #[arg(long = "column", value_name = "STYLE", num_args = 0..=1, default_missing_value = "always")]
    pub column: Option<String>,

    /// Disable columnar output.
    #[arg(long = "no-column")]
    pub no_column: bool,

    /// Abbreviation length for object names.
    #[arg(long = "abbrev", value_name = "N", num_args = 0..=1, default_missing_value = "7")]
    pub abbrev: Option<String>,

    /// Don't abbreviate.
    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    /// Show branches that point at a given object.
    #[arg(long = "points-at")]
    pub points_at: Option<String>,

    /// Editing mode.
    #[arg(long = "edit-description")]
    pub edit_description: bool,

    /// Use color in output.
    #[arg(long = "color", value_name = "WHEN", num_args = 0..=1, default_missing_value = "always")]
    pub color: Option<String>,

    /// Disable color output.
    #[arg(long = "no-color")]
    pub no_color: bool,
}

/// Run the `branch` command.
pub fn run(args: Args) -> Result<()> {
    // Note: previously delegated to system git, now handled natively.

    let repo = Repository::discover(None).context("not a git repository")?;
    let head = resolve_head(&repo.git_dir)?;

    // Validate mutually exclusive mode options
    {
        let mut modes = Vec::new();
        if args.delete || args.force_delete {
            modes.push("delete");
        }
        if args.rename || args.force_rename {
            modes.push("rename");
        }
        if args.copy || args.force_copy {
            modes.push("copy");
        }
        if args.set_upstream_to.is_some() {
            modes.push("set-upstream-to");
        }
        if args.unset_upstream {
            modes.push("unset-upstream");
        }
        if args.show_current {
            modes.push("show-current");
        }
        // --list conflicts with delete/rename/copy but not with filtering
        if args.list && !modes.is_empty() {
            bail!("options are incompatible");
        }
        if modes.len() > 1 {
            bail!("options are incompatible");
        }
    }

    if args.show_current {
        if let Some(name) = head.branch_name() {
            println!("{name}");
        }
        return Ok(());
    }

    if args.set_upstream_to.is_some() {
        return set_upstream(&repo, &head, &args);
    }

    if args.unset_upstream {
        return unset_upstream(&repo, &head, &args);
    }

    if args.delete || args.force_delete {
        return delete_branch(&repo, &head, &args);
    }

    if args.rename || args.force_rename {
        return rename_branch(&repo, &head, &args);
    }

    if args.copy || args.force_copy {
        return copy_branch(&repo, &head, &args);
    }

    // If a name is given and we're not listing/filtering, create a branch
    if let Some(ref name) = args.name {
        if !args.list
            && args.contains.is_none()
            && args.no_contains.is_none()
            && args.merged.is_none()
            && args.no_merged.is_none()
        {
            // Reject invalid branch names
            if name == "HEAD" || name.starts_with('-') {
                bail!("'{name}' is not a valid branch name");
            }
            return create_branch(&repo, &head, name, args.start_point.as_deref(), &args);
        }
    }

    // Default: list branches
    list_branches(&repo, &head, &args)
}

fn should_passthrough_to_system_git(args: &Args) -> bool {
    args.force || args.delete || args.force_delete || args.rename || args.force_rename
}

fn passthrough_current_branch_invocation() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let Some(idx) = argv.iter().position(|arg| arg == "branch") else {
        bail!("failed to determine branch arguments");
    };
    let passthrough_args = argv.get(idx + 1..).map(|s| s.to_vec()).unwrap_or_default();
    crate::commands::git_passthrough::run("branch", &passthrough_args)
}

/// Info about a branch for listing.
struct BranchInfo {
    name: String,
    oid: ObjectId,
    is_remote: bool,
}

/// List branches.
fn list_branches(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let current_branch = head.branch_name().unwrap_or("");

    // Collect branches
    let mut branches: Vec<BranchInfo> = Vec::new();

    if !args.remotes {
        let local = if grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
            grit_lib::reftable::reftable_list_refs(&repo.git_dir, "refs/heads/")
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .into_iter()
                .map(|(name, oid)| {
                    let short = name.strip_prefix("refs/heads/").unwrap_or(&name).to_owned();
                    (short, oid)
                })
                .collect()
        } else {
            let mut v = Vec::new();
            collect_branches(&repo.git_dir.join("refs/heads"), "", &mut v)?;
            v
        };
        for (name, oid) in local {
            branches.push(BranchInfo {
                name,
                oid,
                is_remote: false,
            });
        }
    }

    if args.remotes || args.all {
        let remote = if grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
            grit_lib::reftable::reftable_list_refs(&repo.git_dir, "refs/remotes/")
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .into_iter()
                .map(|(name, oid)| {
                    let short = name
                        .strip_prefix("refs/remotes/")
                        .unwrap_or(&name)
                        .to_owned();
                    (short, oid)
                })
                .collect()
        } else {
            let mut v = Vec::new();
            collect_branches(&repo.git_dir.join("refs/remotes"), "", &mut v)?;
            v
        };
        for (name, oid) in remote {
            branches.push(BranchInfo {
                name: if args.remotes && !args.all {
                    name
                } else {
                    format!("remotes/{name}")
                },
                oid,
                is_remote: true,
            });
        }
    }

    // Apply --merged filter
    if let Some(ref merged_val) = args.merged {
        let target_oid = if merged_val.is_empty() {
            *head
                .oid()
                .ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?
        } else {
            resolve_revision(repo, merged_val)?
        };
        branches.retain(|b| is_ancestor(repo, b.oid, target_oid).unwrap_or(false));
    }

    // Apply --no-merged filter
    if let Some(ref no_merged_val) = args.no_merged {
        let target_oid = if no_merged_val.is_empty() {
            *head
                .oid()
                .ok_or_else(|| anyhow::anyhow!("HEAD does not point to a valid commit"))?
        } else {
            resolve_revision(repo, no_merged_val)?
        };
        branches.retain(|b| !is_ancestor(repo, b.oid, target_oid).unwrap_or(true));
    }

    // Apply --contains filter
    if let Some(ref contains_rev) = args.contains {
        let contains_oid = resolve_revision(repo, contains_rev)?;
        branches.retain(|b| is_ancestor(repo, contains_oid, b.oid).unwrap_or(false));
    }

    // Apply --no-contains filter
    if let Some(ref no_contains_rev) = args.no_contains {
        let no_contains_oid = resolve_revision(repo, no_contains_rev)?;
        branches.retain(|b| !is_ancestor(repo, no_contains_oid, b.oid).unwrap_or(true));
    }

    // Apply pattern filter (branch --list <pattern>)
    if let Some(ref pattern) = args.name {
        branches.retain(|b| glob_match(pattern, &b.name));
    }

    // Sort branches
    // Determine sort key: --no-sort resets config, --sort overrides all
    let config_sort = if args.sort.is_none() && !args.no_sort {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).ok();
        cfg.and_then(|c| c.get("branch.sort"))
    } else {
        None
    };
    let sort_key_owned = args.sort.clone().or(config_sort);
    sort_branches(repo, &mut branches, sort_key_owned.as_deref())?;

    // Custom format
    if let Some(ref fmt) = args.format {
        for b in &branches {
            let line = format_branch(repo, head, b, fmt)?;
            writeln!(out, "{line}")?;
        }
        return Ok(());
    }

    let use_color = if args.no_color {
        false
    } else if let Some(ref when) = args.color {
        when != "never"
    } else {
        false
    };
    let (color_current, color_local, color_remote, color_reset) = if use_color {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).ok();
        let get_color = |key: &str, default: &str| -> String {
            let val = cfg.as_ref().and_then(|c| c.get(key));
            let cs = val.as_deref().unwrap_or(default);
            grit_lib::config::parse_color(cs).unwrap_or_else(|_| String::new())
        };
        (
            get_color("color.branch.current", "green"),
            get_color("color.branch.local", "normal"),
            get_color("color.branch.remote", "red"),
            "[m".to_string(),
        )
    } else {
        (String::new(), String::new(), String::new(), String::new())
    };
    let max_name_len = if args.verbose > 0 {
        branches.iter().map(|b| b.name.len()).max().unwrap_or(0)
    } else {
        0
    };

    for b in &branches {
        let is_current = !b.is_remote && b.name == current_branch;
        let prefix = if is_current { "* " } else { "  " };
        let color = if use_color {
            if is_current {
                &color_current
            } else if b.is_remote {
                &color_remote
            } else {
                &color_local
            }
        } else {
            &color_local
        };
        let reset = if use_color {
            &color_reset
        } else {
            &color_local
        };

        if args.verbose > 0 {
            let short_oid = &b.oid.to_hex()[..7];
            let subject = commit_subject(&repo.odb, &b.oid).unwrap_or_default();
            let padded_name = format!("{:<width$}", b.name, width = max_name_len);

            if args.verbose >= 2 && !b.is_remote {
                // -vv: show tracking info
                let tracking = get_tracking_info(repo, &b.name)?;
                if let Some(ref track_str) = tracking {
                    writeln!(
                        out,
                        "{prefix}{color}{padded_name}{reset} {short_oid} [{track_str}] {subject}"
                    )?;
                } else {
                    writeln!(
                        out,
                        "{prefix}{color}{padded_name}{reset} {short_oid} {subject}"
                    )?;
                }
            } else {
                writeln!(
                    out,
                    "{prefix}{color}{padded_name}{reset} {short_oid} {subject}"
                )?;
            }
        } else {
            writeln!(out, "{prefix}{color}{}{reset}", b.name)?;
        }
    }

    Ok(())
}

/// Sort branches by the given key.
fn sort_branches(
    repo: &Repository,
    branches: &mut [BranchInfo],
    sort_key: Option<&str>,
) -> Result<()> {
    match sort_key {
        None | Some("refname") => {
            branches.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Some("-refname") => {
            branches.sort_by(|a, b| b.name.cmp(&a.name));
        }
        Some("committerdate") => {
            branches.sort_by(|a, b| {
                let ta = committer_time(&repo.odb, &a.oid, &a.name, a.is_remote);
                let tb = committer_time(&repo.odb, &b.oid, &b.name, b.is_remote);
                ta.cmp(&tb)
            });
        }
        Some("-committerdate") => {
            branches.sort_by(|a, b| {
                let ta = committer_time(&repo.odb, &a.oid, &a.name, a.is_remote);
                let tb = committer_time(&repo.odb, &b.oid, &b.name, b.is_remote);
                tb.cmp(&ta)
            });
        }
        Some("authordate") => {
            branches.sort_by(|a, b| {
                let ta = author_time(&repo.odb, &a.oid, &a.name, a.is_remote);
                let tb = author_time(&repo.odb, &b.oid, &b.name, b.is_remote);
                ta.cmp(&tb)
            });
        }
        Some("-authordate") => {
            branches.sort_by(|a, b| {
                let ta = author_time(&repo.odb, &a.oid, &a.name, a.is_remote);
                let tb = author_time(&repo.odb, &b.oid, &b.name, b.is_remote);
                tb.cmp(&ta)
            });
        }
        Some("objecttype") | Some("-objecttype") => {
            // All branches are commit objects, so objecttype is the same for all.
            // Fall back to default (refname) sort as secondary.
            branches.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Some(other) => {
            bail!("unsupported sort key: '{other}'");
        }
    }
    Ok(())
}

/// Get the tracking info string for a local branch, e.g. "origin/main: ahead 1, behind 2".
fn get_tracking_info(repo: &Repository, branch_name: &str) -> Result<Option<String>> {
    let config_path = repo.git_dir.join("config");
    let config_file = match ConfigFile::from_path(&config_path, ConfigScope::Local)? {
        Some(c) => c,
        None => return Ok(None),
    };
    let mut config = ConfigSet::new();
    config.merge(&config_file);

    let merge_key = format!("branch.{branch_name}.merge");
    let remote_key = format!("branch.{branch_name}.remote");

    let merge = match config.get(&merge_key) {
        Some(m) => m,
        None => return Ok(None),
    };
    let remote = config
        .get(&remote_key)
        .unwrap_or_else(|| "origin".to_string());

    // Strip refs/heads/ prefix from merge to get the upstream branch name
    let upstream_branch = merge.strip_prefix("refs/heads/").unwrap_or(&merge);

    let upstream_display = format!("{remote}/{upstream_branch}");

    // Try to compute ahead/behind
    let upstream_ref_path = repo
        .git_dir
        .join("refs/remotes")
        .join(&remote)
        .join(upstream_branch);

    if let Ok(content) = fs::read_to_string(&upstream_ref_path) {
        if let Ok(upstream_oid) = ObjectId::from_hex(content.trim()) {
            // Read local branch OID
            let local_ref_path = repo.git_dir.join("refs/heads").join(branch_name);
            if let Ok(local_content) = fs::read_to_string(&local_ref_path) {
                if let Ok(local_oid) = ObjectId::from_hex(local_content.trim()) {
                    let (ahead, behind) = count_ahead_behind(repo, local_oid, upstream_oid)?;
                    if ahead == 0 && behind == 0 {
                        return Ok(Some(upstream_display));
                    }
                    let mut parts = Vec::new();
                    if ahead > 0 {
                        parts.push(format!("ahead {ahead}"));
                    }
                    if behind > 0 {
                        parts.push(format!("behind {behind}"));
                    }
                    return Ok(Some(format!("{upstream_display}: {}", parts.join(", "))));
                }
            }
        }
    }

    // Upstream ref doesn't exist locally — just show the name with "gone"
    Ok(Some(format!("{upstream_display}: gone")))
}

/// Count how many commits local is ahead of and behind upstream.
fn count_ahead_behind(
    repo: &Repository,
    local: ObjectId,
    upstream: ObjectId,
) -> Result<(usize, usize)> {
    if local == upstream {
        return Ok((0, 0));
    }

    let local_ancestors = collect_ancestors(repo, local)?;
    let upstream_ancestors = collect_ancestors(repo, upstream)?;

    let mut ahead = 0usize;
    let mut behind = 0usize;

    for oid in &local_ancestors {
        if !upstream_ancestors.contains(oid) {
            ahead += 1;
        }
    }
    for oid in &upstream_ancestors {
        if !local_ancestors.contains(oid) {
            behind += 1;
        }
    }

    Ok((ahead, behind))
}

/// Collect all ancestor OIDs of a commit (including itself).
fn collect_ancestors(
    repo: &Repository,
    start: ObjectId,
) -> Result<std::collections::HashSet<ObjectId>> {
    use std::collections::HashSet;
    let mut visited = HashSet::new();
    let mut queue = vec![start];

    while let Some(oid) = queue.pop() {
        if !visited.insert(oid) {
            continue;
        }
        if let Ok(obj) = repo.odb.read(&oid) {
            if let Ok(commit) = parse_commit(&obj.data) {
                for parent in &commit.parents {
                    if !visited.contains(parent) {
                        queue.push(*parent);
                    }
                }
            }
        }
    }

    Ok(visited)
}

/// Set upstream tracking branch.
fn set_upstream(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let upstream = args
        .set_upstream_to
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("upstream name required"))?;

    let branch_name = match args.name.as_deref() {
        Some(n) => n.to_owned(),
        None => head
            .branch_name()
            .ok_or_else(|| anyhow::anyhow!("no current branch; specify branch name"))?
            .to_owned(),
    };

    // Parse upstream as remote/branch
    let (remote, upstream_branch) = parse_upstream(repo, upstream)?;

    let config_path = repo.git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let mut config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;

    let remote_key = format!("branch.{branch_name}.remote");
    let merge_key = format!("branch.{branch_name}.merge");

    config.set(&remote_key, &remote)?;
    config.set(&merge_key, &format!("refs/heads/{upstream_branch}"))?;
    config.write()?;

    if !args.quiet {
        eprintln!("branch '{branch_name}' set up to track '{remote}/{upstream_branch}'.");
    }

    Ok(())
}

/// Remove upstream tracking configuration.
fn unset_upstream(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let branch_name = match args.name.as_deref() {
        Some(n) => n.to_owned(),
        None => head
            .branch_name()
            .ok_or_else(|| anyhow::anyhow!("no current branch; specify branch name"))?
            .to_owned(),
    };

    let config_path = repo.git_dir.join("config");
    let content = fs::read_to_string(&config_path).unwrap_or_default();
    let mut config = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;

    let merge_key = format!("branch.{branch_name}.merge");

    // Check if there's actually tracking info — use ConfigSet to read
    let mut cs = ConfigSet::new();
    cs.merge(&config);
    if cs.get(&merge_key).is_none() {
        bail!("branch '{branch_name}' has no upstream configuration");
    }

    let remote_key = format!("branch.{branch_name}.remote");
    let _ = config.unset(&remote_key);
    let _ = config.unset(&merge_key);
    config.write()?;

    if !args.quiet {
        eprintln!("branch '{branch_name}' upstream information removed.");
    }

    Ok(())
}

/// Parse an upstream spec like "origin/main" into (remote, branch).
fn parse_upstream(repo: &Repository, upstream: &str) -> Result<(String, String)> {
    // Try to find a matching remote
    let remotes_dir = repo.git_dir.join("refs/remotes");
    if let Ok(entries) = fs::read_dir(&remotes_dir) {
        for entry in entries.flatten() {
            let remote_name = entry.file_name().to_string_lossy().to_string();
            if let Some(branch) = upstream.strip_prefix(&format!("{remote_name}/")) {
                if !branch.is_empty() {
                    return Ok((remote_name, branch.to_string()));
                }
            }
        }
    }

    // Check if upstream is a local branch — use "." as the remote.
    let local_ref = repo.git_dir.join("refs/heads").join(upstream);
    if local_ref.exists() {
        return Ok((".".to_string(), upstream.to_string()));
    }

    // Fallback: split on first /
    if let Some(idx) = upstream.find('/') {
        let remote = &upstream[..idx];
        let branch = &upstream[idx + 1..];
        if !branch.is_empty() {
            return Ok((remote.to_string(), branch.to_string()));
        }
    }

    bail!("cannot parse upstream '{upstream}' — expected format: remote/branch");
}

/// Format a branch using for-each-ref style format atoms.
fn format_branch(
    repo: &Repository,
    head: &HeadState,
    branch: &BranchInfo,
    fmt: &str,
) -> Result<String> {
    let mut result = fmt.to_string();

    // %(refname) — full refname
    let full_refname = if branch.is_remote {
        format!("refs/remotes/{}", branch.name)
    } else {
        format!("refs/heads/{}", branch.name)
    };

    // %(refname:short)
    result = result.replace("%(refname:short)", &branch.name);
    // %(refname) after short to avoid double-replace
    result = result.replace("%(refname)", &full_refname);

    // %(objectname:short) before %(objectname)
    let hex = branch.oid.to_hex();
    let short_oid = &hex[..7];
    result = result.replace("%(objectname:short)", short_oid);
    result = result.replace("%(objectname)", &hex);

    // %(HEAD) — * if current
    let current_branch = head.branch_name().unwrap_or("");
    let is_current = !branch.is_remote && branch.name == current_branch;
    result = result.replace("%(HEAD)", if is_current { "*" } else { " " });

    // %(upstream:short) before %(upstream)
    if result.contains("%(upstream") {
        let tracking = get_tracking_info(repo, &branch.name)?;
        let upstream_name = if let Some(ref t) = tracking {
            t.split(':').next().unwrap_or(t).trim().to_string()
        } else {
            String::new()
        };
        result = result.replace("%(upstream:short)", &upstream_name);
        result = result.replace(
            "%(upstream)",
            &if upstream_name.is_empty() {
                String::new()
            } else {
                format!("refs/remotes/{upstream_name}")
            },
        );
    }

    // %(subject) — commit message first line
    let subject = commit_subject(&repo.odb, &branch.oid).unwrap_or_default();
    result = result.replace("%(subject)", &subject);

    // %(committerdate) and %(authordate) — raw signature strings
    if result.contains("%(committerdate)") || result.contains("%(authordate)") {
        if let Ok(obj) = repo.odb.read(&branch.oid) {
            if let Ok(commit) = parse_commit(&obj.data) {
                result = result.replace("%(committerdate)", &commit.committer);
                result = result.replace("%(authordate)", &commit.author);
            }
        }
    }

    Ok(result)
}

/// Create a new branch.
fn create_branch(
    repo: &Repository,
    head: &HeadState,
    name: &str,
    start_point: Option<&str>,
    args: &Args,
) -> Result<()> {
    let refname = format!("refs/heads/{name}");
    let exists = grit_lib::refs::resolve_ref(&repo.git_dir, &refname).is_ok();

    if exists && !args.force {
        bail!("A branch named '{name}' already exists.");
    }

    // Cannot force-update the current branch
    if args.force {
        let current = head.branch_name().unwrap_or("");
        if name == current {
            let wt_path = repo
                .work_tree
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| repo.git_dir.display().to_string());
            bail!(
                "cannot force update the branch '{}' used by worktree at '{}'",
                name,
                wt_path
            );
        }
        if let Some(wt_path) = branch_used_by_other_worktree(repo, name)? {
            bail!(
                "cannot force update the branch '{}' used by worktree at '{}'",
                name,
                wt_path
            );
        }
    }

    let oid = match start_point {
        Some(rev) => resolve_revision(repo, rev)?,
        None => *head
            .oid()
            .ok_or_else(|| anyhow::anyhow!("not a valid object name: 'HEAD'"))?,
    };

    grit_lib::refs::write_ref(&repo.git_dir, &refname, &oid).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Create reflog when explicitly requested or when core.logAllRefUpdates
    // enables branch reflogs for this repository.
    if args.create_reflog || should_log_ref_updates(repo) {
        let reflog_path = repo.git_dir.join("logs").join(&refname);
        if let Some(parent) = reflog_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let ident = get_reflog_identity();
        let zero = "0000000000000000000000000000000000000000";
        // Use the branch name of start_point, or the current branch name
        let from = match start_point {
            Some(sp) => sp.to_string(),
            None => head.branch_name().unwrap_or("HEAD").to_string(),
        };
        let entry = format!("{zero} {oid} {ident}\tbranch: Created from {from}\n");
        let _ = fs::write(&reflog_path, entry);
    }

    // Set up tracking if --track was used or start_point is a remote tracking branch
    if args.track.is_some() || (!args.no_track && start_point.is_some()) {
        if let Some(sp) = start_point {
            // Try to parse as remote tracking branch: origin/branch or refs/remotes/origin/branch
            let remote_ref = if sp.starts_with("refs/remotes/") {
                Some(sp.to_string())
            } else if grit_lib::refs::resolve_ref(&repo.git_dir, &format!("refs/remotes/{sp}"))
                .is_ok()
            {
                Some(format!("refs/remotes/{sp}"))
            } else {
                None
            };
            if let Some(rref) = remote_ref {
                // Parse remote/branch from refs/remotes/remote/branch
                let stripped = rref.strip_prefix("refs/remotes/").unwrap_or(&rref);
                if let Some(slash) = stripped.find('/') {
                    let remote = &stripped[..slash];
                    let branch = &stripped[slash + 1..];
                    let config_path = repo.git_dir.join("config");
                    let mut cfg = std::fs::read_to_string(&config_path).unwrap_or_default();
                    cfg.push_str(&format!("\n[branch \"{}\"]", name));
                    cfg.push_str(&format!("\n\tremote = {}", remote));
                    cfg.push_str(&format!("\n\tmerge = refs/heads/{}\n", branch));
                    std::fs::write(&config_path, cfg)?;
                }
            }
        }
    }

    Ok(())
}

fn should_log_ref_updates(repo: &Repository) -> bool {
    ConfigSet::load(Some(&repo.git_dir), true)
        .ok()
        .and_then(|cfg| cfg.get("core.logallrefupdates"))
        .map(|v| {
            let lowered = v.trim().to_ascii_lowercase();
            lowered == "true" || lowered == "always"
        })
        .unwrap_or(false)
}

/// Delete a branch.
fn delete_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let name_input = args
        .name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("branch name required"))?;
    let resolved_ref =
        symbolic_full_name(repo, name_input).filter(|full| full.starts_with("refs/heads/"));
    let (name, refname) = if let Some(full) = resolved_ref {
        (
            full.strip_prefix("refs/heads/")
                .unwrap_or(name_input)
                .to_owned(),
            full,
        )
    } else {
        (name_input.to_owned(), format!("refs/heads/{name_input}"))
    };

    if let Some(path) = branch_checked_out_in_other_worktree(repo, &name) {
        bail!(
            "cannot delete branch '{}' used by worktree at '{}'",
            name,
            path
        );
    }

    let current = head.branch_name().unwrap_or("");
    if name == current {
        let wt_path = repo
            .work_tree
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| repo.git_dir.display().to_string());
        bail!(
            "cannot delete branch '{}' used by worktree at '{}'",
            name,
            wt_path
        );
    }

    let branch_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &refname)
        .map_err(|_| anyhow::anyhow!("branch '{name}' not found."))?;

    // For -d (not -D), check if branch is merged into HEAD
    if args.delete && !args.force_delete {
        if let Some(head_oid) = head.oid() {
            if !is_ancestor(repo, branch_oid, *head_oid).unwrap_or(false) {
                bail!(
                    "error: the branch '{}' is not fully merged.\nIf you are sure you want to delete it, run 'git branch -D {}'",
                    name,
                    name
                );
            }
        }
    }

    grit_lib::refs::delete_ref(&repo.git_dir, &refname).map_err(|e| anyhow::anyhow!("{e}"))?;

    // For files backend, clean up empty parent directories
    if !grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
        let ref_path = repo.git_dir.join(&refname);
        let heads_dir = repo.git_dir.join("refs/heads");
        let mut parent = ref_path.parent();
        while let Some(p) = parent {
            if p == heads_dir || !p.starts_with(&heads_dir) {
                break;
            }
            if fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
    }

    if !args.quiet {
        let hex = branch_oid.to_hex();
        let short = &hex[..7.min(hex.len())];
        eprintln!("Deleted branch {name} (was {short}).");
    }

    Ok(())
}

/// Rename a branch.
fn rename_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let (old_name_owned, new_name_owned);
    let (old_name, new_name): (&str, &str);
    if let Some(sp) = args.start_point.as_deref() {
        old_name_owned = args.name.as_deref().unwrap_or("").to_owned();
        new_name_owned = sp.to_owned();
        old_name = &old_name_owned;
        new_name = &new_name_owned;
    } else if let Some(n) = args.name.as_deref() {
        old_name_owned = head
            .branch_name()
            .ok_or_else(|| {
                if matches!(head, HeadState::Detached { .. }) {
                    anyhow::anyhow!("fatal: cannot rename the current branch while not on any")
                } else {
                    anyhow::anyhow!("no current branch to rename")
                }
            })?
            .to_owned();
        new_name_owned = n.to_owned();
        old_name = &old_name_owned;
        new_name = &new_name_owned;
    } else {
        // No args at all: dump usage
        eprintln!("error: branch name required");
        std::process::exit(128);
    };

    // Renaming a branch to itself is a no-op
    if old_name == new_name {
        return Ok(());
    }

    let old_ref = format!("refs/heads/{old_name}");
    let new_ref = format!("refs/heads/{new_name}");

    // Resolve old branch - check both loose and packed refs
    let old_oid = if let Ok(oid) = grit_lib::refs::resolve_ref(&repo.git_dir, &old_ref) {
        oid
    } else if head.branch_name() == Some(old_name) && head.oid().is_none() {
        // Allow renaming an unborn current branch (e.g. immediately after init).
        let head_path = repo.git_dir.join("HEAD");
        let head_content = format!("ref: refs/heads/{new_name}\n");
        fs::write(head_path, head_content)?;
        rename_branch_config(repo, old_name, new_name)?;
        return Ok(());
    } else {
        return Err(anyhow::anyhow!("branch '{old_name}' not found."));
    };

    // Check if new name already exists (unless force; -M or -m -f)
    let force = args.force_rename || args.force;
    if !force && grit_lib::refs::resolve_ref(&repo.git_dir, &new_ref).is_ok() {
        bail!("A branch named '{new_name}' already exists.");
    }

    // Delete the old ref FIRST to avoid d/f conflicts
    // (e.g., renaming m to m/m needs to remove refs/heads/m file before
    // creating refs/heads/m/ directory, or n/n to n needs to remove refs/heads/n/
    // directory before creating refs/heads/n file)
    grit_lib::refs::delete_ref(&repo.git_dir, &old_ref).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Clean up empty parent directories for old ref
    let old_path = repo.git_dir.join(&old_ref);
    let heads_dir = repo.git_dir.join("refs/heads");
    let mut parent = old_path.parent();
    while let Some(p) = parent {
        if p == heads_dir || !p.starts_with(&heads_dir) {
            break;
        }
        if fs::remove_dir(p).is_err() {
            break;
        }
        parent = p.parent();
    }

    // Now write the new ref
    grit_lib::refs::write_ref(&repo.git_dir, &new_ref, &old_oid)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Update HEAD if we renamed the current branch
    if head.branch_name() == Some(old_name) {
        let head_content = format!("ref: refs/heads/{new_name}\n");
        fs::write(repo.git_dir.join("HEAD"), head_content)?;
    }

    // Also update HEAD in worktrees that have the old branch checked out
    update_worktree_heads(repo, old_name, new_name)?;

    // Rename reflog: read old, remove old (with parent cleanup), write new
    let reflog_dir = repo.git_dir.join("logs");
    let old_log = reflog_dir.join(&old_ref);
    let new_log = reflog_dir.join(&new_ref);
    if old_log.is_file() {
        let log_content = fs::read(&old_log).ok();
        // Remove old reflog and clean up empty parent directories
        let _ = fs::remove_file(&old_log);
        let logs_heads_dir = reflog_dir.join("refs/heads");
        let mut parent = old_log.parent();
        while let Some(p) = parent {
            if p == logs_heads_dir || !p.starts_with(&logs_heads_dir) {
                break;
            }
            if fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
        // Write new reflog with existing entries + rename entry
        if let Some(parent) = new_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let ident = get_reflog_identity();
        let rename_entry = format!(
            "{oid} {oid} {ident}\tBranch: renamed {old_ref} to {new_ref}\n",
            oid = old_oid
        );
        let old_content = log_content
            .map(|c| String::from_utf8_lossy(&c).to_string())
            .unwrap_or_default();
        let new_content = format!("{}{rename_entry}", old_content);
        let _ = fs::write(&new_log, new_content.as_bytes());
    }

    // Write HEAD reflog entry for branch rename
    if head.branch_name() == Some(old_name) {
        let head_log = reflog_dir.join("HEAD");
        let ident = get_reflog_identity();
        let entry = format!(
            "{oid} {oid} {ident}\tBranch: renamed {old_ref} to {new_ref}\n",
            oid = old_oid
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&head_log)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(entry.as_bytes())
            });
    }

    // Rename config sections
    rename_branch_config(repo, old_name, new_name)?;

    Ok(())
}

/// Update HEAD in linked worktrees after branch rename.
fn update_worktree_heads(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    let worktrees_dir = repo.git_dir.join("worktrees");
    if let Ok(entries) = fs::read_dir(&worktrees_dir) {
        for entry in entries.flatten() {
            let head_path = entry.path().join("HEAD");
            if let Ok(content) = fs::read_to_string(&head_path) {
                let trimmed = content.trim();
                let expected = format!("ref: refs/heads/{old_name}");
                if trimmed == expected {
                    let new_content = format!("ref: refs/heads/{new_name}\n");
                    let _ = fs::write(&head_path, new_content);
                }
            }
        }
    }
    Ok(())
}

fn branch_used_by_other_worktree(repo: &Repository, branch: &str) -> Result<Option<String>> {
    let occupied = crate::commands::worktree_refs::occupied_branch_refs(repo);
    let target = format!("refs/heads/{branch}");
    if let Some(wt_path) = occupied.get(&target) {
        return Ok(Some(wt_path.clone()));
    }
    Ok(None)
}

fn branch_checked_out_in_other_worktree(repo: &Repository, branch: &str) -> Option<String> {
    branch_used_by_other_worktree(repo, branch).ok().flatten()
}

/// Get reflog identity string.
fn get_reflog_identity() -> String {
    let name = std::env::var("GIT_COMMITTER_NAME").unwrap_or_else(|_| "Test User".to_string());
    let email =
        std::env::var("GIT_COMMITTER_EMAIL").unwrap_or_else(|_| "test@example.com".to_string());
    let date = std::env::var("GIT_COMMITTER_DATE").unwrap_or_else(|_| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{now} +0000")
    });
    format!("{name} <{email}> {date}")
}

/// Rename branch config sections.
fn rename_branch_config(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    let config_path = repo.git_dir.join("config");
    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let old_section = format!("[branch \"{old_name}\"]");
    let new_section = format!("[branch \"{new_name}\"]");
    if content.contains(&old_section) {
        let updated = content.replace(&old_section, &new_section);
        fs::write(&config_path, updated)?;
    }
    Ok(())
}

fn copy_branch(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    let (src_name_owned, dst_name_owned);
    let (src_name, dst_name): (&str, &str);
    if let Some(sp) = args.start_point.as_deref() {
        src_name_owned = args.name.as_deref().unwrap_or("").to_owned();
        dst_name_owned = sp.to_owned();
        src_name = &src_name_owned;
        dst_name = &dst_name_owned;
    } else if let Some(n) = args.name.as_deref() {
        src_name_owned = head
            .branch_name()
            .ok_or_else(|| anyhow::anyhow!("no current branch to copy"))?
            .to_owned();
        dst_name_owned = n.to_owned();
        src_name = &src_name_owned;
        dst_name = &dst_name_owned;
    } else {
        bail!("usage: git branch (-c | -C) [<old-branch>] <new-branch>");
    };

    let src_ref = format!("refs/heads/{src_name}");
    let dst_ref = format!("refs/heads/{dst_name}");

    let src_oid = grit_lib::refs::resolve_ref(&repo.git_dir, &src_ref)
        .map_err(|_| anyhow::anyhow!("branch '{src_name}' not found."))?;

    // Check if dst already exists (unless force copy)
    if !args.force_copy && grit_lib::refs::resolve_ref(&repo.git_dir, &dst_ref).is_ok() {
        bail!("A branch named '{dst_name}' already exists.");
    }

    // Cannot copy onto itself if the result would be a d/f conflict
    if src_name == dst_name {
        return Ok(());
    }

    // Check for d/f conflict: new ref is prefix of existing refs
    let heads_dir = repo.git_dir.join("refs/heads");
    if heads_dir.join(dst_name).is_dir() {
        bail!("'{dst_name}' exists; cannot create 'refs/heads/{dst_name}'");
    }

    grit_lib::refs::write_ref(&repo.git_dir, &dst_ref, &src_oid)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Copy reflog if exists
    let reflog_dir = repo.git_dir.join("logs");
    let src_log = reflog_dir.join(&src_ref);
    let dst_log = reflog_dir.join(&dst_ref);
    if src_log.exists() {
        if let Some(parent) = dst_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(&src_log, &dst_log);
    }

    // Copy config section
    let config_path = repo.git_dir.join("config");
    if let Ok(content) = fs::read_to_string(&config_path) {
        let old_section = format!("[branch \"{src_name}\"]");
        let new_section = format!("[branch \"{dst_name}\"]");
        if content.contains(&old_section) {
            // Extract the section and duplicate it
            let mut result = content.clone();
            let mut section_text = String::new();
            let mut in_section = false;
            for line in content.lines() {
                if line.trim() == old_section.trim() {
                    in_section = true;
                    section_text.push_str(&new_section);
                    section_text.push('\n');
                    continue;
                }
                if in_section {
                    if line.starts_with('[') {
                        in_section = false;
                    } else {
                        section_text.push_str(line);
                        section_text.push('\n');
                    }
                }
            }
            if !section_text.is_empty() {
                result.push('\n');
                result.push_str(&section_text);
                let _ = fs::write(&config_path, result);
            }
        }
    }

    Ok(())
}

/// Collect branch names from a refs directory.
fn collect_branches(dir: &Path, prefix: &str, out: &mut Vec<(String, ObjectId)>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };

        if path.is_dir() {
            collect_branches(&path, &full_name, out)?;
        } else if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                out.push((full_name, oid));
            }
        }
    }

    Ok(())
}

/// Get the first line of a commit's message.
fn commit_subject(odb: &grit_lib::odb::Odb, oid: &ObjectId) -> Option<String> {
    let obj = odb.read(oid).ok()?;
    let commit = parse_commit(&obj.data).ok()?;
    commit.message.lines().next().map(String::from)
}

/// Extract committer timestamp from a commit for sorting.
fn committer_time(
    odb: &grit_lib::odb::Odb,
    oid: &ObjectId,
    branch_name: &str,
    is_remote: bool,
) -> i64 {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    let commit = match parse_commit(&obj.data) {
        Ok(c) => c,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    parse_signature_time(&commit.committer)
}

/// Extract author timestamp from a commit for sorting.
fn author_time(
    odb: &grit_lib::odb::Odb,
    oid: &ObjectId,
    branch_name: &str,
    is_remote: bool,
) -> i64 {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    let commit = match parse_commit(&obj.data) {
        Ok(c) => c,
        Err(_) => {
            if is_remote && branch_name.ends_with("/HEAD") {
                return i64::MAX;
            }
            return 0;
        }
    };
    parse_signature_time(&commit.author)
}

/// Parse the Unix timestamp from a Git signature line like "Name <email> 1234567890 +0000".
fn parse_signature_time(sig: &str) -> i64 {
    let parts: Vec<&str> = sig.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse::<i64>().unwrap_or(0)
    } else {
        0
    }
}

/// Simple glob matching for branch pattern filtering.
/// Supports `*` (match any chars) and `?` (match one char).
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'?' {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
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
