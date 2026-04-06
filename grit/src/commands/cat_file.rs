//! `grit cat-file` — provide contents or details of repository objects.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Read as _, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use grit_lib::config::ConfigSet;
use grit_lib::crlf::{
    convert_to_worktree, get_file_attrs, load_gitattributes, load_gitattributes_from_index,
    ConversionConfig, GitAttributes,
};
use grit_lib::index::{Index, MODE_REGULAR};
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse;
use grit_lib::wildmatch::wildmatch;

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
    let transform_ctx = TransformContext::new(&repo);

    if args.is_batch_mode() {
        return run_batch(&repo, &args, &transform_ctx);
    }

    if args.textconv || args.filters {
        let obj_str = args
            .type_or_object
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("object required when not in batch mode"))?;
        return run_transform_single(&repo, &args, &transform_ctx, obj_str);
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

    let oid = match resolve_object(&repo, obj_str) {
        Ok(oid) => oid,
        Err(_) if args.exists => std::process::exit(1),
        Err(_) => {
            if args.show_type || args.size {
                eprintln!("fatal: git cat-file: could not get object info");
                std::process::exit(1);
            }
            eprintln!("fatal: Not a valid object name {obj_str}");
            std::process::exit(128);
        }
    };
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
                        _current_oid = target_hex
                            .parse::<ObjectId>()
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
                        bail!(
                            "object {} is of type {}, not {}",
                            oid,
                            current_obj.kind,
                            kind
                        );
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
    if args.exists {
        cmdmodes.push("-e");
    }
    if args.pretty {
        cmdmodes.push("-p");
    }
    if args.show_type {
        cmdmodes.push("-t");
    }
    if args.size {
        cmdmodes.push("-s");
    }
    if args.textconv {
        cmdmodes.push("--textconv");
    }
    if args.filters {
        cmdmodes.push("--filters");
    }

    // --batch-all-objects conflicts with mode flags as a cmdmode
    if args.batch_all_objects && !cmdmodes.is_empty() {
        let mode = cmdmodes[0];
        usage_error(&format!(
            "error: {} cannot be used together with --batch-all-objects",
            mode
        ));
    }

    // Check mutual exclusivity of cmdmode flags
    if cmdmodes.len() > 1 {
        usage_error(&format!(
            "error: {} cannot be used together with {}",
            cmdmodes[1], cmdmodes[0]
        ));
    }

    let is_batch =
        args.batch.is_some() || args.batch_check.is_some() || args.batch_command.is_some();
    let has_mode = !cmdmodes.is_empty();
    let mode_name = cmdmodes.first().copied().unwrap_or("");

    // --path requires --textconv or --filters.
    if args.path.is_some() && !args.textconv && !args.filters {
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

    // Mode flags are incompatible with batch mode, except --textconv/--filters.
    if has_mode && is_batch && mode_name != "--textconv" && mode_name != "--filters" {
        usage_error(&format!(
            "fatal: '{}' is incompatible with batch mode",
            mode_name
        ));
    }

    // Mode flags are incompatible with --follow-symlinks (a batch-only option)
    // (already handled above since --follow-symlinks requires batch mode)

    // --textconv/--filters require exactly one object argument (unless in batch mode)
    if (args.textconv || args.filters) && !is_batch && args.type_or_object.is_none() {
        let opt = if args.textconv {
            "--textconv"
        } else {
            "--filters"
        };
        usage_error(&format!("fatal: <rev> required with '{}'", opt));
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

    // --textconv/--filters: allow exactly one positional argument.
    if (args.textconv || args.filters) && !is_batch {
        let positional_count = args.type_or_object.as_ref().map_or(0, |_| 1)
            + args.object.as_ref().map_or(0, |_| 1)
            + args.trailing.len();
        if positional_count > 1 {
            usage_error("fatal: too many arguments");
        }
    }

    // Batch modes reject positional arguments
    if is_batch && args.type_or_object.is_some() {
        usage_error("fatal: batch modes take no arguments");
    }

    Ok(())
}

fn run_batch(repo: &Repository, args: &Args, transform_ctx: &TransformContext) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    let format = args.batch_format().unwrap_or("");

    let nul_input = args.nul_input || args.nul_both;
    let nul_output = args.nul_both;
    let use_app_buffer = args.buffer && args.batch_command.is_some();

    // Check if we should suppress the final flush (for testing --buffer behavior)
    let no_flush_on_exit = std::env::var("GIT_TEST_CAT_FILE_NO_FLUSH_ON_EXIT").is_ok();

    // In --buffer mode for --batch-command, accumulate output in an
    // application-level buffer and only write to stdout on `flush` commands.
    let mut app_buf: Vec<u8> = Vec::new();

    let records = read_input_records(&stdin, nul_input)?;

    for line in &records {
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
                    if use_app_buffer {
                        print_batch_entry(
                            repo,
                            args,
                            transform_ctx,
                            obj_str,
                            true,
                            format,
                            nul_output,
                            &mut app_buf,
                        )?;
                    } else {
                        print_batch_entry(
                            repo,
                            args,
                            transform_ctx,
                            obj_str,
                            true,
                            format,
                            nul_output,
                            &mut stdout_lock,
                        )?;
                    }
                }
                Some("info") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    if obj_str.is_empty() {
                        eprintln!("fatal: info requires arguments");
                        std::process::exit(128);
                    }
                    if use_app_buffer {
                        print_batch_entry(
                            repo,
                            args,
                            transform_ctx,
                            obj_str,
                            false,
                            format,
                            nul_output,
                            &mut app_buf,
                        )?;
                    } else {
                        print_batch_entry(
                            repo,
                            args,
                            transform_ctx,
                            obj_str,
                            false,
                            format,
                            nul_output,
                            &mut stdout_lock,
                        )?;
                    }
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
                    stdout_lock.write_all(&app_buf)?;
                    stdout_lock.flush()?;
                    app_buf.clear();
                }
                Some(other) => {
                    eprintln!("fatal: unknown command: '{}'", other);
                    std::process::exit(128);
                }
                None => {}
            }
        } else {
            let include_content = args.batch_includes_content();
            print_batch_entry(
                repo,
                args,
                transform_ctx,
                trimmed,
                include_content,
                format,
                nul_output,
                &mut stdout_lock,
            )?;
        }
    }

    if no_flush_on_exit && use_app_buffer {
        // Discard the application buffer — the test verifies that --buffer
        // mode holds output until an explicit flush command.
        return Ok(());
    }
    // Flush any remaining buffered data.
    if use_app_buffer {
        stdout_lock.write_all(&app_buf)?;
    }
    stdout_lock.flush()?;
    Ok(())
}

