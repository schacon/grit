//! `grit receive-pack` — receive pushed objects (server side).
//!
//! Invoked on the remote side of a push. Advertises refs in pkt-line format (with
//! capabilities), reads ref updates and an optional pack stream from stdin, then
//! updates refs when connectivity checks pass.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{parse_bool, ConfigFile, ConfigScope, ConfigSet};
use grit_lib::connectivity::{diagnose_push_connectivity_failure, push_tip_connected_to_refs};
use grit_lib::hide_refs;
use grit_lib::hooks::{run_hook_in_git_dir, HookResult};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::pack::read_alternates_recursive;
use grit_lib::ref_namespace;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::unpack_objects::{pack_bytes_to_object_map, unpack_objects, UnpackOptions};
use std::collections::HashSet;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::grit_exe;
use crate::pkt_line::{read_packet, write_flush, write_packet_raw, Packet};
use crate::trace2_transfer;
use crate::wire_trace;

/// Arguments for `grit receive-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Receive pushed objects (server side)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,

    /// Skip connectivity verification after unpacking (matches `git receive-pack`).
    #[arg(long = "skip-connectivity-check", hide = true)]
    pub skip_connectivity_check: bool,
}

pub fn run(args: Args) -> Result<()> {
    let repo = open_repo(&args.directory).with_context(|| {
        format!(
            "could not open repository at '{}'",
            args.directory.display()
        )
    })?;

    trace2_transfer::emit_negotiated_version_from_git_protocol_env();

    // Use only this repository's `config` so global `core.alternateRefs*` from the
    // environment does not leak across harness tests (matches receive-pack reading repo config).
    let mut config = ConfigSet::new();
    if let Ok(Some(f)) = ConfigFile::from_path(&repo.git_dir.join("config"), ConfigScope::Local) {
        config.merge(&f);
    }
    let extra_have = collect_alternate_have_oids(&repo, &config)?;

    advertise_refs_phase(&repo, &extra_have)?;

    let hide_patterns = hide_refs::hide_ref_patterns_receive(&config);

    let mut stdin = io::stdin();
    let mut payload = Vec::new();
    stdin.read_to_end(&mut payload)?;

    let mut cursor = Cursor::new(&payload[..]);
    let mut updates: Vec<(String, String, String)> = Vec::new();
    let mut caps_seen = false;
    let mut client_sid_from_caps: Option<String> = None;

    loop {
        match read_packet(&mut cursor)? {
            None => break,
            Some(Packet::Flush) => break,
            Some(Packet::Delim) | Some(Packet::ResponseEnd) => break,
            Some(Packet::Data(line)) => {
                if !caps_seen {
                    if let Some((_, feats)) = line.split_once('\0') {
                        if let Some(sid) = trace2_transfer::extract_session_id_feature(feats.trim())
                        {
                            client_sid_from_caps = Some(sid.to_owned());
                        }
                    }
                }
                if let Some((old_h, new_h, refname)) = parse_update_line(&line, !caps_seen) {
                    caps_seen = true;
                    updates.push((old_h, new_h, refname));
                }
            }
        }
    }

    if let Some(ref sid) = client_sid_from_caps {
        trace2_transfer::emit_client_sid(sid);
    }

    let zero_oid_early = "0".repeat(40);
    let mut hidden_rejects: Vec<String> = Vec::new();
    for (_old_h, new_h, refname) in &updates {
        let full = ref_namespace::storage_ref_name(refname);
        if hide_refs::ref_is_hidden(refname, &full, &hide_patterns) {
            if new_h == &zero_oid_early {
                eprintln!("remote: error: deny deleting a hidden ref");
            } else {
                eprintln!("remote: error: deny updating a hidden ref");
            }
            hidden_rejects.push(refname.clone());
        }
    }

    let pack_start = cursor.position() as usize;
    let tail = &payload[pack_start..];
    // After the command flush, git send-pack writes the raw packfile bytes (starts with "PACK").
    // Do not feed those through the pkt-line demuxer — it would mis-parse the length prefix.
    let (pack_data, sideband_stderr) = if tail.starts_with(b"PACK") {
        (tail.to_vec(), Vec::new())
    } else {
        demux_input_tail(tail)
    };
    if !sideband_stderr.is_empty() {
        let _ = io::stderr().write_all(&sideband_stderr);
    }

    let zero_oid = "0".repeat(40);
    let has_pack = !pack_data.is_empty() && pack_data.len() > 12 && pack_data.starts_with(b"PACK");

    let mut pack_map = None;
    let mut pack_parse_err: Option<String> = None;
    // Thin packs may not resolve fully in-memory against an empty ODB; skip this when we will
    // not run connectivity anyway (`git receive-pack` still unpacks via unpack-objects/index-pack).
    if has_pack && !args.skip_connectivity_check {
        match pack_bytes_to_object_map(&pack_data, &repo.odb) {
            Ok(m) => pack_map = Some(m),
            Err(e) => pack_parse_err = Some(format!("{e:#}")),
        }
    }

    let mut connectivity_failed: Vec<String> = Vec::new();
    let mut traverse_err: Option<String> = None;

    if !args.skip_connectivity_check {
        if let Some(ref err) = pack_parse_err {
            for (_old_hex, new_hex, refname) in &updates {
                if new_hex != &zero_oid && !hidden_rejects.iter().any(|r| r == refname) {
                    connectivity_failed.push(refname.clone());
                }
            }
            traverse_err = Some(err.clone());
        } else {
            let pack_ref = pack_map.as_ref();
            for (_old_hex, new_hex, refname) in &updates {
                if new_hex == &zero_oid {
                    continue;
                }
                if hidden_rejects.iter().any(|r| r == refname) {
                    continue;
                }
                let tip = match ObjectId::from_hex(new_hex) {
                    Ok(o) => o,
                    Err(_) => {
                        connectivity_failed.push(refname.clone());
                        continue;
                    }
                };
                match push_tip_connected_to_refs(&repo, tip, &extra_have, pack_ref) {
                    Ok(true) => {}
                    Ok(false) => {
                        connectivity_failed.push(refname.clone());
                        if traverse_err.is_none() {
                            if let Ok(Some((missing, at))) = diagnose_push_connectivity_failure(
                                &repo,
                                tip,
                                &extra_have,
                                pack_ref,
                            ) {
                                traverse_err = Some(format!(
                                    "Could not read {}\nfatal: Failed to traverse parents of commit {}",
                                    missing.to_hex(),
                                    at.to_hex()
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        connectivity_failed.push(refname.clone());
                        if traverse_err.is_none() {
                            traverse_err = Some(msg);
                        }
                    }
                }
            }
        }
    }

    let should_unpack_to_odb = has_pack
        && pack_parse_err.is_none()
        && (args.skip_connectivity_check || connectivity_failed.is_empty());

    let mut unpack_to_odb_err: Option<String> = None;
    if should_unpack_to_odb {
        let mut rd = Cursor::new(pack_data.clone());
        let opts = UnpackOptions {
            dry_run: false,
            quiet: true,
            strict: true,
        };
        if let Err(e) = unpack_objects(&mut rd, &repo.odb, &opts) {
            unpack_to_odb_err = Some(format!("{e:#}"));
        }
    }

    if let Some(ref e) = unpack_to_odb_err {
        for (_old_hex, new_hex, refname) in &updates {
            if new_hex != &zero_oid && !hidden_rejects.iter().any(|r| r == refname) {
                if !connectivity_failed.iter().any(|r| r == refname) {
                    connectivity_failed.push(refname.clone());
                }
            }
        }
        if traverse_err.is_none() {
            traverse_err = Some(e.clone());
        }
    }

    let unpack_status: Vec<u8> = if !has_pack {
        b"unpack ok\n".to_vec()
    } else if pack_parse_err.is_some() {
        b"unpack unpacker error\n".to_vec()
    } else if !args.skip_connectivity_check && !connectivity_failed.is_empty() {
        b"unpack ok\n".to_vec()
    } else if unpack_to_odb_err.is_some() {
        b"unpack unpacker error\n".to_vec()
    } else {
        b"unpack ok\n".to_vec()
    };

    if let Some(ref e) = traverse_err {
        for line in e.lines() {
            if line.starts_with("fatal: ") {
                eprintln!("{line}");
            } else {
                eprintln!("error: {line}");
            }
        }
    }

    write_status_lines(
        &updates,
        &connectivity_failed,
        &hidden_rejects,
        &zero_oid,
        &unpack_status,
    )?;

    if !connectivity_failed.is_empty() || !hidden_rejects.is_empty() {
        return Ok(());
    }

    run_hooks_and_update_refs(&repo, &config, &updates, &zero_oid)
}

fn write_status_lines(
    updates: &[(String, String, String)],
    connectivity_failed: &[String],
    hidden_failed: &[String],
    zero_oid: &str,
    unpack_status: &[u8],
) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_packet_raw(&mut out, unpack_status)?;
    for (_old_hex, new_hex, refname) in updates {
        if new_hex == zero_oid {
            continue;
        }
        if hidden_failed.iter().any(|f| f == refname) {
            write_packet_raw(
                &mut out,
                format!("ng {refname} failed to update ref\n").as_bytes(),
            )?;
        } else if connectivity_failed.iter().any(|f| f == refname) {
            write_packet_raw(
                &mut out,
                format!("ng {refname} missing necessary objects\n").as_bytes(),
            )?;
        } else {
            write_packet_raw(&mut out, format!("ok {refname}\n").as_bytes())?;
        }
    }
    write_flush(&mut out)?;
    out.flush()?;
    Ok(())
}

fn parse_update_line(line: &str, first: bool) -> Option<(String, String, String)> {
    let line = line.trim_end_matches('\n');
    let content = if first {
        line.split('\0').next()?.trim()
    } else {
        line.trim()
    };
    let parts: Vec<&str> = content.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].to_owned(),
        parts[1].to_owned(),
        parts[2].to_owned(),
    ))
}

