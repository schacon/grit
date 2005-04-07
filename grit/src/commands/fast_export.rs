//! `grit fast-export` — export repository history in fast-import stream format.
//!
//! Supports the subset used by upstream tests: `--all`, `--no-data`, optional
//! `--use-done-feature`, and passes unknown flags through for compatibility.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{diff_trees, DiffEntry, DiffStatus};
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::refs::list_refs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, OrderingMode, RevListOptions};
use std::collections::HashMap;
use std::io::Write;

/// Arguments for `grit fast-export`.
#[derive(Debug, ClapArgs)]
#[command(about = "Export repository as fast-import stream")]
pub struct Args {
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-export`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let parsed = parse_fast_export_args(&args.args)?;

    if parsed.use_done_feature {
        println!("feature done");
    }

    let mut opts = RevListOptions::default();
    opts.all_refs = parsed.export_all;
    opts.ordering = OrderingMode::Topo;
    opts.reverse = true;

    let result = rev_list(&repo, &[], &[], &opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    let commits = result.commits;

    let mut commit_marks: HashMap<ObjectId, u32> = HashMap::new();
    let mut next_mark: u32 = 1;

    let all_pairs = list_refs(&repo.git_dir, "refs/").unwrap_or_default();
    let mut best_ref_for_tip: HashMap<ObjectId, String> = HashMap::new();
    for (name, oid) in all_pairs {
        let obj = match repo.odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit_oid = oid;
        let pick = |existing: &str, candidate: &str| -> String {
            fn rank(r: &str) -> u8 {
                if r.starts_with("refs/tags/") {
                    0
                } else if r.starts_with("refs/heads/") && !r.starts_with("refs/heads/export-") {
                    1
                } else if r.starts_with("refs/remotes/") {
                    2
                } else if r.starts_with("refs/heads/export-") {
                    4
                } else {
                    3
                }
            }
            let re = rank(existing);
            let rc = rank(candidate);
            if rc < re || (rc == re && candidate < existing) {
                candidate.to_string()
            } else {
                existing.to_string()
            }
        };
        best_ref_for_tip
            .entry(commit_oid)
            .and_modify(|e| {
                let n = pick(e, &name);
                *e = n;
            })
            .or_insert(name);
    }

    let mut out = std::io::stdout().lock();

    for oid in commits {
        let hex = oid.to_hex();
        let refname = best_ref_for_tip
            .get(&oid)
            .cloned()
            .unwrap_or_else(|| format!("refs/heads/export-{}", &hex[..7.min(hex.len())]));

        let obj = repo.odb.read(&oid)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = parse_commit(&obj.data)?;

        let parent_oid = commit.parents.first().copied();
        let diff_entries = if let Some(p) = parent_oid {
            let p_obj = repo.odb.read(&p)?;
            let p_commit = parse_commit(&p_obj.data)?;
            let mut entries = diff_trees(&repo.odb, Some(&p_commit.tree), Some(&commit.tree), "")?;
            sort_diff_for_export(&mut entries);
            entries
        } else {
            let mut entries = diff_trees(&repo.odb, None, Some(&commit.tree), "")?;
            sort_diff_for_export(&mut entries);
            entries
        };

        let my_mark = next_mark;
        commit_marks.insert(oid, my_mark);
        next_mark += 1;

        if commit.parents.is_empty() {
            writeln!(out, "reset {refname}")?;
        }
        writeln!(out, "commit {refname}")?;
        writeln!(out, "mark :{my_mark}")?;

        let author_line = commit_line_field(&commit.author, "author");
        let committer_line = commit_line_field(&commit.committer, "committer");
        writeln!(out, "{author_line}")?;
        writeln!(out, "{committer_line}")?;

        let msg_bytes = commit
            .raw_message
            .as_deref()
            .unwrap_or(commit.message.as_bytes());
        writeln!(out, "data {}", msg_bytes.len())?;
        out.write_all(msg_bytes)?;
        if !msg_bytes.ends_with(b"\n") {
            writeln!(out)?;
        }

        for (i, parent) in commit.parents.iter().enumerate() {
            let label = if i == 0 { "from" } else { "merge" };
            if let Some(&m) = commit_marks.get(parent) {
                writeln!(out, "{label} :{m}")?;
            } else {
                writeln!(out, "{label} {}", parent.to_hex())?;
            }
        }

        for e in &diff_entries {
            write_diff_entry(&mut out, e, parsed.no_data)?;
        }
        writeln!(out)?;
    }

    if parsed.use_done_feature {
        println!("done");
    }

    Ok(())
}

struct ParsedFastExport {
    export_all: bool,
    no_data: bool,
    use_done_feature: bool,
}

fn parse_fast_export_args(args: &[String]) -> Result<ParsedFastExport> {
    let mut export_all = false;
    let mut no_data = false;
    let mut use_done_feature = false;

    for a in args {
        match a.as_str() {
            "--all" => export_all = true,
            "--no-data" => no_data = true,
            "--use-done-feature" => use_done_feature = true,
            _ => {}
        }
    }

    if !export_all {
        bail!("fast-export: only --all is currently supported");
    }

    Ok(ParsedFastExport {
        export_all,
        no_data,
        use_done_feature,
    })
}

fn commit_line_field(raw: &str, header: &str) -> String {
    if raw.starts_with(header) && raw.as_bytes().get(header.len()) == Some(&b' ') {
        raw.to_string()
    } else {
        format!("{header} {raw}")
    }
}

fn sort_diff_for_export(entries: &mut [DiffEntry]) {
    entries.sort_by(|a, b| {
        let name_a = a
            .old_path
            .as_deref()
            .or(a.new_path.as_deref())
            .unwrap_or("");
        let name_b = b
            .old_path
            .as_deref()
            .or(b.new_path.as_deref())
            .unwrap_or("");
        let len_a = name_a.len();
        let len_b = name_b.len();
        let len = len_a.min(len_b);
        let cmp = name_a.as_bytes()[..len].cmp(&name_b.as_bytes()[..len]);
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }
        let cmp_len = len_b.cmp(&len_a);
        if cmp_len != std::cmp::Ordering::Equal {
            return cmp_len;
        }
        let ra = matches!(a.status, DiffStatus::Renamed);
        let rb = matches!(b.status, DiffStatus::Renamed);
        ra.cmp(&rb)
    });
}

fn write_diff_entry(out: &mut impl Write, e: &DiffEntry, no_data: bool) -> Result<()> {
    match e.status {
        DiffStatus::Deleted => {
            let p = e
                .new_path
                .as_deref()
                .or(e.old_path.as_deref())
                .unwrap_or("");
            writeln!(out, "D {p}")?;
        }
        DiffStatus::Renamed | DiffStatus::Copied => {
            let old_p = e.old_path.as_deref().unwrap_or("");
            let new_p = e.new_path.as_deref().unwrap_or("");
            if e.old_oid == e.new_oid && e.old_mode == e.new_mode {
                let c = if e.status == DiffStatus::Renamed {
                    'R'
                } else {
                    'C'
                };
                writeln!(out, "{c} {old_p} {new_p}")?;
                return Ok(());
            }
            // Modified rename: fall through to M on destination
            write_modify_line(out, e, no_data)?;
        }
        DiffStatus::Added
        | DiffStatus::Modified
        | DiffStatus::TypeChanged
        | DiffStatus::Unmerged => {
            write_modify_line(out, e, no_data)?;
        }
    }
    Ok(())
}

fn write_modify_line(out: &mut impl Write, e: &DiffEntry, no_data: bool) -> Result<()> {
    let path = e
        .new_path
        .as_deref()
        .or(e.old_path.as_deref())
        .unwrap_or("");
    let mode = parse_mode_octal(&e.new_mode)?;
    if no_data || mode == 0o160000 {
        writeln!(out, "M {:o} {} {path}", mode, e.new_oid.to_hex())?;
        return Ok(());
    }
    bail!("fast-export: blob export without --no-data is not implemented");
}

fn parse_mode_octal(s: &str) -> Result<u32> {
    let t = s.strip_prefix("0o").unwrap_or(s);
    u32::from_str_radix(t, 8).map_err(|_| anyhow::anyhow!("invalid mode: {s}"))
}
