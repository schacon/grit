//! `grit symbolic-ref` — read, update, and delete symbolic refs.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::refs::{append_reflog, read_ref_file, Ref};
use grit_lib::repo::Repository;
use std::fs;
use std::io;
use std::path::Path;

/// Arguments for `grit symbolic-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Suppress non-symbolic-ref error output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Delete symbolic ref.
    #[arg(short = 'd', long = "delete")]
    pub delete: bool,

    /// Shorten ref output.
    #[arg(long = "short")]
    pub short: bool,

    /// Stop after one dereference.
    #[arg(long = "no-recurse")]
    pub no_recurse: bool,

    /// Reflog message when updating a symbolic ref.
    #[arg(short = 'm')]
    pub message: Option<String>,

    /// The symbolic ref name.
    pub name: Option<String>,

    /// New target ref.
    pub reference: Option<String>,
}

/// Run `grit symbolic-ref`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    if matches!(args.message.as_deref(), Some("")) {
        bail!("Refusing to perform update with empty message");
    }

    if args.delete {
        let Some(name) = args.name.as_deref() else {
            bail!("usage: grit symbolic-ref --delete [-q] <name>");
        };
        if args.reference.is_some() {
            bail!("usage: grit symbolic-ref --delete [-q] <name>");
        }
        if !is_symbolic_ref(&repo.git_dir, name)? {
            bail!("Cannot delete {name}, not a symbolic ref");
        }
        if name == "HEAD" {
            bail!("deleting '{name}' is not allowed");
        }
        delete_loose_ref(&repo.git_dir, name)?;
        return Ok(());
    }

    match (args.name.as_deref(), args.reference.as_deref()) {
        (Some(name), None) => {
            match read_symbolic_ref_target(&repo.git_dir, name, !args.no_recurse)? {
                Some(target) => {
                    if args.short {
                        println!("{}", shorten_ref(&target));
                    } else {
                        println!("{target}");
                    }
                    Ok(())
                }
                None if args.quiet => {
                    std::process::exit(1);
                }
                None => bail!("ref {name} is not a symbolic ref"),
            }
        }
        (Some(name), Some(target)) => {
            if name == "HEAD" && !target.starts_with("refs/") {
                bail!("Refusing to point HEAD outside of refs/");
            }
            if !is_valid_refname(target, true) {
                bail!("Refusing to set '{name}' to invalid ref '{target}'");
            }
            let old_oid = resolve_for_reflog(&repo, name);
            write_symbolic_ref(&repo.git_dir, name, target)?;
            if let Some(message) = args.message.as_deref() {
                let new_oid = resolve_for_reflog(&repo, name);
                write_symref_reflog(&repo, name, &old_oid, &new_oid, message)?;
            }
            Ok(())
        }
        _ => bail!("usage: grit symbolic-ref [-m <reason>] <name> <ref>"),
    }
}

fn read_symbolic_ref_target(git_dir: &Path, name: &str, recurse: bool) -> Result<Option<String>> {
    let path = git_dir.join(name);
    match read_ref_file(&path) {
        Ok(Ref::Direct(_)) => Ok(None),
        Ok(Ref::Symbolic(mut target)) => {
            if !recurse {
                return Ok(Some(target));
            }
            for _ in 0..10 {
                let next_path = git_dir.join(&target);
                match read_ref_file(&next_path) {
                    Ok(Ref::Direct(_)) => return Ok(Some(target)),
                    Ok(Ref::Symbolic(next)) => target = next,
                    Err(grit_lib::error::Error::Io(err))
                        if err.kind() == io::ErrorKind::NotFound =>
                    {
                        return Ok(Some(target));
                    }
                    Err(_) => return Ok(Some(target)),
                }
            }
            Ok(Some(target))
        }
        Err(grit_lib::error::Error::Io(err)) if err.kind() == io::ErrorKind::NotFound => {
            bail!("No such ref: {name}")
        }
        Err(err) => Err(err.into()),
    }
}

fn is_symbolic_ref(git_dir: &Path, name: &str) -> Result<bool> {
    let path = git_dir.join(name);
    match read_ref_file(&path) {
        Ok(Ref::Symbolic(_)) => Ok(true),
        Ok(Ref::Direct(_)) => Ok(false),
        Err(grit_lib::error::Error::Io(err)) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn write_symbolic_ref(git_dir: &Path, name: &str, target: &str) -> Result<()> {
    let path = git_dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = path.with_extension("lock");
    fs::write(&lock_path, format!("ref: {target}\n"))?;
    fs::rename(lock_path, path)?;
    Ok(())
}

fn delete_loose_ref(git_dir: &Path, name: &str) -> Result<()> {
    let path = git_dir.join(name);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn write_symref_reflog(
    repo: &Repository,
    name: &str,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    message: &str,
) -> Result<()> {
    append_reflog(
        &repo.git_dir,
        name,
        old_oid,
        new_oid,
        "grit <grit> 0 +0000",
        message,
    )?;
    Ok(())
}

fn resolve_for_reflog(repo: &Repository, name: &str) -> ObjectId {
    match grit_lib::refs::resolve_ref(&repo.git_dir, name) {
        Ok(oid) => oid,
        Err(_) => zero_oid(),
    }
}

fn zero_oid() -> ObjectId {
    match ObjectId::from_bytes(&[0u8; 20]) {
        Ok(oid) => oid,
        Err(_) => unreachable!("20-byte zero OID is always valid"),
    }
}

fn is_valid_refname(name: &str, allow_onelevel: bool) -> bool {
    if name.is_empty()
        || name.starts_with('/')
        || name.ends_with('/')
        || name.contains("//")
        || name.contains("..")
        || name.contains("@{")
        || name.ends_with(".lock")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        return false;
    }
    if !allow_onelevel && !name.contains('/') {
        return false;
    }
    for comp in name.split('/') {
        if comp.is_empty()
            || comp == "."
            || comp == ".."
            || comp.starts_with('.')
            || comp.ends_with('.')
        {
            return false;
        }
    }
    true
}

fn shorten_ref(name: &str) -> String {
    for prefix in ["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if prefix == "refs/remotes/" {
                if let Some((remote, tail)) = rest.split_once("/HEAD") {
                    if tail.is_empty() {
                        return remote.to_owned();
                    }
                }
                return rest.to_owned();
            }
            return rest.to_owned();
        }
    }
    if let Some(rest) = name.strip_prefix("refs/") {
        return rest.to_owned();
    }
    name.to_owned()
}
