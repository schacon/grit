//! `gust cat-file` — provide contents or details of repository objects.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead, Write};

use gust_lib::objects::{parse_tree, ObjectId, ObjectKind};
use gust_lib::repo::Repository;

/// Arguments for `gust cat-file`.
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
    #[arg(long, conflicts_with_all = ["show_type", "size", "pretty", "exists"])]
    pub batch: bool,

    /// Print info (type, size) for each object ID on stdin.
    #[arg(
        long = "batch-check",
        num_args = 0..=1,
        default_missing_value = "",
        conflicts_with_all = ["show_type", "size", "pretty", "exists", "batch"]
    )]
    pub batch_check: Option<String>,

    /// Read commands from stdin.
    #[arg(long, conflicts_with_all = ["show_type", "size", "pretty", "exists", "batch", "batch_check"])]
    pub batch_command: bool,

    /// Buffer output in batch-command mode; use `flush` command to emit output.
    #[arg(long, requires = "batch_command", conflicts_with = "no_buffer")]
    pub buffer: bool,

    /// Do not buffer output in batch-command mode.
    #[arg(long, requires = "batch_command", conflicts_with = "buffer")]
    pub no_buffer: bool,

    /// Follow tag objects to the tagged object.
    #[arg(long = "follow-symlinks")]
    pub follow_symlinks: bool,

    /// `<object>` or `<type> <object>` (Git-compatible); not used with batch modes.
    #[arg(trailing_var_arg = true, value_name = "args")]
    pub rest: Vec<String>,
}

/// Run `gust cat-file`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let batch_mode = args.batch || args.batch_check.is_some() || args.batch_command;
    if batch_mode {
        if !args.rest.is_empty() {
            bail!("unexpected argument: {}", args.rest[0]);
        }
        return run_batch(&repo, &args);
    }

    let (expected_kind, obj_str) = match args.rest.len() {
        0 => bail!("object required when not in batch mode"),
        1 => (None, args.rest[0].as_str()),
        2 => {
            if args.show_type || args.size || args.pretty || args.exists {
                bail!("cannot use <type> <object> form with -t, -s, -p, or -e");
            }
            (
                Some(parse_object_kind(&args.rest[0])?),
                args.rest[1].as_str(),
            )
        }
        _ => bail!("too many arguments (expected at most 2)"),
    };

    let oid = resolve_object(&repo, obj_str)?;
    let obj = repo.odb.read(&oid)?;

    if let Some(kind) = expected_kind {
        if obj.kind != kind {
            bail!("object {oid} is a {}, not a {}", obj.kind, kind);
        }
    }

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

    // Default: print raw content
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(&obj.data)?;

    Ok(())
}

fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let buffered_mode = args.batch_command && args.buffer && !args.no_buffer;
    let mut out: Box<dyn Write> = if buffered_mode {
        Box::new(io::BufWriter::new(stdout.lock()))
    } else {
        Box::new(stdout.lock())
    };

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim_end();

        if args.batch_command {
            // <command> <args>
            let mut parts = trimmed.trim_start().splitn(2, char::is_whitespace);
            match parts.next() {
                Some("contents") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    print_batch_entry_default(repo, obj_str, true, &mut out)?;
                    if !buffered_mode {
                        out.flush()?;
                    }
                }
                Some("info") => {
                    let obj_str = parts.next().unwrap_or("").trim();
                    print_batch_entry_default(repo, obj_str, false, &mut out)?;
                    if !buffered_mode {
                        out.flush()?;
                    }
                }
                Some("flush") => {
                    if !buffered_mode {
                        bail!("flush is only valid in --buffer mode");
                    }
                    out.flush()?;
                }
                Some(other) => bail!("unknown batch command: {other}"),
                None => {}
            }
        } else {
            let (obj_str, rest) = split_batch_input(trimmed);
            let batch_check_format =
                args.batch_check
                    .as_deref()
                    .and_then(|s| if s.is_empty() { None } else { Some(s) });
            print_batch_entry(
                repo,
                obj_str,
                rest,
                args.batch,
                batch_check_format,
                &mut out,
            )?;
        }
    }

    out.flush()?;

    Ok(())
}

fn print_batch_entry(
    repo: &Repository,
    obj_str: &str,
    rest: &str,
    include_content: bool,
    batch_check_format: Option<&str>,
    out: &mut impl Write,
) -> Result<()> {
    match resolve_object(repo, obj_str) {
        Err(_) => {
            writeln!(out, "{obj_str} missing")?;
        }
        Ok(oid) => match repo.odb.read(&oid) {
            Err(_) => {
                writeln!(out, "{obj_str} missing")?;
            }
            Ok(obj) => {
                if include_content {
                    writeln!(out, "{} {} {}", oid, obj.kind, obj.data.len())?;
                } else if let Some(fmt) = batch_check_format {
                    writeln!(
                        out,
                        "{}",
                        render_batch_check_format(fmt, &oid, &obj.kind, obj.data.len(), rest)
                    )?;
                } else {
                    writeln!(out, "{} {} {}", oid, obj.kind, obj.data.len())?;
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

fn print_batch_entry_default(
    repo: &Repository,
    obj_str: &str,
    include_content: bool,
    out: &mut impl Write,
) -> Result<()> {
    print_batch_entry(repo, obj_str, "", include_content, None, out)
}

fn split_batch_input(line: &str) -> (&str, &str) {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return ("", "");
    }

    let oid_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let obj = &trimmed[..oid_end];
    let rest = trimmed[oid_end..].trim_start();
    (obj, rest)
}

fn render_batch_check_format(
    fmt: &str,
    oid: &ObjectId,
    kind: &ObjectKind,
    size: usize,
    rest: &str,
) -> String {
    fmt.replace("%(objecttype)", &kind.to_string())
        .replace("%(objectname)", &oid.to_string())
        .replace("%(objectsize)", &format!("{size}"))
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
/// Handles full hex OIDs and simple ref names.
fn resolve_object(repo: &Repository, obj_str: &str) -> Result<ObjectId> {
    // Try as a raw OID first
    if let Ok(oid) = obj_str.parse::<ObjectId>() {
        return Ok(oid);
    }

    // Try resolving as a ref
    if let Ok(oid) = gust_lib::refs::resolve_ref(&repo.git_dir, obj_str) {
        return Ok(oid);
    }

    // Try "refs/heads/<name>"
    let as_branch = format!("refs/heads/{obj_str}");
    if let Ok(oid) = gust_lib::refs::resolve_ref(&repo.git_dir, &as_branch) {
        return Ok(oid);
    }

    bail!("not a valid object name: '{obj_str}'")
}

fn parse_object_kind(value: &str) -> Result<ObjectKind> {
    match value {
        "blob" => Ok(ObjectKind::Blob),
        "tree" => Ok(ObjectKind::Tree),
        "commit" => Ok(ObjectKind::Commit),
        "tag" => Ok(ObjectKind::Tag),
        _ => bail!("invalid object type '{value}'"),
    }
}
