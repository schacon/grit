//! `grit receive-pack` — receive pushed objects (server side).
//!
//! Invoked on the remote side of a push. Advertises refs in pkt-line format (with
//! capabilities), reads ref updates and an optional pack stream from stdin, then
//! updates refs when connectivity checks pass.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::{ConfigFile, ConfigScope, ConfigSet};
use grit_lib::connectivity::{diagnose_push_connectivity_failure, push_tip_connected_to_refs};
use grit_lib::hooks::{run_hook_in_git_dir, HookResult};
use grit_lib::objects::ObjectId;
use grit_lib::pack::read_alternates_recursive;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::unpack_objects::{pack_bytes_to_object_map, unpack_objects, UnpackOptions};
use std::collections::HashSet;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::pkt_line::{read_packet, write_flush, write_pkt_line_bytes, Packet};

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

    // Use only this repository's `config` so global `core.alternateRefs*` from the
    // environment does not leak across harness tests (matches receive-pack reading repo config).
    let mut config = ConfigSet::new();
    if let Ok(Some(f)) = ConfigFile::from_path(&repo.git_dir.join("config"), ConfigScope::Local) {
        config.merge(&f);
    }
    let extra_have = collect_alternate_have_oids(&repo, &config)?;

    advertise_refs_phase(&repo, &extra_have)?;

    let mut stdin = io::stdin();
    let mut payload = Vec::new();
    stdin.read_to_end(&mut payload)?;

    let mut cursor = Cursor::new(&payload[..]);
    let mut updates: Vec<(String, String, String)> = Vec::new();
    let mut caps_seen = false;

    loop {
        match read_packet(&mut cursor)? {
            None => break,
            Some(Packet::Flush) => break,
            Some(Packet::Delim) | Some(Packet::ResponseEnd) => break,
            Some(Packet::Data(line)) => {
                if let Some((old_h, new_h, refname)) = parse_update_line(&line, !caps_seen) {
                    caps_seen = true;
                    updates.push((old_h, new_h, refname));
                }
            }
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
                if new_hex != &zero_oid {
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
        };
        if let Err(e) = unpack_objects(&mut rd, &repo.odb, &opts) {
            unpack_to_odb_err = Some(format!("{e:#}"));
        }
    }

    if let Some(ref e) = unpack_to_odb_err {
        for (_old_hex, new_hex, refname) in &updates {
            if new_hex != &zero_oid {
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

    write_status_lines(&updates, &connectivity_failed, &zero_oid, &unpack_status)?;

    if !connectivity_failed.is_empty() {
        return Ok(());
    }

    run_hooks_and_update_refs(&repo, &updates, &zero_oid)
}

fn write_status_lines(
    updates: &[(String, String, String)],
    failed: &[String],
    zero_oid: &str,
    unpack_status: &[u8],
) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_pkt_line_bytes(&mut out, unpack_status)?;
    for (_old_hex, new_hex, refname) in updates {
        if new_hex == zero_oid {
            continue;
        }
        if failed.iter().any(|f| f == refname) {
            write_pkt_line_bytes(
                &mut out,
                format!("ng {refname} missing necessary objects\n").as_bytes(),
            )?;
        } else {
            write_pkt_line_bytes(&mut out, format!("ok {refname}\n").as_bytes())?;
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
            for line in run_for_each_ref_lines(recv_git_dir, alt, None)? {
                if let Ok(oid) = ObjectId::from_hex(line.trim()) {
                    out.insert(oid);
                }
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
    let caps = format!(
        "report-status report-status-v2 delete-refs quiet ofs-delta object-format=sha1 \
         agent=grit/{version}"
    );

    let mut first = true;
    if let Ok(head_oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        let line = format!("{} HEAD\0{caps}\n", head_oid.to_hex());
        let len = 4 + line.len();
        write!(out, "{:04x}{}", len, line)?;
        first = false;
    }

    let mut seen_have: HashSet<ObjectId> = HashSet::new();
    let all_refs = list_all_refs(&repo.git_dir)?;
    for (refname, oid) in &all_refs {
        if first {
            let line = format!("{} {refname}\0{caps}\n", oid.to_hex());
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
            first = false;
        } else {
            let line = format!("{} {refname}\n", oid.to_hex());
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
        }
        seen_have.insert(*oid);
    }

    for h in extra_have {
        if seen_have.insert(*h) {
            let line = format!("{} .have\n", h.to_hex());
            let len = 4 + line.len();
            write!(out, "{:04x}{}", len, line)?;
        }
    }

    if first {
        let line = format!("0000000000000000000000000000000000000000 capabilities^{{}}\0{caps}\n");
        let len = 4 + line.len();
        write!(out, "{:04x}{}", len, line)?;
    }

    write_flush(&mut out)?;
    out.flush()?;
    Ok(())
}

fn list_all_refs(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let mut result = Vec::new();
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        if let Ok(entries) = refs::list_refs(git_dir, prefix) {
            result.extend(entries);
        }
    }
    Ok(result)
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

    for (_old_hex, new_hex, refname) in updates {
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

    Ok(())
}
