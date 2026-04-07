//! `grit hash-object` — compute object ID and optionally write to object store.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use grit_lib::config::ConfigSet;
use grit_lib::crlf;
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use std::borrow::Cow;

/// Arguments for `grit hash-object`.
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

    /// Logical path for clean filters / attributes (matches Git `--path=`).
    #[arg(long = "path", value_name = "path")]
    pub hash_path: Option<String>,

    /// Hash raw bytes without clean filters (matches Git `--no-filters`).
    #[arg(long = "no-filters")]
    pub no_filters: bool,

    /// File(s) to hash.
    pub files: Vec<PathBuf>,
}

/// Run `grit hash-object`.
pub fn run(args: Args) -> Result<()> {
    if args.stdin && args.stdin_paths {
        bail!("options '--stdin' and '--stdin-paths' cannot be used together");
    }
    if args.stdin_paths && !args.files.is_empty() {
        bail!("can't pass filenames with --stdin-paths");
    }
    if args.stdin_paths && args.hash_path.is_some() {
        bail!("Can't use --stdin-paths with --path");
    }
    if args.hash_path.is_some() && args.no_filters {
        bail!("Can't use --path with --no-filters");
    }

    let kind = ObjectKind::from_str(&args.object_type)
        .with_context(|| format!("unknown object type '{}'", args.object_type))?;

    let repo_opt = if args.write || (!args.literally && !args.no_filters) {
        Repository::discover(None).ok()
    } else {
        None
    };

    // We only need the odb if -w is given
    let odb = if args.write {
        let repo = repo_opt
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("not a git repository"))?;
        Some(repo.odb.clone())
    } else {
        None
    };

    let apply_filters = !args.literally && !args.no_filters;

    if args.stdin {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .context("reading stdin")?;
        let data = prepare_blob_data(
            &data,
            args.hash_path.as_deref(),
            repo_opt.as_ref(),
            apply_filters,
        )?;
        validate_object_data(kind, &data, args.literally)?;
        let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
        println!("{oid}");
        for path in &args.files {
            let file_data =
                std::fs::read(path).with_context(|| format!("cannot read '{}'", path.display()))?;
            let logical = logical_path_for_hash(args.hash_path.as_deref(), path, apply_filters);
            let file_data = prepare_blob_data(
                &file_data,
                logical.as_deref(),
                repo_opt.as_ref(),
                apply_filters,
            )?;
            validate_object_data(kind, &file_data, args.literally)?;
            let file_oid = hash_and_maybe_write(kind, &file_data, odb.as_ref())?;
            println!("{file_oid}");
        }
    } else if args.stdin_paths {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading stdin paths")?;
        for line in buf.lines() {
            let path = PathBuf::from(line);
            let data = std::fs::read(&path)
                .with_context(|| format!("cannot read '{}'", path.display()))?;
            let logical = logical_path_for_hash(None, &path, apply_filters);
            let data =
                prepare_blob_data(&data, logical.as_deref(), repo_opt.as_ref(), apply_filters)?;
            validate_object_data(kind, &data, args.literally)?;
            let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
            println!("{oid}");
        }
    } else {
        for path in &args.files {
            let data =
                std::fs::read(path).with_context(|| format!("cannot read '{}'", path.display()))?;
            let logical = logical_path_for_hash(args.hash_path.as_deref(), path, apply_filters);
            let data =
                prepare_blob_data(&data, logical.as_deref(), repo_opt.as_ref(), apply_filters)?;
            validate_object_data(kind, &data, args.literally)?;
            let oid = hash_and_maybe_write(kind, &data, odb.as_ref())?;
            println!("{oid}");
        }
    }

    Ok(())
}

fn logical_path_for_hash(
    explicit: Option<&str>,
    file: &std::path::Path,
    apply_filters: bool,
) -> Option<String> {
    if !apply_filters {
        return None;
    }
    explicit
        .map(String::from)
        .or_else(|| file.to_str().map(std::string::ToString::to_string))
}

/// Apply the same clean pipeline as `git add` / Git's `hash-object` when a logical path is known.
fn prepare_blob_data(
    data: &[u8],
    logical_path: Option<&str>,
    repo: Option<&Repository>,
    apply_filters: bool,
) -> Result<Vec<u8>> {
    if !apply_filters {
        return Ok(data.to_vec());
    }
    let Some(path) = logical_path else {
        return Ok(data.to_vec());
    };
    let Some(repo) = repo else {
        return Ok(data.to_vec());
    };
    let attrs = repo
        .work_tree
        .as_ref()
        .map(|wt| crlf::load_gitattributes(wt))
        .unwrap_or_default();
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let conv = crlf::ConversionConfig::from_config(&config);
    let file_attrs = crlf::get_file_attrs(&attrs, path, &config);
    let raw: Cow<'_, [u8]> = if let Some(ref enc) = file_attrs.working_tree_encoding {
        Cow::Owned(crate::commands::add::convert_from_working_tree_encoding(
            data, enc,
        )?)
    } else {
        Cow::Borrowed(data)
    };
    crlf::convert_to_git(raw.as_ref(), path, &conv, &file_attrs).map_err(anyhow::Error::msg)
}

fn validate_object_data(kind: ObjectKind, data: &[u8], literally: bool) -> Result<()> {
    if literally {
        return Ok(());
    }
    match kind {
        ObjectKind::Commit => {
            parse_commit(data).context("corrupt commit object")?;
            Ok(())
        }
        ObjectKind::Tag => validate_tag_data(data),
        ObjectKind::Tree => {
            parse_tree(data).context("corrupt tree object")?;
            Ok(())
        }
        ObjectKind::Blob => Ok(()),
    }
}

fn validate_tag_data(data: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(data).context("corrupt tag object")?;
    let mut has_object = false;
    let mut has_type = false;
    let mut has_tag = false;
    let mut has_tagger = false;
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if line.starts_with("object ") {
            has_object = true;
        } else if line.starts_with("type ") {
            has_type = true;
        } else if line.starts_with("tag ") {
            has_tag = true;
        } else if line.starts_with("tagger ") {
            has_tagger = true;
        }
    }
    if has_object && has_type && has_tag && has_tagger {
        return Ok(());
    }
    anyhow::bail!("corrupt tag object")
}

fn hash_and_maybe_write(
    kind: ObjectKind,
    data: &[u8],
    odb: Option<&Odb>,
) -> Result<grit_lib::objects::ObjectId> {
    if let Some(db) = odb {
        db.write(kind, data).context("writing object")
    } else {
        Ok(Odb::hash_object_data(kind, data))
    }
}
