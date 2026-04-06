//! `grit receive-pack` — receive pushed objects (server side).
//!
//! Invoked on the remote side of a push.  Advertises refs, then reads
//! pack data from stdin and updates refs.  Only local transport is supported.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::hooks::{run_hook_in_git_dir, HookResult};
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

/// Arguments for `grit receive-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Receive pushed objects (server side)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let repo = open_repo(&args.directory).with_context(|| {
        format!(
            "could not open repository at '{}'",
            args.directory.display()
        )
    })?;

    // Phase 1: Advertise refs
    advertise_refs(&repo.git_dir)?;

    // Flush packet (empty line signals end of advertisement)
    println!("0000");

    // Phase 2: Read ref update commands from stdin
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut updates: Vec<(String, String, String)> = Vec::new(); // (old_hex, new_hex, refname)

    while let Some(Ok(line)) = lines.next() {
        let line = line.trim().to_string();
        if line.is_empty() || line == "0000" {
            break;
        }
        // Format: <old-oid> <new-oid> <refname>
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() != 3 {
            bail!("protocol error: malformed update line: {}", line);
        }
        updates.push((
            parts[0].to_owned(),
            parts[1].to_owned(),
            parts[2].to_owned(),
        ));
    }

    // Phase 3: Read pack data from stdin (if any updates have new objects)
    // For local transport, objects are typically already copied.
    // We attempt to read any remaining stdin as pack data.
    let mut pack_data = Vec::new();
    let _ = io::stdin().lock().read_to_end(&mut pack_data);

    if !pack_data.is_empty() {
        // Write pack data to objects/pack/ if it looks like a packfile
        if pack_data.len() > 12 && &pack_data[..4] == b"PACK" {
            // Use SHA-1 of the pack data as the pack name
            use sha1::{Digest, Sha1};
            let mut hasher = Sha1::new();
            hasher.update(&pack_data);
            let hash = hasher.finalize();
            let pack_dir = repo.git_dir.join("objects/pack");
            fs::create_dir_all(&pack_dir)?;
            let pack_path = pack_dir.join(format!("pack-{}.pack", hex::encode(hash)));
            fs::write(&pack_path, &pack_data)?;
        }
    }

    // Build stdin payload for receive-side hooks.
    let hook_stdin = updates
        .iter()
        .map(|(old_hex, new_hex, refname)| format!("{old_hex} {new_hex} {refname}\n"))
        .collect::<String>();

    // Phase 4: Run receive-side hooks and apply updates
    let zero_oid = "0".repeat(40);

    let mut push_option_env_owned: Vec<(String, String)> = Vec::new();
    if let Ok(count_raw) = std::env::var("GIT_PUSH_OPTION_COUNT") {
        if let Ok(count) = count_raw.parse::<usize>() {
            push_option_env_owned.push(("GIT_PUSH_OPTION_COUNT".to_owned(), count.to_string()));
            for idx in 0..count {
                let key = format!("GIT_PUSH_OPTION_{idx}");
                if let Ok(val) = std::env::var(&key) {
                    push_option_env_owned.push((key, val));
                }
            }
        }
    }
    let push_option_env: Vec<(&str, &str)> = push_option_env_owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let (pre_receive_result, pre_receive_output) = run_hook_in_git_dir(
        &repo,
        "pre-receive",
        &[],
        Some(hook_stdin.as_bytes()),
        &push_option_env,
    );
    if !pre_receive_output.is_empty() {
        let _ = io::stderr().write_all(&pre_receive_output);
    }
    if let HookResult::Failed(_code) = pre_receive_result {
        bail!("pre-receive hook declined the push");
    }

    let mut ref_tx_lines = Vec::with_capacity(updates.len());
    for (old_hex, new_hex, refname) in &updates {
        let old_display = if old_hex == &zero_oid {
            zero_oid.clone()
        } else {
            old_hex.clone()
        };
        ref_tx_lines.push(format!("{old_display} {new_hex} {refname}"));
    }
    let ref_tx_stdin = format!("{}\n", ref_tx_lines.join("\n"));

    let (tx_preparing_result, tx_preparing_output) = run_hook_in_git_dir(
        &repo,
        "reference-transaction",
        &["preparing"],
        Some(ref_tx_stdin.as_bytes()),
        &push_option_env,
    );
    if !tx_preparing_output.is_empty() {
        let _ = io::stderr().write_all(&tx_preparing_output);
    }
    if let HookResult::Failed(_code) = tx_preparing_result {
        bail!("reference-transaction hook declined the update");
    }

    let (tx_prepared_result, tx_prepared_output) = run_hook_in_git_dir(
        &repo,
        "reference-transaction",
        &["prepared"],
        Some(ref_tx_stdin.as_bytes()),
        &push_option_env,
    );
    if !tx_prepared_output.is_empty() {
        let _ = io::stderr().write_all(&tx_prepared_output);
    }
    if let HookResult::Failed(_code) = tx_prepared_result {
        let _ = run_hook_in_git_dir(
            &repo,
            "reference-transaction",
            &["aborted"],
            Some(ref_tx_stdin.as_bytes()),
            &push_option_env,
        );
        bail!("reference-transaction hook declined the update");
    }

    for (_old_hex, new_hex, refname) in &updates {
        let old_for_update = refs::resolve_ref(&repo.git_dir, refname)
            .map(|oid| oid.to_hex())
            .unwrap_or_else(|_| zero_oid.clone());
        let (update_result, update_output) = run_hook_in_git_dir(
            &repo,
            "update",
            &[refname, &old_for_update, new_hex],
            None,
            &push_option_env,
        );
        if !update_output.is_empty() {
            let _ = io::stderr().write_all(&update_output);
        }
        if let HookResult::Failed(_code) = update_result {
            let _ = run_hook_in_git_dir(
                &repo,
                "reference-transaction",
                &["aborted"],
                Some(ref_tx_stdin.as_bytes()),
                &push_option_env,
            );
            bail!("update hook declined the update");
        }

        if new_hex == &zero_oid {
            // Delete ref
            refs::delete_ref(&repo.git_dir, refname)
                .with_context(|| format!("deleting ref {refname}"))?;
            println!("ok {refname}");
        } else {
            let new_oid =
                ObjectId::from_hex(new_hex).with_context(|| format!("invalid oid: {new_hex}"))?;
            refs::write_ref(&repo.git_dir, refname, &new_oid)
                .with_context(|| format!("updating ref {refname}"))?;
            println!("ok {refname}");
        }
    }

    let (tx_committed_result, tx_committed_output) = run_hook_in_git_dir(
        &repo,
        "reference-transaction",
        &["committed"],
        Some(ref_tx_stdin.as_bytes()),
        &push_option_env,
    );
    if !tx_committed_output.is_empty() {
        let _ = io::stderr().write_all(&tx_committed_output);
    }
    if let HookResult::Failed(_code) = tx_committed_result {
        // per githooks(5), committed state exit status is ignored.
    }

    let (post_receive_result, post_receive_output) = run_hook_in_git_dir(
        &repo,
        "post-receive",
        &[],
        Some(hook_stdin.as_bytes()),
        &push_option_env,
    );
    if !post_receive_output.is_empty() {
        let _ = io::stderr().write_all(&post_receive_output);
    }
    if let HookResult::Failed(_code) = post_receive_result {
        // post-receive is informational only.
    }

    Ok(())
}

/// Advertise all refs in the repository to stdout.
fn advertise_refs(git_dir: &Path) -> Result<()> {
    // HEAD first
    if let Ok(head_oid) = refs::resolve_ref(git_dir, "HEAD") {
        println!("{}\tHEAD", head_oid.to_hex());
    }

    // All refs
    let all_refs = list_all_refs(git_dir)?;
    for (refname, oid) in &all_refs {
        println!("{}\t{}", oid.to_hex(), refname);
    }

    Ok(())
}

/// List all refs under refs/.
fn list_all_refs(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let mut result = Vec::new();
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Ok(entries) = refs::list_refs(git_dir, prefix) {
            result.extend(entries);
        }
    }
    Ok(result)
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}
