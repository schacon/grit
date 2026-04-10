//! Interactive `git add -p` — stage selected hunks from the index↔worktree diff.
//!
//! Uses the same Myers line-diff and hunk-splitting approach as [`crate::commands::stash`] patch
//! mode, then writes blended blob content and updated modes into the index.

use anyhow::{bail, Context, Result};
use grit_lib::crlf::{self, ConvertToGitOpts};
use grit_lib::diff::{diff_index_to_worktree, mode_from_metadata, DiffStatus};
use grit_lib::index::{Index, IndexEntry};
use grit_lib::merge_file::is_binary;
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use similar::{Algorithm, TextDiff};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::commands::add::{resolved_env_index_path, AddConfig};
use crate::commands::checkout::{patch_path_filter_matches, resolve_pathspec};
use crate::commands::stash::{partial_unified_for_op_range, split_hunk_at_first_gap};
use grit_lib::index::entry_from_metadata;

/// Blend index and worktree bytes for **staging** (`git add -p`).
///
/// [`checkout::blend_line_diff_by_hunk_ranges`] uses `accepted` with **revert/checkout** semantics
/// (accepted ⇒ keep the index/source side). For `add -p`, user `y` means take the **worktree**
/// side, so we invert the boolean vector.
fn blend_for_stage_hunks(
    index_bytes: &[u8],
    work_bytes: &[u8],
    ranges: &[(usize, usize)],
    stage_yes: &[bool],
) -> String {
    let revert_accepted: Vec<bool> = stage_yes.iter().map(|a| !*a).collect();
    crate::commands::checkout::blend_line_diff_by_hunk_ranges(
        index_bytes,
        work_bytes,
        ranges,
        &revert_accepted,
    )
}

