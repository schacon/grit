//! `reference-transaction` hook phases for ref updates.
//!
//! Git's files backend opens a nested `packed-refs` transaction for each ref deletion (non-pruning).
//! If rewriting `packed-refs` is unnecessary (`is_packed_transaction_needed` is false), Git aborts
//! that nested transaction before the main transaction reaches `prepared`, and `ref_transaction_abort`
//! still runs the `reference-transaction` hook with state `aborted` and one stdin line per packed
//! delete (`0 0 <refname>`). We mirror that for the loose-ref backend only.

use anyhow::{bail, Result};
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::refs;
use grit_lib::repo::Repository;

const ZERO_OID_HEX: &str = "0000000000000000000000000000000000000000";

/// One line of stdin for the `reference-transaction` hook (`old new refname`).
#[derive(Clone, Debug)]
pub struct HookUpdate {
    /// Old value: 40-char hex or `ref:<target>` for symbolic refs.
    pub old_value: String,
    /// New value: 40-char hex or `ref:<target>`.
    pub new_value: String,
    pub refname: String,
    /// True when this update removes the ref (including OID updates to the null object).
    ///
    /// Used to decide whether a nested packed-refs transaction could run; verify-only updates
    /// use a null new value in the hook stdin but must not trigger the packed `aborted` preview.
    pub deletes_ref: bool,
}

/// Run `preparing`, optional packed-refs `aborted` preview, then `prepared`.
pub fn run_ref_transaction_prepare(repo: &Repository, updates: &[HookUpdate]) -> Result<()> {
    match run_ref_transaction_state(repo, "preparing", updates) {
        HookResult::NotFound => return Ok(()),
        HookResult::Success => {}
        HookResult::Failed(_) => {
            bail!("in 'preparing' phase, update aborted by the reference-transaction hook");
        }
    }

    if !grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
        let deletes: Vec<&HookUpdate> = updates.iter().filter(|u| u.deletes_ref).collect();
        if !deletes.is_empty() {
            let mut any_in_packed = false;
            for u in &deletes {
                if refs::packed_refs_entry_exists(&repo.git_dir, &u.refname)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                {
                    any_in_packed = true;
                    break;
                }
            }
            if !any_in_packed {
                let packed_lines: Vec<HookUpdate> = deletes
                    .iter()
                    .map(|u| HookUpdate {
                        old_value: ZERO_OID_HEX.to_owned(),
                        new_value: ZERO_OID_HEX.to_owned(),
                        refname: u.refname.clone(),
                        deletes_ref: false,
                    })
                    .collect();
                let _ = run_ref_transaction_state(repo, "aborted", &packed_lines);
            }
        }
    }

    match run_ref_transaction_state(repo, "prepared", updates) {
        HookResult::NotFound | HookResult::Success => Ok(()),
        HookResult::Failed(_) => {
            bail!("in 'prepared' phase, update aborted by the reference-transaction hook");
        }
    }
}

/// Run `committed` (best-effort; hook failures are ignored like Git).
pub fn run_ref_transaction_committed(repo: &Repository, updates: &[HookUpdate]) {
    let _ = run_ref_transaction_state(repo, "committed", updates);
}

/// Run `aborted` (best-effort).
pub fn run_ref_transaction_aborted(repo: &Repository, updates: &[HookUpdate]) {
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
