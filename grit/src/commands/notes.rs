//! `grit notes` — add, show, list, remove, and append object notes.
//!
//! Notes are stored as blobs in a tree referenced by `refs/notes/commits`
//! (or a custom namespace via `--ref`).  Each entry in the notes tree is
//! named by the full hex SHA of the annotated object.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};

use grit_lib::config::ConfigSet;
use grit_lib::objects::{
    parse_tree, serialize_commit, serialize_tree, CommitData, ObjectId, ObjectKind, TreeEntry,
};
use grit_lib::refs::{resolve_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;

use std::io::{self, Write};
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

        /// Overwrite an existing note.
        #[arg(short = 'f', long = "force")]
        force: bool,

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
        .unwrap_or_else(|| "refs/notes/commits".to_owned());

    match args.command {
        None | Some(NotesSubcommand::List { object: None }) => list_all_notes(&repo, &notes_ref),
        Some(NotesSubcommand::List {
            object: Some(object),
        }) => list_note_for_object(&repo, &notes_ref, &object),
        Some(NotesSubcommand::Add {
            message,
            force,
            object,
        }) => add_note(&repo, &notes_ref, object.as_deref(), message, force),
        Some(NotesSubcommand::Show { object }) => show_note(&repo, &notes_ref, object.as_deref()),
        Some(NotesSubcommand::Remove { object }) => {
            remove_note(&repo, &notes_ref, object.as_deref())
        }
        Some(NotesSubcommand::Append { message, object }) => {
            append_note(&repo, &notes_ref, object.as_deref(), message)
        }
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

/// Read the notes tree entries from the notes ref.  Returns an empty vec if
/// the ref doesn't exist yet.
fn read_notes_tree(repo: &Repository, notes_ref: &str) -> Result<Vec<TreeEntry>> {
    let commit_oid = match resolve_ref(&repo.git_dir, notes_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(Vec::new()),
    };

    let commit_obj = repo.odb.read(&commit_oid)?;
    if commit_obj.kind != ObjectKind::Commit {
        bail!("{notes_ref} does not point to a commit");
    }
    let commit = grit_lib::objects::parse_commit(&commit_obj.data)?;

    let tree_obj = repo.odb.read(&commit.tree)?;
    if tree_obj.kind != ObjectKind::Tree {
        bail!("notes commit has invalid tree");
    }

    parse_tree(&tree_obj.data).map_err(Into::into)
}

/// Write a new notes tree and commit, updating the notes ref.
fn write_notes_commit(
    repo: &Repository,
    notes_ref: &str,
    entries: &[TreeEntry],
    message: &str,
) -> Result<()> {
    // Write the tree
    let tree_data = serialize_tree(entries);
    let tree_oid = repo.odb.write(ObjectKind::Tree, &tree_data)?;

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
        message: message.to_owned(),
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
        let name = String::from_utf8_lossy(&entry.name);
        writeln!(out, "{} {}", entry.oid.to_hex(), name)?;
    }
    Ok(())
}

/// List the note for a specific object.
fn list_note_for_object(repo: &Repository, notes_ref: &str, object: &str) -> Result<()> {
    let oid = resolve_object(repo, Some(object))?;
    let hex = oid.to_hex();
    let entries = read_notes_tree(repo, notes_ref)?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        if *name == hex {
            println!("{}", entry.oid.to_hex());
            return Ok(());
        }
    }

    bail!("No note found for object {hex}");
}

/// Add a note to an object.
fn add_note(
    repo: &Repository,
    notes_ref: &str,
    object: Option<&str>,
    message: Option<String>,
    force: bool,
) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();

    let msg = message.ok_or_else(|| anyhow::anyhow!("note message required (-m)"))?;

    let mut entries = read_notes_tree(repo, notes_ref)?;

    // Check for existing note
    if entries.iter().any(|e| String::from_utf8_lossy(&e.name) == hex) {
        if !force {
            bail!(
                "Cannot add notes. Found existing notes for object {}. Use '-f' to overwrite existing notes",
                hex
            );
        }
        // Remove existing note (will re-add below)
        entries.retain(|e| String::from_utf8_lossy(&e.name) != hex);
    }

    // Write the note blob
    let note_oid = repo.odb.write(ObjectKind::Blob, msg.as_bytes())?;

    // Add entry
    entries.push(TreeEntry {
        mode: 0o100644,
        name: hex.as_bytes().to_vec(),
        oid: note_oid,
    });

    // Sort entries by name
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    write_notes_commit(repo, notes_ref, &entries, "Notes added by 'git notes add'")?;

    Ok(())
}

