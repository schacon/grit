//! `grit send-pack` — push objects to a remote repository (plumbing).
//!
//! Speaks the Git wire protocol to a local `receive-pack` child (default:
//! `grit receive-pack <git-dir>`). Path remotes only.

use anyhow::{bail, Context, Result};
use grit_lib::merge_base::is_ancestor;

use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::grit_exe::grit_executable;
use crate::pkt_line;
use crate::trace_packet;

/// Parsed `send-pack` invocation (manual argv parsing so refspecs like `:branch` work).
#[derive(Debug)]
pub struct Args {
    pub receive_pack: String,
    pub all: bool,
    pub force: bool,
    pub dry_run: bool,
    pub remote: String,
    pub refs: Vec<String>,
}

/// Entry point from `main`: parse argv (not clap — leading-colon refspecs are not options).
pub fn run_from_argv(rest: &[String]) -> Result<()> {
    if rest.len() == 1 && (rest[0] == "-h" || rest[0] == "--help") {
        let code = if rest[0] == "--help" { 0i32 } else { 129 };
        // Tests redirect stdout (`>usage`); match Git/clap behavior on stdout.
        println!(
            "usage: git send-pack [--mirror] [--dry-run] [--force]\n\
             [--receive-pack=<git-receive-pack>]\n\
             [--verbose] [--thin] [--atomic]\n\
             [<host>:]<directory> (--all | <ref>...)"
        );
        std::process::exit(code);
    }
    let args = parse_send_pack_argv(rest)?;
    run(args)
}

fn parse_send_pack_argv(rest: &[String]) -> Result<Args> {
    let mut receive_pack = String::new();
    let mut all = false;
    let mut force = false;
    let mut dry_run = false;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 0usize;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if arg == "--" {
            i += 1;
            while i < rest.len() {
                positionals.push(rest[i].clone());
                i += 1;
            }
            break;
        }
        if let Some(v) = arg.strip_prefix("--receive-pack=") {
            receive_pack = v.to_owned();
            i += 1;
            continue;
        }
        if arg == "--receive-pack" || arg == "--exec" {
            i += 1;
            let v = rest
                .get(i)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("option `{}` requires a value", arg))?;
            receive_pack = v;
            i += 1;
            continue;
        }
        if arg == "-f" || arg == "--force" {
            force = true;
            i += 1;
            continue;
        }
        if arg == "-n" || arg == "--dry-run" {
            dry_run = true;
            i += 1;
            continue;
        }
        if arg == "--all" {
            all = true;
            i += 1;
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            bail!("unknown option `{arg}`");
        }
        positionals.push(rest[i].clone());
        i += 1;
    }

    let Some(remote) = positionals.first().cloned() else {
        bail!("usage: git send-pack [<options>] <remote> [<refs>...]");
    };
    let refs: Vec<String> = positionals.into_iter().skip(1).collect();

    Ok(Args {
        receive_pack,
        all,
        force,
        dry_run,
        remote,
        refs,
    })
}

struct RefUpdate {
    local_ref: Option<String>,
    remote_ref: String,
    old_oid: Option<ObjectId>,
    new_oid: Option<ObjectId>,
    refspec_force: bool,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let remote_path = resolve_remote_path(&args.remote);
    let remote_repo = open_repo(&remote_path).with_context(|| {
        format!(
            "could not open remote repository at '{}'",
            remote_path.display()
        )
    })?;

    let mut updates = build_updates(&repo, &remote_repo, &args)?;

    if updates.is_empty() {
        bail!("no refs specified; nothing to push");
    }

    if args.dry_run {
        print_updates(&updates, true);
        return Ok(());
    }

    let remote_git_dir = remote_repo.git_dir.clone();
    let mut child = spawn_receive_pack(&args.receive_pack, &remote_git_dir)?;
    let mut child_stdin = child.stdin.take().context("receive-pack stdin")?;
    let mut child_stdout = child.stdout.take().context("receive-pack stdout")?;

    let advertised = read_advertisement(&mut child_stdout)?;
    let mut ref_to_advertised: HashMap<String, ObjectId> = HashMap::new();
    let mut have_oids: Vec<ObjectId> = Vec::new();
    let mut seen_have: HashSet<ObjectId> = HashSet::new();

    for (name, oid) in &advertised {
        if name == ".have" {
            if seen_have.insert(*oid) {
                have_oids.push(*oid);
            }
            continue;
        }
        if name == "HEAD" || name.starts_with("refs/") {
            ref_to_advertised.insert(name.clone(), *oid);
        }
    }

    for u in &mut updates {
        if let Some(adv) = ref_to_advertised.get(&u.remote_ref) {
            u.old_oid = Some(*adv);
        }
    }