/// Run `git add -p` / `git add --patch`.
pub(crate) fn run_add_patch(
    repo: &Repository,
    pathspecs: &[String],
    add_cfg: &AddConfig,
) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cwd = std::env::current_dir().context("resolving cwd")?;
    let filter_paths: Vec<String> = pathspecs
        .iter()
        .map(|p| resolve_pathspec(p, work_tree, &cwd))
        .collect();

    let index_path = resolved_env_index_path(repo);
    let mut index = repo.load_index_at(&index_path).context("loading index")?;

    let mut entries = diff_index_to_worktree(&repo.odb, &index, work_tree, false, false)?;
    entries.retain(|e| {
        if e.status == DiffStatus::Unmerged {
            return false;
        }
        patch_path_filter_matches(e.path(), &filter_paths)
    });
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    if entries.is_empty() {
        println!("No changes.");
        return Ok(());
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut out = io::stdout();

    let odb = &repo.odb;
    let conv = &add_cfg.conv;
    let attrs = &add_cfg.attrs;

    for entry in entries {
        let path_str = entry.path().to_owned();
        let path_bytes = path_str.as_bytes();

        let Some(ie) = index.get(path_bytes, 0).cloned() else {
            continue;
        };

        if ie.mode == 0o160000 {
            continue;
        }

        let abs_path = work_tree.join(&path_str);
        let meta = match fs::symlink_metadata(&abs_path) {
            Ok(m) => m,
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(20) /* ENOTDIR */ =>
            {
                if entry.status != DiffStatus::Deleted {
                    continue;
                }
                handle_deleted_file(
                    repo,
                    &mut index,
                    index_path.as_path(),
                    &path_str,
                    &ie,
                    &mut reader,
                    &mut out,
                    odb,
                )?;
                continue;
            }
            Err(_) => continue,
        };

        let file_attrs = crlf::get_file_attrs(attrs, &path_str, false, &add_cfg.config);

        let index_blob = if ie.oid == ObjectId::zero() {
            Vec::new()
        } else {
            let obj = match odb.read(&ie.oid) {
                Ok(o) if o.kind == ObjectKind::Blob => o.data,
                _ => continue,
            };
            obj
        };

        let work_blob = if meta.file_type().is_symlink() {
            let target = fs::read_link(&abs_path)?;
            target.to_string_lossy().into_owned().into_bytes()
        } else {
            let raw = fs::read(&abs_path).unwrap_or_default();
            let prior_blob = if ie.oid != ObjectId::zero() {
                Some(index_blob.clone())
            } else {
                None
            };
            let opts = ConvertToGitOpts {
                index_blob: prior_blob.as_deref(),
                renormalize: false,
                check_safecrlf: true,
            };
            match crlf::convert_to_git_with_opts(&raw, &path_str, conv, &file_attrs, opts) {
                Ok(c) => c,
                Err(msg) => {
                    eprintln!("{msg}");
                    continue;
                }
            }
        };

        if is_binary(&index_blob) || is_binary(&work_blob) {
            continue;
        }

        let mode_differs = parse_mode_u32(&entry.old_mode) != parse_mode_u32(&entry.new_mode);
        let content_differs = index_blob != work_blob;

        let mut effective_mode = ie.mode;
        let index_side_bytes = index_blob.clone();

        if mode_differs {
            write!(out, "(1/1) Stage mode change [y,n,q,a,d,s,e,p,P,?]? ").ok();
            out.flush().ok();
            match read_one_command(&mut reader, &mut out)? {
                ReadCmd::Eof => {
                    repo.write_index_at(&index_path, &mut index)?;
                    return Ok(());
                }
                ReadCmd::Invalid => {}
                ReadCmd::Char(c) => match c {
                    'y' => effective_mode = mode_from_metadata(&meta),
                    'q' => {
                        repo.write_index_at(&index_path, &mut index)?;
                        return Ok(());
                    }
                    _ => {}
                },
            }
        }

        if !content_differs {
            if mode_differs && effective_mode != ie.mode {
                write_index_blob_and_mode(
                    odb,
                    &mut index,
                    &path_str,
                    &abs_path,
                    &index_side_bytes,
                    effective_mode,
                )?;
            }
            continue;
        }

        let mut cur_work = work_blob;

        'rediff: loop {
            let index_str = String::from_utf8_lossy(&index_side_bytes);
            let work_str = String::from_utf8_lossy(&cur_work);
            let text_diff = TextDiff::configure()
                .algorithm(Algorithm::Myers)
                .diff_lines(index_str.as_ref(), work_str.as_ref());
            let ops: Vec<_> = text_diff.ops().to_vec();
            let has_change = ops
                .iter()
                .any(|o| !matches!(o, similar::DiffOp::Equal { .. }));
            if !has_change {
                if mode_differs && effective_mode != ie.mode {
                    write_index_blob_and_mode(
                        odb,
                        &mut index,
                        &path_str,
                        &abs_path,
                        &index_side_bytes,
                        effective_mode,
                    )?;
                }
                break 'rediff;
            }

            let n_ops = ops.len();
            let mut hunk_ranges: Vec<(usize, usize)> = vec![(0, n_ops)];
            let mut accepted = vec![false; hunk_ranges.len()];
            let mut hunk_cursor = 0usize;

            'hunk_loop: loop {
                let n_hunks = hunk_ranges.len();
                if hunk_cursor >= n_hunks {
                    break;
                }

                let display_idx = hunk_cursor + 1;
                let (s, e) = hunk_ranges[hunk_cursor];
                let hunk_only = partial_unified_for_op_range(
                    path_str.as_str(),
                    &index_side_bytes,
                    &cur_work,
                    &ops[s..e],
                    3,
                );

                writeln!(out, "diff --git a/{path_str} b/{path_str}").ok();
                write!(out, "--- a/{path_str}\n+++ b/{path_str}\n").ok();
                write!(out, "{hunk_only}").ok();
                write!(
                    out,
                    "({display_idx}/{n_hunks}) Stage this hunk [y,n,q,a,d,s,e,p,P,?]? "
                )
                .ok();
                out.flush().ok();

                match read_one_command(&mut reader, &mut out)? {
                    ReadCmd::Eof => {
                        let blended = blend_for_stage_hunks(
                            &index_side_bytes,
                            &cur_work,
                            &hunk_ranges,
                            &accepted,
                        );
                        write_index_blob_and_mode(
                            odb,
                            &mut index,
                            &path_str,
                            &abs_path,
                            blended.as_bytes(),
                            effective_mode,
                        )?;
                        repo.write_index_at(&index_path, &mut index)?;
                        return Ok(());
                    }
                    ReadCmd::Invalid => continue 'hunk_loop,
                    ReadCmd::Char(c) => match c {
                        'y' => {
                            accepted[hunk_cursor] = true;
                            hunk_cursor += 1;
                        }
                        'n' => {
                            hunk_cursor += 1;
                        }
                        'a' => {
                            for j in hunk_cursor..n_hunks {
                                accepted[j] = true;
                            }
                            break 'hunk_loop;
                        }
                        'd' => break 'hunk_loop,
                        'q' => {
                            let blended = blend_for_stage_hunks(
                                &index_side_bytes,
                                &cur_work,
                                &hunk_ranges,
                                &accepted,
                            );
                            write_index_blob_and_mode(
                                odb,
                                &mut index,
                                &path_str,
                                &abs_path,
                                blended.as_bytes(),
                                effective_mode,
                            )?;
                            repo.write_index_at(&index_path, &mut index)?;
                            return Ok(());
                        }
                        's' => {
                            if !split_hunk_at_first_gap(&mut hunk_ranges, hunk_cursor, &ops) {
                                writeln!(out, "Sorry, cannot split this hunk").ok();
                                continue 'hunk_loop;
                            }
                            let n = hunk_ranges.len();
                            accepted.resize(n, false);
                            continue 'hunk_loop;
                        }
                        'e' => match edit_worktree_via_editor(&cur_work) {
                            Ok(edited) => {
                                cur_work = edited;
                                continue 'rediff;
                            }
                            Err(_) => continue 'hunk_loop,
                        },
                        '?' => {
                            writeln!(
                                out,
                                "y - stage this hunk\n\
                                 n - do not stage this hunk\n\
                                 q - quit; do not stage this hunk or any of the remaining ones\n\
                                 a - stage this hunk and all later hunks in the file\n\
                                 d - do not stage this hunk or any of the later hunks in the file\n\
                                 s - split the current hunk into smaller hunks\n\
                                 e - manually edit the current hunk\n"
                            )
                            .ok();
                            continue 'hunk_loop;
                        }
                        _ => continue 'hunk_loop,
                    },
                }
            }

            let blended =
                blend_for_stage_hunks(&index_side_bytes, &cur_work, &hunk_ranges, &accepted);

            if accepted.iter().any(|&a| a) || (mode_differs && effective_mode != ie.mode) {
                write_index_blob_and_mode(
                    odb,
                    &mut index,
                    &path_str,
                    &abs_path,
                    blended.as_bytes(),
                    effective_mode,
                )?;
            }
            break 'rediff;
        }
    }

    repo.write_index_at(&index_path, &mut index)
        .context("writing index")?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadCmd {
    Eof,
    Invalid,
    Char(char),
}

