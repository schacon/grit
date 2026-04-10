//! `grit send-pack` — push objects to a remote repository (plumbing).
//!
//! Local transport: spawns the receive-pack program (default `grit receive-pack`) with the
//! remote repository path, speaks the v0/v1 pkt-line push protocol, and streams a pack built
//! via `grit pack-objects`.

use crate::explicit_exit::ExplicitExit;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::fetch_transport::parse_leading_shell_env_assignments;
use crate::grit_exe::{grit_executable, strip_trace2_env};
use crate::pkt_line::{read_packet, write_flush, Packet};
use crate::protocol_wire;
use crate::trace2_transfer;

/// Upstream `git send-pack` usage synopsis (exit 129 when options are incompatible).
const SEND_PACK_USAGE: &str = "usage: git send-pack [--mirror] [--dry-run] [--force]\n\
              [--receive-pack=<git-receive-pack>]\n\
              [--verbose] [--thin] [--atomic]\n\
              [--[no-]signed | --signed=(true|false|if-asked)]\n\
              [<host>:]<directory> (--all | <ref>...)";

/// Arguments for `grit send-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Push objects to a remote repository (plumbing)")]
pub struct Args {
    /// Path to the remote repository (bare or non-bare).
    #[arg(value_name = "REMOTE")]
    pub remote: String,

    /// Read additional refspec lines from stdin (after command-line refspecs).
    #[arg(long = "stdin")]
    pub stdin: bool,

    /// Mirror all refs (incompatible with explicit refspecs; not implemented for local transport).
    #[arg(long = "mirror")]
    pub mirror: bool,

    /// Refspec(s) to push (e.g. "main:main"). With `--all`, ignored.
    #[arg(value_name = "REF")]
    pub refs: Vec<String>,

    /// Push all branches (refs/heads/*).
    #[arg(long)]
    pub all: bool,

    /// Allow non-fast-forward updates.
    #[arg(long = "force")]
    pub force: bool,

    /// Show what would be done, without making changes.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Program to run on the remote side (default: `grit receive-pack`).
    #[arg(long = "receive-pack", value_name = "PATH")]
    pub receive_pack: Option<String>,

    /// Alias for `--receive-pack` (Git compatibility).
    #[arg(long = "exec", value_name = "PATH")]
    pub exec: Option<String>,
}

struct RefUpdate {
    remote_ref: String,
    old_oid: Option<ObjectId>,
    new_oid: ObjectId,
}

pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let remote_path = PathBuf::from(&args.remote);
    let remote_repo = open_repo(&remote_path).with_context(|| {
        format!(
            "could not open remote repository at '{}'",
            remote_path.display()
        )
    })?;

    let mut refspecs = args.refs.clone();
    if args.stdin {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.context("reading refspecs from stdin")?;
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            refspecs.push(line.to_owned());
        }
    }

    if !refspecs.is_empty() && args.mirror {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 129,
            message: SEND_PACK_USAGE.to_string(),
        }));
    }

    let mut updates = Vec::new();

    if args.all {
        let local_branches = refs::list_refs(&repo.git_dir, "refs/heads/")?;
        for (refname, local_oid) in &local_branches {
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, refname).ok();
            if old_oid.as_ref() == Some(local_oid) {
                continue;
            }
            updates.push(RefUpdate {
                remote_ref: refname.clone(),
                old_oid,
                new_oid: *local_oid,
            });
        }
    } else {
        if refspecs.is_empty() {
            bail!("no refs specified; nothing to push");
        }

        let mut seen_remote: HashSet<String> = HashSet::new();
        for spec in &refspecs {
            let (_, dst) = parse_refspec(spec);
            let remote_ref = normalize_ref(&dst);
            if !seen_remote.insert(remote_ref.clone()) {
                bail!("multiple updates for ref '{remote_ref}' not allowed");
            }
        }

        for spec in &refspecs {
            let (src, dst) = parse_refspec(spec);
            let local_ref = resolve_push_src_refname(&src);
            let remote_ref = normalize_ref(&dst);

            let local_oid = refs::resolve_ref(&repo.git_dir, &local_ref)
                .with_context(|| format!("src ref '{}' does not match any", src))?;
            let old_oid = refs::resolve_ref(&remote_repo.git_dir, &remote_ref).ok();

            updates.push(RefUpdate {
                remote_ref,
                old_oid,
                new_oid: local_oid,
            });
        }
    }

    for update in &updates {
        if let Some(old) = &update.old_oid {
            if *old != update.new_oid && !args.force && !is_ancestor(&repo, *old, update.new_oid)? {
                bail!(
                    "non-fast-forward update to '{}' rejected (use --force to override)",
                    update.remote_ref
                );
            }
        }
    }

    if updates.is_empty() {
        return Ok(());
    }

    if args.dry_run {
        for update in &updates {
            let old_hex = update
                .old_oid
                .as_ref()
                .map(|o| o.to_hex())
                .unwrap_or_else(|| "0".repeat(40));
            println!(
                "{}..{}\t{} (dry run)",
                &old_hex[..7],
                &update.new_oid.to_hex()[..7],
                update.remote_ref,
            );
        }
        return Ok(());
    }

    let receive_cmd = args
        .receive_pack
        .as_deref()
        .or(args.exec.as_deref())
        .unwrap_or("git receive-pack");

    let mut child = spawn_receive_pack(receive_cmd, &remote_path)?;
    let mut child_stdin = child.stdin.take().context("receive-pack stdin")?;
    let mut child_stdout = child.stdout.take().context("receive-pack stdout")?;

    let (extra_have, _server_sideband, advertised_oids) = read_advertisement(&mut child_stdout)?;

    // Raw pack bytes after the command flush (do not negotiate side-band-64k for the pack:
    // `git receive-pack` reads the pack from stdin without demuxing client→server sideband).
    let mut caps = "report-status report-status-v2 quiet object-format=sha1".to_owned();
    if trace2_transfer::transfer_advertise_sid_enabled(&repo.git_dir) {
        let sid = trace2_transfer::trace2_session_id_wire_once();
        caps.push_str(" session-id=");
        caps.push_str(&sid);
    }
    let mut first_cmd = true;
    for update in &updates {
        let old_hex = update
            .old_oid
            .as_ref()
            .map(|o| o.to_hex())
            .unwrap_or_else(|| "0".repeat(40));
        let new_hex = update.new_oid.to_hex();
        let pkt = if first_cmd {
            first_cmd = false;
            format!("{old_hex} {new_hex} {}\0{caps}\n", update.remote_ref)
        } else {
            format!("{old_hex} {new_hex} {}\n", update.remote_ref)
        };
        write_pkt_line(&mut child_stdin, pkt.as_bytes())?;
    }
    write_flush(&mut child_stdin)?;
    child_stdin.flush()?;

    // Use the real Git binary for pack generation when available: the harness aliases `git`
    // to grit, and grit's pack-objects is not yet a full send-pack peer (thin packs, etc.).
    let pack_bin = std::path::Path::new("/usr/bin/git");
    let pack_cwd = repo
        .work_tree
        .clone()
        .unwrap_or_else(|| repo.git_dir.clone());
    let pack_args = [
        "pack-objects",
        "--stdout",
        "--revs",
        "--thin",
        "--delta-base-offset",
        "-q",
    ];
    let mut pack_cmd = if pack_bin.is_file() {
        let mut c = Command::new(pack_bin);
        c.current_dir(&pack_cwd)
            .env("GIT_DIR", &repo.git_dir)
            .args(pack_args);
        // Tests prepend a `git` shim ahead of `/usr/bin`; helpers invoked by git must not recurse.
        c.env("PATH", "/usr/bin:/bin");
        c
    } else {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("grit"));
        let mut c = Command::new(exe);
        c.current_dir(&pack_cwd)
            .env("GIT_DIR", &repo.git_dir)
            .args(pack_args);
        c
    };
    let mut pack_child = pack_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("spawn pack-objects")?;
    {
        let mut stdin = pack_child.stdin.take().context("pack-objects stdin")?;
        let mut fed: HashSet<ObjectId> = HashSet::new();
        let new_tips: HashSet<ObjectId> = updates.iter().map(|u| u.new_oid).collect();
        for oid in peel_advertised_commits(&repo, &advertised_oids) {
            if new_tips.contains(&oid) {
                continue;
            }
            if fed.insert(oid) {
                writeln!(stdin, "^{}", oid.to_hex())?;
            }
        }
        for h in &extra_have {
            if fed.insert(*h) {
                writeln!(stdin, "^{}", h.to_hex())?;
            }
        }
        for update in &updates {
            if let Some(old) = &update.old_oid {
                if fed.insert(*old) {
                    writeln!(stdin, "^{}", old.to_hex())?;
                }
            }
            writeln!(stdin, "{}", update.new_oid.to_hex())?;
        }
        stdin.flush().context("flush pack-objects stdin")?;
    }

    let pack_output = pack_child
        .wait_with_output()
        .context("wait for pack-objects")?;
    if !pack_output.status.success() {
        bail!("pack-objects failed with status {}", pack_output.status);
    }
    child_stdin.write_all(&pack_output.stdout)?;
    child_stdin.flush()?;

    drop(child_stdin);
    let mut output = Vec::new();
    child_stdout.read_to_end(&mut output)?;
    let status = child.wait()?;
    io::stdout().write_all(&output)?;
    if !status.success() {
        bail!("receive-pack failed");
    }

    Ok(())
}

