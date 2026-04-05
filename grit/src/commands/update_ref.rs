//! `grit update-ref` — update the object name stored in a ref safely.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::{self, Read};

use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::objects::ObjectId;
use grit_lib::refs::{append_reflog, delete_ref, read_symbolic_ref, resolve_ref, write_ref};
use grit_lib::repo::Repository;

/// Arguments for `grit update-ref`.
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

/// Run `grit update-ref`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    if args.stdin {
        return run_batch(&repo, &args);
    }

    let refname = args
        .refname
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("ref name required"))?;
    let target_refname = effective_refname(&repo, refname, args.no_deref)?;

    if args.delete {
        if let Some(expected) =
            parse_old_expectation(args.old_value.as_deref().or(args.new_value.as_deref()))?
        {
            verify_expected_old(&repo, &target_refname, expected)?;
        }
        delete_ref(&repo.git_dir, &target_refname).context("deleting ref")?;
        return Ok(());
    }

    let new_str = args
        .new_value
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("new value required"))?;
    let new_oid: ObjectId = resolve_oid_or_ref(&repo, new_str)?;

    if let Some(expected) = parse_old_expectation(args.old_value.as_deref())? {
        verify_expected_old(&repo, &target_refname, expected)?;
    }

    let old_oid = resolve_ref(&repo.git_dir, &target_refname).unwrap_or_else(|_| zero_oid());

    // Zero OID means delete
    if new_oid == zero_oid() {
        run_ref_transaction_hook(&repo, &old_oid, &new_oid, &target_refname)?;
        delete_ref(&repo.git_dir, &target_refname).context("deleting ref")?;
        if let Some(msg) = &args.log_message {
            let _ = append_reflog(
                &repo.git_dir,
                &target_refname,
                &old_oid,
                &new_oid,
                "grit <grit> 0 +0000",
                msg,
            );
        }
        return Ok(());
    }

    // Run reference-transaction hook
    run_ref_transaction_hook(&repo, &old_oid, &new_oid, &target_refname)?;

    write_ref(&repo.git_dir, &target_refname, &new_oid).context("writing ref")?;

    // Reflog
    if let Some(msg) = &args.log_message {
        let identity = "grit <grit> 0 +0000";
        let _ = append_reflog(
            &repo.git_dir,
            &target_refname,
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
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let records: Vec<&str> = if args.null_terminated {
        input.split('\0').collect()
    } else {
        input.lines().collect()
    };

    let mut transaction_active = false;
    let mut staged = Vec::new();

    for line in records {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "update" => {
                if parts.len() < 3 {
                    bail!("update requires ref and new-value");
                }
                let op = BatchOp::Update {
                    refname: parts[1].to_owned(),
                    new_oid: resolve_oid_or_ref(repo, parts[2])?,
                    expected_old: parse_old_expectation(parts.get(3).copied())?,
                };
                queue_or_apply(repo, args, transaction_active, &mut staged, op)?;
            }
            "create" => {
                if parts.len() < 3 {
                    bail!("create requires ref and new-value");
                }
                let op = BatchOp::Create {
                    refname: parts[1].to_owned(),
                    new_oid: resolve_oid_or_ref(repo, parts[2])?,
                };
                queue_or_apply(repo, args, transaction_active, &mut staged, op)?;
            }
            "delete" => {
                if parts.len() < 2 {
                    bail!("delete requires ref");
                }
                let op = BatchOp::Delete {
                    refname: parts[1].to_owned(),
                    expected_old: parse_old_expectation(parts.get(2).copied())?,
                };
                queue_or_apply(repo, args, transaction_active, &mut staged, op)?;
            }
            "verify" => {
                if parts.len() < 2 {
                    bail!("verify requires ref");
                }
                let op = BatchOp::Verify {
                    refname: parts[1].to_owned(),
                    expected_old: parse_old_expectation(parts.get(2).copied())?,
                };
                queue_or_apply(repo, args, transaction_active, &mut staged, op)?;
            }
            "start" => {
                if transaction_active {
                    bail!("transaction already started");
                }
                transaction_active = true;
                staged.clear();
                println!("start: ok");
            }
            "commit" => {
                if !transaction_active {
                    bail!("no transaction started");
                }
                for op in staged.drain(..) {
                    apply_batch_op(repo, args, op)?;
                }
                transaction_active = false;
                println!("commit: ok");
            }
            "abort" => {
                staged.clear();
                break;
            }
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
    // Try DWIM-style resolution: refs/heads/<s>, refs/tags/<s>, refs/remotes/<s>
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        let full = format!("{prefix}{s}");
        if let Ok(oid) = resolve_ref(&repo.git_dir, &full) {
            return Ok(oid);
        }
    }
    bail!("not a valid object name: '{s}'")
}

#[derive(Clone, Copy)]
enum OldExpectation {
    MustNotExist,
    MustEqual(ObjectId),
}

enum BatchOp {
    Update {
        refname: String,
        new_oid: ObjectId,
        expected_old: Option<OldExpectation>,
    },
    Create {
        refname: String,
        new_oid: ObjectId,
    },
    Delete {
        refname: String,
        expected_old: Option<OldExpectation>,
    },
    Verify {
        refname: String,
        expected_old: Option<OldExpectation>,
    },
}

fn queue_or_apply(
    repo: &Repository,
    args: &Args,
    transaction_active: bool,
    staged: &mut Vec<BatchOp>,
    op: BatchOp,
) -> Result<()> {
    if transaction_active {
        staged.push(op);
        Ok(())
    } else {
        apply_batch_op(repo, args, op)
    }
}

fn apply_batch_op(repo: &Repository, args: &Args, op: BatchOp) -> Result<()> {
    match op {
        BatchOp::Update {
            refname,
            new_oid,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, args.no_deref)?;
            if let Some(expected) = expected_old {
                verify_expected_old(repo, &target_refname, expected)?;
            }
            let old_oid =
                resolve_ref(&repo.git_dir, &target_refname).unwrap_or_else(|_| zero_oid());
            write_ref(&repo.git_dir, &target_refname, &new_oid)?;
            if let Some(msg) = &args.log_message {
                let _ = append_reflog(
                    &repo.git_dir,
                    &target_refname,
                    &old_oid,
                    &new_oid,
                    "grit <grit> 0 +0000",
                    msg,
                );
            }
        }
        BatchOp::Create { refname, new_oid } => {
            let target_refname = effective_refname(repo, &refname, args.no_deref)?;
            if resolve_ref(&repo.git_dir, &target_refname).is_ok() {
                bail!("ref '{target_refname}' already exists");
            }
            write_ref(&repo.git_dir, &target_refname, &new_oid)?;
        }
        BatchOp::Delete {
            refname,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, args.no_deref)?;
            if let Some(expected) = expected_old {
                verify_expected_old(repo, &target_refname, expected)?;
            }
            delete_ref(&repo.git_dir, &target_refname)?;
        }
        BatchOp::Verify {
            refname,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, args.no_deref)?;
            if let Some(expected) = expected_old {
                verify_expected_old(repo, &target_refname, expected)?;
            } else if resolve_ref(&repo.git_dir, &target_refname).is_err() {
                bail!("ref '{target_refname}' does not exist");
            }
        }
    }

    Ok(())
}

