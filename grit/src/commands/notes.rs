//! `grit notes` — add, show, list, remove, append, and merge object notes.
//!
//! Notes are stored as blobs in a tree referenced by `refs/notes/commits`
//! (or a custom namespace via `--ref`).  Each entry in the notes tree is
//! named by the full hex SHA of the annotated object.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use merge3::{Merge3, StandardMarkers};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use grit_lib::config::ConfigSet;
use grit_lib::diff::{diff_trees, zero_oid, DiffStatus};
use grit_lib::merge_base::merge_bases_first_vs_rest;
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, serialize_tree, tree_entry_cmp, CommitData,
    ObjectId, ObjectKind, TreeEntry,
};
use grit_lib::refs::{
    common_dir, delete_ref, read_symbolic_ref, resolve_ref, write_ref, write_symbolic_ref,
};

use crate::commands::worktree_refs;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;

use std::io::{self, Read, Write};
use time::OffsetDateTime;

/// Arguments for `grit notes`.
#[derive(Debug, ClapArgs)]
#[command(about = "Add or inspect object notes")]
pub struct Args {
    /// Use notes ref <ref> instead of refs/notes/commits.
    #[arg(long = "ref", global = true)]
    pub notes_ref: Option<String>,

    #[command(subcommand)]
    pub command: Option<NotesSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum NotesSubcommand {
    /// List notes.
    List {
        /// Object to list notes for (if omitted, list all notes).
        #[arg()]
        object: Option<String>,
    },
    /// Add a note to an object.
    Add {
        /// Note message.
        #[arg(short = 'm', long = "message")]
        message: Option<String>,

        /// Read note message from file ('-' for stdin).
        #[arg(short = 'F', long = "file", value_name = "FILE")]
        file: Option<std::path::PathBuf>,

        /// Reuse an existing blob object as the note.
        #[arg(short = 'C', long = "reuse-message", value_name = "OBJECT")]
        reuse_message: Option<String>,

        /// Overwrite an existing note.
        #[arg(short = 'f', long = "force")]
        force: bool,

        /// Allow empty note.
        #[arg(long = "allow-empty")]
        allow_empty: bool,

        /// Object to annotate (defaults to HEAD).
        #[arg()]
        object: Option<String>,
    },
    /// Show the note for an object.
    Show {
        /// Object whose note to show (defaults to HEAD).
        #[arg()]
        object: Option<String>,
    },
    /// Remove the note for an object.
    Remove {
        /// Object whose note to remove (defaults to HEAD).
        #[arg()]
        object: Option<String>,
    },
    /// Append to the note for an object.
    Append {
        /// Message to append.
        #[arg(short = 'm', long = "message")]
        message: Option<String>,

        /// Read message from file ('-' for stdin).
        #[arg(short = 'F', long = "file", value_name = "FILE")]
        file: Option<std::path::PathBuf>,

        /// Object to annotate (defaults to HEAD).
        #[arg()]
        object: Option<String>,
    },
    /// Copy the note from one object to another.
    Copy {
        /// Overwrite an existing note on the target.
        #[arg(short = 'f', long = "force")]
        force: bool,

        /// Source object.
        #[arg()]
        from: String,

        /// Target object.
        #[arg()]
        to: String,
    },
    /// Edit an existing note (launches editor).
    Edit {
        /// Object whose note to edit (defaults to HEAD).
        #[arg()]
        object: Option<String>,
    },
    /// Merge notes refs.
    Merge {
        /// Finalize a notes merge after resolving conflicts.
        #[arg(long)]
        commit: bool,

        /// Abort an in-progress notes merge.
        #[arg(long)]
        abort: bool,

        /// Merge strategy (manual, ours, theirs, union, cat_sort_uniq).
        #[arg(short = 's', long = "strategy")]
        strategy: Option<String>,

        /// Notes ref to merge from (with `git notes merge <ref>`).
        #[arg()]
        source_ref: Option<String>,
    },
    /// Remove notes for non-existent objects.
    Prune {
        /// Only report what would be done.
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Report pruned entries.
        #[arg(short, long)]
        verbose: bool,
    },
    /// Print the current notes ref.
    #[command(name = "get-ref")]
    GetRef,
}

/// Run the `notes` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let notes_ref = args
        .notes_ref
        .or_else(|| std::env::var("GIT_NOTES_REF").ok())
        .unwrap_or_else(|| {
            ConfigSet::load(Some(&repo.git_dir), true)
                .ok()
                .and_then(|c| c.get("core.notesRef"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "refs/notes/commits".to_owned())
        });
    let notes_ref = if notes_ref.starts_with("refs/") {
        notes_ref
    } else {
        format!("refs/notes/{notes_ref}")
    };

    // Validate the notes ref — refuse refs outside refs/notes/.
    if notes_ref.starts_with("refs/heads/") || notes_ref.starts_with("refs/remotes/") {
        bail!(
            "refusing to {} notes in {}",
            match &args.command {
                Some(NotesSubcommand::Add { .. }) => "add",
                Some(NotesSubcommand::Edit { .. }) => "edit",
                Some(NotesSubcommand::Append { .. }) => "append",
                Some(NotesSubcommand::Remove { .. }) => "remove",
                Some(NotesSubcommand::Copy { .. }) => "copy",
                _ => "use",
            },
            notes_ref
        );
    }
    if notes_ref == "/" {
        bail!("refusing to use notes ref '/'");
    }

    match args.command {
        None | Some(NotesSubcommand::List { object: None }) => list_all_notes(&repo, &notes_ref),
        Some(NotesSubcommand::List {
            object: Some(object),
        }) => list_note_for_object(&repo, &notes_ref, &object),
        Some(NotesSubcommand::Add {
            message,
            file,
            reuse_message,
            force,
            allow_empty,
            object,
        }) => add_note(
            &repo,
            &notes_ref,
            object.as_deref(),
            message,
            file,
            reuse_message,
            force,
            allow_empty,
        ),
        Some(NotesSubcommand::Show { object }) => show_note(&repo, &notes_ref, object.as_deref()),
        Some(NotesSubcommand::Remove { object }) => {
            remove_note(&repo, &notes_ref, object.as_deref())
        }
        Some(NotesSubcommand::Append {
            message,
            file,
            object,
        }) => append_note(&repo, &notes_ref, object.as_deref(), message, file),
        Some(NotesSubcommand::Edit { object }) => edit_note(&repo, &notes_ref, object.as_deref()),
        Some(NotesSubcommand::Copy { force, from, to }) => {
            copy_note(&repo, &notes_ref, &from, &to, force)
        }
        Some(NotesSubcommand::Merge {
            commit,
            abort,
            strategy,
            source_ref,
        }) => merge_notes_dispatch(
            &repo,
            &notes_ref,
            commit,
            abort,
            strategy.as_deref(),
            source_ref.as_deref(),
        ),
        Some(NotesSubcommand::Prune { dry_run, verbose }) => {
            prune_notes(&repo, &notes_ref, dry_run, verbose)
        }
        Some(NotesSubcommand::GetRef) => {
            println!("{}", notes_ref);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve an object spec to an ObjectId, defaulting to HEAD.
fn resolve_object(repo: &Repository, spec: Option<&str>) -> Result<ObjectId> {
    match spec {
        Some(s) => resolve_revision(repo, s).with_context(|| format!("cannot resolve '{s}'")),
        None => {
            let head = resolve_head(&repo.git_dir)?;
            match head {
                grit_lib::state::HeadState::Branch { oid: Some(oid), .. } => Ok(oid),
                grit_lib::state::HeadState::Detached { oid } => Ok(oid),
                _ => bail!("HEAD does not point to a valid object"),
            }
        }
    }
}

#[derive(Clone)]
struct NotesTreeEntry {
    mode: u32,
    path: Vec<u8>,
    oid: ObjectId,
}

enum NotesTreeChild {
    Blob { mode: u32, oid: ObjectId },
    Tree(Vec<NotesTreeEntry>),
}

fn note_object_name(path: &[u8]) -> Option<String> {
    let compact: Vec<u8> = path.iter().copied().filter(|byte| *byte != b'/').collect();
    if compact.len() != 40 || !compact.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    String::from_utf8(compact)
        .ok()
        .map(|name| name.to_ascii_lowercase())
}

fn display_note_path(entry: &NotesTreeEntry) -> Cow<'_, str> {
    if let Some(name) = note_object_name(&entry.path) {
        Cow::Owned(name)
    } else {
        String::from_utf8_lossy(&entry.path)
    }
}

fn collect_notes_tree_entries(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &[u8],
    out: &mut Vec<NotesTreeEntry>,
) -> Result<()> {
    let tree_obj = repo.odb.read(tree_oid)?;
    if tree_obj.kind != ObjectKind::Tree {
        bail!("notes commit has invalid tree");
    }

    for entry in parse_tree(&tree_obj.data)? {
        let mut path = prefix.to_vec();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(&entry.name);

        if entry.mode == 0o040000 {
            collect_notes_tree_entries(repo, &entry.oid, &path, out)?;
        } else {
            out.push(NotesTreeEntry {
                mode: entry.mode,
                path,
                oid: entry.oid,
            });
        }
    }

    Ok(())
}

/// Read the notes tree entries from the notes ref.  Returns an empty vec if
/// the ref doesn't exist yet.
fn read_notes_tree(repo: &Repository, notes_ref: &str) -> Result<Vec<NotesTreeEntry>> {
    let commit_oid = match resolve_ref(&repo.git_dir, notes_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(Vec::new()),
    };

    let commit_obj = repo.odb.read(&commit_oid)?;
    if commit_obj.kind != ObjectKind::Commit {
        bail!("{notes_ref} does not point to a commit");
    }
    let commit = grit_lib::objects::parse_commit(&commit_obj.data)?;
    let mut entries = Vec::new();
    collect_notes_tree_entries(repo, &commit.tree, b"", &mut entries)?;
    Ok(entries)
}

fn notes_fanout(entries: &[NotesTreeEntry]) -> usize {
    let mut note_count = entries
        .iter()
        .filter(|entry| note_object_name(&entry.path).is_some())
        .count();
    let mut fanout = 0usize;
    while note_count > 0xff {
        note_count >>= 8;
        fanout += 1;
    }
    fanout
}

fn path_with_fanout(hex: &str, fanout: usize) -> Vec<u8> {
    let mut path = Vec::with_capacity(hex.len() + fanout);
    let bytes = hex.as_bytes();
    let split = fanout.min(bytes.len() / 2);
    for idx in 0..split {
        let start = idx * 2;
        path.extend_from_slice(&bytes[start..start + 2]);
        path.push(b'/');
    }
    path.extend_from_slice(&bytes[split * 2..]);
    path
}

fn write_notes_subtree(repo: &Repository, entries: &[NotesTreeEntry]) -> Result<ObjectId> {
    let mut children: BTreeMap<Vec<u8>, NotesTreeChild> = BTreeMap::new();

    for entry in entries {
        if let Some(slash_pos) = entry.path.iter().position(|byte| *byte == b'/') {
            let child_name = entry.path[..slash_pos].to_vec();
            let child_entry = NotesTreeEntry {
                mode: entry.mode,
                path: entry.path[slash_pos + 1..].to_vec(),
                oid: entry.oid,
            };
            children
                .entry(child_name)
                .or_insert_with(|| NotesTreeChild::Tree(Vec::new()));
            if let Some(NotesTreeChild::Tree(tree_entries)) =
                children.get_mut(&entry.path[..slash_pos])
            {
                tree_entries.push(child_entry);
            }
        } else {
            children.insert(
                entry.path.clone(),
                NotesTreeChild::Blob {
                    mode: entry.mode,
                    oid: entry.oid,
                },
            );
        }
    }

    let mut tree_entries = Vec::with_capacity(children.len());
    for (name, child) in children {
        match child {
            NotesTreeChild::Blob { mode, oid } => tree_entries.push(TreeEntry { mode, name, oid }),
            NotesTreeChild::Tree(child_entries) => {
                let oid = write_notes_subtree(repo, &child_entries)?;
                tree_entries.push(TreeEntry {
                    mode: 0o040000,
                    name,
                    oid,
                });
            }
        }
    }

    tree_entries
        .sort_by(|a, b| tree_entry_cmp(&a.name, a.mode == 0o040000, &b.name, b.mode == 0o040000));

    let tree_data = serialize_tree(&tree_entries);
    repo.odb
        .write(ObjectKind::Tree, &tree_data)
        .map_err(Into::into)
}

/// Write a new notes tree and commit, updating the notes ref.
fn write_notes_commit(
    repo: &Repository,
    notes_ref: &str,
    entries: &[NotesTreeEntry],
    message: &str,
) -> Result<()> {
    let fanout = notes_fanout(entries);
    let rewritten_entries: Vec<_> = entries
        .iter()
        .map(|entry| NotesTreeEntry {
            mode: entry.mode,
            path: note_object_name(&entry.path)
                .map(|name| path_with_fanout(&name, fanout))
                .unwrap_or_else(|| entry.path.clone()),
            oid: entry.oid,
        })
        .collect();
    let tree_oid = write_notes_subtree(repo, &rewritten_entries)?;

    // Get existing notes commit as parent (if any)
    let parent = resolve_ref(&repo.git_dir, notes_ref).ok();

    // Build committer/author ident
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let ident = build_ident(&config, now);

    let commit = CommitData {
        tree: tree_oid,
        parents: parent.into_iter().collect(),
        author: ident.clone(),
        committer: ident,
        author_raw: Vec::new(),
        committer_raw: Vec::new(),
        encoding: None,
        message: if message.ends_with('\n') {
            message.to_owned()
        } else {
            format!("{message}\n")
        },
        raw_message: None,
    };

    let commit_data = serialize_commit(&commit);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_data)?;

    write_ref(&repo.git_dir, notes_ref, &commit_oid)?;
    Ok(())
}

/// Build a Git ident string from config.
fn build_ident(config: &ConfigSet, now: OffsetDateTime) -> String {
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());

    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();

    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

/// List all notes.
fn list_all_notes(repo: &Repository, notes_ref: &str) -> Result<()> {
    let entries = read_notes_tree(repo, notes_ref)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for entry in &entries {
        writeln!(out, "{} {}", entry.oid.to_hex(), display_note_path(entry))?;
    }
    Ok(())
}

/// List the note for a specific object.
fn list_note_for_object(repo: &Repository, notes_ref: &str, object: &str) -> Result<()> {
    let oid = resolve_object(repo, Some(object))?;
    let hex = oid.to_hex();
    let entries = read_notes_tree(repo, notes_ref)?;

    for entry in &entries {
        if note_object_name(&entry.path).as_deref() == Some(hex.as_str()) {
            println!("{}", entry.oid.to_hex());
            return Ok(());
        }
    }

    bail!("No note found for object {hex}");
}

/// Add a note to an object.
/// Resolve the editor to use for notes.
fn resolve_editor(repo: &Repository) -> String {
    if let Ok(e) = std::env::var("GIT_EDITOR") {
        return e;
    }
    if let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) {
        if let Some(e) = config.get("core.editor") {
            return e;
        }
    }
    if let Ok(e) = std::env::var("VISUAL") {
        return e;
    }
    if let Ok(e) = std::env::var("EDITOR") {
        return e;
    }
    "vi".to_owned()
}

/// Launch the editor on a temporary file and return its contents.
fn launch_editor(repo: &Repository, initial: &str) -> Result<String> {
    let editor = resolve_editor(repo);
    let tmp_dir = repo.git_dir.join("tmp");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let tmp_path = tmp_dir.join("NOTES_EDITMSG");
    std::fs::write(&tmp_path, initial)?;

    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$@\"", editor))
        .arg("--")
        .arg(tmp_path.to_string_lossy().as_ref())
        .status()
        .with_context(|| format!("failed to launch editor '{editor}'"))?;

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        bail!("editor exited with non-zero status");
    }

    let result = std::fs::read_to_string(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);
    Ok(result)
}