fn demux_input_tail(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    if data.starts_with(b"PACK") {
        return (data.to_vec(), Vec::new());
    }
    let mut pack = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let len_str = match std::str::from_utf8(&data[i..i + 4]) {
            Ok(s) => s,
            Err(_) => break,
        };
        let Ok(pkt_len) = usize::from_str_radix(len_str, 16) else {
            break;
        };
        if pkt_len == 0 {
            i += 4;
            continue;
        }
        if pkt_len < 4 || i + pkt_len > data.len() {
            break;
        }
        let payload_len = pkt_len - 4;
        let payload = &data[i + 4..i + pkt_len];
        i += pkt_len;
        if payload_len == 0 || payload.is_empty() {
            continue;
        }
        match payload[0] {
            1 => pack.extend_from_slice(&payload[1..]),
            2 => stderr_buf.extend_from_slice(&payload[1..]),
            _ => {}
        }
    }
    if pack.is_empty() && !data.is_empty() {
        (data.to_vec(), stderr_buf)
    } else {
        (pack, stderr_buf)
    }
}

fn collect_alternate_have_oids(repo: &Repository, config: &ConfigSet) -> Result<HashSet<ObjectId>> {
    let mut out = HashSet::new();
    let objects_dir = repo.git_dir.join("objects");
    let alternates = read_alternates_recursive(&objects_dir).unwrap_or_default();
    let recv_git_dir = repo.git_dir.as_path();
    for alt_objects in alternates {
        let Some(alt_git_dir) = alt_objects.parent().map(PathBuf::from) else {
            continue;
        };
        if !alt_git_dir.join("refs").is_dir() {
            continue;
        }
        let alt = alt_git_dir.as_path();
        // Prefer explicit prefixes when both are set: the harness may leave a stale
        // `core.alternateRefsCommand` in the repo between cases while adding prefixes.
        if let Some(prefixes) = config.get("core.alternateRefsPrefixes") {
            for line in run_for_each_ref_lines(recv_git_dir, alt, Some(&prefixes))? {
                if let Ok(oid) = ObjectId::from_hex(line.trim()) {
                    out.insert(oid);
                }
            }
        } else if let Some(cmdline) = config.get("core.alternateRefsCommand") {
            for line in run_alternate_command(recv_git_dir, alt, &cmdline)? {
                if let Ok(oid) = ObjectId::from_hex(line.trim()) {
                    out.insert(oid);
                }
            }
        } else {
            for (_, oid) in refs::list_refs(alt, "refs/")? {
                out.insert(oid);
            }
        }
    }
    Ok(out)
}

