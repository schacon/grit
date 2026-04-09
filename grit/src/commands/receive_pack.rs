//! `grit receive-pack` — receive pushed objects (server side).
//!
//! Invoked on the remote side of a push.  Advertises refs, then reads
//! pack data from stdin and updates refs.  Only local transport is supported.
//!
//! When the client requests the `report-status` capability (as `git send-pack`
//! does), successful and failed ref updates are reported as pkt-lines so the
//! client can complete the protocol.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{parse_bool, ConfigSet};
use grit_lib::hooks::{run_hook_in_git_dir, HookResult};
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::pkt_line;

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

    let remote_config = ConfigSet::load(Some(&repo.git_dir), false)?;

    // Phase 1: Advertise refs (pkt-line)
    let mut out = io::stdout();
    let caps = build_receive_pack_capabilities(&remote_config);
    write_receive_pack_advertisement(&mut out, &repo.git_dir, &caps)?;
    pkt_line::write_flush(&mut out)?;
    out.flush()?;

    // Phase 2: Read ref update commands from stdin (pkt-line until flush)
    let mut stdin = io::stdin();
    let mut updates: Vec<(String, String, String)> = Vec::new(); // (old_hex, new_hex, refname)
    let mut client_wants_report_status = false;

    loop {
        match pkt_line::read_packet(&mut stdin)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                if !client_wants_report_status && line.contains('\0') {
                    let caps = line.split('\0').nth(1).unwrap_or("");
                    client_wants_report_status =
                        caps.split_whitespace().any(|c| c == "report-status");
                }
                let (old_hex, new_hex, refname) = parse_update_command(&line)?;
                updates.push((old_hex, new_hex, refname));
            }
            _ => {}
        }
    }

    // Phase 3: Read pack data from stdin (if any updates have new objects)
    let mut pack_data = Vec::new();
    let _ = stdin.read_to_end(&mut pack_data);

    let mut unpack_status: Option<String> = None;
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
            if let Err(e) = fs::write(&pack_path, &pack_data) {
                unpack_status = Some(format!("failed to store pack: {e}"));
            }
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
        if client_wants_report_status {
            write_push_report_all_ng(
                &mut out,
                "pre-receive hook declined",
                &updates,
                "pre-receive hook declined",
            )?;
        }
        bail!("pre-receive hook declined the push");
    }

    if let Some(msg) = &unpack_status {
        if client_wants_report_status {
            write_push_report_all_ng(&mut out, msg, &updates, msg)?;
        }
        bail!("unpack failed: {msg}");
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
        if client_wants_report_status {
            write_push_report_all_ng(
                &mut out,
                "reference-transaction hook declined",
                &updates,
                "reference-transaction hook declined",
            )?;
        }
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
        if client_wants_report_status {
            write_push_report_all_ng(
                &mut out,
                "reference-transaction hook declined",
                &updates,
                "reference-transaction hook declined",
            )?;
        }
        bail!("reference-transaction hook declined the update");
    }

    // `(refname, oid before our push)` for rollback on mid-flight failure.
    let mut applied_stack: Vec<(String, Option<ObjectId>)> = Vec::new();
    let mut per_ref_error: Option<(String, String)> = None;

    'update_loop: for (_old_hex, new_hex, refname) in &updates {
        let server_tip_before = refs::resolve_ref(&repo.git_dir, refname).ok();
        let old_for_update = server_tip_before
            .map(|oid| oid.to_hex())
            .unwrap_or_else(|| zero_oid.clone());
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
            per_ref_error = Some((refname.clone(), "hook declined".to_owned()));
            break 'update_loop;
        }

        let apply_res = if new_hex == &zero_oid {
            refs::delete_ref(&repo.git_dir, refname)
                .with_context(|| format!("deleting ref {refname}"))
                .map_err(|e| e.to_string())
        } else {
            let new_oid = ObjectId::from_hex(new_hex)
                .map_err(|e| e.to_string())
                .and_then(|oid| {
                    refs::write_ref(&repo.git_dir, refname, &oid)
                        .with_context(|| format!("updating ref {refname}"))
                        .map_err(|e| e.to_string())
                });
            new_oid
        };

        if let Err(e) = apply_res {
            let _ = run_hook_in_git_dir(
                &repo,
                "reference-transaction",
                &["aborted"],
                Some(ref_tx_stdin.as_bytes()),
                &push_option_env,
            );
            per_ref_error = Some((refname.clone(), e));
            break 'update_loop;
        }

        applied_stack.push((refname.clone(), server_tip_before));
        if !client_wants_report_status {
            println!("ok {refname}");
        }
    }

    if let Some((failed_ref, err)) = per_ref_error {
        for (r, prev) in applied_stack.iter().rev() {
            if let Some(oid) = prev {
                let _ = refs::write_ref(&repo.git_dir, r, oid);
            } else {
                let _ = refs::delete_ref(&repo.git_dir, r);
            }
        }
        if client_wants_report_status {
            write_push_report_partial_failure(&mut out, &updates, &failed_ref, &err)?;
        }
        bail!("update hook declined the update");
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

    if client_wants_report_status {
        write_push_report_ok(&mut out, &updates)?;
    }

    Ok(())
}