fn add_note(
    repo: &Repository,
    notes_ref: &str,
    object: Option<&str>,
    message: Option<String>,
    file: Option<std::path::PathBuf>,
    reuse_message: Option<String>,
    force: bool,
    allow_empty: bool,
) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();

    let mut entries = read_notes_tree(repo, notes_ref)?;
    let has_message_source = message.is_some() || file.is_some() || reuse_message.is_some();

    // Get existing note content (if any)
    let existing_content = entries
        .iter()
        .find(|e| note_object_name(&e.path).as_deref() == Some(hex.as_str()))
        .and_then(|e| repo.odb.read(&e.oid).ok())
        .map(|obj| String::from_utf8_lossy(&obj.data).to_string());

    // Check for existing note
    if existing_content.is_some() && has_message_source && !force {
        bail!(
            "Cannot add notes. Found existing notes for object {}. Use '-f' to overwrite existing notes",
            hex
        );
    }

    // When -C is used, directly reuse the blob OID as the note
    let direct_note_oid = if let Some(reuse_obj) = reuse_message {
        let blob_oid = resolve_revision(repo, &reuse_obj)
            .with_context(|| format!("invalid object: '{reuse_obj}'"))?;
        // Verify the object exists
        let _ = repo
            .odb
            .read(&blob_oid)
            .with_context(|| format!("reading object '{reuse_obj}'"))?;
        Some(blob_oid)
    } else {
        None
    };

    let mut msg = if direct_note_oid.is_some() {
        String::new() // unused when direct_note_oid is set
    } else if let Some(m) = message {
        m
    } else if let Some(f) = file {
        if f.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            std::fs::read_to_string(&f).with_context(|| format!("reading '{}'", f.display()))?
        }
    } else {
        // Launch editor, pre-filling with existing note if present (morph into edit)
        let initial = existing_content.as_deref().unwrap_or("");
        let edited = launch_editor(repo, initial)?;
        if edited.trim().is_empty() && !allow_empty {
            bail!("Aborting due to empty note");
        }
        edited
    };

    if direct_note_oid.is_none() && !msg.is_empty() && !msg.ends_with('\n') {
        msg.push('\n');
    }

    // Remove existing note entry (will re-add below)
    entries.retain(|e| note_object_name(&e.path).as_deref() != Some(hex.as_str()));

    // Write the note blob (or reuse existing blob OID from -C)
    let note_oid = if let Some(oid) = direct_note_oid {
        oid
    } else {
        repo.odb.write(ObjectKind::Blob, msg.as_bytes())?
    };

    // Add entry
    entries.push(NotesTreeEntry {
        mode: 0o100644,
        path: hex.as_bytes().to_vec(),
        oid: note_oid,
    });

    write_notes_commit(repo, notes_ref, &entries, "Notes added by 'git notes add'")?;

    Ok(())
}