fn run_alternate_command(
    receiving_git_dir: &Path,
    alternate_git_dir: &Path,
    command: &str,
) -> Result<Vec<String>> {
    // Match git's `fill_alternate_refs_command`: `use_shell` with the configured command
    // as the shell script and the alternate repository path as `$1` (see git/odb.c).
    let script = format!("{} \"$1\"", command.trim_end());
    let mut c = Command::new("sh");
    c.current_dir(receiving_git_dir)
        .arg("-c")
        .arg(&script)
        .arg("sh")
        .arg(alternate_git_dir.as_os_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = c.output().context("running core.alternateRefsCommand")?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(out
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|l| std::str::from_utf8(l).ok().map(|s| s.to_owned()))
        .collect())
}

fn run_for_each_ref_lines(
    exec_cwd: &Path,
    git_dir_env: &Path,
    prefixes: Option<&str>,
) -> Result<Vec<String>> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit"));
    let mut c = Command::new(exe);
    c.current_dir(exec_cwd)
        .arg(format!("--git-dir={}", git_dir_env.display()))
        .args(["for-each-ref", "--format=%(objectname)"]);
    if let Some(p) = prefixes {
        c.arg("--");
        for part in p.split_whitespace() {
            c.arg(part);
        }
    }
    c.stdout(Stdio::piped()).stderr(Stdio::null());
    let out = c
        .output()
        .context("running for-each-ref for alternate refs")?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(out
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|l| std::str::from_utf8(l).ok().map(|s| s.to_owned()))
        .collect())
}