fn read_one_command(reader: &mut impl BufRead, out: &mut impl Write) -> Result<ReadCmd> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(ReadCmd::Eof);
    }
    let trimmed = line.trim_end_matches(['\n', '\r']);
    let t = trimmed.trim();
    if t.is_empty() {
        return Ok(ReadCmd::Char(' '));
    }
    if t.len() > 1 {
        writeln!(out, "Sorry, only one letter is expected, got '{t}'")?;
        return Ok(ReadCmd::Invalid);
    }
    let c = t.chars().next().unwrap_or(' ');
    Ok(ReadCmd::Char(c.to_ascii_lowercase()))
}

fn parse_mode_u32(m: &str) -> u32 {
    u32::from_str_radix(m, 8).unwrap_or(0)
}

fn handle_deleted_file(
    repo: &Repository,
    index: &mut Index,
    index_path: &Path,
    path_str: &str,
    ie: &IndexEntry,
    reader: &mut impl BufRead,
    out: &mut impl Write,
    odb: &Odb,
) -> Result<()> {
    let index_blob = if ie.oid == ObjectId::zero() {
        Vec::new()
    } else {
        let obj = odb.read(&ie.oid)?;
        if obj.kind != ObjectKind::Blob {
            return Ok(());
        }
        obj.data
    };
    if is_binary(&index_blob) {
        return Ok(());
    }

    let work_blob = Vec::<u8>::new();
    let index_str = String::from_utf8_lossy(&index_blob);
    let work_str = String::from_utf8_lossy(&work_blob);
    let text_diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .diff_lines(index_str.as_ref(), work_str.as_ref());
    let ops: Vec<_> = text_diff.ops().to_vec();
    let n_ops = ops.len();
    let mut hunk_ranges = vec![(0, n_ops)];
    let mut accepted = vec![false; 1];
    let mut hunk_cursor = 0usize;

    loop {
        if hunk_cursor >= hunk_ranges.len() {
            break;
        }
        let display_idx = hunk_cursor + 1;
        let n_hunks = hunk_ranges.len();
        let (s, e) = hunk_ranges[hunk_cursor];
        let hunk_only =
            partial_unified_for_op_range(path_str, &index_blob, &work_blob, &ops[s..e], 3);
        writeln!(out, "diff --git a/{path_str} b/{path_str}").ok();
        write!(out, "--- a/{path_str}\n+++ b/{path_str}\n").ok();
        write!(out, "{hunk_only}").ok();
        write!(
            out,
            "({display_idx}/{n_hunks}) Stage deletion [y,n,q,a,d,s,e,p,P,?]? "
        )
        .ok();
        out.flush().ok();

        match read_one_command(reader, out)? {
            ReadCmd::Eof => {
                repo.write_index_at(index_path, index)?;
                return Ok(());
            }
            ReadCmd::Invalid => continue,
            ReadCmd::Char(c) => match c {
                'y' => {
                    accepted[hunk_cursor] = true;
                    hunk_cursor += 1;
                }
                'n' => {
                    hunk_cursor += 1;
                }
                'a' => {
                    for j in hunk_cursor..n_hunks {
                        accepted[j] = true;
                    }
                    break;
                }
                'd' => break,
                'q' => {
                    repo.write_index_at(index_path, index)?;
                    return Ok(());
                }
                's' => {
                    if !split_hunk_at_first_gap(&mut hunk_ranges, hunk_cursor, &ops) {
                        writeln!(out, "Sorry, cannot split this hunk").ok();
                        continue;
                    }
                    let n = hunk_ranges.len();
                    accepted.resize(n, false);
                }
                '?' => {
                    writeln!(
                        out,
                        "y - stage this hunk for deletion\n\
                         n - do not stage this hunk\n\
                         q - quit\n\
                         a - stage this and all later hunks\n\
                         d - skip remaining hunks in this file\n\
                         s - split hunk\n"
                    )
                    .ok();
                }
                _ => {}
            },
        }
    }

    if accepted.iter().any(|&a| a) {
        let blended = blend_for_stage_hunks(&index_blob, &work_blob, &hunk_ranges, &accepted);
        if blended.is_empty() {
            index.remove(path_str.as_bytes());
        } else {
            let oid = odb.write(ObjectKind::Blob, blended.as_bytes())?;
            if let Some(ent) = index.get_mut(path_str.as_bytes(), 0) {
                ent.oid = oid;
                ent.size = blended.len() as u32;
            }
        }
    }
    Ok(())
}

