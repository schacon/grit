//! `grit update-ref` — update the object name stored in a ref safely.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::{self, Read};
use time::OffsetDateTime;

use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::objects::ObjectId;
use grit_lib::refs::{append_reflog, delete_ref, read_symbolic_ref, resolve_ref, write_ref};
use grit_lib::reftable::is_reftable_repo;
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

    /// Create a reflog for this ref.
    #[arg(long = "create-reflog")]
    pub create_reflog: bool,

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
        let expected =
            parse_old_expectation(args.old_value.as_deref().or(args.new_value.as_deref()))?;
        if let Some(exp) = expected {
            verify_expected_old(&repo, &target_refname, exp)?;
        }

        let hook_update = HookUpdate {
            old_value: hook_old_value_from_expectation(expected),
            new_value: zero_oid_hex().to_owned(),
            refname: target_refname.clone(),
        };
        run_ref_transaction_prepare(&repo, &[hook_update.clone()])?;
        delete_ref(&repo.git_dir, &target_refname).context("deleting ref")?;
        run_ref_transaction_committed(&repo, &[hook_update]);
        return Ok(());
    }

    let new_str = args
        .new_value
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("new value required"))?;
    let new_oid: ObjectId = resolve_oid_or_ref(&repo, new_str)?;

    let expected = parse_old_expectation(args.old_value.as_deref())?;
    if let Some(expected) = expected {
        verify_expected_old(&repo, &target_refname, expected)?;
    }

    let old_oid_for_reflog =
        resolve_ref(&repo.git_dir, &target_refname).unwrap_or_else(|_| zero_oid());
    let hook_update = HookUpdate {
        old_value: hook_old_value_from_expectation(expected),
        new_value: new_oid.to_hex(),
        refname: target_refname.clone(),
    };

    let log_message = args.log_message.as_deref().unwrap_or("");
    let should_write_reflog =
        !log_message.is_empty() || args.create_reflog || should_log_ref_updates(&repo);

    // Zero OID means delete
    if new_oid == zero_oid() {
        run_ref_transaction_prepare(&repo, &[hook_update.clone()])?;
        delete_ref(&repo.git_dir, &target_refname).context("deleting ref")?;
        run_ref_transaction_committed(&repo, &[hook_update]);
        if should_write_reflog {
            append_reflog_or_fail(
                &repo,
                &target_refname,
                &old_oid_for_reflog,
                &new_oid,
                &resolve_reflog_identity(&repo),
                log_message,
            )?;
        }
        return Ok(());
    }

    run_ref_transaction_prepare(&repo, &[hook_update.clone()])?;

    if is_reftable_repo(&repo.git_dir) && should_write_reflog {
        let identity = resolve_reflog_identity(&repo);
        if let Err(err) = grit_lib::reftable::reftable_write_ref(
            &repo.git_dir,
            &target_refname,
            &new_oid,
            Some(&identity),
            Some(log_message),
            None,
        ) {
            if err.to_string().contains("too large") {
                eprintln!(
                    "fatal: update_ref failed for ref '{}': reftable: transaction failure: entry too large",
                    target_refname
                );
                std::process::exit(1);
            }
            return Err(anyhow::Error::new(err)).context("writing ref");
        }
    } else if let Err(err) = write_ref(&repo.git_dir, &target_refname, &new_oid) {
        if is_reftable_repo(&repo.git_dir) && err.to_string().contains("too large") {
            eprintln!(
                "fatal: update_ref failed for ref '{}': reftable: transaction failure: entry too large",
                target_refname
            );
            std::process::exit(1);
        }
        return Err(err).context("writing ref");
    }
    run_ref_transaction_committed(&repo, &[hook_update]);

    if should_write_reflog && !is_reftable_repo(&repo.git_dir) {
        append_reflog_or_fail(
            &repo,
            &target_refname,
            &old_oid_for_reflog,
            &new_oid,
            &resolve_reflog_identity(&repo),
            log_message,
        )?;
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

    let mut state = BatchState::Open;
    let mut transaction_prepared = false;
    let mut staged: Vec<BatchOp> = Vec::new();

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
                let op = BatchOp::UpdateOid {
                    refname: parts[1].to_owned(),
                    new_oid: resolve_oid_or_ref(repo, parts[2])?,
                    expected_old: parse_old_expectation(parts.get(3).copied())?,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "create" => {
                if parts.len() < 3 {
                    bail!("create requires ref and new-value");
                }
                let op = BatchOp::CreateOid {
                    refname: parts[1].to_owned(),
                    new_oid: resolve_oid_or_ref(repo, parts[2])?,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "delete" => {
                if parts.len() < 2 {
                    bail!("delete requires ref");
                }
                let op = BatchOp::DeleteOid {
                    refname: parts[1].to_owned(),
                    expected_old: parse_old_expectation(parts.get(2).copied())?,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "verify" => {
                if parts.len() < 2 {
                    bail!("verify requires ref");
                }
                let op = BatchOp::VerifyOid {
                    refname: parts[1].to_owned(),
                    expected_old: parse_old_expectation(parts.get(2).copied())?,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "symref-update" => {
                if parts.len() < 3 {
                    bail!("symref-update requires ref and new-target");
                }
                let expected_old = match parts.get(3).copied() {
                    None => None,
                    Some("ref") => {
                        let Some(target) = parts.get(4) else {
                            bail!("symref-update requires old-target after 'ref'");
                        };
                        Some(SymrefOldExpectation::MustTarget((*target).to_owned()))
                    }
                    Some("oid") => {
                        let Some(oid) = parts.get(4) else {
                            bail!("symref-update requires old-oid after 'oid'");
                        };
                        let parsed = oid.parse::<ObjectId>().context("invalid old-value OID")?;
                        Some(SymrefOldExpectation::MustOid(parsed))
                    }
                    Some(other) => bail!("symref-update expected 'ref' or 'oid', got '{other}'"),
                };
                let op = BatchOp::UpdateSymref {
                    refname: parts[1].to_owned(),
                    new_target: parts[2].to_owned(),
                    expected_old,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "symref-create" => {
                if parts.len() < 3 {
                    bail!("symref-create requires ref and new-target");
                }
                let op = BatchOp::CreateSymref {
                    refname: parts[1].to_owned(),
                    new_target: parts[2].to_owned(),
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "symref-delete" => {
                if parts.len() < 2 {
                    bail!("symref-delete requires ref");
                }
                let expected_old = parts
                    .get(2)
                    .map(|target| SymrefOldExpectation::MustTarget((*target).to_owned()));
                let op = BatchOp::DeleteSymref {
                    refname: parts[1].to_owned(),
                    expected_old,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "symref-verify" => {
                if !args.no_deref {
                    bail!("symref-verify can only be used in no-deref mode");
                }
                if parts.len() < 2 {
                    bail!("symref-verify requires ref");
                }
                let expected_old = parts
                    .get(2)
                    .map(|target| SymrefOldExpectation::MustTarget((*target).to_owned()))
                    .unwrap_or(SymrefOldExpectation::MustNotExist);
                let op = BatchOp::VerifySymref {
                    refname: parts[1].to_owned(),
                    expected_old,
                };
                ensure_can_enqueue_in_state(state)?;
                staged.push(op);
            }
            "start" => {
                match state {
                    BatchState::Started => bail!("cannot restart ongoing transaction"),
                    BatchState::Prepared => bail!("prepared transactions can only be closed"),
                    BatchState::Open | BatchState::Closed => {
                        transaction_prepared = false;
                        state = BatchState::Started;
                    }
                }
                println!("start: ok");
            }
            "prepare" => {
                if matches!(state, BatchState::Closed) {
                    bail!("no transaction started");
                }
                if matches!(state, BatchState::Prepared) {
                    bail!("prepared transactions can only be closed");
                }
                let hook_updates = hook_updates_for_ops(&staged)?;
                run_ref_transaction_prepare(repo, &hook_updates)?;
                transaction_prepared = true;
                state = BatchState::Prepared;
            }
            "commit" => {
                if matches!(state, BatchState::Closed) {
                    bail!("no transaction started");
                }
                let transaction_base_index = if matches!(state, BatchState::Started) {
                    Some(reftable_transaction_base_index(repo)?)
                } else {
                    None
                };
                commit_staged_batch(
                    repo,
                    args,
                    &mut staged,
                    &mut transaction_prepared,
                    transaction_base_index,
                    true,
                )?;
                state = BatchState::Closed;
            }
            "abort" => {
                if !matches!(state, BatchState::Closed) {
                    let hook_updates = hook_updates_for_ops(&staged)?;
                    if !hook_updates.is_empty() {
                        run_ref_transaction_aborted(repo, &hook_updates);
                    }
                }
                staged.clear();
                transaction_prepared = false;
                state = BatchState::Closed;
            }
            other => bail!("unknown batch command: {other}"),
        }
    }

    match state {
        BatchState::Open => {
            let transaction_base_index = if !staged.is_empty() {
                Some(reftable_transaction_base_index(repo)?)
            } else {
                None
            };
            commit_staged_batch(
                repo,
                args,
                &mut staged,
                &mut transaction_prepared,
                transaction_base_index,
                false,
            )?;
        }
        BatchState::Started | BatchState::Prepared => {
            let hook_updates = hook_updates_for_ops(&staged)?;
            if !hook_updates.is_empty() {
                run_ref_transaction_aborted(repo, &hook_updates);
            }
        }
        BatchState::Closed => {}
    }

    Ok(())
}

fn resolve_oid_or_ref(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    if let Ok(oid) = grit_lib::rev_parse::resolve_revision(repo, s) {
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

fn append_reflog_or_fail(
    repo: &Repository,
    refname: &str,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    identity: &str,
    message: &str,
) -> Result<()> {
    if let Err(err) = append_reflog(&repo.git_dir, refname, old_oid, new_oid, identity, message) {
        if is_reftable_repo(&repo.git_dir) && err.to_string().contains("too large") {
            eprintln!(
                "fatal: update_ref failed for ref '{}': reftable: transaction failure: entry too large",
                refname
            );
            std::process::exit(1);
        }
        return Err(err.into());
    }
    Ok(())
}

fn should_log_ref_updates(repo: &Repository) -> bool {
    let Ok(cfg) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true) else {
        return !repo.is_bare();
    };
    let Some(value) = cfg.get("core.logAllRefUpdates") else {
        return !repo.is_bare();
    };
    let lowered = value.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "always" | "true" | "yes" | "on" | "1" => true,
        "false" | "no" | "off" | "0" => false,
        _ => !repo.is_bare(),
    }
}

#[derive(Clone, Copy)]
enum OldExpectation {
    MustNotExist,
    MustEqual(ObjectId),
}

#[derive(Clone)]
enum SymrefOldExpectation {
    MustNotExist,
    MustTarget(String),
    MustOid(ObjectId),
}

enum BatchOp {
    UpdateOid {
        refname: String,
        new_oid: ObjectId,
        expected_old: Option<OldExpectation>,
    },
    CreateOid {
        refname: String,
        new_oid: ObjectId,
    },
    DeleteOid {
        refname: String,
        expected_old: Option<OldExpectation>,
    },
    VerifyOid {
        refname: String,
        expected_old: Option<OldExpectation>,
    },
    UpdateSymref {
        refname: String,
        new_target: String,
        expected_old: Option<SymrefOldExpectation>,
    },
    CreateSymref {
        refname: String,
        new_target: String,
    },
    DeleteSymref {
        refname: String,
        expected_old: Option<SymrefOldExpectation>,
    },
    VerifySymref {
        refname: String,
        expected_old: SymrefOldExpectation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BatchState {
    Open,
    Started,
    Prepared,
    Closed,
}

fn ensure_can_enqueue_in_state(state: BatchState) -> Result<()> {
    match state {
        BatchState::Open | BatchState::Started => Ok(()),
        BatchState::Prepared => bail!("prepared transactions can only be closed"),
        BatchState::Closed => bail!("transaction is closed"),
    }
}

fn commit_staged_batch(
    repo: &Repository,
    args: &Args,
    staged: &mut Vec<BatchOp>,
    transaction_prepared: &mut bool,
    transaction_base_index: Option<u64>,
    report_ok: bool,
) -> Result<()> {
    let hook_updates = hook_updates_for_ops(staged)?;
    if !*transaction_prepared {
        run_ref_transaction_prepare(repo, &hook_updates)?;
    }
    for op in staged.drain(..) {
        apply_batch_op(repo, args, op, transaction_base_index)?;
    }
    run_ref_transaction_committed(repo, &hook_updates);
    *transaction_prepared = false;
    if report_ok {
        println!("commit: ok");
    }
    Ok(())
}

fn reftable_transaction_base_index(repo: &Repository) -> Result<u64> {
    if !is_reftable_repo(&repo.git_dir) {
        return Ok(0);
    }
    let stack = grit_lib::reftable::ReftableStack::open(&repo.git_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(stack
        .max_update_index()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        + 1)
}

fn apply_batch_op(
    repo: &Repository,
    args: &Args,
    op: BatchOp,
    forced_update_index: Option<u64>,
) -> Result<()> {
    let log_message = args.log_message.as_deref().unwrap_or("");
    let should_write_reflog =
        !log_message.is_empty() || args.create_reflog || should_log_ref_updates(repo);

    match op {
        BatchOp::UpdateOid {
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
            if is_reftable_repo(&repo.git_dir) && should_write_reflog {
                let identity = resolve_reflog_identity(repo);
                if let Err(err) = grit_lib::reftable::reftable_write_ref(
                    &repo.git_dir,
                    &target_refname,
                    &new_oid,
                    Some(&identity),
                    Some(log_message),
                    forced_update_index,
                ) {
                    if err.to_string().contains("too large") {
                        eprintln!(
                            "fatal: update_ref failed for ref '{}': reftable: transaction failure: entry too large",
                            target_refname
                        );
                        std::process::exit(1);
                    }
                    return Err(err.into());
                }
            } else {
                write_ref(&repo.git_dir, &target_refname, &new_oid)?;
                if should_write_reflog {
                    append_reflog_or_fail(
                        repo,
                        &target_refname,
                        &old_oid,
                        &new_oid,
                        &resolve_reflog_identity(repo),
                        log_message,
                    )?;
                }
            }
        }
        BatchOp::CreateOid { refname, new_oid } => {
            let target_refname = effective_refname(repo, &refname, args.no_deref)?;
            if resolve_ref(&repo.git_dir, &target_refname).is_ok() {
                bail!("ref '{target_refname}' already exists");
            }
            let old_oid = zero_oid();
            if is_reftable_repo(&repo.git_dir) && should_write_reflog {
                let identity = resolve_reflog_identity(repo);
                if let Err(err) = grit_lib::reftable::reftable_write_ref(
                    &repo.git_dir,
                    &target_refname,
                    &new_oid,
                    Some(&identity),
                    Some(log_message),
                    forced_update_index,
                ) {
                    if err.to_string().contains("too large") {
                        eprintln!(
                            "fatal: update_ref failed for ref '{}': reftable: transaction failure: entry too large",
                            target_refname
                        );
                        std::process::exit(1);
                    }
                    return Err(err.into());
                }
            } else {
                write_ref(&repo.git_dir, &target_refname, &new_oid)?;
                if should_write_reflog {
                    append_reflog_or_fail(
                        repo,
                        &target_refname,
                        &old_oid,
                        &new_oid,
                        &resolve_reflog_identity(repo),
                        log_message,
                    )?;
                }
            }
        }
        BatchOp::DeleteOid {
            refname,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, args.no_deref)?;
            let old_oid =
                resolve_ref(&repo.git_dir, &target_refname).unwrap_or_else(|_| zero_oid());
            if let Some(expected) = expected_old {
                verify_expected_old(repo, &target_refname, expected)?;
            }
            delete_ref(&repo.git_dir, &target_refname)?;
            if should_write_reflog {
                append_reflog_or_fail(
                    repo,
                    &target_refname,
                    &old_oid,
                    &zero_oid(),
                    &resolve_reflog_identity(repo),
                    log_message,
                )?;
            }
        }
        BatchOp::VerifyOid {
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
        BatchOp::UpdateSymref {
            refname,
            new_target,
            expected_old,
        } => {
            if let Some(expected) = expected_old {
                verify_symref_expected_old(repo, &refname, expected)?;
            }
            write_symbolic_ref(repo, &refname, &new_target)?;
        }
        BatchOp::CreateSymref {
            refname,
            new_target,
        } => {
            if ref_exists_no_deref(repo, &refname)? {
                bail!("ref '{refname}' already exists");
            }
            write_symbolic_ref(repo, &refname, &new_target)?;
        }
        BatchOp::DeleteSymref {
            refname,
            expected_old,
        } => {
            if let Some(expected) = expected_old {
                verify_symref_expected_old(repo, &refname, expected)?;
            }
            delete_ref_no_deref(repo, &refname)?;
        }
        BatchOp::VerifySymref {
            refname,
            expected_old,
        } => verify_symref_expected_old(repo, &refname, expected_old)?,
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

fn hook_old_value_from_expectation(expected: Option<OldExpectation>) -> String {
    match expected {
        Some(OldExpectation::MustNotExist) | None => zero_oid_hex().to_owned(),
        Some(OldExpectation::MustEqual(oid)) => oid.to_hex(),
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
        OldExpectation::MustEqual(oid) => match current {
            None => {
                eprintln!("fatal: unable to resolve reference: {refname}");
                std::process::exit(1);
            }
            Some(cur) if cur != oid => {
                bail!("ref '{refname}' is at {cur} but expected {oid}");
            }
            _ => {}
        },
    }
    Ok(())
}

#[derive(Clone)]
struct HookUpdate {
    old_value: String,
    new_value: String,
    refname: String,
}

fn hook_update_for_op(op: &BatchOp) -> Result<HookUpdate> {
    let update = match op {
        BatchOp::UpdateOid {
            refname,
            new_oid,
            expected_old,
        } => HookUpdate {
            old_value: hook_old_value_from_expectation(*expected_old),
            new_value: new_oid.to_hex(),
            refname: refname.clone(),
        },
        BatchOp::CreateOid { refname, new_oid } => HookUpdate {
            old_value: zero_oid_hex().to_owned(),
            new_value: new_oid.to_hex(),
            refname: refname.clone(),
        },
        BatchOp::DeleteOid {
            refname,
            expected_old,
        } => HookUpdate {
            old_value: hook_old_value_from_expectation(*expected_old),
            new_value: zero_oid_hex().to_owned(),
            refname: refname.clone(),
        },
        BatchOp::VerifyOid {
            refname,
            expected_old,
        } => HookUpdate {
            old_value: hook_old_value_from_expectation(*expected_old),
            new_value: zero_oid_hex().to_owned(),
            refname: refname.clone(),
        },
        BatchOp::UpdateSymref {
            refname,
            new_target,
            expected_old,
        } => HookUpdate {
            old_value: symref_old_for_hook(expected_old.clone()),
            new_value: format!("ref:{new_target}"),
            refname: refname.clone(),
        },
        BatchOp::CreateSymref {
            refname,
            new_target,
        } => HookUpdate {
            old_value: zero_oid_hex().to_owned(),
            new_value: format!("ref:{new_target}"),
            refname: refname.clone(),
        },
        BatchOp::DeleteSymref {
            refname,
            expected_old,
        } => HookUpdate {
            old_value: symref_old_for_hook(expected_old.clone()),
            new_value: zero_oid_hex().to_owned(),
            refname: refname.clone(),
        },
        BatchOp::VerifySymref {
            refname,
            expected_old,
        } => HookUpdate {
            old_value: symref_old_for_hook(Some(expected_old.clone())),
            new_value: zero_oid_hex().to_owned(),
            refname: refname.clone(),
        },
    };

    Ok(update)
}

fn hook_updates_for_ops(ops: &[BatchOp]) -> Result<Vec<HookUpdate>> {
    let mut updates = Vec::with_capacity(ops.len());
    for op in ops {
        updates.push(hook_update_for_op(op)?);
    }
    Ok(updates)
}

fn symref_old_for_hook(expected_old: Option<SymrefOldExpectation>) -> String {
    match expected_old {
        None | Some(SymrefOldExpectation::MustNotExist) => zero_oid_hex().to_owned(),
        Some(SymrefOldExpectation::MustTarget(target)) => format!("ref:{target}"),
        Some(SymrefOldExpectation::MustOid(oid)) => oid.to_hex(),
    }
}

fn run_ref_transaction_prepare(repo: &Repository, updates: &[HookUpdate]) -> Result<()> {
    match run_ref_transaction_state(repo, "preparing", updates) {
        HookResult::NotFound => return Ok(()),
        HookResult::Success => {}
        HookResult::Failed(_) => {
            bail!("in 'preparing' phase, update aborted by the reference-transaction hook");
        }
    }

    match run_ref_transaction_state(repo, "prepared", updates) {
        HookResult::NotFound | HookResult::Success => Ok(()),
        HookResult::Failed(_) => {
            bail!("in 'prepared' phase, update aborted by the reference-transaction hook");
        }
    }
}

fn run_ref_transaction_committed(repo: &Repository, updates: &[HookUpdate]) {
    let _ = run_ref_transaction_state(repo, "committed", updates);
}

fn run_ref_transaction_aborted(repo: &Repository, updates: &[HookUpdate]) {
    let _ = run_ref_transaction_state(repo, "aborted", updates);
}

fn run_ref_transaction_state(repo: &Repository, state: &str, updates: &[HookUpdate]) -> HookResult {
    let mut stdin_data = String::new();
    for update in updates {
        stdin_data.push_str(&format!(
            "{} {} {}\n",
            update.old_value, update.new_value, update.refname
        ));
    }
    run_hook(
        repo,
        "reference-transaction",
        &[state],
        Some(stdin_data.as_bytes()),
    )
}

fn resolve_reflog_identity(repo: &Repository) -> String {
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_NAME").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_EMAIL").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();

    let date_str = std::env::var("GIT_COMMITTER_DATE")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok());

    if let Some(date) = date_str {
        return format!("{name} <{email}> {date}");
    }

    let now = OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}

fn read_symbolic_ref_no_deref(repo: &Repository, refname: &str) -> Result<Option<String>> {
    if grit_lib::reftable::is_reftable_repo(&repo.git_dir) && refname != "HEAD" {
        return grit_lib::reftable::reftable_read_symbolic_ref(&repo.git_dir, refname)
            .map_err(|e| anyhow::anyhow!("{e}"));
    }

    let path = repo.git_dir.join(refname);
    let Ok(content) = fs::read_to_string(path) else {
        return Ok(None);
    };
    let trimmed = content.trim();
    if let Some(target) = trimmed.strip_prefix("ref: ") {
        Ok(Some(target.to_owned()))
    } else {
        Ok(None)
    }
}

fn write_symbolic_ref(repo: &Repository, refname: &str, target: &str) -> Result<()> {
    if grit_lib::reftable::is_reftable_repo(&repo.git_dir) && refname != "HEAD" {
        grit_lib::reftable::reftable_write_symref(&repo.git_dir, refname, target, None, None)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(());
    }
    let path = repo.git_dir.join(refname);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = path.with_extension("lock");
    fs::write(&lock_path, format!("ref: {target}\n"))?;
    fs::rename(lock_path, path)?;
    Ok(())
}

fn delete_ref_no_deref(repo: &Repository, refname: &str) -> Result<()> {
    delete_ref(&repo.git_dir, refname).map_err(Into::into)
}

fn ref_exists_no_deref(repo: &Repository, refname: &str) -> Result<bool> {
    if grit_lib::reftable::is_reftable_repo(&repo.git_dir) && refname != "HEAD" {
        if read_symbolic_ref_no_deref(repo, refname)?.is_some() {
            return Ok(true);
        }
        return Ok(grit_lib::reftable::reftable_resolve_ref(&repo.git_dir, refname).is_ok());
    }

    let path = repo.git_dir.join(refname);
    if path.exists() {
        return Ok(true);
    }
    Ok(resolve_ref(&repo.git_dir, refname).is_ok())
}

fn verify_symref_expected_old(
    repo: &Repository,
    refname: &str,
    expected: SymrefOldExpectation,
) -> Result<()> {
    match expected {
        SymrefOldExpectation::MustNotExist => {
            if ref_exists_no_deref(repo, refname)? {
                bail!("ref '{refname}' already exists");
            }
        }
        SymrefOldExpectation::MustTarget(target) => {
            let current = read_symbolic_ref_no_deref(repo, refname)?;
            match current {
                None => {
                    eprintln!("fatal: unable to resolve reference: {refname}");
                    std::process::exit(1);
                }
                Some(cur) if cur != target => {
                    bail!("ref '{refname}' points to {cur} but expected {target}");
                }
                Some(_) => {}
            }
        }
        SymrefOldExpectation::MustOid(oid) => {
            let current = resolve_ref(&repo.git_dir, refname).ok();
            match current {
                None => {
                    eprintln!("fatal: unable to resolve reference: {refname}");
                    std::process::exit(1);
                }
                Some(cur) if cur != oid => {
                    bail!("ref '{refname}' is at {cur} but expected {oid}");
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}

fn zero_oid_hex() -> &'static str {
    "0000000000000000000000000000000000000000"
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
