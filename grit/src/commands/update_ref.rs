//! `grit update-ref` — update the object name stored in a ref safely.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::{self, Read};
use time::OffsetDateTime;

use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::objects::ObjectId;
use grit_lib::refs::{
    append_reflog, delete_ref, read_head, read_symbolic_ref, resolve_ref, should_autocreate_reflog,
    write_ref,
};
use grit_lib::repo::Repository;
use std::collections::HashSet;

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

    /// Do not create a reflog unless `--create-reflog` or `-m` is given.
    #[arg(long = "no-create-reflog")]
    pub no_create_reflog: bool,

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
pub fn run(mut args: Args) -> Result<()> {
    if args.null_terminated && !args.stdin {
        bail!("-z requires --stdin");
    }

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
        // `git update-ref -d <ref> [<old>]` — the optional old OID is the first "value"
        // positional; clap maps it to `new_value`. Prefer explicit `--` old if given.
        let old_from_positional = args.new_value.take();
        let expected =
            parse_old_expectation(args.old_value.as_deref().or(old_from_positional.as_deref()))?;
        if let Some(exp) = expected {
            verify_expected_old(&repo, &target_refname, exp)?;
        }

        let old_oid_for_reflog =
            resolve_ref(&repo.git_dir, &target_refname).unwrap_or_else(|_| zero_oid());

        let hook_update = HookUpdate {
            old_value: hook_old_value_from_expectation(expected),
            new_value: zero_oid_hex().to_owned(),
            refname: target_refname.clone(),
        };
        run_ref_transaction_prepare(&repo, &[hook_update.clone()])?;
        delete_ref(&repo.git_dir, &target_refname).context("deleting ref")?;
        run_ref_transaction_committed(&repo, &[hook_update]);

        if let Some(msg) = args.log_message.as_deref() {
            let _ = append_reflog(
                &repo.git_dir,
                &reflog_ref_for_delete(&repo.git_dir, refname, &target_refname),
                &old_oid_for_reflog,
                &zero_oid(),
                &resolve_reflog_identity(&repo),
                msg,
                args.create_reflog,
            );
        }
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

    if new_oid == zero_oid() {
        run_ref_transaction_prepare(&repo, &[hook_update.clone()])?;
        delete_ref(&repo.git_dir, &target_refname).context("deleting ref")?;
        run_ref_transaction_committed(&repo, &[hook_update]);
        if let Some(msg) = args.log_message.as_deref() {
            let _ = append_reflog(
                &repo.git_dir,
                &reflog_ref_for_delete(&repo.git_dir, refname, &target_refname),
                &old_oid_for_reflog,
                &zero_oid(),
                &resolve_reflog_identity(&repo),
                msg,
                args.create_reflog,
            );
        }
        return Ok(());
    }

    run_ref_transaction_prepare(&repo, &[hook_update.clone()])?;

    write_ref(&repo.git_dir, &target_refname, &new_oid).context("writing ref")?;
    run_ref_transaction_committed(&repo, &[hook_update]);

    let msg = args.log_message.as_deref().unwrap_or("");
    if should_write_update_reflog(&repo, &args, &target_refname, msg) {
        let identity = resolve_reflog_identity(&repo);
        let _ = append_reflog(
            &repo.git_dir,
            &target_refname,
            &old_oid_for_reflog,
            &new_oid,
            &identity,
            msg,
            args.create_reflog,
        );
    }

    Ok(())
}

fn should_write_update_reflog(
    repo: &Repository,
    args: &Args,
    reflog_refname: &str,
    msg: &str,
) -> bool {
    if args.no_create_reflog {
        return args.create_reflog || !msg.is_empty();
    }
    if args.create_reflog || !msg.is_empty() {
        return true;
    }
    should_autocreate_reflog(&repo.git_dir, reflog_refname)
}

fn reflog_ref_for_delete(git_dir: &std::path::Path, user_arg: &str, leaf_ref: &str) -> String {
    if user_arg.eq_ignore_ascii_case("HEAD") {
        return "HEAD".to_owned();
    }
    match read_head(git_dir).ok().flatten() {
        Some(t) if t == leaf_ref => "HEAD".to_owned(),
        _ => leaf_ref.to_owned(),
    }
}

fn take_batch_no_deref(args: &Args, pending_option: &mut bool) -> bool {
    let nd = args.no_deref || *pending_option;
    *pending_option = false;
    nd
}

