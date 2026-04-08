//! Minimal [`git fast-import`](https://git-scm.com/docs/git-fast-import) stream support.
//!
//! Handles the subset of commands used by upstream tests: `blob` (with optional
//! `mark`), `commit` (with `author`/`committer`, `data`, optional `from`, and
//! `M` / `D` file commands), `reset`, `done`, and comment lines.

use std::collections::HashMap;
use std::io::BufRead;

use crate::error::{Error, Result};
use crate::index::{Index, IndexEntry, MODE_GITLINK, MODE_REGULAR, MODE_TREE};
use crate::objects::{
    parse_commit, serialize_commit, serialize_tag, CommitData, ObjectId, ObjectKind, TagData,
};
use crate::refs::write_ref;
use crate::repo::Repository;
use crate::rev_parse::resolve_revision;
use crate::write_tree::write_tree_from_index;

/// Import objects and refs from a fast-import stream read from `reader`.
///
/// # Errors
///
/// Returns [`Error`] variants for I/O, corrupt stream input, or missing marks/refs.
pub fn import_stream(repo: &Repository, mut reader: impl BufRead) -> Result<()> {
    let mut imp = Importer {
        repo,
        marks: HashMap::new(),
        branch_tips: HashMap::new(),
        feature_done: false,
        stashed_line: None,
        pending_byte: None,
        reader: &mut reader,
    };
    imp.run()
}

struct Importer<'a, R: BufRead> {
    repo: &'a Repository,
    marks: HashMap<u32, ObjectId>,
    branch_tips: HashMap<String, ObjectId>,
    /// When set, a terminating `done` command is required before EOF.
    feature_done: bool,
    /// Line read too far while parsing a `commit` or `reset`; next top-level command.
    stashed_line: Option<String>,
    /// Byte read while handling optional `LF` after a `data` block; must precede next line.
    pending_byte: Option<u8>,
    reader: &'a mut R,
}