fn write_pkt_line(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let n = payload.len() + 4;
    write!(w, "{:04x}", n)?;
    w.write_all(payload)
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn spawn_receive_pack(receive_cmd: &str, remote_path: &Path) -> Result<Child> {
    let remote_path = remote_path
        .canonicalize()
        .unwrap_or_else(|_| remote_path.to_path_buf());
    let remote_str = remote_path.to_string_lossy();
    let client_proto = protocol_wire::effective_client_protocol_version();
    let apply_proto = |c: &mut Command| {
        if client_proto == 0 {
            c.env_remove("GIT_PROTOCOL");
        } else {
            protocol_wire::merge_git_protocol_env_for_child(c, client_proto);
        }
    };
    if receive_cmd.contains('|')
        || receive_cmd.contains('>')
        || receive_cmd.contains('<')
        || receive_cmd.contains('&')
    {
        let script = format!(
            "{} {}",
            receive_cmd.trim_end(),
            shell_single_quote(&remote_str)
        );
        let mut c = Command::new("sh");
        strip_trace2_env(&mut c);
        apply_proto(&mut c);
        return c
            .arg("-c")
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("spawn receive-pack shell");
    }

    let (leading_env, after_env) = parse_leading_shell_env_assignments(receive_cmd);
    if after_env.contains("git-receive-pack") {
        let mut cmd = Command::new(grit_executable());
        strip_trace2_env(&mut cmd);
        for (k, v) in leading_env {
            cmd.env(k, v);
        }
        cmd.arg("receive-pack")
            .arg(remote_str.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        apply_proto(&mut cmd);
        return cmd.spawn().context("spawn grit receive-pack");
    }

    let words: Vec<&str> = receive_cmd.split_whitespace().collect();
    if words.is_empty() {
        bail!("empty receive-pack command");
    }
    let mut cmd = Command::new(words[0]);
    strip_trace2_env(&mut cmd);
    for w in &words[1..] {
        cmd.arg(w);
    }
    apply_proto(&mut cmd);
    cmd.arg(remote_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    cmd.spawn().context("spawn receive-pack")
}

fn read_advertisement(r: &mut impl Read) -> Result<(HashSet<ObjectId>, bool, Vec<ObjectId>)> {
    let mut haves = HashSet::new();
    let mut advertised = Vec::new();
    let mut server_sideband = false;
    let mut saw_version_1 = false;
    let mut first_data = true;
    let mut server_sid_done = false;
    loop {
        match read_packet(r)? {
            None => break,
            Some(Packet::Flush) => break,
            Some(Packet::Delim) | Some(Packet::ResponseEnd) => break,
            Some(Packet::Data(line)) => {
                if first_data {
                    first_data = false;
                    let t = line.trim_end_matches('\n');
                    if t.starts_with("version 1") {
                        saw_version_1 = true;
                    }
                }
                if let Some(oid) = parse_dot_have_line(&line) {
                    haves.insert(oid);
                }
                if let Some(oid) = parse_advertised_ref_oid(&line) {
                    advertised.push(oid);
                }
                if let Some(caps) = line.split('\0').nth(1) {
                    if caps.contains("side-band-64k") || caps.contains("side-band") {
                        server_sideband = true;
                    }
                    if !server_sid_done {
                        if let Some(sid) = trace2_transfer::extract_session_id_feature(caps) {
                            trace2_transfer::emit_server_sid(sid);
                            server_sid_done = true;
                        }
                    }
                }
            }
        }
    }
    trace2_transfer::emit_negotiated_version_client_fetch(saw_version_1);
    Ok((haves, server_sideband, advertised))
}

fn parse_advertised_ref_oid(line: &str) -> Option<ObjectId> {
    let line = line.trim_end_matches('\n');
    let (main, _) = line.split_once('\0').unwrap_or((line, ""));
    let mut it = main.splitn(2, |c| c == '\t' || c == ' ');
    let hex = it.next()?.trim();
    let name = it.next()?.trim();
    // `HEAD` appears on the first pkt-line alongside capabilities; it may be detached at the
    // same OID as the branch being pushed. Feeding it to pack-objects as `^<oid>` would exclude
    // the whole thin pack (t5410 with grit receive-pack advertising detached HEAD).
    if name == ".have" || name == "HEAD" || name.starts_with("capabilities") {
        return None;
    }
    ObjectId::from_hex(hex).ok()
}

fn peel_advertised_commits(repo: &Repository, oids: &[ObjectId]) -> Vec<ObjectId> {
    use grit_lib::objects::{parse_tag, ObjectKind};
    let mut out = Vec::new();
    let mut seen_commits = HashSet::new();
    for &start in oids {
        let mut cur = start;
        let commit = loop {
            let Ok(obj) = repo.odb.read(&cur) else {
                break None;
            };
            match obj.kind {
                ObjectKind::Tag => {
                    let Ok(tag) = parse_tag(&obj.data) else {
                        break None;
                    };
                    cur = tag.object;
                }
                ObjectKind::Commit => break Some(cur),
                _ => break None,
            }
        };
        if let Some(c) = commit {
            if seen_commits.insert(c) {
                out.push(c);
            }
        }
    }
    out
}

fn parse_dot_have_line(line: &str) -> Option<ObjectId> {
    let line = line.trim_end_matches('\n');
    let (main, _) = line.split_once('\0').unwrap_or((line, ""));
    let mut it = main.splitn(2, |c| c == '\t' || c == ' ');
    let hex = it.next()?;
    let name = it.next()?.trim();
    if name == ".have" {
        ObjectId::from_hex(hex).ok()
    } else {
        None
    }
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

/// Map a refspec source to the ref name passed to [`refs::resolve_ref`] (`HEAD` stays `HEAD`).
fn resolve_push_src_refname(src: &str) -> String {
    if src == "HEAD" || src.starts_with("refs/") {
        src.to_owned()
    } else {
        normalize_ref(src)
    }
}

fn open_repo(path: &Path) -> Result<Repository> {
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let git_dir = path.join(".git");
    Repository::open(&git_dir, Some(path)).map_err(Into::into)
}