/// Read all input records, splitting on NUL when `nul_input` is true,
/// otherwise splitting on newlines. A trailing delimiter does not create
/// an extra empty record (same semantics as [`BufRead::lines`]).
fn read_input_records(stdin: &io::Stdin, nul_input: bool) -> Result<Vec<String>> {
    if nul_input {
        let mut buf = Vec::new();
        stdin.lock().read_to_end(&mut buf)?;
        let mut records: Vec<String> = buf
            .split(|&b| b == 0)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        // Strip a single trailing empty record (from a trailing NUL),
        // matching the behaviour of BufRead::lines() with trailing LF.
        if records.last().is_some_and(|s| s.is_empty()) {
            records.pop();
        }
        Ok(records)
    } else {
        let mut records = Vec::new();
        for line in stdin.lock().lines() {
            records.push(line?);
        }
        Ok(records)
    }
}

fn print_batch_entry(
    repo: &Repository,
    args: &Args,
    transform_ctx: &TransformContext,
    input: &str,
    include_content: bool,
    format: &str,
    nul_output: bool,
    out: &mut impl Write,
) -> Result<()> {
    let split_on_whitespace = (args.textconv || args.filters) || format.contains("%(rest)");
    let (obj_str, rest) = parse_batch_input(input, split_on_whitespace);
    let eol: &[u8] = if nul_output { b"\0" } else { b"\n" };

    if obj_str.is_empty() {
        // Empty line: print " missing"
        out.write_all(b" missing")?;
        out.write_all(eol)?;
        return Ok(());
    }

    match resolve_object_with_mode(repo, obj_str) {
        Err(_) => {
            write!(out, "{obj_str} missing")?;
            out.write_all(eol)?;
        }
        Ok((oid, mode)) => match repo.read_replaced(&oid) {
            Err(_) => {
                write!(out, "{obj_str} missing")?;
                out.write_all(eol)?;
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
                    write!(out, "{} {} {}", oid_str, kind_str, size)?;
                } else {
                    write!(
                        out,
                        "{}",
                        apply_format(format, &oid_str, &kind_str, size, rest, &mode_str)
                    )?;
                }
                out.write_all(eol)?;
                if include_content {
                    if args.textconv || args.filters {
                        let path = resolve_batch_transform_path(args, rest, &oid);
                        let mode = mode.unwrap_or(MODE_REGULAR);
                        let transformed = transform_content(
                            repo,
                            transform_ctx,
                            &oid,
                            &path,
                            mode,
                            &obj,
                            args.filters,
                        )?;
                        out.write_all(&transformed)?;
                    } else {
                        out.write_all(&obj.data)?;
                    }
                    out.write_all(eol)?;
                }
            }
        },
    }
    Ok(())
}

