//! `gust hash-object` — compute object ID and optionally create an object from a file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use gust_lib::objects::{parse_commit, ObjectKind};
use gust_lib::odb::Odb;
use gust_lib::repo::Repository;

/// Arguments for `gust hash-object`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Object type (blob, tree, commit, tag).
    #[arg(short = 't', default_value = "blob", value_name = "type")]
    pub object_type: String,

    /// Write the object to the object store.
    #[arg(short = 'w')]
    pub write: bool,

    /// Read object from stdin (may be repeated only once; use with files to hash stdin then files).
    #[arg(long, action = clap::ArgAction::Count, conflicts_with = "stdin_paths")]
    pub stdin: u8,

    /// Read file paths from stdin (one per line).
    #[arg(long = "stdin-paths", conflicts_with_all = ["stdin", "files"])]
    pub stdin_paths: bool,

    /// Don't validate file content, just hash it (with --literally).
    #[arg(long)]
    pub literally: bool,

    /// Filter path (unsupported; rejected with `--stdin-paths` / `--no-filters`).
    #[arg(long = "path", value_name = "path", hide = true)]
    pub path: Option<String>,

    /// Skip filters (unsupported; rejected with `--path`).
    #[arg(long = "no-filters", hide = true)]
    pub no_filters: bool,

    /// File(s) to hash.
    pub files: Vec<PathBuf>,
}

/// Run `gust hash-object`.
pub fn run(args: Args) -> Result<()> {
    if args.stdin > 1 {
        bail!("multiple '--stdin' are not allowed");
    }
    if args.stdin > 0 && args.stdin_paths {
        bail!("cannot use --stdin and --stdin-paths together");
    }
    if args.stdin_paths && !args.files.is_empty() {
        bail!("cannot pass filenames with --stdin-paths");
    }
    if args.stdin_paths && args.path.is_some() {
        bail!("cannot use --path with --stdin-paths");
    }
    if args.no_filters && args.path.is_some() {
        bail!("cannot use --path with --no-filters");
    }

    let kind = ObjectKind::from_str(&args.object_type)
        .with_context(|| format!("unknown object type '{}'", args.object_type))?;

    let odb = if args.write {
        let repo = Repository::discover(None).context("not a git repository")?;
        Some(repo.odb)
    } else {
        None
    };

    if args.stdin_paths {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading stdin paths")?;
        for line in buf.lines() {
            let path = PathBuf::from(line);
            let data = std::fs::read(&path)
                .with_context(|| format!("cannot read '{}'", path.display()))?;
            validate_object_payload(kind, &data)?;
            let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
            println!("{oid}");
        }
        return Ok(());
    }

    if args.stdin > 0 {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .context("reading stdin")?;
        validate_object_payload(kind, &data)?;
        let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
        println!("{oid}");
    }

    for path in &args.files {
        let data =
            std::fs::read(path).with_context(|| format!("cannot read '{}'", path.display()))?;
        validate_object_payload(kind, &data)?;
        let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
        println!("{oid}");
    }

    if args.stdin == 0 && args.files.is_empty() && !args.stdin_paths {
        bail!("no object to hash (provide file, --stdin, or --stdin-paths)");
    }

    Ok(())
}

fn validate_object_payload(kind: ObjectKind, data: &[u8]) -> Result<()> {
    match kind {
        ObjectKind::Commit => {
            parse_commit(data)
                .map(|_| ())
                .context("invalid commit object")?;
        }
        ObjectKind::Tag => {
            if data.is_empty() {
                bail!("invalid tag object");
            }
            let text = std::str::from_utf8(data).context("invalid tag object")?;
            if !(text.contains("object ") && text.contains("type ") && text.contains("tag ")) {
                bail!("invalid tag object");
            }
        }
        _ => {}
    }
    Ok(())
}

fn hash_and_maybe_write(
    kind: ObjectKind,
    data: &[u8],
    odb: Option<&Odb>,
) -> Result<gust_lib::objects::ObjectId> {
    if let Some(db) = odb {
        db.write(kind, data).context("writing object")
    } else {
        Ok(Odb::hash_object_data(kind, data))
    }
}
