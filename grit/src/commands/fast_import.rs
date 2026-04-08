//! `grit fast-import` — import from a fast-export stream.
//!
//! Supports enough of the fast-import format for upstream harness fixtures:
//! `blob` (with optional `mark`), `commit`, `merge`, `from`, `M`/`D` file
//! commands, `reset`, and `done`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{
    parse_tree, serialize_commit, serialize_tree, tree_entry_cmp, CommitData, ObjectId, ObjectKind,
    TreeEntry,
};
use grit_lib::odb::Odb;
use grit_lib::refs::write_ref;
use grit_lib::repo::Repository;
use std::collections::HashMap;
use std::io::{self, BufRead, Read};

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments (reserved for future import options).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Clone, Copy)]
enum MarkId {
    Blob(ObjectId),
    Commit(ObjectId),
}

const EMPTY_TREE_HEX: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

fn empty_tree_oid() -> ObjectId {
    EMPTY_TREE_HEX.parse().expect("valid empty tree oid")
}

fn read_exact_payload<R: Read>(reader: &mut R, size: usize) -> Result<Vec<u8>> {
    let mut payload = vec![0u8; size];
    reader
        .read_exact(&mut payload)
        .context("reading fast-import payload")?;
    Ok(payload)
}

fn parse_mark_line(line: &str) -> Option<u32> {
    let t = line.trim();
    let rest = t.strip_prefix("mark :")?;
    rest.parse().ok()
}

fn parse_fast_import_mode(mode_str: &str) -> Result<u32> {
    u32::from_str_radix(mode_str, 8).map_err(|e| anyhow::anyhow!("invalid file mode: {e}"))
}

fn tree_for_commit(odb: &Odb, oid: &ObjectId) -> Result<ObjectId> {
    let obj = odb.read(oid).context("read commit for tree")?;
    if obj.kind != ObjectKind::Commit {
        anyhow::bail!("mark does not point to a commit");
    }
    let c = grit_lib::objects::parse_commit(&obj.data)?;
    Ok(c.tree)
}

fn read_tree_entries(odb: &Odb, tree_oid: &ObjectId) -> Result<Vec<TreeEntry>> {
    if *tree_oid == empty_tree_oid() {
        return Ok(Vec::new());
    }
    let obj = odb.read(tree_oid).context("read tree")?;
    if obj.kind != ObjectKind::Tree {
        anyhow::bail!("expected tree object");
    }
    parse_tree(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))
}

fn apply_fileops(
    odb: &Odb,
    base_tree: &ObjectId,
    marks: &HashMap<u32, MarkId>,
    ops: &[String],
) -> Result<ObjectId> {
    let mut entries = read_tree_entries(odb, base_tree)?;
    for op in ops {
        let parts: Vec<&str> = op.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "D" => {
                let path = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("D missing path"))?;
                entries.retain(|e| e.name != path.as_bytes());
            }
            "M" => {
                let mode = parse_fast_import_mode(
                    parts
                        .get(1)
                        .ok_or_else(|| anyhow::anyhow!("M missing mode"))?,
                )?;
                let blob_spec = parts
                    .get(2)
                    .ok_or_else(|| anyhow::anyhow!("M missing blob"))?;
                let path = parts
                    .get(3)
                    .ok_or_else(|| anyhow::anyhow!("M missing path"))?;
                let oid = if let Some(rest) = blob_spec.strip_prefix(':') {
                    let id: u32 = rest.parse().context("parse blob mark")?;
                    match marks.get(&id).copied() {
                        Some(MarkId::Blob(b)) => b,
                        _ => anyhow::bail!("unknown blob mark {id}"),
                    }
                } else {
                    blob_spec.parse().context("parse blob oid")?
                };
                entries.retain(|e| e.name != path.as_bytes());
                entries.push(TreeEntry {
                    mode,
                    name: path.as_bytes().to_vec(),
                    oid,
                });
            }
            other => anyhow::bail!("unsupported fast-import file command: {other}"),
        }
    }
    entries
        .sort_by(|a, b| tree_entry_cmp(&a.name, a.mode == 0o040000, &b.name, b.mode == 0o040000));
    let data = serialize_tree(&entries);
    odb.write(ObjectKind::Tree, &data)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn is_top_level_command(line: &str) -> bool {
    let t = line.trim();
    t == "blob" || t.starts_with("commit ") || t.starts_with("reset ") || t == "done"
}

