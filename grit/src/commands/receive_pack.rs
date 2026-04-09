//! `grit receive-pack` — receive pushed objects (server side).
//!
//! Invoked on the remote side of a push.  Advertises refs, then reads
//! pack data from stdin and updates refs.  Only local transport is supported.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::hooks::{run_hook_in_git_dir, HookResult};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::pack;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::grit_exe;
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
    write_receive_pack_advertisement(&mut out, &repo.git_dir)?;
    pkt_line::write_flush(&mut out)?;
    out.flush()?;

    // Phase 2: Read ref update commands from stdin (pkt-line until flush)
    let mut stdin = io::stdin();
    let mut updates: Vec<(String, String, String)> = Vec::new(); // (old_hex, new_hex, refname)

    loop {
        match pkt_line::read_packet(&mut stdin)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => {
                let parts: Vec<&str> = line.splitn(3, ' ').collect();
                if parts.len() != 3 {
                    bail!("protocol error: malformed update line: {line}");
                }
                updates.push((
                    parts[0].to_owned(),
                    parts[1].to_owned(),
                    parts[2].to_owned(),
                ));
            }
            _ => {}
        }
    }

    // Phase 3: Read pack data from stdin (if any updates have new objects).
    // Side-band-64k wraps the PACK stream; strip it before indexing.
    let mut raw_input = Vec::new();
    let _ = stdin.read_to_end(&mut raw_input);
    let pack_data = if raw_input.len() >= 12 && &raw_input[..4] == b"PACK" {
        raw_input
    } else {
        pkt_line::decode_sideband_primary(&raw_input).unwrap_or_default()
    };

    if !pack_data.is_empty() && pack_data.len() >= 12 && &pack_data[..4] == b"PACK" {
        crate::commands::index_pack::ingest_pack_bytes(&repo, &pack_data, true)
            .context("indexing received pack")?;
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

    let deny_deletes = config_bool_any(
        &remote_config,
        &["receive.denyDeletes", "receive.denydeletes"],
        false,
    );
    let deny_nff = config_bool_any(
        &remote_config,
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

    for (old_hex, new_hex, refname) in &updates {
        check_receive_update_policy(
            &repo,
            &remote_config,
            refname,
            old_hex,
            new_hex,
            deny_deletes,
            deny_nff,
            head_ref_for_delete.as_deref(),
        )?;

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

    let auto_gc = config_bool_any(&remote_config, &["receive.autoGc", "receive.autogc"], true);
    if auto_gc && !updates.is_empty() {
        run_auto_maintenance_quiet(&repo.git_dir);
    }

    Ok(())
}

fn read_bool_config(cfg: &ConfigSet, key: &str, default: bool) -> bool {
    cfg.get_bool(key).unwrap_or(Ok(default)).unwrap_or(default)
}

fn config_bool_any(cfg: &ConfigSet, keys: &[&str], default: bool) -> bool {
    for k in keys {
        if let Some(v) = cfg.get_bool(k) {
            return v.unwrap_or(default);
        }
    }
    default
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

    if deny_nff && !is_delete && had_old
        && refname.starts_with("refs/heads/") {
            let old_oid = ObjectId::from_hex(old_hex)
                .with_context(|| format!("invalid old oid on {refname}"))?;
            let new_oid = ObjectId::from_hex(new_hex)
                .with_context(|| format!("invalid new oid on {refname}"))?;
            if old_oid != new_oid && !is_ancestor(repo, old_oid, new_oid)? {
                eprintln!("error: denying non-fast-forward {refname} (you should pull first)");
                bail!("non-fast-forward");
            }
        }

    Ok(())
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
    use grit_lib::config::parse_bool;
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

fn run_auto_maintenance_quiet(git_dir: &Path) {
    let maintenance_auto = ConfigSet::load(Some(git_dir), false)
        .ok()
        .map(|c| config_bool_any(&c, &["maintenance.auto"], true))
        .unwrap_or(true);
    if !maintenance_auto {
        return;
    }
    let _ = std::process::Command::new(grit_exe::grit_executable())
        .args(["maintenance", "run", "--auto"])
        .env("GIT_DIR", git_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn write_receive_pack_advertisement(w: &mut impl Write, git_dir: &Path) -> Result<()> {
    let caps = "report-status report-status-v2 delete-refs side-band-64k quiet ofs-delta object-format=sha1";
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut sent_caps = false;

    if let Ok(head) = resolve_head(git_dir) {
        let oid_opt = match &head {
            HeadState::Branch { oid, .. } => *oid,
            HeadState::Detached { oid } => Some(*oid),
            HeadState::Invalid => None,
        };
        if let Some(oid) = oid_opt {
            if seen.insert(oid) {
                let line = if !sent_caps {
                    sent_caps = true;
                    format!("{} HEAD\0{}\n", oid.to_hex(), caps)
                } else {
                    format!("{} HEAD\n", oid.to_hex())
                };
                let len = 4 + line.len();
                write!(w, "{:04x}{}", len, line)?;
            }
        }
    }

    let mut all_refs = list_all_refs(git_dir)?;
    all_refs.sort_by(|a, b| a.0.cmp(&b.0));
    for (refname, oid) in &all_refs {
        if refname == "refs/heads/HEAD" {
            continue;
        }
        if !seen.insert(*oid) {
            continue;
        }
        let line = if !sent_caps {
            sent_caps = true;
            format!("{} {}\0{}\n", oid.to_hex(), refname, caps)
        } else {
            format!("{} {}\n", oid.to_hex(), refname)
        };
        let len = 4 + line.len();
        write!(w, "{:04x}{}", len, line)?;
    }

    let objects_dir = git_dir.join("objects");
    if let Ok(alts) = pack::read_alternates_recursive(&objects_dir) {
        for alt_objects in alts {
            let Some(parent) = alt_objects.parent() else {
                continue;
            };
            let alt_git_dir = parent.to_path_buf();
            if alt_git_dir == git_dir {
                continue;
            }
            let mut alt_refs = list_all_refs(&alt_git_dir).unwrap_or_default();
            alt_refs.sort_by(|a, b| a.0.cmp(&b.0));
            for (_refname, oid) in alt_refs {
                if seen.insert(oid) {
                    let line = format!("{} .have\n", oid.to_hex());
                    let len = 4 + line.len();
                    write!(w, "{:04x}{}", len, line)?;
                }
            }
        }
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