/// Edit an existing note (or create a new one) by launching the editor.
fn edit_note(repo: &Repository, notes_ref: &str, object: Option<&str>) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();
    let mut entries = read_notes_tree(repo, notes_ref)?;

    // Get existing note content if any
    let existing = entries
        .iter()
        .find(|e| note_object_name(&e.path).as_deref() == Some(hex.as_str()))
        .and_then(|e| repo.odb.read(&e.oid).ok())
        .map(|obj| String::from_utf8_lossy(&obj.data).to_string())
        .unwrap_or_default();

    let msg = launch_editor(repo, &existing)?;
    if msg.trim().is_empty() {
        // Remove the note if the editor content is empty
        entries.retain(|e| note_object_name(&e.path).as_deref() != Some(hex.as_str()));
        write_notes_commit(
            repo,
            notes_ref,
            &entries,
            "Notes removed by 'git notes edit'",
        )?;
        return Ok(());
    }

    // Remove existing and add new
    entries.retain(|e| note_object_name(&e.path).as_deref() != Some(hex.as_str()));
    let note_oid = repo.odb.write(ObjectKind::Blob, msg.as_bytes())?;
    entries.push(NotesTreeEntry {
        mode: 0o100644,
        path: hex.as_bytes().to_vec(),
        oid: note_oid,
    });

    write_notes_commit(repo, notes_ref, &entries, "Notes added by 'git notes edit'")?;
    Ok(())
}

