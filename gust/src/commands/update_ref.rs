//! `gust update-ref` — update the object name stored in a ref safely.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead};

use gust_lib::objects::ObjectId;
use gust_lib::refs::{append_reflog, delete_ref, read_head, resolve_ref, write_ref};
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
    let target_ref = deref_refname(&repo, refname, args.no_deref)?;

    if args.delete {
        let expected_old = args.new_value.as_deref().or(args.old_value.as_deref());

        // Verify old value if provided
        if let Some(old) = expected_old {
            let current = resolve_ref(&repo.git_dir, &target_ref).ok();
            let expected: ObjectId = old.parse().context("invalid old-value OID")?;
            if !matches_expected_old(current, expected) {
                bail!("ref '{refname}' does not point to expected value");
            }
        }
        delete_ref(&repo.git_dir, &target_ref).context("deleting ref")?;
        return Ok(());
    }

    let new_str = args
        .new_value
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("new value required"))?;
    let new_oid: ObjectId = resolve_oid_or_ref(&repo, new_str)?;

    // Verify old value
    if let Some(old_str) = &args.old_value {
        let current = resolve_ref(&repo.git_dir, &target_ref).ok();
        let expected: ObjectId = old_str.parse().context("invalid old-value OID")?;
        if !matches_expected_old(current, expected) {
            bail!("ref '{refname}' does not point to expected value");
        }
    }

    let old_oid = resolve_ref(&repo.git_dir, &target_ref).unwrap_or_else(|_| zero_oid());

    write_ref(&repo.git_dir, &target_ref, &new_oid).context("writing ref")?;

    // Reflog
    if let Some(msg) = &args.log_message {
        let identity = "gust <gust> 0 +0000";
        let _ = append_reflog(
            &repo.git_dir,
            &target_ref,
            &old_oid,
            &new_oid,
            identity,
            msg,
        );
    }

    Ok(())
}

/// Process `--stdin` batch commands.
fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let stdin = io::stdin();
    let lines: Vec<String> = stdin.lock().lines().collect::<std::io::Result<_>>()?;
    let mut in_transaction = false;
    let mut pending = Vec::new();

    for line in lines {
        let parts: Vec<&str> = line.trim().splitn(4, ' ').collect();
        if parts.is_empty() || parts[0].is_empty() {
            continue;
        }

        match parts[0] {
            "start" => {
                in_transaction = true;
                pending.clear();
                println!("start: ok");
            }
            "commit" => {
                if !in_transaction {
                    bail!("commit without start");
                }
                for op in &pending {
                    apply_batch_op(repo, args, op)?;
                }
                pending.clear();
                in_transaction = false;
                println!("commit: ok");
            }
            "update" => {
                if parts.len() < 3 {
                    bail!("update requires ref and new-value");
                }
                let expected = if parts.len() >= 4 && !parts[3].is_empty() {
                    Some(parts[3].parse().context("invalid old-value")?)
                } else {
                    None
                };
                let op = BatchOp::Update {
                    refname: parts[1].to_owned(),
                    new_oid: resolve_oid_or_ref(repo, parts[2])?,
                    expected_old: expected,
                };
                if in_transaction {
                    pending.push(op);
                } else {
                    apply_batch_op(repo, args, &op)?;
                }
            }
            "create" => {
                if parts.len() < 3 {
                    bail!("create requires ref and new-value");
                }
                let op = BatchOp::Create {
                    refname: parts[1].to_owned(),
                    new_oid: resolve_oid_or_ref(repo, parts[2])?,
                };
                if in_transaction {
                    pending.push(op);
                } else {
                    apply_batch_op(repo, args, &op)?;
                }
            }
            "delete" => {
                if parts.len() < 2 {
                    bail!("delete requires ref");
                }
                let expected = if parts.len() >= 3 && !parts[2].is_empty() {
                    Some(parts[2].parse().context("invalid old-value")?)
                } else {
                    None
                };
                let op = BatchOp::Delete {
                    refname: parts[1].to_owned(),
                    expected_old: expected,
                };
                if in_transaction {
                    pending.push(op);
                } else {
                    apply_batch_op(repo, args, &op)?;
                }
            }
            "verify" => {
                if parts.len() < 2 {
                    bail!("verify requires ref");
                }
                let expected = if parts.len() >= 3 && !parts[2].is_empty() {
                    Some(parts[2].parse().context("invalid old-value")?)
                } else {
                    None
                };
                let op = BatchOp::Verify {
                    refname: parts[1].to_owned(),
                    expected_old: expected,
                };
                if in_transaction {
                    pending.push(op);
                } else {
                    apply_batch_op(repo, args, &op)?;
                }
            }
            "abort" => {
                in_transaction = false;
                pending.clear();
            }
            other => bail!("unknown batch command: {other}"),
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
enum BatchOp {
    Update {
        refname: String,
        new_oid: ObjectId,
        expected_old: Option<ObjectId>,
    },
    Create {
        refname: String,
        new_oid: ObjectId,
    },
    Delete {
        refname: String,
        expected_old: Option<ObjectId>,
    },
    Verify {
        refname: String,
        expected_old: Option<ObjectId>,
    },
}

fn apply_batch_op(repo: &Repository, args: &Args, op: &BatchOp) -> Result<()> {
    match op {
        BatchOp::Update {
            refname,
            new_oid,
            expected_old,
        } => {
            let current = resolve_ref(&repo.git_dir, refname).ok();
            if let Some(expected) = expected_old {
                if !matches_expected_old(current, *expected) {
                    bail!("ref '{refname}' does not point to expected value");
                }
            }

            let old = current.unwrap_or_else(zero_oid);
            write_ref(&repo.git_dir, refname, new_oid)?;
            if let Some(msg) = &args.log_message {
                let _ = append_reflog(
                    &repo.git_dir,
                    refname,
                    &old,
                    new_oid,
                    "gust <gust> 0 +0000",
                    msg,
                );
            }
        }
        BatchOp::Create { refname, new_oid } => {
            if resolve_ref(&repo.git_dir, refname).is_ok() {
                bail!("ref '{refname}' already exists");
            }
            write_ref(&repo.git_dir, refname, new_oid)?;
        }
        BatchOp::Delete {
            refname,
            expected_old,
        } => {
            if let Some(expected) = expected_old {
                let current = resolve_ref(&repo.git_dir, refname).ok();
                if !matches_expected_old(current, *expected) {
                    bail!("ref '{refname}' does not point to expected value");
                }
            }
            delete_ref(&repo.git_dir, refname)?;
        }
        BatchOp::Verify {
            refname,
            expected_old,
        } => {
            if let Some(expected) = expected_old {
                let current = resolve_ref(&repo.git_dir, refname).ok();
                if !matches_expected_old(current, *expected) {
                    bail!("ref '{refname}' does not match expected value");
                }
            }
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

fn deref_refname(repo: &Repository, refname: &str, no_deref: bool) -> Result<String> {
    if no_deref || refname != "HEAD" {
        return Ok(refname.to_owned());
    }

    Ok(read_head(&repo.git_dir)?.unwrap_or_else(|| "HEAD".to_owned()))
}

fn zero_oid() -> ObjectId {
    ObjectId::from_bytes(&[0u8; 20]).unwrap_or_else(|_| unreachable!())
}

fn is_zero_oid(oid: ObjectId) -> bool {
    oid.as_bytes().iter().all(|b| *b == 0)
}

fn matches_expected_old(current: Option<ObjectId>, expected: ObjectId) -> bool {
    if is_zero_oid(expected) {
        current.is_none()
    } else {
        current == Some(expected)
    }
}