fn parse_batch_input<'a>(line: &'a str, split_on_whitespace: bool) -> (&'a str, &'a str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ("", "");
    }
    // Only split object from rest when a custom format containing %(rest) is used.
    // Otherwise the entire line is the object name (important for paths with spaces
    // like "HEAD:path with spaces").
    if split_on_whitespace {
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

#[derive(Debug, Clone)]
struct TransformContext {
    config: ConfigSet,
    conversion: ConversionConfig,
    attrs: GitAttributes,
    diff_attrs: Vec<DiffAttrRule>,
}

impl TransformContext {
    fn new(repo: &Repository) -> Self {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conversion = ConversionConfig::from_config(&config);
        let attrs = load_attr_rules(repo);
        let diff_attrs = load_diff_attr_rules(repo);
        Self {
            config,
            conversion,
            attrs,
            diff_attrs,
        }
    }
}

#[derive(Debug, Clone)]
struct DiffAttrRule {
    pattern: String,
    value: DiffAttrValue,
}

#[derive(Debug, Clone)]
enum DiffAttrValue {
    Unset,
    Set,
    Driver(String),
}

fn run_transform_single(
    repo: &Repository,
    args: &Args,
    transform_ctx: &TransformContext,
    obj_str: &str,
) -> Result<()> {
    let (oid, mode, path) = resolve_transform_target(repo, args, obj_str)?;
    let obj = match repo.read_replaced(&oid) {
        Ok(obj) => obj,
        Err(_) => fatal(&format!("Not a valid object name {obj_str}")),
    };
    let transformed =
        transform_content(repo, transform_ctx, &oid, &path, mode, &obj, args.filters)?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(&transformed)?;
    Ok(())
}

fn resolve_transform_target(
    repo: &Repository,
    args: &Args,
    obj_str: &str,
) -> Result<(ObjectId, u32, String)> {
    if let Some(path) = args.path.as_deref() {
        let oid = match resolve_object(repo, obj_str) {
            Ok(oid) => oid,
            Err(_) => fatal(&format!("Not a valid object name {obj_str}")),
        };
        return Ok((oid, MODE_REGULAR, path.to_owned()));
    }

    if let Some(index_path) = obj_str.strip_prefix(':') {
        if index_path.is_empty() {
            fatal(&format!(
                "<object>:<path> required, only <object> '{}' given",
                obj_str
            ));
        }
        let index = match Index::load(&repo.index_path()) {
            Ok(index) => index,
            Err(_) => fatal(&format!("Not a valid object name {obj_str}")),
        };
        if let Some(entry) = index.get(index_path.as_bytes(), 0) {
            return Ok((entry.oid, entry.mode, index_path.to_owned()));
        }
        fatal(&format!("Not a valid object name {obj_str}"));
    }

    if let Some((rev, path)) = obj_str.split_once(':') {
        if rev.is_empty() || path.is_empty() {
            fatal(&format!("Not a valid object name {obj_str}"));
        }

        let rev_oid = match rev_parse::resolve_revision(repo, rev) {
            Ok(oid) => oid,
            Err(_) => fatal(&format!("invalid object name '{}'.", rev)),
        };
        let tree_oid = peel_to_tree_oid(repo, rev_oid)
            .unwrap_or_else(|_| fatal(&format!("invalid object name '{}'.", rev)));
        let (oid, mode) = resolve_path_in_tree(repo, tree_oid, path)
            .unwrap_or_else(|_| fatal(&format!("path '{}' does not exist in '{}'", path, rev)));
        return Ok((oid, mode, path.to_owned()));
    }

    if resolve_object(repo, obj_str).is_ok() {
        fatal(&format!(
            "<object>:<path> required, only <object> '{}' given",
            obj_str
        ));
    }

    fatal(&format!("Not a valid object name {obj_str}"));
}

fn resolve_batch_transform_path(args: &Args, rest: &str, oid: &ObjectId) -> String {
    if let Some(path) = args.path.as_deref() {
        return path.to_owned();
    }
    if !rest.is_empty() {
        return rest.to_owned();
    }
    fatal(&format!("missing path for '{}'", oid));
}

fn transform_content(
    _repo: &Repository,
    transform_ctx: &TransformContext,
    oid: &ObjectId,
    path: &str,
    mode: u32,
    obj: &grit_lib::objects::Object,
    filters_mode: bool,
) -> Result<Vec<u8>> {
    if obj.kind != ObjectKind::Blob || !is_regular_mode(mode) {
        return Ok(obj.data.clone());
    }

    if filters_mode {
        let attrs = get_file_attrs(&transform_ctx.attrs, path, &transform_ctx.config);
        let oid_hex = oid.to_string();
        return Ok(convert_to_worktree(
            &obj.data,
            path,
            &transform_ctx.conversion,
            &attrs,
            Some(&oid_hex),
        ));
    }

    let Some(command) = resolve_textconv_command(transform_ctx, path) else {
        return Ok(obj.data.clone());
    };

    // Git textconv runs on a worktree-view tempfile.
    let attrs = get_file_attrs(&transform_ctx.attrs, path, &transform_ctx.config);
    let oid_hex = oid.to_string();
    let worktree_data = convert_to_worktree(
        &obj.data,
        path,
        &transform_ctx.conversion,
        &attrs,
        Some(&oid_hex),
    );
    run_textconv_command(&command, &worktree_data)
        .map_err(|_| anyhow::anyhow!("could not convert '{}' {}", oid, path))
}

fn run_textconv_command(command: &str, input_data: &[u8]) -> Result<Vec<u8>> {
    let temp_path = create_temp_textconv_file(input_data)?;
    let quoted = shell_quote(temp_path.to_string_lossy().as_ref());
    let shell_command = format!("{command} {quoted}");

    let output = Command::new("sh")
        .arg("-c")
        .arg(&shell_command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("running textconv command '{command}'"))?;

    let _ = std::fs::remove_file(&temp_path);

    if !output.status.success() {
        bail!("textconv command exited with status {}", output.status);
    }

    Ok(output.stdout)
}

fn create_temp_textconv_file(data: &[u8]) -> Result<std::path::PathBuf> {
    let pid = std::process::id();
    for attempt in 0..32u32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grit-textconv-{pid}-{now}-{attempt}"));
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                file.write_all(data)?;
                return Ok(path);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }
    bail!("failed to create temporary textconv input file")
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn resolve_textconv_command(transform_ctx: &TransformContext, path: &str) -> Option<String> {
    let mut selected: Option<DiffAttrValue> = None;
    for rule in &transform_ctx.diff_attrs {
        if diff_attr_pattern_matches(&rule.pattern, path) {
            selected = Some(rule.value.clone());
        }
    }

    match selected {
        Some(DiffAttrValue::Driver(driver)) => {
            transform_ctx.config.get(&format!("diff.{driver}.textconv"))
        }
        _ => None,
    }
}

fn diff_attr_pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains('/') {
        return wildmatch(pattern.as_bytes(), path.as_bytes(), 0);
    }
    let basename = path.rsplit('/').next().unwrap_or(path);
    wildmatch(pattern.as_bytes(), basename.as_bytes(), 0)
}

