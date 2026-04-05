//! `grit notes` — add, show, list, remove, and append object notes.
//!
//! Notes are stored as blobs in a tree referenced by `refs/notes/commits`
//! (or a custom namespace via `--ref`).  Each entry in the notes tree is
//! named by the full hex SHA of the annotated object.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::process::Command;

use grit_lib::config::ConfigSet;
use grit_lib::objects::{
    parse_tree, serialize_commit, serialize_tree, tree_entry_cmp, CommitData, ObjectId, ObjectKind,
    TreeEntry,
};
use grit_lib::refs::{resolve_ref, write_ref};
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
    /// Merge notes refs (no-op placeholder).
    Merge {
        /// Notes ref to merge from.
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
        .unwrap_or_else(|| "refs/notes/commits".to_owned());

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
        Some(NotesSubcommand::Merge { source_ref }) => {
            merge_notes(&repo, &notes_ref, source_ref.as_deref())
        }
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

    let msg = if direct_note_oid.is_some() {
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

    // Build new content
    let new_content = match existing_content {
        Some(old) => {
            let mut s = old;
            if !s.ends_with('\n') {
                s.push('\n');
            }
            s.push('\n');
            s.push_str(&msg);
            s
        }
        None => msg,
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

/// Merge notes refs. This is a simplified implementation that copies
/// non-conflicting notes from the source ref into the target ref.
fn merge_notes(repo: &Repository, notes_ref: &str, source_ref: Option<&str>) -> Result<()> {
    let src =
        source_ref.ok_or_else(|| anyhow::anyhow!("must specify a notes ref to merge from"))?;

    // Try the ref as-is first, then with refs/notes/ prefix
    let src_entries = if let Ok(entries) = read_notes_tree(repo, src) {
        if !entries.is_empty() {
            entries
        } else if !src.starts_with("refs/") {
            let full_src = format!("refs/notes/{}", src);
            read_notes_tree(repo, &full_src)?
        } else {
            entries
        }
    } else if !src.starts_with("refs/") {
        let full_src = format!("refs/notes/{}", src);
        read_notes_tree(repo, &full_src)?
    } else {
        bail!("notes ref '{}' not found", src);
    };
    let mut dst_entries = read_notes_tree(repo, notes_ref)?;

    let dst_names: std::collections::HashSet<Vec<u8>> =
        dst_entries.iter().map(|e| e.path.clone()).collect();

    let mut added = 0usize;
    for entry in &src_entries {
        if !dst_names.contains(&entry.path) {
            dst_entries.push(NotesTreeEntry {
                mode: entry.mode,
                path: entry.path.clone(),
                oid: entry.oid,
            });
            added += 1;
        }
    }

    if added > 0 {
        write_notes_commit(
            repo,
            notes_ref,
            &dst_entries,
            &format!("notes: Merged notes from {src}"),
        )?;
    }

    Ok(())
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