fn write_index_blob_and_mode(
    odb: &Odb,
    index: &mut Index,
    path_str: &str,
    abs_path: &Path,
    blob_data: &[u8],
    mode: u32,
) -> Result<()> {
    let oid = odb.write(ObjectKind::Blob, blob_data)?;
    let meta = fs::symlink_metadata(abs_path).ok();
    let mut new_ent = if let Some(m) = meta.as_ref() {
        let mut e = entry_from_metadata(m, path_str.as_bytes(), oid, mode);
        e.mode = mode;
        e
    } else {
        IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size: blob_data.len() as u32,
            oid,
            flags: path_str.len().min(0xFFF) as u16,
            flags_extended: None,
            path: path_str.as_bytes().to_vec(),
            base_index_pos: 0,
        }
    };
    new_ent.set_intent_to_add(false);
    new_ent.set_assume_unchanged(false);
    new_ent.set_skip_worktree(false);
    index.stage_file(new_ent);
    Ok(())
}

fn edit_worktree_via_editor(content: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().context("temp file for add -p edit")?;
    f.as_file_mut().write_all(content)?;
    f.flush()?;
    let path = f.path().to_owned();
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$1\"", editor))
        .arg("sh")
        .arg(&path)
        .status()
        .context("running editor")?;
    if !status.success() {
        bail!("editor failed");
    }
    fs::read(&path).context("reading edited file")
}
