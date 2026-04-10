//! Branch vs remote-tracking comparison for status, checkout, and commit (matches `git/remote.c`).

use anyhow::Result;
use grit_lib::config::ConfigSet;
use grit_lib::merge_base::count_symmetric_ahead_behind;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{
    abbreviate_ref_name, resolve_push_full_ref_for_branch, resolve_upstream_symbolic_name,
};
use std::collections::HashSet;
use std::fs;

/// How to compare local HEAD to a remote-tracking ref (`AHEAD_BEHIND_FULL` vs `QUICK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AheadBehindMode {
    /// Compute exact ahead/behind counts (`rev-list --left-right`).
    Full,
    /// Only detect same vs different (cheap).
    Quick,
}

/// Outcome of comparing `refs/heads/<branch>` to a tracking ref.
#[derive(Clone, Debug)]
pub enum TrackingStat {
    /// Tips are the same commit.
    UpToDate,
    /// Tracking ref is missing (gone upstream).
    Gone { display_name: String },
    /// Tips differ; counts are zero in [`AheadBehindMode::Quick`] mode.
    Diverged {
        display_name: String,
        ahead: usize,
        behind: usize,
    },
}

/// Short display name for a full ref (`refs/remotes/origin/main` → `origin/main`).
pub fn shorten_tracking_ref(full_ref: &str) -> String {
    abbreviate_ref_name(full_ref)
}

fn branch_head_ref(short_name: &str) -> String {
    format!("refs/heads/{short_name}")
}

/// Full ref for the configured upstream of `branch_short` (`refs/remotes/...` or `refs/heads/...`).
pub fn upstream_tracking_full_ref(repo: &Repository, branch_short: &str) -> Option<String> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).ok()?;
    let remote = config.get(&format!("branch.{branch_short}.remote"))?;
    let merge = config.get(&format!("branch.{branch_short}.merge"))?;
    if remote == "." {
        let m = merge.trim();
        if m.starts_with("refs/") {
            Some(m.to_owned())
        } else {
            Some(format!("refs/heads/{m}"))
        }
    } else {
        let mb = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        Some(format!("refs/remotes/{remote}/{mb}"))
    }
}

/// Compare local branch tip to `base_ref` (full ref like `refs/remotes/origin/main`).
pub fn stat_branch_pair(
    repo: &Repository,
    branch_short: &str,
    base_ref: &str,
    mode: AheadBehindMode,
) -> Result<TrackingStat> {
    let branch_ref = branch_head_ref(branch_short);
    let local_oid = match refs::resolve_ref(&repo.git_dir, &branch_ref) {
        Ok(o) => o,
        Err(_) => {
            return Ok(TrackingStat::Diverged {
                display_name: shorten_tracking_ref(base_ref),
                ahead: 0,
                behind: 0,
            });
        }
    };
    let upstream_oid = match refs::resolve_ref(&repo.git_dir, base_ref) {
        Ok(o) => o,
        Err(_) => {
            return Ok(TrackingStat::Gone {
                display_name: shorten_tracking_ref(base_ref),
            });
        }
    };
    if local_oid == upstream_oid {
        return Ok(TrackingStat::UpToDate);
    }
    if mode == AheadBehindMode::Quick {
        return Ok(TrackingStat::Diverged {
            display_name: shorten_tracking_ref(base_ref),
            ahead: 0,
            behind: 0,
        });
    }
    let (ahead, behind) = count_symmetric_ahead_behind(repo, local_oid, upstream_oid)?;
    Ok(TrackingStat::Diverged {
        display_name: shorten_tracking_ref(base_ref),
        ahead,
        behind,
    })
}

/// Read `status.compareBranches` from `.git/config` (`[status]` section or dotted key).
fn parse_status_compare_branches(config_content: &str) -> Option<String> {
    let mut in_status = false;
    for line in config_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_status = trimmed.eq_ignore_ascii_case("[status]");
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("status.comparebranches") {
            return trimmed.split_once('=').map(|(_, v)| v.trim().to_owned());
        }
        if in_status && lower.starts_with("comparebranches") {
            return trimmed.split_once('=').map(|(_, v)| v.trim().to_owned());
        }
    }
    None
}

