//! `grit test-tool ref-store` support used by upstream ref-store tests.

use anyhow::{bail, Context, Result};
use grit_lib::objects::ObjectId;
use grit_lib::refs::{read_ref_file, Ref};
use grit_lib::repo::Repository;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Run `grit test-tool ref-store`.
pub fn run(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    if args.len() < 2 {
        bail!("usage: test-tool ref-store <store> <function> [args...]");
    }

    let store = open_store(&repo, &args[0])?;
    match args[1].as_str() {
        "resolve-ref" => cmd_resolve_ref(&store, &args[2..]),
        "create-symref" => cmd_create_symref(&store, &args[2..]),
        other => bail!("test-tool ref-store: unknown function '{other}'"),
    }
}

#[derive(Debug, Clone)]
struct RefStore {
    git_dir: PathBuf,
    common_dir: PathBuf,
}

fn open_store(repo: &Repository, spec: &str) -> Result<RefStore> {
    let common_dir = common_dir(&repo.git_dir)?;
    let git_dir = match spec {
        "main" | "worktree:main" => common_dir.clone(),
        _ => {
            let Some(id) = spec.strip_prefix("worktree:") else {
                bail!("unknown backend {spec}");
            };
            let admin_dir = common_dir.join("worktrees").join(id);
            if !admin_dir.join("HEAD").exists() {
                bail!("no such worktree: {id}");
            }
            admin_dir
        }
    };

    Ok(RefStore {
        git_dir,
        common_dir,
    })
}

fn cmd_resolve_ref(store: &RefStore, args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!("usage: test-tool ref-store <store> resolve-ref <refname> <flags>");
    }
    if args[1] != "0" {
        bail!("unknown resolve-ref flags '{}'", args[1]);
    }

    let resolved = resolve_ref_for_store(store, &args[0], 0)?;
    println!("{} {} 0x{:x}", resolved.oid, resolved.name, resolved.flags);
    Ok(())
}

fn cmd_create_symref(store: &RefStore, args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!("usage: test-tool ref-store <store> create-symref <refname> <target> [logmsg]");
    }

    let refname = &args[0];
    let target = &args[1];
    let base_dir = if is_per_worktree_ref(refname) {
        &store.git_dir
    } else {
        &store.common_dir
    };
    let path = base_dir.join(refname);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = path.with_extension("lock");
    fs::write(&lock_path, format!("ref: {target}\n"))?;
    fs::rename(lock_path, path)?;
    Ok(())
}

#[derive(Debug)]
struct ResolvedRef {
    oid: ObjectId,
    name: String,
    flags: u32,
}

fn resolve_ref_for_store(store: &RefStore, refname: &str, depth: usize) -> Result<ResolvedRef> {
    if depth > 10 {
        bail!("ref symlink too deep: {refname}");
    }

    match read_loose_ref_for_store(store, refname)? {
        Ok(Ref::Direct(oid)) => {
            return Ok(ResolvedRef {
                oid,
                name: refname.to_owned(),
                flags: 0,
            });
        }
        Ok(Ref::Symbolic(target)) => {
            let resolved = resolve_ref_for_store(store, &target, depth + 1)?;
            return Ok(ResolvedRef {
                oid: resolved.oid,
                name: target,
                flags: 0x1,
            });
        }
        Err(grit_lib::error::Error::Io(err)) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let packed_dir = if is_per_worktree_ref(refname) {
        &store.git_dir
    } else {
        &store.common_dir
    };
    if let Some(oid) = lookup_packed_ref(packed_dir, refname)? {
        return Ok(ResolvedRef {
            oid,
            name: refname.to_owned(),
            flags: 0,
        });
    }

    bail!("ref not found: {refname}")
}

fn read_loose_ref_for_store(
    store: &RefStore,
    refname: &str,
) -> Result<std::result::Result<Ref, grit_lib::error::Error>> {
    let git_dir_path = store.git_dir.join(refname);
    match read_ref_file(&git_dir_path) {
        Ok(reference) => return Ok(Ok(reference)),
        Err(grit_lib::error::Error::Io(err)) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Ok(Err(err)),
    }

    if is_per_worktree_ref(refname) || store.git_dir == store.common_dir {
        return Ok(Err(grit_lib::error::Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing ref: {refname}"),
        ))));
    }

    Ok(read_ref_file(&store.common_dir.join(refname)))
}

fn common_dir(git_dir: &Path) -> Result<PathBuf> {
    let commondir = git_dir.join("commondir");
    if !commondir.exists() {
        return Ok(git_dir.to_path_buf());
    }

    let raw = fs::read_to_string(&commondir).context("reading commondir")?;
    let rel = raw.trim();
    let path = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    path.canonicalize().context("canonicalizing common dir")
}

fn is_per_worktree_ref(refname: &str) -> bool {
    !refname.starts_with("refs/")
        || refname.starts_with("refs/bisect/")
        || refname.starts_with("refs/worktree/")
        || refname.starts_with("refs/rewritten/")
}

fn lookup_packed_ref(git_dir: &Path, refname: &str) -> Result<Option<ObjectId>> {
    let packed = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    for line in content.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let Some(oid_hex) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        if name.trim() != refname {
            continue;
        }

        let oid: ObjectId = oid_hex.parse()?;
        return Ok(Some(oid));
    }

    Ok(None)
}