/// Show the note for an object.
fn show_note(repo: &Repository, notes_ref: &str, object: Option<&str>) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();
    let entries = read_notes_tree(repo, notes_ref)?;

    for entry in &entries {
        if note_object_name(&entry.path).as_deref() == Some(hex.as_str()) {
            let blob = repo.odb.read(&entry.oid)?;
            if blob.kind != ObjectKind::Blob {
                bail!("note entry is not a blob");
            }
            let stdout = io::stdout();
            let mut out = stdout.lock();
            out.write_all(&blob.data)?;
            return Ok(());
        }
    }

    bail!("No note found for object {hex}");
}

/// Remove the note for an object.
fn remove_note(repo: &Repository, notes_ref: &str, object: Option<&str>) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();
    let mut entries = read_notes_tree(repo, notes_ref)?;

    let len_before = entries.len();
    entries.retain(|e| note_object_name(&e.path).as_deref() != Some(hex.as_str()));

    if entries.len() == len_before {
        bail!("Object {hex} has no note to remove");
    }

    write_notes_commit(
        repo,
        notes_ref,
        &entries,
        "Notes removed by 'git notes remove'",
    )?;

    eprintln!("Removing note for object {hex}");
    Ok(())
}

/// Append to the note for an object.
fn append_note(
    repo: &Repository,
    notes_ref: &str,
    object: Option<&str>,
    message: Option<String>,
    file: Option<std::path::PathBuf>,
) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();

    let msg = if let Some(m) = message {
        m
    } else if let Some(f) = file {
        if f.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            std::fs::read_to_string(&f).with_context(|| format!("reading '{}'", f.display()))?
        }
    } else {
        let edited = launch_editor(repo, "")?;
        if edited.trim().is_empty() {
            bail!("Aborting due to empty note");
        }
        edited
    };

    let mut entries = read_notes_tree(repo, notes_ref)?;

    // Find existing note content
    let existing_content = entries
        .iter()
        .find(|e| note_object_name(&e.path).as_deref() == Some(hex.as_str()))
        .and_then(|e| {
            let blob = repo.odb.read(&e.oid).ok()?;
            Some(String::from_utf8_lossy(&blob.data).into_owned())
        });

    // Remove old entry if present
    entries.retain(|e| note_object_name(&e.path).as_deref() != Some(hex.as_str()));

    // Build new content (match Git: one `\n` between previous blob and new `-m` text when both exist).
    let new_content = match existing_content {
        Some(old) => {
            let mut s = old;
            if !msg.is_empty() && !s.is_empty() {
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                s.push('\n');
            }
            s.push_str(&msg);
            if !s.ends_with('\n') && !s.is_empty() {
                s.push('\n');
            }
            s
        }
        None => {
            if msg.is_empty() {
                msg
            } else if msg.ends_with('\n') {
                msg
            } else {
                format!("{msg}\n")
            }
        }
    };

    // Write new blob
    let note_oid = repo.odb.write(ObjectKind::Blob, new_content.as_bytes())?;

    entries.push(NotesTreeEntry {
        mode: 0o100644,
        path: hex.as_bytes().to_vec(),
        oid: note_oid,
    });

    write_notes_commit(
        repo,
        notes_ref,
        &entries,
        "Notes added by 'git notes append'",
    )?;

    Ok(())
}

/// Copy a note from one object to another.
fn copy_note(repo: &Repository, notes_ref: &str, from: &str, to: &str, force: bool) -> Result<()> {
    let from_oid = resolve_object(repo, Some(from))?;
    let to_oid = resolve_object(repo, Some(to))?;
    let from_hex = from_oid.to_hex();
    let to_hex = to_oid.to_hex();

    let mut entries = read_notes_tree(repo, notes_ref)?;

    // Find the source note
    let source_entry = entries
        .iter()
        .find(|e| note_object_name(&e.path).as_deref() == Some(from_hex.as_str()))
        .ok_or_else(|| anyhow::anyhow!("missing notes on source object {from_hex}"))?;
    let note_blob_oid = source_entry.oid;

    // Check if target already has a note
    if entries
        .iter()
        .any(|e| note_object_name(&e.path).as_deref() == Some(to_hex.as_str()))
    {
        if !force {
            bail!(
                "Cannot copy notes. Found existing notes for object {}. Use '-f' to overwrite existing notes",
                to_hex
            );
        }
        entries.retain(|e| note_object_name(&e.path).as_deref() != Some(to_hex.as_str()));
    }

    entries.push(NotesTreeEntry {
        mode: 0o100644,
        path: to_hex.as_bytes().to_vec(),
        oid: note_blob_oid,
    });

    write_notes_commit(repo, notes_ref, &entries, "Notes added by 'git notes copy'")?;

    Ok(())
}

const NOTES_MERGE_PARTIAL: &str = "NOTES_MERGE_PARTIAL";
const NOTES_MERGE_REF: &str = "NOTES_MERGE_REF";
const NOTES_MERGE_WORKTREE: &str = "NOTES_MERGE_WORKTREE";

/// Per-worktree git directory for `NOTES_MERGE_*` (main: `.git/`, linked: `.git/worktrees/<id>/`).
fn notes_merge_git_dir(repo: &Repository) -> std::path::PathBuf {
    repo.git_dir.clone()
}

/// If another worktree already has `NOTES_MERGE_REF` → `target_ref`, return its working-tree path.
fn find_other_worktree_with_notes_merge_ref(
    repo: &Repository,
    target_ref: &str,
) -> Option<std::path::PathBuf> {
    let current_canon = repo.git_dir.canonicalize().ok()?;
    let common = common_dir(&repo.git_dir).unwrap_or_else(|| repo.git_dir.clone());
    let common_canon = common.canonicalize().unwrap_or(common.clone());

    let mut admins: Vec<PathBuf> = vec![common_canon.clone()];
    let worktrees_dir = common_canon.join("worktrees");
    if let Ok(entries) = fs::read_dir(&worktrees_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                admins.push(p);
            }
        }
    }

    for admin in admins {
        let admin_canon = admin.canonicalize().unwrap_or(admin);
        if admin_canon == current_canon {
            continue;
        }
        let refpath = admin_canon.join(NOTES_MERGE_REF);
        let Ok(content) = fs::read_to_string(&refpath) else {
            continue;
        };
        let line = content.trim_end_matches('\n');
        let Some(sym) = line.strip_prefix("ref: ") else {
            continue;
        };
        if sym.trim() != target_ref {
            continue;
        }
        let path = if admin_canon == common_canon {
            common_canon
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| common_canon.clone())
        } else {
            PathBuf::from(worktree_path_from_admin(&admin_canon))
        };
        return Some(path);
    }
    None
}

