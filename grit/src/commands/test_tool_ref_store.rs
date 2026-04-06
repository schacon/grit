//! `grit test-tool ref-store` support used by upstream ref-store tests.

use anyhow::{bail, Context, Result};
use grit_lib::objects::ObjectId;
use grit_lib::refs::{read_ref_file, Ref};
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Write as _};
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
        "for-each-ref" => cmd_for_each_ref(&store, &args[2..]),
        "for-each-reflog" => cmd_for_each_reflog(&store),
        "for-each-reflog-ent" => cmd_for_each_reflog_ent(&store, &args[2..]),
        "for-each-reflog-ent-reverse" => cmd_for_each_reflog_ent_reverse(&store, &args[2..]),
        "reflog-exists" => cmd_reflog_exists(&store, &args[2..]),
        "verify-ref" => cmd_verify_ref(&store, &args[2..]),
        "delete-reflog" | "create-reflog" | "delete-refs" | "rename-ref" | "pack-refs" => {
            bail!(
                "test-tool ref-store: '{}' not supported on submodule ref store",
                args[1]
            )
        }
        other => bail!("test-tool ref-store: unknown function '{other}'"),
    }
}

#[derive(Debug, Clone)]
struct RefStore {
    git_dir: PathBuf,
    common_dir: PathBuf,
    is_submodule: bool,
}

fn open_store(repo: &Repository, spec: &str) -> Result<RefStore> {
    let common_dir = common_dir(&repo.git_dir)?;
    let git_dir = match spec {
        "main" | "worktree:main" => common_dir.clone(),
        _ if spec.starts_with("submodule:") => {
            // submodule:<name> -> try multiple locations:
            // 1. .git/modules/<name> (standard submodule internals)
            // 2. <name>/.git (direct submodule with separate git dir)
            let name = spec.strip_prefix("submodule:").unwrap();
            let modules_dir = common_dir.join("modules").join(name);
            if modules_dir.join("HEAD").exists() {
                modules_dir
            } else {
                let sub_git = PathBuf::from(name).join(".git");
                if sub_git.join("HEAD").exists() {
                    sub_git
                } else {
                    bail!("no such submodule: {name}");
                }
            }
        }
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

    let is_submodule = spec.starts_with("submodule:");
    Ok(RefStore {
        git_dir,
        common_dir,
        is_submodule,
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
    if store.is_submodule {
        bail!("cannot create symref in submodule ref store");
    }
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

/// List refs matching an optional prefix, outputting "<oid> <refname> <flags>".
fn cmd_for_each_ref(store: &RefStore, args: &[String]) -> Result<()> {
    let prefix = args.first().map(String::as_str).unwrap_or("");
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let base_dir = if store.is_submodule {
        &store.git_dir
    } else {
        &store.common_dir
    };
    let refs_dir = base_dir.join("refs");
    let mut refs = collect_refs(&refs_dir, "", prefix)?;
    // Also check packed-refs
    let packed = base_dir.join("packed-refs");
    if packed.exists() {
        for line in fs::read_to_string(&packed)?.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let (oid_str, refname) = (parts[0], parts[1]);
                if refname.starts_with(prefix) && !refs.iter().any(|(_, r)| r == refname) {
                    refs.push((oid_str.to_owned(), refname.to_owned()));
                }
            }
        }
    }
    refs.sort_by(|a, b| a.1.cmp(&b.1));
    for (oid, refname) in refs {
        // Strip the prefix from the output refname
        let display = refname.strip_prefix(prefix).unwrap_or(&refname);
        writeln!(out, "{oid} {display} 0x0")?;
    }
    Ok(())
}

fn collect_refs(dir: &Path, prefix_path: &str, filter: &str) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    if !dir.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix_path.is_empty() {
            name.clone()
        } else {
            format!("{prefix_path}/{name}")
        };
        let path = entry.path();
        if path.is_dir() {
            result.extend(collect_refs(&path, &rel, filter)?);
        } else if path.is_file() {
            let full_ref = format!("refs/{rel}");
            if filter.is_empty() || full_ref.starts_with(filter) {
                if let Ok(content) = fs::read_to_string(&path) {
                    let oid = content.trim();
                    if oid.len() == 40 && oid.chars().all(|c| c.is_ascii_hexdigit()) {
                        result.push((oid.to_owned(), full_ref));
                    }
                }
            }
        }
    }
    Ok(result)
}

/// List all reflog refs.
fn cmd_for_each_reflog(store: &RefStore) -> Result<()> {
    let logs_dir = store.git_dir.join("logs");
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut logs = collect_log_refs(&logs_dir, "")?;
    logs.sort();
    for r in logs {
        writeln!(out, "{r}")?;
    }
    Ok(())
}

fn collect_log_refs(dir: &Path, prefix: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    if !dir.exists() {
        return Ok(result);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.path().is_dir() {
            result.extend(collect_log_refs(&entry.path(), &rel)?);
        } else {
            result.push(rel);
        }
    }
    Ok(result)
}

/// Print reflog entries for a ref.
fn cmd_for_each_reflog_ent(store: &RefStore, args: &[String]) -> Result<()> {
    let refname = args.first().map(String::as_str).unwrap_or("HEAD");
    let entries = grit_lib::reflog::read_reflog(&store.git_dir, refname)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for e in &entries {
        writeln!(
            out,
            "{} {} {}\t{}",
            e.old_oid.to_hex(),
            e.new_oid.to_hex(),
            e.identity,
            e.message
        )?;
    }
    Ok(())
}

/// Print reflog entries in reverse order.
fn cmd_for_each_reflog_ent_reverse(store: &RefStore, args: &[String]) -> Result<()> {
    let refname = args.first().map(String::as_str).unwrap_or("HEAD");
    let entries = grit_lib::reflog::read_reflog(&store.git_dir, refname)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for e in entries.iter().rev() {
        writeln!(
            out,
            "{} {} {}\t{}",
            e.old_oid.to_hex(),
            e.new_oid.to_hex(),
            e.identity,
            e.message
        )?;
    }
    Ok(())
}

/// Check if reflog exists for a ref.
fn cmd_reflog_exists(store: &RefStore, args: &[String]) -> Result<()> {
    let refname = args.first().map(String::as_str).unwrap_or("HEAD");
    let log_path = store.git_dir.join("logs").join(refname);
    if log_path.exists() {
        Ok(())
    } else {
        anyhow::bail!("reflog does not exist for {refname}");
    }
}

/// Verify a ref exists and output its OID.
fn cmd_verify_ref(store: &RefStore, args: &[String]) -> Result<()> {
    let refname = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("usage: verify-ref <refname>"))?;
    match grit_lib::refs::resolve_ref(&store.git_dir, refname) {
        Ok(oid) => {
            use std::io::Write;
            writeln!(io::stdout(), "{}", oid.to_hex())?;
            Ok(())
        }
        Err(_) => anyhow::bail!("ref {refname} not found"),
    }
}