/// Show the note for an object.
fn show_note(repo: &Repository, notes_ref: &str, object: Option<&str>) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();
    let entries = read_notes_tree(repo, notes_ref)?;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        if *name == hex {
            let blob = repo.odb.read(&entry.oid)?;
            if blob.kind != ObjectKind::Blob {
                bail!("note entry is not a blob");
            }
            let stdout = io::stdout();
            let mut out = stdout.lock();
            out.write_all(&blob.data)?;
            // Ensure trailing newline
            if !blob.data.ends_with(b"\n") {
                out.write_all(b"\n")?;
            }
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
    entries.retain(|e| String::from_utf8_lossy(&e.name) != hex);

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
) -> Result<()> {
    let oid = resolve_object(repo, object)?;
    let hex = oid.to_hex();

    let msg = message.ok_or_else(|| anyhow::anyhow!("note message required (-m)"))?;

    let mut entries = read_notes_tree(repo, notes_ref)?;

    // Find existing note content
    let existing_content = entries
        .iter()
        .find(|e| String::from_utf8_lossy(&e.name) == hex)
        .map(|e| {
            let blob = repo.odb.read(&e.oid).ok()?;
            Some(String::from_utf8_lossy(&blob.data).into_owned())
        })
        .flatten();

    // Remove old entry if present
    entries.retain(|e| String::from_utf8_lossy(&e.name) != hex);

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

    entries.push(TreeEntry {
        mode: 0o100644,
        name: hex.as_bytes().to_vec(),
        oid: note_oid,
    });

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    write_notes_commit(
        repo,
        notes_ref,
        &entries,
        "Notes added by 'git notes append'",
    )?;

    Ok(())
}

/// Copy a note from one object to another.
fn copy_note(
    repo: &Repository,
    notes_ref: &str,
    from: &str,
    to: &str,
    force: bool,
) -> Result<()> {
    let from_oid = resolve_object(repo, Some(from))?;
    let to_oid = resolve_object(repo, Some(to))?;
    let from_hex = from_oid.to_hex();
    let to_hex = to_oid.to_hex();

    let mut entries = read_notes_tree(repo, notes_ref)?;

    // Find the source note
    let source_entry = entries
        .iter()
        .find(|e| String::from_utf8_lossy(&e.name) == from_hex)
        .ok_or_else(|| anyhow::anyhow!("missing notes on source object {from_hex}"))?;
    let note_blob_oid = source_entry.oid;

    // Check if target already has a note
    if entries.iter().any(|e| String::from_utf8_lossy(&e.name) == to_hex) {
        if !force {
            bail!(
                "Cannot copy notes. Found existing notes for object {}. Use '-f' to overwrite existing notes",
                to_hex
            );
        }
        entries.retain(|e| String::from_utf8_lossy(&e.name) != to_hex);
    }

    entries.push(TreeEntry {
        mode: 0o100644,
        name: to_hex.as_bytes().to_vec(),
        oid: note_blob_oid,
    });

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    write_notes_commit(
        repo,
        notes_ref,
        &entries,
        "Notes added by 'git notes copy'",
    )?;

    Ok(())
}

/// Merge notes refs. This is a simplified implementation that copies
/// non-conflicting notes from the source ref into the target ref.
fn merge_notes(
    repo: &Repository,
    notes_ref: &str,
    source_ref: Option<&str>,
) -> Result<()> {
    let src = source_ref.ok_or_else(|| anyhow::anyhow!("must specify a notes ref to merge from"))?;

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
        dst_entries.iter().map(|e| e.name.clone()).collect();

    let mut added = 0usize;
    for entry in &src_entries {
        if !dst_names.contains(&entry.name) {
            dst_entries.push(TreeEntry {
                mode: entry.mode,
                name: entry.name.clone(),
                oid: entry.oid,
            });
            added += 1;
        }
    }

    if added > 0 {
        dst_entries.sort_by(|a, b| a.name.cmp(&b.name));
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
fn prune_notes(
    repo: &Repository,
    notes_ref: &str,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let entries = read_notes_tree(repo, notes_ref)?;
    let mut kept = Vec::new();
    let mut pruned = false;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
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
        write_notes_commit(
            repo,
            notes_ref,
            &kept,
            "Notes removed by 'git notes prune'",
        )?;
    }

    Ok(())
}