fn parse_compare_branch_specs(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn resolve_compare_full_ref(repo: &Repository, branch_short: &str, token: &str) -> Option<String> {
    let t = token.trim();
    if t.eq_ignore_ascii_case("@{upstream}") || t.eq_ignore_ascii_case("@{u}") {
        let spec = if branch_short.is_empty() {
            "@{u}".to_string()
        } else {
            format!("{branch_short}@{{u}}")
        };
        resolve_upstream_symbolic_name(repo, &spec).ok()
    } else if t.eq_ignore_ascii_case("@{push}") {
        resolve_push_full_ref_for_branch(repo, branch_short).ok()
    } else {
        None
    }
}

/// Multi-branch tracking lines for porcelain long status and checkout (Git `format_tracking_info`).
pub fn format_tracking_info(
    repo: &Repository,
    branch_short: &str,
    mode: AheadBehindMode,
    show_divergence_advice: bool,
) -> Result<String> {
    let config_path = repo.git_dir.join("config");
    let config_raw = fs::read_to_string(&config_path).unwrap_or_default();
    let compare_raw =
        parse_status_compare_branches(&config_raw).unwrap_or_else(|| "@{upstream}".to_string());

    let tokens = parse_compare_branch_specs(&compare_raw);
    if tokens.is_empty() {
        return Ok(String::new());
    }

    let upstream_full = resolve_compare_full_ref(repo, branch_short, "@{upstream}");
    let push_full = resolve_compare_full_ref(repo, branch_short, "@{push}");

    let mut seen: HashSet<String> = HashSet::new();
    let mut out = String::new();
    let mut reported = false;

    for tok in tokens {
        let Some(full_ref) = resolve_compare_full_ref(repo, branch_short, &tok) else {
            continue;
        };
        if !seen.insert(full_ref.clone()) {
            continue;
        }

        let is_upstream = upstream_full.as_ref() == Some(&full_ref);
        let mut is_push = push_full.as_ref() == Some(&full_ref);
        if is_upstream && push_full.as_ref().is_none_or(|p| p == &full_ref) {
            is_push = true;
        }

        let stat = stat_branch_pair(repo, branch_short, &full_ref, mode)?;

        match &stat {
            TrackingStat::Gone { display_name } if is_upstream => {
                if reported {
                    out.push('\n');
                }
                out.push_str(&format!(
                    "Your branch is based on '{display_name}', but the upstream is gone.\n"
                ));
                out.push_str("  (use \"git branch --unset-upstream\" to fixup)\n");
                reported = true;
            }
            TrackingStat::Gone { .. } => {}
            TrackingStat::UpToDate => {
                if reported {
                    out.push('\n');
                }
                let d = shorten_tracking_ref(&full_ref);
                out.push_str(&format!("Your branch is up to date with '{d}'.\n"));
                reported = true;
            }
            TrackingStat::Diverged {
                display_name,
                ahead,
                behind,
            } => {
                if reported {
                    out.push('\n');
                }
                if mode == AheadBehindMode::Quick {
                    out.push_str(&format!(
                        "Your branch and '{display_name}' refer to different commits.\n"
                    ));
                    if is_push {
                        out.push_str("  (use \"git status --ahead-behind\" for details)\n");
                    }
                } else if *ahead > 0 && *behind > 0 {
                    out.push_str(&format!(
                        "Your branch and '{display_name}' have diverged,\n\
and have {ahead} and {behind} different commits each, respectively.\n"
                    ));
                    if show_divergence_advice && is_upstream {
                        out.push_str(
                            "  (use \"git pull\" if you want to integrate the remote branch with yours)\n",
                        );
                    }
                } else if *ahead > 0 {
                    out.push_str(&format!(
                        "Your branch is ahead of '{display_name}' by {ahead} commit{}.\n",
                        if *ahead == 1 { "" } else { "s" }
                    ));
                    if is_push {
                        out.push_str("  (use \"git push\" to publish your local commits)\n");
                    }
                } else {
                    out.push_str(&format!(
                        "Your branch is behind '{display_name}' by {behind} commit{}, and can be fast-forwarded.\n",
                        if *behind == 1 { "" } else { "s" }
                    ));
                    if is_upstream {
                        out.push_str("  (use \"git pull\" to update your local branch)\n");
                    }
                }
                reported = true;
            }
        }
    }

    Ok(out)
}