impl<'a, R: BufRead> Importer<'a, R> {
    fn run(&mut self) -> Result<()> {
        loop {
            let line = match self.next_command_line()? {
                Some(l) => l,
                None => break,
            };
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
            if let Some(rest) = trimmed.strip_prefix("feature ") {
                let name = rest.trim();
                if name == "done" {
                    self.feature_done = true;
                }
                continue;
            }
            if trimmed == "blob" {
                self.read_blob()?;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("commit ") {
                let refname = rest.trim().to_string();
                self.read_commit(&refname)?;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("reset ") {
                let refname = rest.trim().to_string();
                self.read_reset(&refname)?;
                continue;
            }
            if trimmed.starts_with("tag ") {
                let name = trimmed["tag ".len()..].trim().to_string();
                self.read_tag(&name)?;
                continue;
            }
            return Err(Error::IndexError(format!(
                "fast-import: unsupported command: {trimmed}"
            )));
        }
        if self.feature_done {
            return Err(Error::IndexError(
                "fast-import: stream ended before required \"done\" command".to_owned(),
            ));
        }
        Ok(())
    }

    fn next_command_line(&mut self) -> Result<Option<String>> {
        if let Some(l) = self.stashed_line.take() {
            return Ok(Some(l));
        }
        self.read_line_nonempty()
    }

    fn read_line_nonempty(&mut self) -> Result<Option<String>> {
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = self.read_line_into(&mut buf)?;
            if n == 0 {
                return Ok(None);
            }
            if !buf.trim().is_empty() {
                return Ok(Some(buf));
            }
        }
    }

    fn read_line_any(&mut self) -> Result<Option<String>> {
        let mut buf = String::new();
        let n = self.read_line_into(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(buf))
    }

    fn read_line_into(&mut self, buf: &mut String) -> Result<usize> {
        buf.clear();
        if let Some(b) = self.pending_byte.take() {
            if b == b'\n' {
                buf.push('\n');
                return Ok(1);
            }
            buf.push(char::from(b));
        }
        let prev = buf.len();
        let n = self.reader.read_line(buf).map_err(Error::Io)?;
        Ok(prev + n)
    }

    fn read_blob(&mut self) -> Result<()> {
        let mut mark: Option<u32> = None;
        loop {
            let line = self.read_line_nonempty()?.ok_or_else(|| {
                Error::IndexError("fast-import: unexpected EOF in blob".to_owned())
            })?;
            let t = line.trim_end();
            if let Some(id) = t.strip_prefix("mark :") {
                mark = Some(
                    id.parse()
                        .map_err(|_| Error::IndexError(format!("fast-import: bad mark: {t}")))?,
                );
                continue;
            }
            if t.starts_with("original-oid ") {
                continue;
            }
            let rest = t.strip_prefix("data ").ok_or_else(|| {
                Error::IndexError(format!("fast-import: expected data line in blob, got: {t}"))
            })?;
            let size: usize = rest.parse().map_err(|_| {
                Error::IndexError(format!("fast-import: invalid data size: {rest}"))
            })?;
            let mut payload = vec![0u8; size];
            self.reader
                .read_exact(&mut payload)
                .map_err(|_| Error::IndexError("fast-import: truncated blob data".to_owned()))?;
            self.consume_optional_lf_after_data()?;
            let oid = self.repo.odb.write(ObjectKind::Blob, &payload)?;
            if let Some(m) = mark {
                self.marks.insert(m, oid);
            }
            return Ok(());
        }
    }

    /// After `data` payload, an extra LF is optional (see git-fast-import docs).
    fn consume_optional_lf_after_data(&mut self) -> Result<()> {
        let mut one = [0u8; 1];
        match self.reader.read(&mut one) {
            Ok(0) => Ok(()),
            Ok(1) => {
                if one[0] != b'\n' {
                    self.pending_byte = Some(one[0]);
                }
                Ok(())
            }
            Ok(_) => unreachable!(),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn read_commit(&mut self, refname: &str) -> Result<()> {
        let mut mark: Option<u32> = None;
        let mut author: Option<String> = None;
        let mut committer: Option<String> = None;

        loop {
            let line = self.read_line_nonempty()?.ok_or_else(|| {
                Error::IndexError("fast-import: unexpected EOF in commit".to_owned())
            })?;
            let t = line.trim_end();
            if let Some(id) = t.strip_prefix("mark :") {
                mark = Some(
                    id.parse()
                        .map_err(|_| Error::IndexError(format!("fast-import: bad mark: {t}")))?,
                );
                continue;
            }
            if t.starts_with("original-oid ") {
                continue;
            }
            if let Some(rest) = t.strip_prefix("author ") {
                author = Some(rest.to_owned());
                continue;
            }
            if let Some(rest) = t.strip_prefix("committer ") {
                committer = Some(rest.to_owned());
                continue;
            }
            if t.starts_with("gpgsig ") || t.starts_with("encoding ") {
                return Err(Error::IndexError(format!(
                    "fast-import: unsupported commit header: {t}"
                )));
            }
            if t.starts_with("data ") {
                // Re-parse: we already have full line with "data N".
                let rest = t.strip_prefix("data ").unwrap();
                let size: usize = rest.parse().map_err(|_| {
                    Error::IndexError(format!("fast-import: invalid data size: {rest}"))
                })?;
                let mut message = vec![0u8; size];
                self.reader.read_exact(&mut message).map_err(|_| {
                    Error::IndexError("fast-import: truncated commit message".to_owned())
                })?;
                self.consume_optional_lf_after_data()?;
                let committer = committer.ok_or_else(|| {
                    Error::IndexError("fast-import: commit missing committer".to_owned())
                })?;
                let author = author.unwrap_or_else(|| committer.clone());
                self.finish_commit(refname, mark, author, committer, message)?;
                return Ok(());
            }
            return Err(Error::IndexError(format!(
                "fast-import: unexpected in commit before message: {t}"
            )));
        }
    }

    fn finish_commit(
        &mut self,
        refname: &str,
        mark: Option<u32>,
        author: String,
        committer: String,
        message: Vec<u8>,
    ) -> Result<()> {
        let mut from_oid: Option<ObjectId> = None;
        let mut modifications: Vec<(u32, ObjectId, Vec<u8>)> = Vec::new();
        let mut deletions: Vec<Vec<u8>> = Vec::new();

        loop {
            let Some(line) = self.read_line_any()? else {
                break;
            };
            let t = line.trim_end();
            if t.is_empty() {
                continue;
            }
            if t.starts_with("from ") {
                let spec = t["from ".len()..].trim();
                from_oid = Some(self.resolve_commit_ish(spec)?);
                continue;
            }
            if t.starts_with("merge ") {
                return Err(Error::IndexError(
                    "fast-import: merge commits not supported".to_owned(),
                ));
            }
            if let Some(rest) = t.strip_prefix("M ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err(Error::IndexError(format!("fast-import: bad M line: {t}")));
                }
                let mode = u32::from_str_radix(parts[0], 8).map_err(|_| {
                    Error::IndexError(format!("fast-import: bad file mode: {}", parts[0]))
                })?;
                let blob_ref = parts[1];
                let path = parts[2].as_bytes().to_vec();
                let blob_oid = self.resolve_blob_ref(blob_ref)?;
                modifications.push((mode, blob_oid, path));
                continue;
            }
            if let Some(rest) = t.strip_prefix("D ") {
                deletions.push(rest.as_bytes().to_vec());
                continue;
            }
            self.stashed_line = Some(line);
            break;
        }

        let empty_tree: ObjectId = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
            .parse()
            .map_err(|_| Error::IndexError("fast-import: empty tree oid".to_owned()))?;

        let (parent_tree, parents) = match from_oid {
            Some(oid) => {
                let obj = self.repo.odb.read(&oid)?;
                if obj.kind != ObjectKind::Commit {
                    return Err(Error::IndexError(format!(
                        "fast-import: from {oid} is not a commit"
                    )));
                }
                let c = parse_commit(&obj.data)?;
                (c.tree, vec![oid])
            }
            None => {
                if let Some(tip) = self.branch_tips.get(refname).copied() {
                    let obj = self.repo.odb.read(&tip)?;
                    if obj.kind != ObjectKind::Commit {
                        return Err(Error::IndexError(format!(
                            "fast-import: branch tip {tip} is not a commit"
                        )));
                    }
                    let c = parse_commit(&obj.data)?;
                    (c.tree, vec![tip])
                } else {
                    (empty_tree, Vec::new())
                }
            }
        };

        let mut index = tree_to_index(&self.repo.odb, &parent_tree)?;
        for path in deletions {
            index.entries.retain(|e| e.path != path);
        }
        for (mode, blob_oid, path) in modifications {
            let mode = normalize_mode(mode)?;
            index.add_or_replace(index_entry(path, mode, blob_oid));
        }
        let tree_oid = write_tree_from_index(&self.repo.odb, &index, "")?;

        let message_str = String::from_utf8_lossy(&message).into_owned();
        let raw_message = (!message.is_empty() && std::str::from_utf8(&message).is_err())
            .then_some(message.clone());

        let commit = CommitData {
            tree: tree_oid,
            parents,
            author,
            committer,
            encoding: None,
            message: message_str,
            raw_message,
        };
        let bytes = serialize_commit(&commit);
        let commit_oid = self.repo.odb.write(ObjectKind::Commit, &bytes)?;

        if let Some(m) = mark {
            self.marks.insert(m, commit_oid);
        }
        self.branch_tips.insert(refname.to_string(), commit_oid);
        write_ref(&self.repo.git_dir, refname, &commit_oid)?;
        Ok(())
    }

    fn resolve_commit_ish(&self, spec: &str) -> Result<ObjectId> {
        if let Some(rest) = spec.strip_prefix(':') {
            let id: u32 = rest
                .parse()
                .map_err(|_| Error::IndexError(format!("fast-import: bad mark ref: {spec}")))?;
            return self
                .marks
                .get(&id)
                .copied()
                .ok_or_else(|| Error::IndexError(format!("fast-import: unknown mark :{id}")));
        }
        if spec.len() == 40 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
            return spec.parse();
        }
        resolve_revision(self.repo, spec)
    }

    fn resolve_blob_ref(&self, spec: &str) -> Result<ObjectId> {
        if let Some(rest) = spec.strip_prefix(':') {
            let id: u32 = rest
                .parse()
                .map_err(|_| Error::IndexError(format!("fast-import: bad mark ref: {spec}")))?;
            return self
                .marks
                .get(&id)
                .copied()
                .ok_or_else(|| Error::IndexError(format!("fast-import: unknown mark :{id}")));
        }
        if spec.len() == 40 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
            return spec.parse();
        }
        Err(Error::IndexError(format!(
            "fast-import: unsupported blob ref: {spec}"
        )))
    }