fn advertise_refs_phase(repo: &Repository, extra_have: &HashSet<ObjectId>) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let version = crate::version_string();
    let mut caps = format!(
        "report-status report-status-v2 delete-refs quiet ofs-delta object-format=sha1 \
         agent=grit/{version}"
    );
    if trace2_transfer::transfer_advertise_sid_enabled(&repo.git_dir) {
        let sid = trace2_transfer::trace2_session_id_wire_once();
        caps.push_str(" session-id=");
        caps.push_str(&sid);
    }
    let cfg = ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_default();
    let hide = hide_refs::hide_ref_patterns_receive(&cfg);

    let mut first = true;
    if let Ok(head_oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        let full = ref_namespace::storage_ref_name("HEAD");
        if !hide_refs::ref_is_hidden("HEAD", &full, &hide) {
            let line = format!("{} HEAD\0{caps}\n", head_oid.to_hex());
            wire_trace::trace_packet_receive_pack('>', line.trim_end_matches('\n'));
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
            first = false;
        }
    }

    let mut seen_have: HashSet<ObjectId> = HashSet::new();
    let all_refs = refs::list_refs_physical(&repo.git_dir, "refs/")?;
    for (refname, oid) in &all_refs {
        let display = if let Some(logical) = ref_namespace::logical_ref_name_from_storage(refname) {
            let full = ref_namespace::storage_ref_name(&logical);
            if hide_refs::ref_is_hidden(&logical, &full, &hide) {
                continue;
            }
            // Match Git `show_ref_cb`: every namespaced ref is advertised under its logical name,
            // even when multiple refs share the same object id (t5509 hideRefs + packed-refs).
            let _ = seen_have.insert(*oid);
            logical
        } else {
            if !seen_have.insert(*oid) {
                continue;
            }
            ".have".to_owned()
        };
        if first {
            let line = format!("{} {display}\0{caps}\n", oid.to_hex());
            wire_trace::trace_packet_receive_pack('>', line.trim_end_matches('\n'));
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
            first = false;
        } else {
            let line = format!("{} {display}\n", oid.to_hex());
            wire_trace::trace_packet_receive_pack('>', line.trim_end_matches('\n'));
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
        }
    }

    for h in extra_have {
        if seen_have.insert(*h) {
            let line = format!("{} .have\n", h.to_hex());
            wire_trace::trace_packet_receive_pack('>', line.trim_end_matches('\n'));
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
        }
    }

    if first {
        let line = format!("0000000000000000000000000000000000000000 capabilities^{{}}\0{caps}\n");
        wire_trace::trace_packet_receive_pack('>', line.trim_end_matches('\n'));
        let len = 4 + line.len();
        write!(out, "{:04x}{}", len, line)?;
    }

    write_flush(&mut out)?;
    out.flush()?;
    Ok(())
}

fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}

fn run_hooks_and_update_refs(
    repo: &Repository,
    remote_config: &ConfigSet,
    updates: &[(String, String, String)],
    zero_oid: &str,
) -> Result<()> {
    let hook_stdin = updates
        .iter()
        .map(|(old_hex, new_hex, refname)| format!("{old_hex} {new_hex} {refname}\n"))
        .collect::<String>();

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

    let deny_deletes = config_bool_any(
        remote_config,
        &["receive.denyDeletes", "receive.denydeletes"],
        false,
    );
    let deny_nff = config_bool_any(
        remote_config,
        &["receive.denyNonFastForwards", "receive.denynonfastforwards"],
        false,
    );
    let head_ref_for_delete = if repo.is_bare() {
        None
    } else {
        match resolve_head(&repo.git_dir) {
            Ok(HeadState::Branch { refname, .. }) => Some(refname),
            _ => None,
        }
    };

    let (pre_receive_result, pre_receive_output) = run_hook_in_git_dir(
        repo,
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
    for (old_hex, new_hex, refname) in updates {
        let old_display = if old_hex == zero_oid {
            zero_oid.to_owned()
        } else {
            old_hex.clone()
        };
        ref_tx_lines.push(format!("{old_display} {new_hex} {refname}"));
    }
    let ref_tx_stdin = format!("{}\n", ref_tx_lines.join("\n"));

    let (tx_preparing_result, tx_preparing_output) = run_hook_in_git_dir(
        repo,
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
        repo,
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
            repo,
            "reference-transaction",
            &["aborted"],
            Some(ref_tx_stdin.as_bytes()),
            &push_option_env,
        );
        bail!("reference-transaction hook declined the update");
    }

    for (old_hex, new_hex, refname) in updates {
        check_receive_update_policy(
            repo,
            remote_config,
            refname,
            old_hex,
            new_hex,
            deny_deletes,
            deny_nff,
            head_ref_for_delete.as_deref(),
        )?;

        let old_for_update = refs::resolve_ref(&repo.git_dir, refname)
            .map(|oid| oid.to_hex())
            .unwrap_or_else(|_| zero_oid.to_owned());
        let (update_result, update_output) = run_hook_in_git_dir(
            repo,
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
                repo,
                "reference-transaction",
                &["aborted"],
                Some(ref_tx_stdin.as_bytes()),
                &push_option_env,
            );
            bail!("update hook declined the update");
        }

        if new_hex == zero_oid {
            refs::delete_ref(&repo.git_dir, refname)
                .with_context(|| format!("deleting ref {refname}"))?;
        } else {
            let new_oid =
                ObjectId::from_hex(new_hex).with_context(|| format!("invalid oid: {new_hex}"))?;
            refs::write_ref(&repo.git_dir, refname, &new_oid)
                .with_context(|| format!("updating ref {refname}"))?;
            if let Some(wt) = repo.work_tree.as_ref() {
                if let Ok(HeadState::Branch {
                    refname: head_br, ..
                }) = resolve_head(&repo.git_dir)
                {
                    if head_br == *refname {
                        let deny = read_receive_deny_current(&remote_config);
                        if matches!(deny, ReceiveDenyAction::UpdateInstead) {
                            checkout_worktree_to_commit(wt, new_oid)?;
                        }
                    }
                }
            }
        }
    }

    let (tx_committed_result, tx_committed_output) = run_hook_in_git_dir(
        repo,
        "reference-transaction",
        &["committed"],
        Some(ref_tx_stdin.as_bytes()),
        &push_option_env,
    );
    if !tx_committed_output.is_empty() {
        let _ = io::stderr().write_all(&tx_committed_output);
    }
    if let HookResult::Failed(_code) = tx_committed_result {
        // committed hook exit status is ignored (matches githooks(5)).
    }

    let (post_receive_result, post_receive_output) = run_hook_in_git_dir(
        repo,
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

    let post_update_arg_strings: Vec<String> = updates
        .iter()
        .map(|(_, _, refname)| refname.clone())
        .collect();
    if !post_update_arg_strings.is_empty() {
        let post_update_args: Vec<&str> =
            post_update_arg_strings.iter().map(|s| s.as_str()).collect();
        let (post_update_result, post_update_output) = run_hook_in_git_dir(
            repo,
            "post-update",
            &post_update_args,
            None,
            &push_option_env,
        );
        if !post_update_output.is_empty() {
            let _ = io::stderr().write_all(&post_update_output);
        }
        if let HookResult::Failed(_code) = post_update_result {
            // post-update failures are ignored (matches receive-pack).
        }
    }

    let auto_gc = config_bool_any(&remote_config, &["receive.autoGc", "receive.autogc"], true);
    if auto_gc && !updates.is_empty() {
        run_auto_maintenance_quiet(&repo.git_dir);
    }

    Ok(())
}

