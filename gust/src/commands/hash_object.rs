//! `gust hash-object` — compute object ID and optionally write to object store.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use gust_lib::objects::ObjectKind;
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

    /// Read object from stdin.
    #[arg(long)]
    pub stdin: bool,

    /// Read file paths from stdin (one per line).
    #[arg(long = "stdin-paths")]
    pub stdin_paths: bool,

    /// Don't validate file content, just hash it (with --literally).
    #[arg(long)]
    pub literally: bool,

    /// File(s) to hash.
    pub files: Vec<PathBuf>,
}

/// Run `gust hash-object`.
pub fn run(args: Args) -> Result<()> {
    let kind = ObjectKind::from_str(&args.object_type)
        .with_context(|| format!("unknown object type '{}'", args.object_type))?;

    // We only need the odb if -w is given
    let odb = if args.write {
        let repo = Repository::discover(None).context("not a git repository")?;
        Some(repo.odb)
    } else {
        None
    };

    if args.stdin {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .context("reading stdin")?;
        let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
        println!("{oid}");
    } else if args.stdin_paths {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading stdin paths")?;
        for line in buf.lines() {
            let path = PathBuf::from(line);
            let data = std::fs::read(&path)
                .with_context(|| format!("cannot read '{}'", path.display()))?;
            let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
            println!("{oid}");
        }
    } else {
        for path in &args.files {
            let data =
                std::fs::read(path).with_context(|| format!("cannot read '{}'", path.display()))?;
            let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
            println!("{oid}");
        }
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
