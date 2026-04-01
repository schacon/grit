//! `gust update-ref` — update the object name stored in a ref safely.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};

use gust_lib::objects::ObjectId;
use gust_lib::refs::{append_reflog, delete_ref, resolve_ref, write_ref};
use gust_lib::repo::Repository;

/// Arguments for `gust update-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Delete the ref (use with --stdin or as: update-ref -d <ref>).
    #[arg(short = 'd')]
    pub delete: bool,

    /// Do not dereference symbolic refs.
    #[arg(long = "no-deref")]
    pub no_deref: bool,

    /// Read commands from stdin.
    #[arg(long)]
    pub stdin: bool,

    /// Use NUL as line terminator.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Log message for reflog.
    #[arg(short = 'm', long = "message")]
    pub log_message: Option<String>,

    /// The reference to update.
    pub refname: Option<String>,

    /// The new value (SHA-1 or ref name).
    pub new_value: Option<String>,

    /// The expected old value (SHA-1).
    pub old_value: Option<String>,
}

/// Run `gust update-ref`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    if args.stdin {
        return run_batch(&repo, &args);
    }

    let refname = args
        .refname
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ref name required"))?;

    if args.delete {
        // Verify old value if provided
        if let Some(old) = &args.old_value {
            let current = resolve_ref(&repo.git_dir, refname).ok();
            let expected: ObjectId = old.parse().context("invalid old-value OID")?;
            if current != Some(expected) {
                bail!("ref '{refname}' does not point to expected value");
            }
        }
        delete_ref(&repo.git_dir, refname).context("deleting ref")?;
        return Ok(());
    }

    let new_str = args
        .new_value
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("new value required"))?;
    let new_oid: ObjectId = resolve_oid_or_ref(&repo, new_str)?;

    // Verify old value
    if let Some(old_str) = &args.old_value {
        let current = resolve_ref(&repo.git_dir, refname).ok();
        let expected: ObjectId = old_str.parse().context("invalid old-value OID")?;
        if current != Some(expected) {
            bail!("ref '{refname}' does not point to expected value");
        }
    }

    let old_oid = resolve_ref(&repo.git_dir, refname)
        .unwrap_or_else(|_| ObjectId::from_bytes(&[0u8; 20]).unwrap_or_else(|_| unreachable!()));

    write_ref(&repo.git_dir, refname, &new_oid).context("writing ref")?;

    // Reflog
    if let Some(msg) = &args.log_message {
        let identity = "gust <gust> 0 +0000";
        let _ = append_reflog(&repo.git_dir, refname, &old_oid, &new_oid, identity, msg);
    }

    Ok(())
}

/// Process `--stdin` batch commands.
fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let stdin = io::stdin();
    let lines: Vec<String> = stdin.lock().lines().collect::<std::io::Result<_>>()?;

    for line in &lines {
        let parts: Vec<&str> = line.trim().splitn(4, ' ').collect();
        if parts.is_empty() || parts[0].is_empty() {
            continue;
        }

        match parts[0] {
            "update" => {
                if parts.len() < 3 {
                    bail!("update requires ref and new-value");
                }
                let refname = parts[1];
                let new_oid = resolve_oid_or_ref(repo, parts[2])?;
                let old_oid = if parts.len() >= 4 && !parts[3].is_empty() {
                    let o: ObjectId = parts[3].parse().context("invalid old-value")?;
                    let current = resolve_ref(&repo.git_dir, refname).ok();
                    if current != Some(o) {
                        bail!("ref '{refname}' does not point to expected value");
                    }
                    current
                } else {
                    resolve_ref(&repo.git_dir, refname).ok()
                };
                let old = old_oid.unwrap_or_else(|| {
                    ObjectId::from_bytes(&[0u8; 20]).unwrap_or_else(|_| unreachable!())
                });
                write_ref(&repo.git_dir, refname, &new_oid)?;
                if let Some(msg) = &args.log_message {
                    let _ = append_reflog(
                        &repo.git_dir,
                        refname,
                        &old,
                        &new_oid,
                        "gust <gust> 0 +0000",
                        msg,
                    );
                }
            }
            "create" => {
                if parts.len() < 3 {
                    bail!("create requires ref and new-value");
                }
                let refname = parts[1];
                // create fails if ref already exists
                if resolve_ref(&repo.git_dir, refname).is_ok() {
                    bail!("ref '{refname}' already exists");
                }
                let new_oid = resolve_oid_or_ref(repo, parts[2])?;
                write_ref(&repo.git_dir, refname, &new_oid)?;
            }
            "delete" => {
                if parts.len() < 2 {
                    bail!("delete requires ref");
                }
                let refname = parts[1];
                if parts.len() >= 3 && !parts[2].is_empty() {
                    let expected: ObjectId = parts[2].parse().context("invalid old-value")?;
                    let current = resolve_ref(&repo.git_dir, refname).ok();
                    if current != Some(expected) {
                        bail!("ref '{refname}' does not point to expected value");
                    }
                }
                delete_ref(&repo.git_dir, refname)?;
            }
            "verify" => {
                if parts.len() < 2 {
                    bail!("verify requires ref");
                }
                let refname = parts[1];
                if parts.len() >= 3 && !parts[2].is_empty() {
                    let expected: ObjectId = parts[2].parse().context("invalid old-value")?;
                    let current = resolve_ref(&repo.git_dir, refname)
                        .map_err(|_| anyhow::anyhow!("ref '{refname}' does not exist"))?;
                    if current != expected {
                        bail!("ref '{refname}' does not match expected value");
                    }
                }
            }
            "abort" => break,
            other => bail!("unknown batch command: {other}"),
        }
    }

    Ok(())
}

fn resolve_oid_or_ref(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return Ok(oid);
    }
    bail!("not a valid object name: '{s}'")
}
