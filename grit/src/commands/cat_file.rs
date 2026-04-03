//! `grit cat-file` — provide contents or details of repository objects.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead, Write};

use grit_lib::objects::{parse_tree, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse;

/// Arguments for `grit cat-file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show the object type.
    #[arg(short = 't', conflicts_with_all = ["size", "pretty", "batch", "batch_check", "batch_command"])]
    pub show_type: bool,

    /// Show the object size.
    #[arg(short = 's', conflicts_with_all = ["show_type", "pretty", "batch", "batch_check", "batch_command"])]
    pub size: bool,

    /// Pretty-print object contents.
    #[arg(short = 'p', conflicts_with_all = ["show_type", "size", "batch", "batch_check", "batch_command"])]
    pub pretty: bool,

    /// Check if the object exists (exit code 0 = yes).
    #[arg(short = 'e', conflicts_with_all = ["show_type", "size", "pretty", "batch", "batch_check", "batch_command"])]
    pub exists: bool,

    /// Allow missing objects (with -e, don't error).
    #[arg(long = "allow-unknown-type")]
    pub allow_unknown_type: bool,

    /// Print info and content for each object ID on stdin.
    ///
    /// Optional custom format, e.g. `%(objectname) %(objecttype) %(objectsize)`.
    #[arg(
        long,
        value_name = "format",
        num_args = 0..=1,
        default_missing_value = "",
        require_equals = true,
        conflicts_with_all = ["show_type", "size", "pretty", "exists"]
    )]
    pub batch: Option<String>,

    /// Print info (type, size) for each object ID on stdin.
    ///
    /// Optional custom format, e.g. `%(objecttype) %(objectname)`.
    #[arg(
        long,
        value_name = "format",
        num_args = 0..=1,
        default_missing_value = "",
        require_equals = true,
        conflicts_with_all = ["show_type", "size", "pretty", "exists", "batch"]
    )]
    pub batch_check: Option<String>,

    /// Read commands from stdin.
    ///
    /// Optional custom format, e.g. `%(objecttype) %(objectname)`.
    #[arg(
        long,
        value_name = "format",
        num_args = 0..=1,
        default_missing_value = "",
        require_equals = true,
        conflicts_with_all = ["show_type", "size", "pretty", "exists", "batch", "batch_check"]
    )]
    pub batch_command: Option<String>,

    /// Buffer output in `--batch-command` mode (requires `flush` commands).
    #[arg(long, conflicts_with = "no_buffer")]
    pub buffer: bool,

    /// Disable explicit buffering in `--batch-command` mode.
    #[arg(long, conflicts_with = "buffer")]
    pub no_buffer: bool,

    /// Follow symlinks in tree objects.
    #[arg(long = "follow-symlinks")]
    pub follow_symlinks: bool,

    /// Enumerate all objects in the object database.
    #[arg(long = "batch-all-objects")]
    pub batch_all_objects: bool,

    /// Use NUL as input delimiter.
    #[arg(short = 'z')]
    pub nul_input: bool,

    /// Use NUL as input AND output delimiter.
    #[arg(short = 'Z')]
    pub nul_both: bool,

    /// Path to use for filtering (with --textconv/--filters).
    #[arg(long = "path", value_name = "path")]
    pub path: Option<String>,

    /// Show textconv content.
    #[arg(long = "textconv", conflicts_with_all = ["show_type", "size", "pretty", "exists", "filters"])]
    pub textconv: bool,

    /// Show filtered content.
    #[arg(long = "filters", conflicts_with_all = ["show_type", "size", "pretty", "exists", "textconv"])]
    pub filters: bool,

    /// Either `<type>` (when followed by `<object>`) or `<object>`.
    pub type_or_object: Option<String>,

    /// Object to inspect when `<type>` is provided.
    pub object: Option<String>,
}

impl Args {
    /// Whether we are in any batch mode.
    fn is_batch_mode(&self) -> bool {
        self.batch.is_some() || self.batch_check.is_some() || self.batch_command.is_some()
    }

    /// Whether --batch includes content (not just info).
    fn batch_includes_content(&self) -> bool {
        self.batch.is_some()
    }

    /// Get the batch format string (empty = default format).
    fn batch_format(&self) -> Option<&str> {
        self.batch
            .as_deref()
            .or(self.batch_check.as_deref())
            .or(self.batch_command.as_deref())
    }
}