fn effective_refname(repo: &Repository, refname: &str, no_deref: bool) -> Result<String> {
    if no_deref {
        return Ok(refname.to_owned());
    }
    if let Some(target) = read_symbolic_ref(&repo.git_dir, refname)? {
        Ok(target)
    } else {
        Ok(refname.to_owned())
    }
}

fn parse_old_expectation(raw: Option<&str>) -> Result<Option<OldExpectation>> {
    let Some(old) = raw else {
        return Ok(None);
    };
    let expected: ObjectId = old.parse().context("invalid old-value OID")?;
    if is_zero_oid(&expected) {
        Ok(Some(OldExpectation::MustNotExist))
    } else {
        Ok(Some(OldExpectation::MustEqual(expected)))
    }
}

fn verify_expected_old(repo: &Repository, refname: &str, expected: OldExpectation) -> Result<()> {
    let current = resolve_ref(&repo.git_dir, refname).ok();
    match expected {
        OldExpectation::MustNotExist => {
            if current.is_some() {
                bail!("ref '{refname}' already exists");
            }
        }
        OldExpectation::MustEqual(oid) => {
            if current != Some(oid) {
                bail!("ref '{refname}' does not point to expected value");
            }
        }
    }
    Ok(())
}

fn is_zero_oid(oid: &ObjectId) -> bool {
    oid.as_bytes().iter().all(|byte| *byte == 0)
}

fn zero_oid() -> ObjectId {
    match ObjectId::from_bytes(&[0u8; 20]) {
        Ok(oid) => oid,
        Err(err) => panic!("20-byte zero OID should always be valid: {err}"),
    }
}

/// Run the reference-transaction hook through three phases: preparing, prepared, committed.
/// If the hook fails during "preparing", the update is aborted.
fn run_ref_transaction_hook(
    repo: &Repository,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    refname: &str,
) -> Result<()> {
    let stdin_data = format!("{} {} {}\n", old_oid.to_hex(), new_oid.to_hex(), refname);
    let stdin_bytes = stdin_data.as_bytes();

    // Phase 1: preparing — abort if hook fails
    match run_hook(
        repo,
        "reference-transaction",
        &["preparing"],
        Some(stdin_bytes),
    ) {
        HookResult::Failed(code) => {
            bail!("reference-transaction hook (preparing) exited with status {code}");
        }
        HookResult::NotFound => return Ok(()),
        HookResult::Success => {}
    }

    // Phase 2: prepared — informational (don't abort)
    let _ = run_hook(
        repo,
        "reference-transaction",
        &["prepared"],
        Some(stdin_bytes),
    );

    // Phase 3: committed — informational (don't abort)
    let _ = run_hook(
        repo,
        "reference-transaction",
        &["committed"],
        Some(stdin_bytes),
    );

    Ok(())
}