fn load_attr_rules(repo: &Repository) -> GitAttributes {
    if let Some(work_tree) = repo.work_tree.as_deref() {
        let rules = load_gitattributes(work_tree);
        if !rules.is_empty() {
            return rules;
        }
    }

    if let Ok(index) = Index::load(&repo.index_path()) {
        return load_gitattributes_from_index(&index, &repo.odb);
    }

    Vec::new()
}

fn load_diff_attr_rules(repo: &Repository) -> Vec<DiffAttrRule> {
    let mut rules = Vec::new();

    if let Some(work_tree) = repo.work_tree.as_deref() {
        parse_diff_attr_file(&work_tree.join(".gitattributes"), &mut rules);
        parse_diff_attr_file(&work_tree.join(".git/info/attributes"), &mut rules);
    }

    if rules.is_empty() {
        if let Ok(index) = Index::load(&repo.index_path()) {
            if let Some(entry) = index.get(b".gitattributes", 0) {
                if let Ok(obj) = repo.odb.read(&entry.oid) {
                    if let Ok(content) = String::from_utf8(obj.data) {
                        parse_diff_attr_content(&content, &mut rules);
                    }
                }
            }
        }
        parse_diff_attr_file(&repo.git_dir.join("info/attributes"), &mut rules);
    }

    rules
}