fn worktree_path_from_admin(admin_dir: &std::path::Path) -> String {
    worktree_refs::worktree_path_from_admin(admin_dir)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotesMergeStrategy {
    Manual,
    Ours,
    Theirs,
    Union,
    CatSortUniq,
}

#[derive(Clone, Debug)]
enum LocalNoteState {
    Unset,
    Deleted,
    Present(ObjectId),
}

#[derive(Clone, Debug)]
struct NotesMergePair {
    obj: ObjectId,
    base_blob: Option<ObjectId>,
    remote_blob: Option<ObjectId>,
    local: LocalNoteState,
}

fn expand_notes_ref(short_or_full: &str) -> String {
    if short_or_full.starts_with("refs/") {
        short_or_full.to_owned()
    } else {
        format!("refs/notes/{short_or_full}")
    }
}

fn notes_merge_worktree_path(repo: &Repository) -> std::path::PathBuf {
    notes_merge_git_dir(repo).join(NOTES_MERGE_WORKTREE)
}

/// True when `NOTES_MERGE_WORKTREE` exists and is not empty (matches Git `is_empty_dir`).
fn notes_merge_worktree_nonempty(worktree: &std::path::Path) -> bool {
    if !worktree.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(worktree) else {
        return false;
    };
    entries.flatten().next().is_some()
}

fn parse_notes_merge_strategy_value(s: &str) -> Option<NotesMergeStrategy> {
    match s {
        "manual" => Some(NotesMergeStrategy::Manual),
        "ours" => Some(NotesMergeStrategy::Ours),
        "theirs" => Some(NotesMergeStrategy::Theirs),
        "union" => Some(NotesMergeStrategy::Union),
        "cat_sort_uniq" => Some(NotesMergeStrategy::CatSortUniq),
        _ => None,
    }
}

fn parse_notes_merge_strategy_cli(s: &str) -> Result<NotesMergeStrategy> {
    parse_notes_merge_strategy_value(s).ok_or_else(|| anyhow::anyhow!("unknown -s/--strategy: {s}"))
}

fn parse_notes_merge_strategy_config(s: &str) -> Result<NotesMergeStrategy> {
    parse_notes_merge_strategy_value(s).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown notes merge strategy {s}\n\
fatal: unable to parse 'notes.mergeStrategy' from command-line config"
        )
    })
}

fn note_path_to_object_id(path: &str) -> Option<ObjectId> {
    let compact: String = path.chars().filter(|c| *c != '/').collect();
    if compact.len() != 40 || !compact.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    ObjectId::from_hex(&compact).ok()
}