    for u in &updates {
        if let (Some(old), Some(new)) = (&u.old_oid, &u.new_oid) {
            if old != new && !args.force && !u.refspec_force && !is_ancestor(&repo, *old, *new)? {
                bail!(
                    "non-fast-forward update to '{}' rejected (use --force to override)",
                    u.remote_ref
                );
            }
        }
    }

    let zero_hex = "0".repeat(40);
    for u in &updates {
        let old_hex = u
            .old_oid
            .map(|o| o.to_hex())
            .unwrap_or_else(|| zero_hex.clone());
        let new_hex = u
            .new_oid
            .map(|o| o.to_hex())
            .unwrap_or_else(|| zero_hex.clone());
        let line = format!("{old_hex} {new_hex} {}", u.remote_ref);
        pkt_line::write_line(&mut child_stdin, &line)?;
    }
    pkt_line::write_flush(&mut child_stdin)?;
    child_stdin.flush()?;

    let mut want_commits: Vec<ObjectId> = Vec::new();
    for u in &updates {
        if let Some(oid) = u.new_oid {
            want_commits.push(oid);
        }
    }
    want_commits.sort_by_key(|o| o.to_hex());
    want_commits.dedup();

    if !want_commits.is_empty() {
        let mut pack_child = crate::pack_objects_upload::spawn_pack_objects_upload(&repo.git_dir)?;
        let mut pin = pack_child.stdin.take().context("pack-objects stdin")?;
        crate::pack_objects_upload::write_pack_objects_revs_stdin(
            &mut pin,
            &want_commits,
            &have_oids,
        )?;
        drop(pin);

        let mut sideband_out = Vec::new();
        crate::pack_objects_upload::drain_pack_objects_child(pack_child, &mut sideband_out, true)?;
        child_stdin.write_all(&sideband_out)?;
        child_stdin.flush()?;
    }

    drop(child_stdin);

    let mut status_out = Vec::new();
    child_stdout.read_to_end(&mut status_out)?;
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "receive-pack exited with status {}",
            status.code().unwrap_or(-1)
        );
    }

    print_updates(&updates, false);
    Ok(())
}

fn print_updates(updates: &[RefUpdate], dry_run: bool) {
    let suffix = if dry_run { " (dry run)" } else { "" };
    for u in updates {
        let old_hex = u
            .old_oid
            .as_ref()
            .map(|o| o.to_hex())
            .unwrap_or_else(|| "0".repeat(40));
        let new_hex = u
            .new_oid
            .as_ref()
            .map(|o| o.to_hex())
            .unwrap_or_else(|| "0".repeat(40));
        println!(
            "{}..{}\t{}{suffix}",
            &old_hex[..7],
            &new_hex[..7],
            u.remote_ref,
        );
    }
}

fn resolve_remote_path(remote: &str) -> PathBuf {
    if let Some(idx) = remote.rfind(':') {
        if idx > 0 {
            let hostish = &remote[..idx];
            let path_part = &remote[idx + 1..];
            if hostish.contains('@')
                || hostish == "file"
                || hostish.starts_with("ssh.")
                || hostish.len() == 1
            {
                return PathBuf::from(path_part);
            }
        }
    }
    PathBuf::from(remote)
}

fn build_updates(local: &Repository, remote: &Repository, args: &Args) -> Result<Vec<RefUpdate>> {
    let mut updates = Vec::new();
    if args.all {
        let branches = refs::list_refs(&local.git_dir, "refs/heads/")?;
        for (refname, oid) in branches {
            let old_oid = refs::resolve_ref(&remote.git_dir, &refname).ok();
            if old_oid.as_ref() == Some(&oid) {
                continue;
            }
            updates.push(RefUpdate {
                local_ref: Some(refname.clone()),
                remote_ref: refname,
                old_oid,
                new_oid: Some(oid),
                refspec_force: args.force,
            });
        }
        return Ok(updates);
    }

    if args.refs.is_empty() {
        bail!("no refs specified; nothing to push");
    }

    for spec in &args.refs {
        let (per_force, spec_clean) = if let Some(s) = spec.strip_prefix('+') {
            (true, s)
        } else {
            (false, spec.as_str())
        };
        let (src, dst) = parse_refspec(spec_clean);

        if src.is_empty() {
            let remote_ref = normalize_ref(&dst);
            let old_oid = refs::resolve_ref(&remote.git_dir, &remote_ref).ok();
            if old_oid.is_none() {
                continue;
            }
            updates.push(RefUpdate {
                local_ref: None,
                remote_ref,
                old_oid,
                new_oid: None,
                refspec_force: per_force || args.force,
            });
            continue;
        }

        if src.contains('*') {
            let local_refs = refs::list_refs(&local.git_dir, "refs/")?;
            for (refname, local_oid) in &local_refs {
                if let Some(matched) = match_glob(&src, refname) {
                    if refs::read_symbolic_ref(&local.git_dir, refname)?.is_some() {
                        continue;
                    }
                    let remote_ref = dst.replacen('*', matched, 1);
                    let old_oid = refs::resolve_ref(&remote.git_dir, &remote_ref).ok();
                    if old_oid.as_ref() == Some(local_oid) {
                        continue;
                    }
                    updates.push(RefUpdate {
                        local_ref: Some(refname.clone()),
                        remote_ref,
                        old_oid,
                        new_oid: Some(*local_oid),
                        refspec_force: per_force || args.force,
                    });
                }
            }
            continue;
        }

        let resolved_src = if src == "HEAD" {
            match resolve_head(&local.git_dir)? {
                HeadState::Branch { refname, .. } => refname,
                HeadState::Detached { oid } => oid.to_hex(),
                HeadState::Invalid => src.clone(),
            }
        } else {
            src.clone()
        };

        let effective_dst = if dst == "HEAD" && src == "HEAD" {
            resolved_src.clone()
        } else {
            dst.clone()
        };

        let (local_ref, local_oid) = resolve_push_src(&local.git_dir, &resolved_src)
            .with_context(|| format!("src ref '{}' does not match any", src))?;

        let remote_ref = if !spec_clean.contains(':') && !effective_dst.starts_with("refs/") {
            if local_ref.starts_with("refs/tags/") {
                format!("refs/tags/{effective_dst}")
            } else {
                normalize_ref(&effective_dst)
            }
        } else {
            normalize_ref(&effective_dst)
        };

        let old_oid = refs::resolve_ref(&remote.git_dir, &remote_ref).ok();

        updates.push(RefUpdate {
            local_ref: Some(local_ref),
            remote_ref,
            old_oid,
            new_oid: Some(local_oid),
            refspec_force: per_force || args.force,
        });
    }

    Ok(updates)
}