/// Process `--stdin` batch commands.
fn run_batch(repo: &Repository, args: &Args) -> Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    if args.null_terminated {
        return run_batch_nul(repo, args, &input);
    }

    let text = String::from_utf8_lossy(&input);
    let mut transaction_active = false;
    let mut transaction_prepared = false;
    let mut staged: Vec<(bool, BatchOp)> = Vec::new();
    let mut pending_option_no_deref = false;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.chars().next().is_some_and(|c| c.is_whitespace()) {
            bail!("whitespace before command: {line}");
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        process_batch_command(
            repo,
            args,
            &parts,
            &mut transaction_active,
            &mut transaction_prepared,
            &mut staged,
            &mut pending_option_no_deref,
        )?;
    }

    if transaction_active {
        let hook_updates = hook_updates_for_ops(&staged)?;
        if !hook_updates.is_empty() {
            run_ref_transaction_aborted(repo, &hook_updates);
        }
    }

    Ok(())
}

fn process_batch_command(
    repo: &Repository,
    args: &Args,
    parts: &[&str],
    transaction_active: &mut bool,
    transaction_prepared: &mut bool,
    staged: &mut Vec<(bool, BatchOp)>,
    pending_option_no_deref: &mut bool,
) -> Result<()> {
    match parts[0] {
        "update" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
            if parts.len() < 3 {
                bail!("update requires ref and new-value");
            }
            let op = BatchOp::UpdateOid {
                refname: parts[1].to_owned(),
                new_oid: resolve_oid_or_ref(repo, parts[2])?,
                expected_old: parse_old_expectation(parts.get(3).copied())?,
            };
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "create" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
            if parts.len() < 3 {
                bail!("create requires ref and new-value");
            }
            let op = BatchOp::CreateOid {
                refname: parts[1].to_owned(),
                new_oid: resolve_oid_or_ref(repo, parts[2])?,
            };
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "delete" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
            if parts.len() < 2 {
                bail!("delete requires ref");
            }
            let op = BatchOp::DeleteOid {
                refname: parts[1].to_owned(),
                expected_old: parse_old_expectation(parts.get(2).copied())?,
            };
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "verify" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
            if parts.len() < 2 {
                bail!("verify requires ref");
            }
            let op = BatchOp::VerifyOid {
                refname: parts[1].to_owned(),
                expected_old: parse_old_expectation(parts.get(2).copied())?,
            };
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "symref-update" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
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
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "symref-create" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
            if parts.len() < 3 {
                bail!("symref-create requires ref and new-target");
            }
            let op = BatchOp::CreateSymref {
                refname: parts[1].to_owned(),
                new_target: parts[2].to_owned(),
            };
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "symref-delete" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
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
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "symref-verify" => {
            let nd = take_batch_no_deref(args, pending_option_no_deref);
            if !nd {
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
            queue_or_apply(repo, args, nd, *transaction_active, staged, op)?;
        }
        "option" => {
            if parts.len() == 2 && parts[1] == "no-deref" {
                *pending_option_no_deref = true;
            } else {
                bail!("option unknown: {}", parts.get(1).copied().unwrap_or(""));
            }
        }
        "start" => {
            if *transaction_active {
                bail!("transaction already started");
            }
            *transaction_active = true;
            *transaction_prepared = false;
            staged.clear();
            println!("start: ok");
        }
        "prepare" => {
            if !*transaction_active {
                bail!("no transaction started");
            }
            let hook_updates = hook_updates_for_ops(staged)?;
            run_ref_transaction_prepare(repo, &hook_updates)?;
            *transaction_prepared = true;
            println!("prepare: ok");
        }
        "commit" => {
            if !*transaction_active {
                bail!("no transaction started");
            }

            let hook_updates = hook_updates_for_ops(staged)?;
            if !*transaction_prepared {
                run_ref_transaction_prepare(repo, &hook_updates)?;
            }
            for (nd, op) in staged.drain(..) {
                apply_batch_op(repo, args, nd, op)?;
            }
            run_ref_transaction_committed(repo, &hook_updates);
            *transaction_active = false;
            *transaction_prepared = false;
            println!("commit: ok");
        }
        "abort" => {
            if *transaction_active {
                let hook_updates = hook_updates_for_ops(staged)?;
                if !hook_updates.is_empty() {
                    run_ref_transaction_aborted(repo, &hook_updates);
                }
            }
            staged.clear();
            *transaction_active = false;
            *transaction_prepared = false;
            println!("abort: ok");
        }
        other => bail!("unknown batch command: {other}"),
    }
    Ok(())
}

fn run_batch_nul(repo: &Repository, args: &Args, input: &[u8]) -> Result<()> {
    let mut transaction_active = false;
    let mut transaction_prepared = false;
    let mut staged: Vec<(bool, BatchOp)> = Vec::new();
    let mut pending_option_no_deref = false;

    for chunk in input.split(|b| *b == 0) {
        if chunk.is_empty() {
            continue;
        }
        let line = std::str::from_utf8(chunk).context("invalid utf-8 in stdin")?;
        if line.chars().next().is_some_and(|c| c.is_whitespace()) {
            bail!("whitespace before command: {line}");
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        process_batch_command(
            repo,
            args,
            &parts,
            &mut transaction_active,
            &mut transaction_prepared,
            &mut staged,
            &mut pending_option_no_deref,
        )?;
    }

    if transaction_active {
        let hook_updates = hook_updates_for_ops(&staged)?;
        if !hook_updates.is_empty() {
            run_ref_transaction_aborted(repo, &hook_updates);
        }
    }

    Ok(())
}

fn resolve_oid_or_ref(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    // Full ref names must be resolved via the ref store first; `rev_parse` can
    // treat `refs/...` as ambiguous against worktree paths.
    if s.starts_with("refs/") {
        if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
            return Ok(oid);
        }
    }
    if let Ok(oid) = grit_lib::rev_parse::resolve_revision(repo, s) {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return Ok(oid);
    }
    // Try DWIM-style resolution: refs/heads/<s>, refs/tags/<s>, refs/remotes/<s>
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/", "refs/notes/"] {
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

fn queue_or_apply(
    repo: &Repository,
    args: &Args,
    no_deref: bool,
    transaction_active: bool,
    staged: &mut Vec<(bool, BatchOp)>,
    op: BatchOp,
) -> Result<()> {
    if transaction_active {
        staged.push((no_deref, op));
        Ok(())
    } else {
        let hook_update = hook_update_for_op(&op)?;
        run_ref_transaction_prepare(repo, std::slice::from_ref(&hook_update))?;
        apply_batch_op(repo, args, no_deref, op)?;
        run_ref_transaction_committed(repo, &[hook_update]);
        Ok(())
    }
}

fn apply_batch_op(repo: &Repository, args: &Args, no_deref: bool, op: BatchOp) -> Result<()> {
    match op {
        BatchOp::UpdateOid {
            refname,
            new_oid,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, no_deref)?;
            if let Some(expected) = expected_old {
                verify_expected_old(repo, &target_refname, expected)?;
            }
            let old_oid =
                resolve_ref(&repo.git_dir, &target_refname).unwrap_or_else(|_| zero_oid());
            write_ref(&repo.git_dir, &target_refname, &new_oid)?;
            let msg = args.log_message.as_deref().unwrap_or("");
            if should_write_update_reflog(repo, args, &target_refname, msg) {
                let _ = append_reflog(
                    &repo.git_dir,
                    &target_refname,
                    &old_oid,
                    &new_oid,
                    &resolve_reflog_identity(repo),
                    msg,
                    args.create_reflog,
                );
            }
        }
        BatchOp::CreateOid { refname, new_oid } => {
            let target_refname = effective_refname(repo, &refname, no_deref)?;
            if resolve_ref(&repo.git_dir, &target_refname).is_ok() {
                bail!("ref '{target_refname}' already exists");
            }
            write_ref(&repo.git_dir, &target_refname, &new_oid)?;
        }
        BatchOp::DeleteOid {
            refname,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, no_deref)?;
            if let Some(expected) = expected_old {
                verify_expected_old(repo, &target_refname, expected)?;
            }
            delete_ref(&repo.git_dir, &target_refname)?;
        }
        BatchOp::VerifyOid {
            refname,
            expected_old,
        } => {
            let target_refname = effective_refname(repo, &refname, no_deref)?;
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
    let mut current = refname.to_owned();
    let mut seen = HashSet::new();
    for _ in 0..64 {
        if !seen.insert(current.clone()) {
            bail!("symref cycle involving '{refname}'");
        }
        match read_symbolic_ref(&repo.git_dir, &current)? {
            Some(target) => current = target,
            None => return Ok(current),
        }
    }
    bail!("symref chain depth exceeded for '{refname}'");
}

fn parse_old_expectation(raw: Option<&str>) -> Result<Option<OldExpectation>> {
    let Some(old) = raw else {
        return Ok(None);
    };
    if old.is_empty() {
        return Ok(None);
    }
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

fn hook_updates_for_ops(ops: &[(bool, BatchOp)]) -> Result<Vec<HookUpdate>> {
    let mut updates = Vec::with_capacity(ops.len());
    for (_, op) in ops {
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

pub(crate) fn resolve_reflog_identity(repo: &Repository) -> String {
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
        if let Ok(canonical) = grit_lib::git_date::parse::parse_date(&date) {
            return format!("{name} <{email}> {canonical}");
        }
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