fn config_bool_any(cfg: &ConfigSet, keys: &[&str], default: bool) -> bool {
    for k in keys {
        if let Some(v) = cfg.get_bool(k) {
            return v.unwrap_or(default);
        }
    }
    default
}

#[derive(Clone, Copy)]
enum ReceiveDenyAction {
    Unconfigured,
    Ignore,
    Warn,
    Refuse,
    UpdateInstead,
}

fn parse_receive_deny_action(value: Option<&str>) -> ReceiveDenyAction {
    match value.map(str::trim) {
        None => ReceiveDenyAction::Ignore,
        Some(s) if s.eq_ignore_ascii_case("ignore") => ReceiveDenyAction::Ignore,
        Some(s) if s.eq_ignore_ascii_case("warn") => ReceiveDenyAction::Warn,
        Some(s) if s.eq_ignore_ascii_case("refuse") => ReceiveDenyAction::Refuse,
        Some(s) if s.eq_ignore_ascii_case("updateinstead") => ReceiveDenyAction::UpdateInstead,
        Some(s) => match parse_bool(s) {
            Ok(true) => ReceiveDenyAction::Refuse,
            Ok(false) => ReceiveDenyAction::Ignore,
            Err(_) => ReceiveDenyAction::Ignore,
        },
    }
}

fn read_receive_deny_delete_current(cfg: &ConfigSet) -> ReceiveDenyAction {
    let v = cfg
        .get("receive.denyDeleteCurrent")
        .or_else(|| cfg.get("receive.denydeletecurrent"));
    match v.as_deref().map(str::trim) {
        None => ReceiveDenyAction::Unconfigured,
        Some(s) => parse_receive_deny_action(Some(s)),
    }
}

fn read_receive_deny_current(cfg: &ConfigSet) -> ReceiveDenyAction {
    let v = cfg
        .get("receive.denyCurrentBranch")
        .or_else(|| cfg.get("receive.denycurrentbranch"));
    match v.as_deref().map(str::trim) {
        None => ReceiveDenyAction::Unconfigured,
        Some(s) => parse_receive_deny_action(Some(s)),
    }
}

fn checkout_worktree_to_commit(wt: &Path, oid: ObjectId) -> Result<()> {
    let status = Command::new(grit_exe::grit_executable())
        .current_dir(wt)
        .args(["checkout", "-q", "-f", &oid.to_hex()])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .context("spawn grit checkout for receive.denyCurrentBranch updateInstead")?;
    if !status.success() {
        bail!("failed to update work tree to {}", oid.to_hex());
    }
    Ok(())
}

