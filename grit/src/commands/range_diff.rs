//! `grit range-diff` — compare two commit ranges.
//!
//! Shows which commits differ between two ranges.  Each commit in range A is
//! matched against commits in range B by patch-id.  Paired commits are shown
//! side-by-side, and unpaired commits are flagged as only in one range.
//!
//! Supports two calling conventions:
//! - `grit range-diff <base1>..<rev1> <base2>..<rev2>`
//! - `grit range-diff <rev1>...<rev2>` (symmetric difference via merge-base)

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::patch_ids::compute_patch_id;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, RevListOptions, RevListResult};
use std::collections::HashMap;
use std::io::{self, Write};

/// Arguments for `grit range-diff`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Compare two commit ranges",
    override_usage = "grit range-diff <base1>..<rev1> <base2>..<rev2>\n       grit range-diff <rev1>...<rev2>"
)]
pub struct Args {
    /// Revision range arguments.
    #[arg(required = true)]
    pub ranges: Vec<String>,
}

/// A commit in a range with its summary and optional patch-id.
struct RangeCommit {
    oid: ObjectId,
    summary: String,
    patch_id: Option<ObjectId>,
}

/// Run the `range-diff` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let (range_a, range_b) = parse_ranges(&args.ranges)?;

    let commits_a = walk_range(&repo, &range_a.0, &range_a.1)?;
    let commits_b = walk_range(&repo, &range_b.0, &range_b.1)?;

    let enriched_a = enrich_commits(&repo, commits_a)?;
    let enriched_b = enrich_commits(&repo, commits_b)?;

    // Build patch-id → index map for range B.
    let mut b_by_patch_id: HashMap<ObjectId, usize> = HashMap::new();
    for (i, c) in enriched_b.iter().enumerate() {
        if let Some(pid) = &c.patch_id {
            b_by_patch_id.entry(*pid).or_insert(i);
        }
    }

    // Match commits from A to B by patch-id.
    let mut b_matched = vec![false; enriched_b.len()];
    let mut matches: Vec<Option<usize>> = Vec::with_capacity(enriched_a.len());

    for c in &enriched_a {
        if let Some(pid) = &c.patch_id {
            if let Some(&bi) = b_by_patch_id.get(pid) {
                if !b_matched[bi] {
                    b_matched[bi] = true;
                    matches.push(Some(bi));
                    continue;
                }
            }
        }
        matches.push(None);
    }

    // Output.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let a_width = enriched_a.len().to_string().len().max(1);
    let b_width = enriched_b.len().to_string().len().max(1);

    for (i, c) in enriched_a.iter().enumerate() {
        let short_a = &c.oid.to_hex()[..12];
        let num_a = i + 1;
        match matches[i] {
            Some(bi) => {
                let bc = &enriched_b[bi];
                let short_b = &bc.oid.to_hex()[..12];
                let num_b = bi + 1;
                let marker = if c.oid == bc.oid { "=" } else { "!" };
                writeln!(
                    out,
                    "{num_a:>a_width$}: {short_a} {marker} {num_b:>b_width$}: {short_b} {}",
                    c.summary
                )?;
            }
            None => {
                let pad = " ".repeat(b_width + 2 + 12);
                writeln!(out, "{num_a:>a_width$}: {short_a} < {pad} {}", c.summary)?;
            }
        }
    }

    // Unmatched commits in B.
    for (i, c) in enriched_b.iter().enumerate() {
        if !b_matched[i] {
            let short_b = &c.oid.to_hex()[..12];
            let num_b = i + 1;
            let pad_a = " ".repeat(a_width + 2 + 12);
            writeln!(out, "{pad_a} > {num_b:>b_width$}: {short_b} {}", c.summary)?;
        }
    }

    Ok(())
}

/// Parse range arguments into two `(base, tip)` pairs.
fn parse_ranges(args: &[String]) -> Result<((String, String), (String, String))> {
    if args.len() == 1 {
        // Symmetric: rev1...rev2
        let arg = &args[0];
        if let Some((left, right)) = arg.split_once("...") {
            if left.is_empty() || right.is_empty() {
                bail!("invalid symmetric range: {arg}");
            }
            // The base for both is the merge-base of left and right.
            // We represent it as the range base..tip where base is
            // derived via merge-base at walk time. For simplicity,
            // use left as base for right's range and right as base for left's.
            return Ok((
                (right.to_string(), left.to_string()),
                (left.to_string(), right.to_string()),
            ));
        }
        bail!("expected <rev1>...<rev2> or two <base>..<tip> arguments");
    }
    if args.len() == 2 {
        let a = parse_dotdot(&args[0])?;
        let b = parse_dotdot(&args[1])?;
        return Ok((a, b));
    }
    if args.len() == 3 {
        // Three-arg form: base rev1 rev2
        return Ok((
            (args[0].clone(), args[1].clone()),
            (args[0].clone(), args[2].clone()),
        ));
    }
    bail!("usage: range-diff <base1>..<rev1> <base2>..<rev2> or range-diff <rev1>...<rev2>");
}

/// Parse a `base..tip` range specification.
fn parse_dotdot(s: &str) -> Result<(String, String)> {
    if let Some((base, tip)) = s.split_once("..") {
        if base.is_empty() || tip.is_empty() {
            bail!("invalid range: {s}");
        }
        Ok((base.to_string(), tip.to_string()))
    } else {
        bail!("expected <base>..<tip>, got: {s}");
    }
}

/// Walk a range of commits from base..tip (exclusive base).
fn walk_range(repo: &Repository, base: &str, tip: &str) -> Result<Vec<ObjectId>> {
    let options = RevListOptions {
        max_count: None,
        skip: 0,
        reverse: true,
        ..Default::default()
    };

    let positive = vec![tip.to_string()];
    let negative = vec![base.to_string()];

    let result: RevListResult = rev_list(repo, &positive, &negative, &options)?;
    Ok(result.commits)
}

/// Enrich commit OIDs with summary lines and patch-ids.
fn enrich_commits(repo: &Repository, oids: Vec<ObjectId>) -> Result<Vec<RangeCommit>> {
    let mut out = Vec::with_capacity(oids.len());
    for oid in oids {
        let obj = repo.odb.read(&oid)?;
        let summary = if obj.kind == ObjectKind::Commit {
            let commit = parse_commit(&obj.data)?;
            let msg = &commit.message;
            msg.lines().next().unwrap_or("").to_string()
        } else {
            String::new()
        };

        let patch_id = compute_patch_id(&repo.odb, &oid).unwrap_or(None);

        out.push(RangeCommit {
            oid,
            summary,
            patch_id,
        });
    }
    Ok(out)
}