    fn read_tag(&mut self, short_name: &str) -> Result<()> {
        let mut mark: Option<u32> = None;
        let mut from_oid: Option<ObjectId> = None;
        let mut tagger: Option<String> = None;

        loop {
            let line = self.read_line_nonempty()?.ok_or_else(|| {
                Error::IndexError("fast-import: unexpected EOF in tag".to_owned())
            })?;
            let t = line.trim_end();
            if let Some(id) = t.strip_prefix("mark :") {
                mark = Some(
                    id.parse()
                        .map_err(|_| Error::IndexError(format!("fast-import: bad mark: {t}")))?,
                );
                continue;
            }
            if t.starts_with("original-oid ") {
                continue;
            }
            if let Some(rest) = t.strip_prefix("from ") {
                let spec = rest.trim();
                from_oid = Some(self.resolve_commit_ish(spec)?);
                continue;
            }
            if let Some(rest) = t.strip_prefix("tagger ") {
                tagger = Some(rest.to_owned());
                continue;
            }
            if t.starts_with("data ") {
                let rest = t.strip_prefix("data ").unwrap();
                let size: usize = rest.parse().map_err(|_| {
                    Error::IndexError(format!("fast-import: invalid data size: {rest}"))
                })?;
                let mut message = vec![0u8; size];
                self.reader.read_exact(&mut message).map_err(|_| {
                    Error::IndexError("fast-import: truncated tag message".to_owned())
                })?;
                self.consume_optional_lf_after_data()?;

                let target = from_oid
                    .ok_or_else(|| Error::IndexError("fast-import: tag missing from".to_owned()))?;
                let target_obj = self.repo.odb.read(&target)?;
                let object_type = target_obj.kind.as_str().to_owned();
                let msg_str = String::from_utf8_lossy(&message).into_owned();

                let tag_data = TagData {
                    object: target,
                    object_type,
                    tag: short_name.to_owned(),
                    tagger,
                    message: msg_str,
                };
                let bytes = serialize_tag(&tag_data);
                let tag_oid = self.repo.odb.write(ObjectKind::Tag, &bytes)?;

                if let Some(m) = mark {
                    self.marks.insert(m, tag_oid);
                }

                let full_ref = format!("refs/tags/{short_name}");
                write_ref(&self.repo.git_dir, &full_ref, &tag_oid)?;
                return Ok(());
            }
            return Err(Error::IndexError(format!(
                "fast-import: unexpected in tag: {t}"
            )));
        }
    }

