//! `grit cat-file` — provide contents or details of repository objects.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead, Write};

use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse;

/// Arguments for `grit cat-file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show the object type.
    #[arg(short = 't')]
    pub show_type: bool,

    /// Show the object size.
    #[arg(short = 's')]
    pub size: bool,

    /// Pretty-print object contents.
    #[arg(short = 'p')]
    pub pretty: bool,

    /// Check if the object exists (exit code 0 = yes).
    #[arg(short = 'e')]
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
    )]
    pub batch_command: Option<String>,

    /// Buffer output in `--batch-command` mode (requires `flush` commands).
    #[arg(long)]
    pub buffer: bool,

    /// Disable explicit buffering in `--batch-command` mode.
    #[arg(long)]
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
    #[arg(long = "textconv")]
    pub textconv: bool,

    /// Show filtered content.
    #[arg(long = "filters")]
    pub filters: bool,

    /// Either `<type>` (when followed by `<object>`) or `<object>`.
    pub type_or_object: Option<String>,

    /// Object to inspect when `<type>` is provided.
    pub object: Option<String>,

    /// Trailing arguments (used for "too many arguments" detection).
    #[arg(trailing_var_arg = true, hide = true)]
    pub trailing: Vec<String>,
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
    let obj = repo.read_replaced(&oid)?;

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
        // Dereference tags/commits to reach the requested object type.
        let mut _current_oid = oid;
        let mut current_obj = obj;
        while current_obj.kind != kind {
            match current_obj.kind {
                ObjectKind::Tag => {
                    // Parse tag and follow to target
                    let tag_data = String::from_utf8_lossy(&current_obj.data);
                    if let Some(target_line) = tag_data.lines().find(|l| l.starts_with("object ")) {
                        let target_hex = &target_line["object ".len()..];
                        _current_oid = target_hex.parse::<ObjectId>()
                            .map_err(|_| anyhow::anyhow!("bad tag target"))?;
                        current_obj = repo.read_replaced(&_current_oid)?;
                    } else {
                        bail!("object {} is of type tag, not {}", oid, kind);
                    }
                }
                ObjectKind::Commit if kind == ObjectKind::Tree => {
                    let commit = parse_commit(&current_obj.data)?;
                    _current_oid = commit.tree;
                    current_obj = repo.read_replaced(&_current_oid)?;
                }
                _ => {
                    if !args.allow_unknown_type {
                        bail!("object {} is of type {}, not {}", oid, current_obj.kind, kind);
                    }
                    break;
                }
            }
        }

        // Pretty-print or output the dereferenced object
        if args.pretty {
            pretty_print(&current_obj.kind, &current_obj.data)?;
            return Ok(());
        }

        let stdout = io::stdout();
        let mut out = stdout.lock();
        out.write_all(&current_obj.data)?;
        return Ok(());
    }

    // Default: print raw content
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(&obj.data)?;

    Ok(())
}

/// Print a usage error to stderr and exit with code 129 (git convention).
fn usage_error(msg: &str) -> ! {
    eprintln!("{}", msg);
    std::process::exit(129);
}