fn parse_diff_attr_file(path: &Path, rules: &mut Vec<DiffAttrRule>) {
    if let Ok(content) = std::fs::read_to_string(path) {
        parse_diff_attr_content(&content, rules);
    }
}

fn parse_diff_attr_content(content: &str, rules: &mut Vec<DiffAttrRule>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            continue;
        };

        let mut value: Option<DiffAttrValue> = None;
        for token in parts {
            if token == "binary" || token == "-diff" {
                value = Some(DiffAttrValue::Unset);
            } else if token == "diff" {
                value = Some(DiffAttrValue::Set);
            } else if let Some(driver) = token.strip_prefix("diff=") {
                value = Some(DiffAttrValue::Driver(driver.to_owned()));
            }
        }

        if let Some(value) = value {
            rules.push(DiffAttrRule {
                pattern: pattern.to_owned(),
                value,
            });
        }
    }
}

fn is_regular_mode(mode: u32) -> bool {
    mode & 0o170000 == 0o100000
}

fn peel_to_tree_oid(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Commit => return Ok(parse_commit(&obj.data)?.tree),
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data)?;
                oid = tag.object;
            }
            _ => bail!("not a tree-ish"),
        }
    }
}

fn resolve_path_in_tree(
    repo: &Repository,
    tree_oid: ObjectId,
    path: &str,
) -> Result<(ObjectId, u32)> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if parts.is_empty() {
        bail!("empty path");
    }

    let mut current_tree = tree_oid;
    for (index, part) in parts.iter().enumerate() {
        let tree_obj = repo.odb.read(&current_tree)?;
        let entries = parse_tree(&tree_obj.data)?;
        let Some(entry) = entries.iter().find(|entry| entry.name == part.as_bytes()) else {
            bail!("path missing");
        };
        if index == parts.len() - 1 {
            return Ok((entry.oid, entry.mode));
        }
        if entry.mode != 0o040000 {
            bail!("path missing");
        }
        current_tree = entry.oid;
    }

    bail!("path missing")
}

fn fatal(msg: &str) -> ! {
    eprintln!("fatal: {msg}");
    std::process::exit(128);
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
    rev_parse::resolve_revision(repo, obj_str).map_err(|e| anyhow::anyhow!("{}", e))
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