fn read_advertisement(r: &mut impl Read) -> Result<Vec<(String, ObjectId)>> {
    let mut out = Vec::new();
    loop {
        match pkt_line::read_packet(r)? {
            None => break,
            Some(pkt_line::Packet::Flush) => {
                trace_packet::trace_packet_git_style("push", '<', b"0000");
                break;
            }
            Some(pkt_line::Packet::Data(line)) => {
                trace_packet::trace_packet_git_style("push", '<', line.as_bytes());
                let line = line.trim_end_matches('\n');
                let mut parts = line.splitn(2, '\t');
                let hex = parts.next().unwrap_or("").trim();
                let rest = parts.next().unwrap_or("");
                let rest = rest.split('\0').next().unwrap_or("").trim();
                if rest.is_empty() {
                    continue;
                }
                let oid =
                    ObjectId::from_hex(hex).with_context(|| format!("bad ref line: {line}"))?;
                out.push((rest.to_string(), oid));
            }
            _ => {}
        }
    }
    Ok(out)
}

fn spawn_receive_pack(receive_pack: &str, remote_git_dir: &Path) -> Result<std::process::Child> {
    let grit = grit_executable();
    let inner = if receive_pack.trim().is_empty() {
        format!("\"{}\" receive-pack", grit.display())
    } else {
        receive_pack.trim().to_owned()
    };
    Command::new("sh")
        .arg("-c")
        .arg(format!("{inner} \"$1\""))
        .arg("_")
        .arg(remote_git_dir)
        .env_remove("GIT_CONFIG_PARAMETERS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn receive-pack: {inner}"))
}

fn parse_refspec(spec: &str) -> (String, String) {
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    if let Some((src, dst)) = spec.split_once(':') {
        (src.to_owned(), dst.to_owned())
    } else {
        (spec.to_owned(), spec.to_owned())
    }
}

fn normalize_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

fn match_glob<'a>(pattern: &str, refname: &'a str) -> Option<&'a str> {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        if refname.starts_with(prefix)
            && refname.ends_with(suffix)
            && refname.len() >= prefix.len() + suffix.len()
        {
            Some(&refname[prefix.len()..refname.len() - suffix.len()])
        } else {
            None
        }
    } else if pattern == refname {
        Some(refname)
    } else {
        None
    }
}

fn resolve_push_src(git_dir: &Path, src: &str) -> Result<(String, ObjectId)> {
    if src.starts_with("refs/") {
        let oid = refs::resolve_ref(git_dir, src)?;
        return Ok((src.to_owned(), oid));
    }
    if src.len() == 40 {
        if let Ok(oid) = src.parse::<ObjectId>() {
            return Ok((src.to_owned(), oid));
        }
    }
    let mut matches: Vec<(String, ObjectId)> = Vec::new();
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/"] {
        let full = format!("{prefix}{src}");
        if let Ok(oid) = refs::resolve_ref(git_dir, &full) {
            matches.push((full, oid));
        }
    }
    match matches.len() {
        0 => bail!("ref not found: {}", src),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            eprintln!("error: src refspec {} matches more than one", src);
            bail!("failed to push some refs");
        }
    }
}

fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}