/// Build the capability string for the first ref advertisement line (Git wire protocol).
fn build_receive_pack_capabilities(remote_config: &ConfigSet) -> String {
    let mut caps = String::from(
        "report-status delete-refs quiet ofs-delta object-format=sha1 agent=grit/0.1 push-options",
    );
    let advertise_atomic = remote_config
        .get("receive.advertiseatomic")
        .or_else(|| remote_config.get("receive.advertiseAtomic"));
    let atomic_on = match advertise_atomic.as_deref() {
        None | Some("1") | Some("true") | Some("yes") | Some("on") => true,
        Some(v) => parse_bool(v).unwrap_or(true),
    };
    if atomic_on {
        caps.push_str(" atomic");
    }
    let push_opts_off = remote_config
        .get("receive.advertisepushoptions")
        .or_else(|| remote_config.get("receive.advertisePushOptions"))
        .is_some_and(|v| matches!(v.as_str(), "0" | "false" | "no" | "off"));
    if push_opts_off {
        caps = caps.replace(" push-options", "");
    }
    caps
}

fn parse_update_command(line: &str) -> Result<(String, String, String)> {
    let line = line.trim_end_matches('\n');
    let line = line.split('\0').next().unwrap_or(line);
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        bail!("protocol error: malformed update line: {line}");
    }
    Ok((
        parts[0].to_owned(),
        parts[1].to_owned(),
        parts[2].to_owned(),
    ))
}

fn write_push_report_ok(
    w: &mut impl Write,
    all_updates: &[(String, String, String)],
) -> Result<()> {
    pkt_line::write_line(w, "unpack ok")?;
    for (_o, _n, refname) in all_updates {
        pkt_line::write_line(w, &format!("ok {refname}"))?;
    }
    pkt_line::write_flush(w)?;
    w.flush()?;
    Ok(())
}

fn write_push_report_all_ng(
    w: &mut impl Write,
    unpack_detail: &str,
    all_updates: &[(String, String, String)],
    per_ref_reason: &str,
) -> Result<()> {
    let unpack_line = format!("unpack {unpack_detail}");
    pkt_line::write_line(w, &unpack_line)?;
    for (_o, _n, refname) in all_updates {
        pkt_line::write_line(w, &format!("ng {refname} {per_ref_reason}"))?;
    }
    pkt_line::write_flush(w)?;
    w.flush()?;
    Ok(())
}

fn write_push_report_partial_failure(
    w: &mut impl Write,
    all_updates: &[(String, String, String)],
    failed_ref: &str,
    failed_reason: &str,
) -> Result<()> {
    pkt_line::write_line(w, "unpack ok")?;
    for (_o, _n, refname) in all_updates {
        if refname == failed_ref {
            pkt_line::write_line(w, &format!("ng {refname} {failed_reason}"))?;
        } else {
            pkt_line::write_line(w, &format!("ng {refname} aborted"))?;
        }
    }
    pkt_line::write_flush(w)?;
    w.flush()?;
    Ok(())
}

fn write_receive_pack_advertisement(w: &mut impl Write, git_dir: &Path, caps: &str) -> Result<()> {
    let all_refs = list_all_refs(git_dir)?;
    let mut wrote_caps = false;

    if let Ok(head_oid) = refs::resolve_ref(git_dir, "HEAD") {
        let line = if !wrote_caps {
            wrote_caps = true;
            format!("{}\tHEAD\0{caps}\n", head_oid.to_hex())
        } else {
            format!("{}\tHEAD\n", head_oid.to_hex())
        };
        let len = 4 + line.len();
        write!(w, "{:04x}{}", len, line)?;
    }

    for (refname, oid) in &all_refs {
        let line = if !wrote_caps {
            wrote_caps = true;
            format!("{}\t{}\0{caps}\n", oid.to_hex(), refname)
        } else {
            format!("{}\t{}\n", oid.to_hex(), refname)
        };
        let len = 4 + line.len();
        write!(w, "{:04x}{}", len, line)?;
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