fn diff_note_blob_changes(
    repo: &Repository,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
) -> Result<Vec<(ObjectId, Option<ObjectId>, Option<ObjectId>)>> {
    let entries = diff_trees(&repo.odb, old_tree, new_tree, "")?;
    let mut out = Vec::new();
    for e in entries {
        let Some(obj) = e
            .path()
            .parse::<ObjectId>()
            .ok()
            .or_else(|| note_path_to_object_id(e.path()))
        else {
            continue;
        };
        match e.status {
            DiffStatus::Added => {
                if e.new_oid != zero_oid() {
                    out.push((obj, None, Some(e.new_oid)));
                }
            }
            DiffStatus::Deleted => {
                if e.old_oid != zero_oid() {
                    out.push((obj, Some(e.old_oid), None));
                }
            }
            DiffStatus::Modified | DiffStatus::TypeChanged => {
                out.push((obj, Some(e.old_oid), Some(e.new_oid)));
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn build_merge_pairs(
    base_tree: Option<ObjectId>,
    local_tree: ObjectId,
    remote_tree: ObjectId,
    repo: &Repository,
) -> Result<Vec<NotesMergePair>> {
    let remote_raw = diff_note_blob_changes(repo, base_tree.as_ref(), Some(&remote_tree))?;
    let local_raw = diff_note_blob_changes(repo, base_tree.as_ref(), Some(&local_tree))?;
    let mut map: HashMap<ObjectId, NotesMergePair> = HashMap::new();
    for (obj, old_b, new_b) in remote_raw {
        map.insert(
            obj,
            NotesMergePair {
                obj,
                base_blob: old_b,
                remote_blob: new_b,
                local: LocalNoteState::Unset,
            },
        );
    }

    for (obj, old_b, new_b) in local_raw {
        if let Some(p) = map.get_mut(&obj) {
            if new_b.is_none() {
                p.local = LocalNoteState::Deleted;
            } else {
                let new_oid = new_b.expect("add or modify has new blob");
                p.local = LocalNoteState::Present(new_oid);
            }
        } else {
            map.insert(
                obj,
                NotesMergePair {
                    obj,
                    base_blob: old_b,
                    remote_blob: old_b,
                    local: if new_b.is_none() {
                        LocalNoteState::Deleted
                    } else {
                        LocalNoteState::Present(new_b.expect("local add/modify"))
                    },
                },
            );
        }
    }

    let mut v: Vec<_> = map.into_values().collect();
    v.sort_by(|a, b| a.obj.cmp(&b.obj));
    Ok(v)
}

fn read_blob_bytes(repo: &Repository, oid: &ObjectId) -> Result<Vec<u8>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Blob {
        bail!("expected blob for note");
    }
    Ok(obj.data)
}

/// Matches Git's `combine_notes_concatenate`: join two note blobs with a blank line between them.
fn combine_notes_concatenate(
    repo: &Repository,
    cur: Option<&ObjectId>,
    new_oid: Option<&ObjectId>,
) -> Result<ObjectId> {
    let new_data = match new_oid {
        Some(n) => {
            let obj = repo.odb.read(n)?;
            if obj.kind != ObjectKind::Blob || obj.data.is_empty() {
                Vec::new()
            } else {
                obj.data
            }
        }
        None => Vec::new(),
    };
    if new_data.is_empty() {
        let Some(c) = cur else {
            bail!("combine_notes_concatenate: empty new and no current");
        };
        return Ok(*c);
    }

    let cur_data = match cur {
        Some(c) => {
            let obj = repo.odb.read(c)?;
            if obj.kind != ObjectKind::Blob || obj.data.is_empty() {
                Vec::new()
            } else {
                obj.data
            }
        }
        None => Vec::new(),
    };

    if cur_data.is_empty() {
        return repo
            .odb
            .write(ObjectKind::Blob, &new_data)
            .map_err(|e| anyhow::anyhow!(e));
    }

    let mut cur_len = cur_data.len();
    if cur_len > 0 && cur_data[cur_len - 1] == b'\n' {
        cur_len -= 1;
    }
    let mut buf = Vec::with_capacity(cur_len + 2 + new_data.len());
    buf.extend_from_slice(&cur_data[..cur_len]);
    buf.push(b'\n');
    buf.push(b'\n');
    buf.extend_from_slice(&new_data);
    repo.odb
        .write(ObjectKind::Blob, &buf)
        .map_err(|e| anyhow::anyhow!(e))
}

fn note_blob_lines(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return Vec::new();
    }
    let s = String::from_utf8_lossy(data);
    s.split('\n').map(|l| l.to_owned()).collect()
}

/// Matches Git's `combine_notes_cat_sort_uniq`: all lines from both blobs, de-duplicated and sorted.
fn combine_notes_cat_sort_uniq(
    repo: &Repository,
    cur: Option<&ObjectId>,
    new_oid: Option<&ObjectId>,
) -> Result<ObjectId> {
    let mut lines: Vec<String> = Vec::new();
    for oid in [cur, new_oid].into_iter().flatten() {
        let obj = repo.odb.read(oid)?;
        if obj.kind == ObjectKind::Blob && !obj.data.is_empty() {
            lines.extend(note_blob_lines(&obj.data));
        }
    }
    lines.retain(|l| !l.is_empty());
    lines.sort();
    lines.dedup();
    let mut buf = String::new();
    for l in &lines {
        buf.push_str(l);
        buf.push('\n');
    }
    repo.odb
        .write(ObjectKind::Blob, buf.as_bytes())
        .map_err(|e| anyhow::anyhow!(e))
}

fn blob_to_lines(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return vec![String::new()];
    }
    let s = String::from_utf8_lossy(data).into_owned();
    s.split_inclusive('\n').map(|l| l.to_owned()).collect()
}

fn merge_note_blobs_conflict_markers(
    repo: &Repository,
    base: Option<&ObjectId>,
    local: &ObjectId,
    remote: &ObjectId,
    local_ref: &str,
    remote_ref: &str,
) -> Result<Vec<u8>> {
    let base_lines: Vec<String> = match base {
        Some(b) => blob_to_lines(&read_blob_bytes(repo, b)?),
        None => vec![String::new()],
    };
    let local_lines = blob_to_lines(&read_blob_bytes(repo, local)?);
    let remote_lines = blob_to_lines(&read_blob_bytes(repo, remote)?);

    let base_refs: Vec<&str> = base_lines.iter().map(|s| s.as_str()).collect();
    let local_refs: Vec<&str> = local_lines.iter().map(|s| s.as_str()).collect();
    let remote_refs: Vec<&str> = remote_lines.iter().map(|s| s.as_str()).collect();
    let m3 = Merge3::new(&base_refs, &local_refs, &remote_refs);
    let markers = StandardMarkers::new(Some(local_ref), Some(remote_ref));
    let merged: String = m3
        .merge_lines(false, &markers)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect();
    Ok(merged.into_bytes())
}

fn write_note_conflict_file(
    path: &std::path::Path,
    repo: &Repository,
    pair: &NotesMergePair,
    local_ref: &str,
    remote_ref: &str,
) -> Result<()> {
    let data = match (&pair.local, &pair.remote_blob) {
        (LocalNoteState::Deleted, Some(r)) => read_blob_bytes(repo, r)?,
        (LocalNoteState::Present(l), None) => read_blob_bytes(repo, l)?,
        (LocalNoteState::Present(l), Some(r)) => merge_note_blobs_conflict_markers(
            repo,
            pair.base_blob.as_ref(),
            l,
            r,
            local_ref,
            remote_ref,
        )?,
        _ => bail!("unexpected notes merge conflict shape"),
    };
    fs::write(path, data)?;
    Ok(())
}

fn merge_one_note_change(
    repo: &Repository,
    pair: &NotesMergePair,
    strategy: NotesMergeStrategy,
    local_ref: &str,
    remote_ref: &str,
    worktree: &std::path::Path,
    commit_msg: &mut String,
    entries: &mut Vec<NotesTreeEntry>,
    has_worktree: &mut bool,
) -> Result<bool> {
    let obj_hex = pair.obj.to_hex();
    let path = worktree.join(&obj_hex);
    match strategy {
        NotesMergeStrategy::Manual => {
            if !*has_worktree && notes_merge_worktree_nonempty(worktree) {
                bail!(
                    "You have not concluded your previous notes merge (.git/NOTES_MERGE_* exists).\n\
Please, use 'git notes merge --commit' or 'git notes merge --abort' to commit/abort the \
previous merge before you start a new notes merge."
                );
            }
            if !commit_msg.contains("Conflicts:") {
                commit_msg.push_str("\n\nConflicts:\n");
            }
            commit_msg.push_str(&format!("\t{obj_hex}\n"));
            if !*has_worktree {
                let test = worktree.join(".test");
                fs::create_dir_all(worktree)?;
                fs::write(&test, b"")?;
                let _ = fs::remove_file(test);
                *has_worktree = true;
            }
            write_note_conflict_file(&path, repo, pair, local_ref, remote_ref)?;
            entries.retain(|e| note_object_name(&e.path).as_deref() != Some(obj_hex.as_str()));
            Ok(true)
        }
        NotesMergeStrategy::Ours => Ok(false),
        NotesMergeStrategy::Theirs => {
            if let Some(r) = pair.remote_blob {
                upsert_note_entry(entries, &obj_hex, r);
            } else {
                entries.retain(|e| note_object_name(&e.path).as_deref() != Some(obj_hex.as_str()));
            }
            Ok(false)
        }
        NotesMergeStrategy::Union => {
            match (&pair.local, &pair.remote_blob) {
                (LocalNoteState::Deleted, None) => {
                    entries
                        .retain(|e| note_object_name(&e.path).as_deref() != Some(obj_hex.as_str()));
                }
                (LocalNoteState::Deleted, Some(r)) => {
                    let out = combine_notes_concatenate(repo, None, Some(r))?;
                    upsert_note_entry(entries, &obj_hex, out);
                }
                (LocalNoteState::Present(_), None) => {}
                (LocalNoteState::Present(l), Some(r)) => {
                    let out = combine_notes_concatenate(repo, Some(l), Some(r))?;
                    upsert_note_entry(entries, &obj_hex, out);
                }
                (LocalNoteState::Unset, _) => {
                    bail!("unexpected notes merge pair: local unset in union strategy");
                }
            }
            Ok(false)
        }
        NotesMergeStrategy::CatSortUniq => {
            match (&pair.local, &pair.remote_blob) {
                (LocalNoteState::Deleted, None) => {
                    entries
                        .retain(|e| note_object_name(&e.path).as_deref() != Some(obj_hex.as_str()));
                }
                (LocalNoteState::Deleted, Some(r)) => {
                    let out = combine_notes_cat_sort_uniq(repo, None, Some(r))?;
                    upsert_note_entry(entries, &obj_hex, out);
                }
                (LocalNoteState::Present(_), None) => {}
                (LocalNoteState::Present(l), Some(r)) => {
                    let out = combine_notes_cat_sort_uniq(repo, Some(l), Some(r))?;
                    upsert_note_entry(entries, &obj_hex, out);
                }
                (LocalNoteState::Unset, _) => {
                    bail!("unexpected notes merge pair: local unset in cat_sort_uniq strategy");
                }
            }
            Ok(false)
        }
    }
}

fn upsert_note_entry(entries: &mut Vec<NotesTreeEntry>, hex: &str, blob: ObjectId) {
    entries.retain(|e| note_object_name(&e.path).as_deref() != Some(hex));
    entries.push(NotesTreeEntry {
        mode: 0o100644,
        path: hex.as_bytes().to_vec(),
        oid: blob,
    });
}

fn remote_unchanged(base: Option<ObjectId>, remote: Option<ObjectId>) -> bool {
    match (base, remote) {
        (Some(b), Some(r)) => b == r,
        (None, None) => true,
        _ => false,
    }
}

fn same_change_local_remote(p: &NotesMergePair) -> bool {
    match (&p.local, p.remote_blob) {
        (LocalNoteState::Present(l), Some(r)) => l == &r,
        (LocalNoteState::Deleted, None) => true,
        (LocalNoteState::Unset, Some(r)) => Some(r) == p.base_blob,
        (LocalNoteState::Unset, None) => p.base_blob.is_none(),
        _ => false,
    }
}

fn no_local_change(local: &LocalNoteState, base: Option<ObjectId>) -> bool {
    match local {
        LocalNoteState::Unset => true,
        LocalNoteState::Present(l) => Some(*l) == base,
        LocalNoteState::Deleted => false,
    }
}

fn adopt_remote_note(entries: &mut Vec<NotesTreeEntry>, obj_hex: &str, remote: Option<ObjectId>) {
    match remote {
        Some(oid) => upsert_note_entry(entries, obj_hex, oid),
        None => entries.retain(|e| note_object_name(&e.path).as_deref() != Some(obj_hex)),
    }
}

fn merge_changes_into_entries(
    repo: &Repository,
    pairs: &[NotesMergePair],
    strategy: NotesMergeStrategy,
    local_ref: &str,
    remote_ref: &str,
    worktree: &std::path::Path,
    commit_msg: &mut String,
    entries: &mut Vec<NotesTreeEntry>,
) -> Result<usize> {
    let mut conflicts = 0usize;
    let mut has_worktree = false;
    for p in pairs {
        if remote_unchanged(p.base_blob, p.remote_blob) {
            continue;
        }
        if same_change_local_remote(p) {
            continue;
        }
        if no_local_change(&p.local, p.base_blob) {
            adopt_remote_note(entries, &p.obj.to_hex(), p.remote_blob);
            continue;
        }
        if merge_one_note_change(
            repo,
            p,
            strategy,
            local_ref,
            remote_ref,
            worktree,
            commit_msg,
            entries,
            &mut has_worktree,
        )? {
            conflicts += 1;
        }
    }
    Ok(conflicts)
}

fn resolve_commit_tree(repo: &Repository, commit_oid: &ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(commit_oid)?;
    if obj.kind != ObjectKind::Commit {
        bail!("expected commit");
    }
    Ok(parse_commit(&obj.data)?.tree)
}

fn resolve_notes_commit_optional(repo: &Repository, notes_ref: &str) -> Result<Option<ObjectId>> {
    let oid = match resolve_ref(&repo.git_dir, notes_ref) {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };
    let obj = repo.odb.read(&oid)?;
    if obj.kind != ObjectKind::Commit {
        bail!("{notes_ref} does not point to a commit");
    }
    Ok(Some(oid))
}

fn write_notes_commit_with_parents(
    repo: &Repository,
    _notes_ref: &str,
    entries: &[NotesTreeEntry],
    message: &str,
    parents: &[ObjectId],
) -> Result<ObjectId> {
    let fanout = notes_fanout(entries);
    let rewritten_entries: Vec<_> = entries
        .iter()
        .map(|entry| NotesTreeEntry {
            mode: entry.mode,
            path: note_object_name(&entry.path)
                .map(|name| path_with_fanout(&name, fanout))
                .unwrap_or_else(|| entry.path.clone()),
            oid: entry.oid,
        })
        .collect();
    let tree_oid = write_notes_subtree(repo, &rewritten_entries)?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let ident = build_ident(&config, now);
    let commit = CommitData {
        tree: tree_oid,
        parents: parents.to_vec(),
        author: ident.clone(),
        committer: ident,
        author_raw: Vec::new(),
        committer_raw: Vec::new(),
        encoding: None,
        message: if message.ends_with('\n') {
            message.to_owned()
        } else {
            format!("{message}\n")
        },
        raw_message: None,
    };
    let commit_data = serialize_commit(&commit);
    repo.odb
        .write(ObjectKind::Commit, &commit_data)
        .map_err(|e| anyhow::anyhow!(e))
}

fn notes_merge_inner(
    repo: &Repository,
    local_ref: &str,
    remote_ref: &str,
    strategy: NotesMergeStrategy,
) -> Result<std::result::Result<ObjectId, ObjectId>> {
    let local_commit = resolve_notes_commit_optional(repo, local_ref)?;
    let remote_commit = resolve_notes_commit_optional(repo, remote_ref)?;
    match (local_commit, remote_commit) {
        (None, None) => {
            bail!("Cannot merge empty notes ref ({remote_ref}) into empty notes ref ({local_ref})")
        }
        (None, Some(r)) => Ok(Ok(r)),
        (Some(l), None) => Ok(Ok(l)),
        (Some(local_oid), Some(remote_oid)) => {
            if local_oid == remote_oid {
                return Ok(Ok(local_oid));
            }
            let bases = merge_bases_first_vs_rest(repo, local_oid, &[remote_oid])?;
            let base_commit = bases.into_iter().next();
            if Some(local_oid) == base_commit {
                return Ok(Ok(remote_oid));
            }
            if Some(remote_oid) == base_commit {
                return Ok(Ok(local_oid));
            }
            let base_tree = base_commit
                .map(|bc| resolve_commit_tree(repo, &bc))
                .transpose()?;
            let local_tree = resolve_commit_tree(repo, &local_oid)?;
            let remote_tree = resolve_commit_tree(repo, &remote_oid)?;
            let mut commit_msg = format!("Merged notes from {remote_ref} into {local_ref}\n\n");
            let pairs = build_merge_pairs(base_tree, local_tree, remote_tree, repo)?;
            let mut entries = read_notes_tree(repo, local_ref)?;
            let worktree = notes_merge_worktree_path(repo);
            let conflicts = merge_changes_into_entries(
                repo,
                &pairs,
                strategy,
                local_ref,
                remote_ref,
                &worktree,
                &mut commit_msg,
                &mut entries,
            )?;
            let merge_parents = vec![local_oid, remote_oid];
            if conflicts > 0 {
                let partial = write_notes_commit_with_parents(
                    repo,
                    local_ref,
                    &entries,
                    &commit_msg,
                    &merge_parents,
                )?;
                return Ok(Err(partial));
            }
            let new_oid = write_notes_commit_with_parents(
                repo,
                local_ref,
                &entries,
                &commit_msg,
                &merge_parents,
            )?;
            Ok(Ok(new_oid))
        }
    }
}

fn clean_notes_merge_worktree(worktree: &std::path::Path) -> Result<()> {
    if !worktree.is_dir() {
        return Ok(());
    }
    for e in fs::read_dir(worktree)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_file() {
            let _ = fs::remove_file(e.path());
        }
    }
    Ok(())
}