/// Validate argument combinations and produce git-compatible error messages.
fn validate_args(args: &Args) -> Result<()> {
    // Collect the "command mode" flags that were set.
    // These are mutually exclusive with each other.
    let mut cmdmodes: Vec<&str> = Vec::new();
    if args.exists { cmdmodes.push("-e"); }
    if args.pretty { cmdmodes.push("-p"); }
    if args.show_type { cmdmodes.push("-t"); }
    if args.size { cmdmodes.push("-s"); }
    if args.textconv { cmdmodes.push("--textconv"); }
    if args.filters { cmdmodes.push("--filters"); }

    // --batch-all-objects conflicts with mode flags as a cmdmode
    if args.batch_all_objects && !cmdmodes.is_empty() {
        let mode = cmdmodes[0];
        usage_error(&format!("error: {} cannot be used together with --batch-all-objects", mode));
    }

    // Check mutual exclusivity of cmdmode flags
    if cmdmodes.len() > 1 {
        usage_error(&format!(
            "error: {} cannot be used together with {}",
            cmdmodes[1], cmdmodes[0]
        ));
    }

    let is_batch = args.batch.is_some() || args.batch_check.is_some() || args.batch_command.is_some();
    let has_mode = !cmdmodes.is_empty();
    let mode_name = cmdmodes.first().copied().unwrap_or("");

    // --path requires --textconv or --filters (not batch)
    if args.path.is_some() && !args.textconv && !args.filters {
        if is_batch {
            usage_error("fatal: '--path=<path|tree-ish>' needs '--filters' or '--textconv'");
        }
        // --path without --textconv/--filters in non-batch mode
        usage_error("fatal: '--path=<path|tree-ish>' needs '--filters' or '--textconv'");
    }

    // Batch-only options require a batch mode
    if args.buffer && !is_batch {
        usage_error("fatal: '--buffer' requires a batch mode");
    }
    if args.follow_symlinks && !is_batch {
        usage_error("fatal: '--follow-symlinks' requires a batch mode");
    }
    if args.batch_all_objects && !is_batch {
        usage_error("fatal: '--batch-all-objects' requires a batch mode");
    }
    if args.nul_input && !is_batch {
        usage_error("fatal: '-z' requires a batch mode");
    }
    if args.nul_both && !is_batch {
        usage_error("fatal: '-Z' requires a batch mode");
    }

    // Mode flags are incompatible with batch mode
    if has_mode && is_batch {
        usage_error(&format!("fatal: '{}' is incompatible with batch mode", mode_name));
    }

    // Mode flags are incompatible with --follow-symlinks (a batch-only option)
    // (already handled above since --follow-symlinks requires batch mode)

    // --textconv/--filters require an object argument (unless in batch mode)
    if (args.textconv || args.filters) && !is_batch {
        if args.type_or_object.is_none() {
            let opt = if args.textconv { "--textconv" } else { "--filters" };
            usage_error(&format!("fatal: <rev> required with '{}'", opt));
        }
    }

    // -e, -p, -t, -s require an object argument
    if (args.exists || args.pretty || args.show_type || args.size) && !is_batch {
        // Check for too many arguments first
        let positional_count = args.type_or_object.as_ref().map_or(0, |_| 1)
            + args.object.as_ref().map_or(0, |_| 1)
            + args.trailing.len();

        if positional_count > 2 {
            usage_error("fatal: too many arguments");
        }

        if args.type_or_object.is_none() {
            usage_error(&format!("fatal: <object> required with '{}'", mode_name));
        }
    }

    // --textconv/--filters: too many arguments check
    if (args.textconv || args.filters) && !is_batch {
        let positional_count = args.type_or_object.as_ref().map_or(0, |_| 1)
            + args.object.as_ref().map_or(0, |_| 1)
            + args.trailing.len();
        if positional_count > 2 {
            usage_error("fatal: too many arguments");
        }
    }

    // Batch modes reject positional arguments
    if is_batch {
        if args.type_or_object.is_some() {
            usage_error("fatal: batch modes take no arguments");
        }
    }

    Ok(())
}

fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let format = args.batch_format().unwrap_or("");

    // Check if we should suppress the final flush (for testing --buffer behavior)
    let no_flush_on_exit = std::env::var("GIT_TEST_CAT_FILE_NO_FLUSH_ON_EXIT").is_ok();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();

        if args.batch_command.is_some() {
            // Empty command
            if trimmed.is_empty() {
                eprintln!("fatal: empty command in input");
                std::process::exit(128);
            }
            // Whitespace before command
            if line.starts_with(' ') || line.starts_with('\t') {
                eprintln!("fatal: whitespace before command: '{}'", trimmed);
                std::process::exit(128);
            }
            // <command> <args>
            let mut parts = trimmed.splitn(2, ' ');
            match parts.next() {
                Some("contents") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    if obj_str.is_empty() {
                        eprintln!("fatal: contents requires arguments");
                        std::process::exit(128);
                    }
                    print_batch_entry(repo, obj_str, true, format, &mut out)?;
                }
                Some("info") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    if obj_str.is_empty() {
                        eprintln!("fatal: info requires arguments");
                        std::process::exit(128);
                    }
                    print_batch_entry(repo, obj_str, false, format, &mut out)?;
                }
                Some("flush") => {
                    let rest = parts.next().unwrap_or("").trim();
                    if !rest.is_empty() {
                        eprintln!("fatal: flush takes no arguments");
                        std::process::exit(128);
                    }
                    if !args.buffer {
                        eprintln!("fatal: flush is only for --buffer mode");
                        std::process::exit(128);
                    }
                    out.flush()?;
                }
                Some(other) => {
                    eprintln!("fatal: unknown command: '{}'", other);
                    std::process::exit(128);
                }
                None => {}
            }
        } else {
            let include_content = args.batch_includes_content();
            print_batch_entry(repo, trimmed, include_content, format, &mut out)?;
        }
    }

    if !no_flush_on_exit || !args.buffer {
        out.flush()?;
    }
    Ok(())
}