    fn read_reset(&mut self, refname: &str) -> Result<()> {
        let Some(line) = self.read_line_any()? else {
            return Ok(());
        };
        let t = line.trim_end();
        if t.is_empty() {
            return Ok(());
        }
        if let Some(spec) = t.strip_prefix("from ") {
            let oid = self.resolve_commit_ish(spec.trim())?;
            self.branch_tips.insert(refname.to_string(), oid);
            write_ref(&self.repo.git_dir, refname, &oid)?;
            return Ok(());
        }
        self.stashed_line = Some(line);
        Ok(())
    }
}

fn normalize_mode(mode: u32) -> Result<u32> {
    match mode {
        0o100644 | 0o644 => Ok(MODE_REGULAR),
        0o100755 | 0o755 => Ok(crate::index::MODE_EXECUTABLE),
        0o120000 => Ok(crate::index::MODE_SYMLINK),
        0o160000 => Ok(MODE_GITLINK),
        0o040000 => Ok(MODE_TREE),
        _ => Err(Error::IndexError(format!(
            "fast-import: unsupported mode {mode:o}"
        ))),
    }
}

fn index_entry(path: Vec<u8>, mode: u32, oid: ObjectId) -> IndexEntry {
    let path_len = path.len().min(0xFFF) as u16;
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
        size: 0,
        oid,
        flags: path_len,
        flags_extended: Some(0),
        path,
    }
}

fn tree_to_index(odb: &crate::odb::Odb, tree_oid: &ObjectId) -> Result<Index> {
    let obj = odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Err(Error::IndexError(format!("expected tree at {tree_oid}")));
    }
    let entries = crate::objects::parse_tree(&obj.data)?;
    let mut index = Index::new();
    for te in entries {
        let path = te.name;
        if te.mode == MODE_TREE {
            let sub = tree_to_index(odb, &te.oid)?;
            for mut e in sub.entries {
                let mut full = path.clone();
                full.push(b'/');
                full.extend_from_slice(&e.path);
                e.path = full;
                let pl = e.path.len().min(0xFFF) as u16;
                e.flags = pl;
                index.add_or_replace(e);
            }
        } else {
            index.add_or_replace(index_entry(path, te.mode, te.oid));
        }
    }
    Ok(index)
}