fn merge_notes_abort(repo: &Repository) -> Result<()> {
    let merge_git = notes_merge_git_dir(repo);
    let _ = delete_ref(&merge_git, NOTES_MERGE_PARTIAL);
    let _ = delete_ref(&merge_git, NOTES_MERGE_REF);
    clean_notes_merge_worktree(&notes_merge_worktree_path(repo))?;
    Ok(())
}

fn merge_notes_commit_cmd(repo: &Repository) -> Result<()> {
    let merge_git = notes_merge_git_dir(repo);
    let partial_oid = resolve_ref(&merge_git, NOTES_MERGE_PARTIAL)?;
    let target_ref = read_symbolic_ref(&merge_git, NOTES_MERGE_REF)?
        .ok_or_else(|| anyhow::anyhow!("failed to resolve NOTES_MERGE_REF"))?;
    let partial_obj = repo.odb.read(&partial_oid)?;
    if partial_obj.kind != ObjectKind::Commit {
        bail!("could not parse commit from NOTES_MERGE_PARTIAL.");
    }
    let partial_commit = parse_commit(&partial_obj.data)?;
    let worktree = notes_merge_worktree_path(repo);
    let mut entries = read_notes_tree(repo, NOTES_MERGE_PARTIAL)?;
    if worktree.is_dir() {
        for e in fs::read_dir(&worktree)? {
            let e = e?;
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "." || name_str == ".." {
                continue;
            }
            if !e.file_type()?.is_file() {
                continue;
            }
            let Ok(obj) = ObjectId::from_hex(name_str.trim()) else {
                continue;
            };
            let data = fs::read(e.path())?;
            let blob_oid = repo.odb.write(ObjectKind::Blob, &data)?;
            upsert_note_entry(&mut entries, &obj.to_hex(), blob_oid);
        }
    }
    let msg = partial_commit.message.clone();
    if msg.trim().is_empty() {
        bail!("partial notes commit has empty message");
    }
    let current_target = resolve_ref(&repo.git_dir, &target_ref).ok();
    let expected_first_parent = partial_commit.parents.first().copied();
    if let (Some(cur), Some(exp)) = (current_target, expected_first_parent) {
        if cur != exp {
            bail!(
                "The notes ref {} is at {} but NOTES_MERGE_PARTIAL^1 expects {}. \
Finalize the merge from the correct ref or abort.",
                target_ref,
                cur.to_hex(),
                exp.to_hex()
            );
        }
    }
    let new_oid = write_notes_commit_with_parents(
        repo,
        &target_ref,
        &entries,
        &msg,
        &partial_commit.parents,
    )?;
    write_ref(&repo.git_dir, &target_ref, &new_oid)?;
    merge_notes_abort(repo)?;
    Ok(())
}