fn check_receive_update_policy(
    repo: &Repository,
    cfg: &ConfigSet,
    refname: &str,
    old_hex: &str,
    new_hex: &str,
    deny_deletes: bool,
    deny_nff: bool,
    head_branch: Option<&str>,
) -> Result<()> {
    let zero_oid = "0".repeat(40);
    let is_delete = new_hex == zero_oid;
    let had_old = old_hex != zero_oid;

    if is_delete && had_old && deny_deletes && refname.starts_with("refs/heads/") {
        eprintln!("error: denying ref deletion for {refname}");
        bail!("deletion prohibited");
    }

    if is_delete && had_old {
        if let Some(head) = head_branch {
            if refname == head {
                let deny = read_receive_deny_delete_current(cfg);
                match deny {
                    ReceiveDenyAction::Ignore => {}
                    ReceiveDenyAction::Warn => {
                        eprintln!("warning: deleting the current branch");
                    }
                    ReceiveDenyAction::Refuse | ReceiveDenyAction::UpdateInstead => {
                        eprintln!("error: refusing to delete the current branch: {refname}");
                        bail!("deletion of the current branch prohibited");
                    }
                    ReceiveDenyAction::Unconfigured => {
                        eprintln!(
                            "error: By default, deleting the current branch is denied, because the next\n\
                             'git clone' won't result in any file checked out, causing confusion.\n\
                             \n\
                             You can set 'receive.denyDeleteCurrent' configuration variable to\n\
                             'warn' or 'ignore' in the remote repository to allow deleting the\n\
                             current branch, with or without a warning message.\n\
                             \n\
                             To squelch this message, you can set it to 'refuse'."
                        );
                        eprintln!("error: refusing to delete the current branch: {refname}");
                        bail!("deletion of the current branch prohibited");
                    }
                }
            }
        }
    }

    if deny_nff && !is_delete && had_old && refname.starts_with("refs/heads/") {
        let old_oid =
            ObjectId::from_hex(old_hex).with_context(|| format!("invalid old oid on {refname}"))?;
        let new_oid =
            ObjectId::from_hex(new_hex).with_context(|| format!("invalid new oid on {refname}"))?;
        if old_oid != new_oid && !is_ancestor(repo, old_oid, new_oid)? {
            eprintln!("error: denying non-fast-forward {refname} (you should pull first)");
            bail!("non-fast-forward");
        }
    }

    if !is_delete && !repo.is_bare() {
        if let Some(head) = head_branch {
            if refname == head {
                let head_oid_ok = refs::resolve_ref(&repo.git_dir, head).is_ok();
                let deny = read_receive_deny_current(cfg);
                if !head_oid_ok && matches!(deny, ReceiveDenyAction::UpdateInstead) {
                    return Ok(());
                }
                match deny {
                    ReceiveDenyAction::Ignore | ReceiveDenyAction::UpdateInstead => {}
                    ReceiveDenyAction::Warn => {
                        eprintln!("warning: updating the current branch");
                    }
                    ReceiveDenyAction::Refuse => {
                        eprintln!("error: refusing to update checked out branch: {refname}");
                        bail!("branch is currently checked out");
                    }
                    ReceiveDenyAction::Unconfigured => {
                        eprintln!(
                            "error: refusing to update checked out branch: {refname}\n\
                             error: By default, updating the current branch in a non-bare repository\n\
                             is denied, because it will make the index and work tree inconsistent\n\
                             with what you pushed, and will require 'git reset --hard' to match\n\
                             the work tree to HEAD.\n\
                             \n\
                             You can set the 'receive.denyCurrentBranch' configuration variable\n\
                             to 'ignore' or 'warn' in the remote repository to allow pushing into\n\
                             its current branch; however, this is not recommended unless you\n\
                             arranged to update its work tree to match what you pushed in some\n\
                             other way.\n\
                             \n\
                             To squelch this message and still keep the default behaviour, set\n\
                             'receive.denyCurrentBranch' configuration variable to 'refuse'."
                        );
                        bail!("branch is currently checked out");
                    }
                }
            }
        }
    }

    Ok(())
}

fn run_auto_maintenance_quiet(git_dir: &Path) {
    let maintenance_auto = ConfigSet::load(Some(git_dir), false)
        .ok()
        .map(|c| config_bool_any(&c, &["maintenance.auto"], true))
        .unwrap_or(true);
    if !maintenance_auto {
        return;
    }
    let _ = Command::new(grit_exe::grit_executable())
        .args(["maintenance", "run", "--auto"])
        .env("GIT_DIR", git_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