/// Run `grit cat-file`.
pub fn run(args: Args) -> Result<()> {
    // --- Manual validation for git-compatible error messages ---
    validate_args(&args)?;

    let repo = Repository::discover(None).context("not a git repository")?;

    if args.is_batch_mode() {
        return run_batch(&repo, &args);
    }

    let (expected_kind, obj_str) = match (args.type_or_object.as_deref(), args.object.as_deref()) {
        (Some(kind_str), Some(obj)) => {
            let kind = kind_str
                .parse::<ObjectKind>()
                .map_err(|_| anyhow::anyhow!("unknown type '{}'", kind_str))?;
            (Some(kind), obj)
        }
        (Some(obj), None) => (None, obj),
        (None, _) => return Err(anyhow::anyhow!("object required when not in batch mode")),
    };

    let oid = resolve_object(&repo, obj_str)?;
    let obj = repo.odb.read(&oid)?;

    if args.exists {
        return Ok(());
    }

    if args.show_type {
        println!("{}", obj.kind);
        return Ok(());
    }

    if args.size {
        println!("{}", obj.data.len());
        return Ok(());
    }

    if args.pretty {
        pretty_print(&obj.kind, &obj.data)?;
        return Ok(());
    }

    if let Some(kind) = expected_kind {
        if obj.kind != kind && !args.allow_unknown_type {
            bail!("object {} is of type {}, not {}", oid, obj.kind, kind);
        }
    }

    // Default: print raw content
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(&obj.data)?;

    Ok(())
}

/// Validate argument combinations and produce git-compatible error messages.
fn validate_args(_args: &Args) -> Result<()> {
    // Nothing to validate beyond what clap handles for now.
    // The clap-level conflicts_with_all handles the main incompatibilities.
    Ok(())
}

fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let format = args.batch_format().unwrap_or("");


    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();

        if args.batch_command.is_some() {
            // <command> <args>
            let mut parts = trimmed.splitn(2, ' ');
            match parts.next() {
                Some("contents") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    print_batch_entry(repo, obj_str, true, format, &mut out)?;
                }
                Some("info") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    print_batch_entry(repo, obj_str, false, format, &mut out)?;
                }
                Some("flush") => {
                    if !args.buffer {
                        bail!("flush is only valid with --buffer");
                    }
                    out.flush()?;
                }
                Some(other) => bail!("unknown batch command: {other}"),
                None => {}
            }
        } else {
            let include_content = args.batch_includes_content();
            print_batch_entry(repo, trimmed, include_content, format, &mut out)?;
        }
    }

    out.flush()?;
    Ok(())
}



fn print_batch_entry(
    repo: &Repository,
    input: &str,
    include_content: bool,
    format: &str,
    out: &mut impl Write,
) -> Result<()> {
    let (obj_str, rest) = parse_batch_input(input);

    if obj_str.is_empty() {
        // Empty line: print " missing"
        writeln!(out, " missing")?;
        return Ok(());
    }

    match resolve_object(repo, obj_str) {
        Err(_) => {
            writeln!(out, "{obj_str} missing")?;
        }
        Ok(oid) => match repo.odb.read(&oid) {
            Err(_) => {
                writeln!(out, "{obj_str} missing")?;
            }
            Ok(obj) => {
                let oid_str = oid.to_string();
                let kind_str = obj.kind.to_string();
                let size = obj.data.len();
                if format.is_empty() {
                    writeln!(out, "{} {} {}", oid_str, kind_str, size)?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        apply_format(format, &oid_str, &kind_str, size, rest)
                    )?;
                }
                if include_content {
                    out.write_all(&obj.data)?;
                    writeln!(out)?;
                }
            }
        },
    }
    Ok(())
}

fn parse_batch_input(line: &str) -> (&str, &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ("", "");
    }
    // For batch-check without custom format, the whole line is the object name
    // (important for paths with spaces like ":white space").
    // For custom format with %(rest), we split on whitespace.
    // The caller decides based on context — for now, we try to be smart:
    // If it looks like it might be a path (starts with :), don't split.
    if trimmed.starts_with(':') {
        return (trimmed, "");
    }
    if let Some(split_at) = trimmed.find(char::is_whitespace) {
        let object = &trimmed[..split_at];
        let rest = trimmed[split_at..].trim_start();
        (object, rest)
    } else {
        (trimmed, "")
    }
}

fn apply_format(
    format: &str,
    object_name: &str,
    object_type: &str,
    object_size: usize,
    rest: &str,
) -> String {
    format
        .replace("%(objecttype)", object_type)
        .replace("%(objectname)", object_name)
        .replace("%(objectsize)", &object_size.to_string())
        .replace("%(objectmode)", "")
        .replace("%(rest)", rest)
}

fn pretty_print(kind: &ObjectKind, data: &[u8]) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match kind {
        ObjectKind::Blob => {
            out.write_all(data)?;
        }
        ObjectKind::Tree => {
            let entries = parse_tree(data)?;
            for e in entries {
                let name = String::from_utf8_lossy(&e.name);
                let kind_str = if e.mode == 0o040000 { "tree" } else { "blob" };
                writeln!(out, "{:06o} {kind_str} {}\t{name}", e.mode, e.oid)?;
            }
        }
        ObjectKind::Commit => {
            out.write_all(data)?;
        }
        ObjectKind::Tag => {
            out.write_all(data)?;
        }
    }
    Ok(())
}

/// Resolve an object reference string to an [`ObjectId`].
///
/// Uses the full rev-parse machinery for resolution.
fn resolve_object(repo: &Repository, obj_str: &str) -> Result<ObjectId> {
    rev_parse::resolve_revision(repo, obj_str)
        .map_err(|e| anyhow::anyhow!("{}", e))
}