/// Run `grit fast-import`.
pub fn run(_args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut marks: HashMap<u32, MarkId> = HashMap::new();
    let mut line = String::new();
    let mut peek: Option<String> = None;

    let next_line = |r: &mut io::StdinLock<'_>,
                     peek_buf: &mut Option<String>,
                     buf: &mut String|
     -> Result<String> {
        if let Some(taken) = peek_buf.take() {
            return Ok(taken);
        }
        loop {
            buf.clear();
            if r.read_line(buf)? == 0 {
                anyhow::bail!("unexpected EOF");
            }
            let t = buf.trim_end().to_string();
            if !t.is_empty() {
                return Ok(t);
            }
        }
    };

    loop {
        let trimmed = if let Some(p) = peek.take() {
            p
        } else {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let t = line.trim_end().to_string();
            if t.is_empty() {
                continue;
            }
            t
        };

        if trimmed == "done" {
            break;
        }
        if trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "blob" {
            let mut mark: Option<u32> = None;
            loop {
                let l = next_line(&mut reader, &mut peek, &mut line)?;
                if let Some(m) = parse_mark_line(&l) {
                    mark = Some(m);
                    continue;
                }
                if l.starts_with("data ") {
                    let size: usize = l["data ".len()..]
                        .parse()
                        .map_err(|e| anyhow::anyhow!("invalid data size: {e}"))?;
                    let payload = read_exact_payload(&mut reader, size)?;
                    let oid = repo.odb.write(ObjectKind::Blob, &payload)?;
                    if let Some(m) = mark {
                        marks.insert(m, MarkId::Blob(oid));
                    }
                    // One `blob` command is `mark?` + single `data`; stream may insert LF after payload.
                    let mut nl = [0u8; 1];
                    let _ = reader.read(&mut nl);
                    break;
                }
                anyhow::bail!("unexpected line in blob: {l}");
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("reset ") {
            let refname = rest.trim().to_string();
            let mut from_oid: Option<ObjectId> = None;
            loop {
                let l = if let Some(p) = peek.take() {
                    p
                } else {
                    line.clear();
                    if reader.read_line(&mut line)? == 0 {
                        break;
                    }
                    let t = line.trim_end().to_string();
                    if t.is_empty() {
                        continue;
                    }
                    t
                };
                if l.starts_with("from ") {
                    let spec = l["from ".len()..].trim();
                    if let Some(id_str) = spec.strip_prefix(':') {
                        let id: u32 = id_str.parse().context("parse reset from mark")?;
                        let MarkId::Commit(oid) = marks
                            .get(&id)
                            .copied()
                            .ok_or_else(|| anyhow::anyhow!("unknown reset from mark {id}"))?
                        else {
                            anyhow::bail!("reset from mark {id} is not a commit");
                        };
                        from_oid = Some(oid);
                    } else {
                        from_oid = Some(spec.parse().context("parse reset from oid")?);
                    }
                    continue;
                }
                peek = Some(l);
                break;
            }
            if let Some(oid) = from_oid {
                write_ref(&repo.git_dir, &refname, &oid).context("write ref from reset")?;
            }
            continue;
        }

        let Some(rest) = trimmed.strip_prefix("commit ") else {
            return Err(anyhow::anyhow!(
                "unsupported fast-import command: {trimmed}"
            ));
        };
        let refname = rest.trim().to_string();

        let mut mark: Option<u32> = None;
        let mut author: Option<String> = None;
        let mut committer: Option<String> = None;

        loop {
            let l = next_line(&mut reader, &mut peek, &mut line)?;
            if let Some(m) = parse_mark_line(&l) {
                mark = Some(m);
                continue;
            }
            if let Some(a) = l.strip_prefix("author ") {
                author = Some(a.to_owned());
                continue;
            }
            if let Some(c) = l.strip_prefix("committer ") {
                committer = Some(c.to_owned());
                continue;
            }
            if l.starts_with("data ") {
                let size: usize = l["data ".len()..]
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid data size: {e}"))?;
                let msg = read_exact_payload(&mut reader, size)?;
                let author = author.ok_or_else(|| anyhow::anyhow!("commit missing author"))?;
                let committer =
                    committer.ok_or_else(|| anyhow::anyhow!("commit missing committer"))?;

                let mut parents: Vec<ObjectId> = Vec::new();
                let mut fileops: Vec<String> = Vec::new();

                loop {
                    let cmd = next_line(&mut reader, &mut peek, &mut line)?;
                    if is_top_level_command(&cmd) {
                        peek = Some(cmd);
                        break;
                    }
                    if let Some(rest) = cmd.strip_prefix("from ") {
                        let spec = rest.trim();
                        let id: u32 = spec
                            .strip_prefix(':')
                            .ok_or_else(|| anyhow::anyhow!("from mark"))?
                            .parse()
                            .context("parse from mark")?;
                        let MarkId::Commit(oid) = marks
                            .get(&id)
                            .copied()
                            .ok_or_else(|| anyhow::anyhow!("unknown from mark {id}"))?
                        else {
                            anyhow::bail!("from mark {id} is not a commit");
                        };
                        parents.push(oid);
                        continue;
                    }
                    if let Some(rest) = cmd.strip_prefix("merge ") {
                        let spec = rest.trim();
                        let id: u32 = spec
                            .strip_prefix(':')
                            .ok_or_else(|| anyhow::anyhow!("merge mark"))?
                            .parse()
                            .context("parse merge mark")?;
                        let MarkId::Commit(oid) = marks
                            .get(&id)
                            .copied()
                            .ok_or_else(|| anyhow::anyhow!("unknown merge mark {id}"))?
                        else {
                            anyhow::bail!("merge mark {id} is not a commit");
                        };
                        parents.push(oid);
                        continue;
                    }
                    if cmd.starts_with('M') || cmd.starts_with('D') {
                        fileops.push(cmd);
                        continue;
                    }
                    anyhow::bail!("unexpected commit trailer line: {cmd}");
                }

                let base_tree = if let Some(p0) = parents.first() {
                    tree_for_commit(&repo.odb, p0)?
                } else {
                    empty_tree_oid()
                };
                let tree_oid = apply_fileops(&repo.odb, &base_tree, &marks, &fileops)?;

                let commit = CommitData {
                    tree: tree_oid,
                    parents: parents.clone(),
                    author,
                    committer,
                    encoding: None,
                    message: String::from_utf8_lossy(&msg).into_owned(),
                    raw_message: Some(msg),
                };
                let raw = serialize_commit(&commit);
                let commit_oid = repo.odb.write(ObjectKind::Commit, &raw)?;
                if let Some(m) = mark {
                    marks.insert(m, MarkId::Commit(commit_oid));
                }
                write_ref(&repo.git_dir, &refname, &commit_oid)
                    .with_context(|| format!("write ref {refname}"))?;
                break;
            }
            anyhow::bail!("unexpected commit header line: {l}");
        }
    }

    Ok(())
}