fn print_batch_entry(
    repo: &Repository,
    input: &str,
    include_content: bool,
    format: &str,
    out: &mut impl Write,
) -> Result<()> {
    let (obj_str, rest) = parse_batch_input(input, format);

    if obj_str.is_empty() {
        // Empty line: print " missing"
        writeln!(out, " missing")?;
        return Ok(());
    }

    match resolve_object_with_mode(repo, obj_str) {
        Err(_) => {
            writeln!(out, "{obj_str} missing")?;
        }
        Ok((oid, mode)) => match repo.read_replaced(&oid) {
            Err(_) => {
                writeln!(out, "{obj_str} missing")?;
            }
            Ok(obj) => {
                let oid_str = oid.to_string();
                let kind_str = obj.kind.to_string();
                let size = obj.data.len();
                let mode_str = match mode {
                    Some(m) => format!("{:o}", m),
                    None => String::new(),
                };
                if format.is_empty() {
                    writeln!(out, "{} {} {}", oid_str, kind_str, size)?;
                } else {
                    writeln!(
                        out,
                        "{}",
                        apply_format(format, &oid_str, &kind_str, size, rest, &mode_str)
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

fn parse_batch_input<'a>(line: &'a str, format: &str) -> (&'a str, &'a str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ("", "");
    }
    // Only split object from rest when a custom format containing %(rest) is used.
    // Otherwise the entire line is the object name (important for paths with spaces
    // like "HEAD:path with spaces").
    if format.contains("%(rest)") {
        if let Some(split_at) = trimmed.find(char::is_whitespace) {
            let object = &trimmed[..split_at];
            let rest = trimmed[split_at..].trim_start();
            return (object, rest);
        }
    }
    (trimmed, "")
}

fn apply_format(
    format: &str,
    object_name: &str,
    object_type: &str,
    object_size: usize,
    rest: &str,
    object_mode: &str,
) -> String {
    format
        .replace("%(objecttype)", object_type)
        .replace("%(objectname)", object_name)
        .replace("%(objectsize)", &object_size.to_string())
        .replace("%(objectmode)", object_mode)
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

/// Resolve an object reference and also return the file mode if the reference
/// is a tree path (e.g., `HEAD:file` or `<tree_oid>:path`).
fn resolve_object_with_mode(repo: &Repository, obj_str: &str) -> Result<(ObjectId, Option<u32>)> {
    // Check if this is a treeish:path reference
    if let Some(colon_pos) = obj_str.find(':') {
        let treeish_part = &obj_str[..colon_pos];
        let path_part = &obj_str[colon_pos + 1..];
        if !treeish_part.is_empty() && !path_part.is_empty() {
            // Resolve the treeish part
            if let Ok(treeish_oid) = rev_parse::resolve_revision(repo, treeish_part) {
                // Get the tree oid
                let object = repo.odb.read(&treeish_oid)?;
                let tree_oid = match object.kind {
                    ObjectKind::Commit => {
                        let commit = parse_commit(&object.data)?;
                        commit.tree
                    }
                    ObjectKind::Tree => treeish_oid,
                    _ => {
                        // Fall back to regular resolution
                        let oid = resolve_object(repo, obj_str)?;
                        return Ok((oid, None));
                    }
                };

                // Walk the tree path to find the entry mode
                let parts: Vec<&str> = path_part.split('/').filter(|p| !p.is_empty()).collect();
                let mut current_tree = tree_oid;
                for (i, part) in parts.iter().enumerate() {
                    let tree_obj = repo.odb.read(&current_tree)?;
                    let entries = parse_tree(&tree_obj.data)?;
                    if let Some(entry) = entries.iter().find(|e| e.name == part.as_bytes()) {
                        if i == parts.len() - 1 {
                            // Last component: return the entry's oid and mode
                            return Ok((entry.oid, Some(entry.mode)));
                        }
                        current_tree = entry.oid;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    // No tree path or couldn't resolve mode
    let oid = resolve_object(repo, obj_str)?;
    Ok((oid, None))
}