fn merge_notes_dispatch(
    repo: &Repository,
    notes_ref: &str,
    do_commit: bool,
    do_abort: bool,
    strategy: Option<&str>,
    source_ref: Option<&str>,
) -> Result<()> {
    let do_merge = strategy.is_some() || (!do_commit && !do_abort);
    if (do_merge as u8) + (do_commit as u8) + (do_abort as u8) != 1 {
        bail!("cannot mix --commit, --abort or -s/--strategy");
    }
    if do_merge && source_ref.is_none() {
        bail!("must specify a notes ref to merge");
    }
    if !do_merge && source_ref.is_some() {
        bail!("too many arguments");
    }
    if do_abort {
        return merge_notes_abort(repo);
    }
    if do_commit {
        return merge_notes_commit_cmd(repo);
    }
    let src_raw = source_ref.expect("checked");
    let remote_ref = expand_notes_ref(src_raw);
    let strategy = if let Some(s) = strategy {
        parse_notes_merge_strategy_cli(s)?
    } else {
        let config = ConfigSet::load(Some(&repo.git_dir), true)?;
        let short = notes_ref.strip_prefix("refs/notes/").unwrap_or(notes_ref);
        let key = format!("notes.{short}.mergeStrategy");
        if let Some(v) = config.get(&key) {
            parse_notes_merge_strategy_config(&v)?
        } else if let Some(v) = config.get("notes.mergeStrategy") {
            parse_notes_merge_strategy_config(&v)?
        } else {
            NotesMergeStrategy::Manual
        }
    };
    let merge_result = notes_merge_inner(repo, notes_ref, &remote_ref, strategy)?;
    match merge_result {
        Ok(new_oid) => {
            write_ref(&repo.git_dir, notes_ref, &new_oid)?;
            Ok(())
        }
        Err(partial_oid) => {
            let merge_git = notes_merge_git_dir(repo);
            if let Some(other_path) = find_other_worktree_with_notes_merge_ref(repo, notes_ref) {
                bail!(
                    "a notes merge into {} is already in-progress at {}",
                    notes_ref,
                    other_path.display()
                );
            }
            write_ref(&merge_git, NOTES_MERGE_PARTIAL, &partial_oid)?;
            write_symbolic_ref(&merge_git, NOTES_MERGE_REF, notes_ref)?;
            let wt_display = if let Some(wt) = repo.work_tree.as_ref() {
                match merge_git.strip_prefix(wt) {
                    Ok(rel) if !rel.as_os_str().is_empty() => {
                        format!("{}/NOTES_MERGE_WORKTREE", rel.display())
                    }
                    _ => format!("{}/NOTES_MERGE_WORKTREE", merge_git.display()),
                }
            } else {
                format!("{}/NOTES_MERGE_WORKTREE", merge_git.display())
            };
            bail!(
                "Automatic notes merge failed. Fix conflicts in {} \
and commit the result with 'git notes merge --commit', \
or abort the merge with 'git notes merge --abort'.",
                wt_display
            );
        }
    }
}

/// Prune notes for objects that no longer exist in the object database.
fn prune_notes(repo: &Repository, notes_ref: &str, dry_run: bool, verbose: bool) -> Result<()> {
    let entries = read_notes_tree(repo, notes_ref)?;
    let mut kept = Vec::new();
    let mut pruned = false;

    for entry in &entries {
        let name = display_note_path(entry);
        // The note name is the hex OID of the annotated object
        let obj_exists = if let Ok(oid) = ObjectId::from_hex(&name) {
            repo.odb.read(&oid).is_ok()
        } else {
            // Non-hex name — keep it
            true
        };

        if obj_exists {
            kept.push(entry.clone());
        } else {
            pruned = true;
            if verbose || dry_run {
                eprintln!("Removing notes for non-existent object {}", name);
            }
        }
    }

    if pruned && !dry_run {
        write_notes_commit(repo, notes_ref, &kept, "Notes removed by 'git notes prune'")?;
    }

    Ok(())
}
