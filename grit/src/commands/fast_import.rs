//! `grit fast-import` — import from a fast-export stream.
//!
//! Supports a subset used by the test harness: `blob`, `commit` with
//! `author`/`committer`/`data <<EOF`/`from`/`M <mode> inline` file lines, and
//! `done`. This is enough for `test_commit_bulk` (via `git fast-import`).

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{
    parse_tree, serialize_commit, serialize_tree, tree_entry_cmp, CommitData, ObjectId, ObjectKind,
    TreeEntry,
};
use grit_lib::odb::Odb;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse;
use std::collections::HashMap;
use std::io::{self, BufRead};

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments (reserved for future import options).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-import`.
pub fn run(_args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    // Last commit OID written for each `commit <ref>` ref during this import
    // (matches git fast-import chaining when `from` is omitted).
    let mut branch_tips: HashMap<String, ObjectId> = HashMap::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "done" {
            break;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "blob" {
            read_blob_command(&repo.odb, &mut reader)?;
            continue;
        }
        if let Some(refname) = trimmed.strip_prefix("commit ") {
            read_commit_command(&repo, refname, &mut branch_tips, &mut reader)?;
            continue;
        }
        bail!("unsupported fast-import command: {trimmed}");
    }
    Ok(())
}

fn read_blob_command(odb: &Odb, reader: &mut impl BufRead) -> Result<()> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("reading data line after blob")?;
    let data_line = line.trim_end();
    let rest = data_line
        .strip_prefix("data ")
        .ok_or_else(|| anyhow::anyhow!("expected 'data <n>' after blob"))?;
    let size: usize = rest
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid data size: {e}"))?;
    let mut payload = vec![0u8; size];
    reader
        .read_exact(&mut payload)
        .context("reading blob payload")?;
    odb.write(ObjectKind::Blob, &payload)?;
    Ok(())
}

fn read_commit_command(
    repo: &Repository,
    refname: &str,
    branch_tips: &mut HashMap<String, ObjectId>,
    reader: &mut impl BufRead,
) -> Result<()> {
    let mut author = String::new();
    let mut committer = String::new();
    let mut message = Vec::new();
    let mut from_spec: Option<String> = None;
    let mut file_change: Option<(u32, Vec<u8>, Vec<u8>)> = None;

    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("unexpected EOF in fast-import commit");
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("author ") {
            author = rest.to_owned();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("committer ") {
            committer = rest.to_owned();
            continue;
        }
        if trimmed == "data <<EOF" {
            message = read_heredoc(reader, "EOF")?;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("from ") {
            from_spec = Some(rest.to_owned());
            continue;
        }
        if let Some((mode, path)) = parse_inline_modify(trimmed) {
            line.clear();
            reader
                .read_line(&mut line)
                .context("reading data line after M")?;
            let data_line = line.trim_end();
            if data_line != "data <<EOF" {
                bail!("expected 'data <<EOF' after inline modify, got: {data_line}");
            }
            let contents = read_heredoc(reader, "EOF")?;
            file_change = Some((mode, path, contents));
            continue;
        }
        bail!("unsupported line in fast-import commit: {trimmed}");
    }

    if author.is_empty() || committer.is_empty() {
        bail!("commit missing author or committer");
    }

    let parent_oid = if let Some(spec) = from_spec {
        let spec = spec.trim();
        let spec = spec.strip_suffix("^0").unwrap_or(spec);
        Some(rev_parse::resolve_revision(repo, spec).context("resolving fast-import 'from'")?)
    } else {
        branch_tips.get(refname).copied()
    };

    let empty_tree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        .parse::<ObjectId>()
        .map_err(|_| anyhow::anyhow!("invalid empty tree id"))?;

    let base_tree = if let Some(p) = parent_oid {
        let obj = repo.odb.read(&p).context("reading parent commit")?;
        if obj.kind != ObjectKind::Commit {
            bail!("fast-import 'from' is not a commit");
        }
        let c = grit_lib::objects::parse_commit(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))?;
        c.tree
    } else {
        empty_tree
    };

    let tree_oid = if let Some((mode, path, contents)) = file_change {
        let blob_oid = repo.odb.write(ObjectKind::Blob, &contents)?;
        merge_blob_into_tree(&repo.odb, base_tree, &path, mode, blob_oid)?
    } else {
        base_tree
    };

    let mut parents = Vec::new();
    if let Some(p) = parent_oid {
        parents.push(p);
    }

    let commit = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding: None,
        message: String::from_utf8_lossy(&message).into_owned(),
        raw_message: None,
    };
    let raw = serialize_commit(&commit);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &raw)?;

    update_fast_import_ref(repo, refname, &commit_oid)?;

    branch_tips.insert(refname.to_owned(), commit_oid);

    Ok(())
}

fn parse_inline_modify(line: &str) -> Option<(u32, Vec<u8>)> {
    let rest = line.strip_prefix("M ")?;
    let (mode_s, rest) = rest.split_once(' ')?;
    let mode = u32::from_str_radix(mode_s, 8).ok()?;
    let rest = rest.strip_prefix("inline ")?;
    Some((mode, rest.as_bytes().to_vec()))
}

fn read_heredoc(reader: &mut impl BufRead, marker: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("unexpected EOF in heredoc (expected '{marker}')");
        }
        if line.trim_end() == marker {
            break;
        }
        out.extend_from_slice(line.as_bytes());
    }
    Ok(out)
}

fn merge_blob_into_tree(
    odb: &Odb,
    base_tree: ObjectId,
    path: &[u8],
    mode: u32,
    blob_oid: ObjectId,
) -> Result<ObjectId> {
    let components: Vec<&[u8]> = path
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect();
    if components.is_empty() {
        bail!("empty path in fast-import tree update");
    }
    let new_root = merge_blob_at_components(odb, base_tree, &components, mode, blob_oid)?;
    Ok(new_root)
}

fn merge_blob_at_components(
    odb: &Odb,
    tree_oid: ObjectId,
    components: &[&[u8]],
    mode: u32,
    blob_oid: ObjectId,
) -> Result<ObjectId> {
    let mut entries = if tree_oid.to_hex() == "4b825dc642cb6eb9a060e54bf8d69288fbee4904" {
        Vec::new()
    } else {
        let obj = odb.read(&tree_oid).context("read tree for merge")?;
        if obj.kind != ObjectKind::Tree {
            bail!("expected tree object");
        }
        parse_tree(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))?
    };

    if components.len() == 1 {
        let name = components[0].to_vec();
        entries.retain(|e| e.name != name);
        entries.push(TreeEntry {
            mode,
            name: name.clone(),
            oid: blob_oid,
        });
        entries.sort_by(|a, b| {
            tree_entry_cmp(&a.name, a.mode == 0o040000, &b.name, b.mode == 0o040000)
        });
        let data = serialize_tree(&entries);
        return Ok(odb.write(ObjectKind::Tree, &data)?);
    }

    let dir_name = components[0].to_vec();
    let sub_path = &components[1..];

    let mut sub_tree_oid = None;
    let mut idx_remove = None;
    for (i, e) in entries.iter().enumerate() {
        if e.name == dir_name && e.mode == 0o040000 {
            sub_tree_oid = Some(e.oid);
            idx_remove = Some(i);
            break;
        }
    }
    if let Some(i) = idx_remove {
        entries.remove(i);
    }
    let child_base = sub_tree_oid.unwrap_or_else(|| {
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
            .parse()
            .expect("empty tree")
    });
    let new_child = merge_blob_at_components(odb, child_base, sub_path, mode, blob_oid)?;
    entries.push(TreeEntry {
        mode: 0o040000,
        name: dir_name,
        oid: new_child,
    });
    entries
        .sort_by(|a, b| tree_entry_cmp(&a.name, a.mode == 0o040000, &b.name, b.mode == 0o040000));
    let data = serialize_tree(&entries);
    Ok(odb.write(ObjectKind::Tree, &data)?)
}

fn update_fast_import_ref(repo: &Repository, refname: &str, oid: &ObjectId) -> Result<()> {
    let git_dir = &repo.git_dir;
    if refname == "HEAD" {
        match refs::read_head(git_dir)? {
            Some(target) if target.starts_with("refs/") => {
                refs::write_ref(git_dir, &target, oid)?;
            }
            _ => {
                let head_path = git_dir.join("HEAD");
                let content = format!("{oid}\n");
                std::fs::write(&head_path, content).context("write detached HEAD")?;
            }
        }
        return Ok(());
    }
    let full = if refname.starts_with("refs/") {
        refname.to_owned()
    } else {
        format!("refs/heads/{refname}")
    };
    refs::write_ref(git_dir, &full, oid)?;
    Ok(())
}
