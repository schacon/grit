//! `ext::` remote URLs (Git's `git-remote-ext` / connect helper).
//!
//! See `git/Documentation/git-remote-ext.adoc` and `git/builtin/remote-ext.c`.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use grit_lib::merge_base::is_ancestor;
use grit_lib::objects::ObjectId;
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::unpack_objects::{unpack_objects, UnpackOptions};

use crate::commands::send_pack;
use crate::fetch_transport;
use crate::grit_exe::grit_executable;
use crate::pkt_line;

/// Parsed `ext::<command> <args>...` URL (without the `ext::` prefix).
pub struct RemoteExtSpec {
    pub argv: Vec<String>,
    pub git_repo_path: Option<String>,
    pub git_vhost: Option<String>,
}

fn service_noprefix(service: &str) -> &str {
    service.strip_prefix("git-").unwrap_or(service)
}

/// Length of one `remote-ext` argument starting at `input` (matches `strip_escapes` scan in
/// `git/builtin/remote-ext.c`).
fn remote_ext_arg_byte_len(input: &str) -> Result<usize> {
    let bytes = input.as_bytes();
    let mut rpos = 0usize;
    let mut escape = false;
    while rpos < bytes.len() && (escape || bytes[rpos] != b' ') {
        if escape {
            let c = bytes[rpos] as char;
            match c {
                ' ' | '%' | 's' | 'S' => {}
                'G' | 'V' => {
                    if rpos != 1 {
                        bail!("remote-ext: '%{c}' must be first character of an argument");
                    }
                }
                _ => bail!("remote-ext: bad placeholder '%{c}'"),
            }
            escape = false;
        } else {
            escape = bytes[rpos] == b'%';
        }
        rpos += 1;
    }
    if escape {
        bail!("remote-ext: incomplete placeholder");
    }
    Ok(rpos)
}

/// Split `input` into the first argument and the remainder (skips one inter-arg space).
fn next_remote_ext_arg<'a>(input: &'a str) -> Result<(&'a str, &'a str)> {
    if input.is_empty() {
        return Ok(("", ""));
    }
    let len = remote_ext_arg_byte_len(input)?;
    let tok = &input[..len];
    let rest = input[len..].trim_start_matches(' ');
    Ok((tok, rest))
}

/// Expand placeholders in one remote-ext argument for `service`.
/// `%G` / `%V` arguments are returned as `Err` with the special kind and payload (Git does not pass
/// these argv entries to the child).
fn expand_one_remote_ext_arg(token: &str, service: &str) -> Result<Result<String, (char, String)>> {
    let service_np = service_noprefix(service);
    let arg_len = remote_ext_arg_byte_len(token)?;
    if arg_len != token.len() {
        bail!("remote-ext: trailing junk after argument");
    }
    let bytes = token.as_bytes();
    let special = if bytes.len() >= 2 && bytes[0] == b'%' {
        let c = bytes[1] as char;
        if c == 'G' || c == 'V' {
            Some(c)
        } else {
            None
        }
    } else {
        None
    };

    let skip = if special.is_some() { 2 } else { 0 };
    let mut out = String::new();
    let mut i = skip;
    let mut escape = false;
    while i < bytes.len() {
        if escape {
            let c = bytes[i] as char;
            match c {
                ' ' | '%' => out.push(c),
                's' => out.push_str(service_np),
                'S' => out.push_str(service),
                _ => bail!("remote-ext: bad placeholder '%{c}' in expansion"),
            }
            escape = false;
        } else if bytes[i] == b'%' {
            escape = true;
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    if escape {
        bail!("remote-ext: incomplete placeholder");
    }

    if let Some(sp) = special {
        return Ok(Err((sp, out)));
    }
    Ok(Ok(out))
}

/// Parse `ext::...` into argv and optional git:// request fields (`%G` / `%V`).
///
/// `parse_service` controls `%s` / `%S` placeholder expansion (e.g. `git-upload-pack` vs
/// `git-receive-pack`).
pub fn parse_remote_ext_url_with_service(url: &str, parse_service: &str) -> Result<RemoteExtSpec> {
    let rest = url
        .strip_prefix("ext::")
        .with_context(|| format!("not an ext:: URL: {url}"))?;
    if rest.is_empty() {
        bail!("ext:: URL is empty");
    }

    let mut argv: Vec<String> = Vec::new();
    let mut git_repo: Option<String> = None;
    let mut git_vhost: Option<String> = None;

    let mut cursor = rest;
    while !cursor.is_empty() {
        let (tok, next) = next_remote_ext_arg(cursor)?;
        cursor = next;
        if tok.is_empty() {
            break;
        }
        match expand_one_remote_ext_arg(tok, parse_service)? {
            Ok(arg) => argv.push(arg),
            Err(('G', payload)) => git_repo = Some(payload),
            Err(('V', payload)) => git_vhost = Some(payload),
            Err((c, _)) => bail!("remote-ext: unknown special argument '%{c}'"),
        }
    }

    if argv.is_empty() {
        bail!("ext:: URL: no command");
    }
    Ok(RemoteExtSpec {
        argv,
        git_repo_path: git_repo,
        git_vhost,
    })
}

/// Parse `ext::...` for fetch/clone (`%s` → `upload-pack`).
pub fn parse_remote_ext_url(url: &str) -> Result<RemoteExtSpec> {
    parse_remote_ext_url_with_service(url, "git-upload-pack")
}

fn argv0_basename(argv0: &str) -> Option<&str> {
    Path::new(argv0).file_name()?.to_str()
}

fn is_grit_subcommand_child(prog: &Path, child_args: &[String], sub: &str) -> bool {
    let grit = grit_executable();
    if prog == grit && child_args.len() >= 2 && child_args[0] == sub {
        return true;
    }
    let Some(b0) = prog.to_str().and_then(|s| argv0_basename(s)) else {
        return false;
    };
    b0 == "git" && child_args.len() >= 2 && child_args[0] == sub
}

/// When the URL is `sh -c '…git-upload-pack <args…>…'` (t5802), run `grit upload-pack <args…>` as
/// the child process instead of nesting shells. The inner script may prefix the command (e.g.
/// `echo … && git-upload-pack …`); match the upload-pack argv segment anywhere in the string.
pub(crate) fn resolve_ext_child_argv(
    parsed: &RemoteExtSpec,
) -> (PathBuf, Vec<String>, Option<String>) {
    if parsed.argv.len() == 3
        && argv0_basename(&parsed.argv[0]).is_some_and(|b| b == "sh" || b == "dash")
        && parsed.argv[1] == "-c"
    {
        let inner = parsed.argv[2].trim();
        if let Some(rest) = extract_git_upload_pack_args(inner) {
            let grit = grit_executable();
            let mut args = vec!["upload-pack".to_owned()];
            args.extend(rest.split_whitespace().map(|s| s.to_owned()));
            return (grit, args, None);
        }
    }
    let grit = grit_executable();
    if parsed.argv.len() >= 2 {
        let b0 = argv0_basename(&parsed.argv[0]).unwrap_or(parsed.argv[0].as_str());
        if b0 == "git" || b0 == "grit" {
            let mut i = 1usize;
            let mut argv_ns: Option<String> = None;
            while i < parsed.argv.len() {
                let a = parsed.argv[i].as_str();
                if a == "--namespace" {
                    if i + 1 < parsed.argv.len() {
                        argv_ns = Some(parsed.argv[i + 1].clone());
                    }
                    i = i.saturating_add(2);
                    continue;
                }
                if let Some(rest) = a.strip_prefix("--namespace=") {
                    if !rest.is_empty() {
                        argv_ns = Some(rest.to_owned());
                    }
                    i += 1;
                    continue;
                }
                break;
            }
            if i < parsed.argv.len() && parsed.argv[i] == "upload-pack" {
                let mut args = vec!["upload-pack".to_owned()];
                args.extend(parsed.argv[i + 1..].iter().cloned());
                return (grit, args, argv_ns);
            }
            if i < parsed.argv.len() && parsed.argv[i] == "receive-pack" {
                let mut args = vec!["receive-pack".to_owned()];
                args.extend(parsed.argv[i + 1..].iter().cloned());
                return (grit, args, argv_ns);
            }
        }
    }
    (
        PathBuf::from(&parsed.argv[0]),
        parsed.argv[1..].to_vec(),
        None,
    )
}

/// Returns upload-pack arguments (after the service name) when `script` contains
/// `git-upload-pack` or `git upload-pack` as a command word.
fn extract_git_upload_pack_args(script: &str) -> Option<&str> {
    let bytes = script.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let is_word_start = i == 0 || bytes[i - 1].is_ascii_whitespace();
        if !is_word_start {
            i += 1;
            continue;
        }
        let rest = &script[i..];
        let rest = if let Some(r) = rest.strip_prefix("git-upload-pack") {
            r
        } else if let Some(r) = rest.strip_prefix("git upload-pack") {
            r
        } else {
            i += 1;
            continue;
        };
        let rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let first = rest.as_bytes()[0];
        if first == b'&' || first == b'|' || first == b';' || first == b'>' || first == b'<' {
            i += 1;
            continue;
        }
        return Some(rest.trim());
    }
    None
}

/// When an `ext::` URL runs `grit upload-pack <dir>` or `grit receive-pack <dir>` (or equivalent),
/// return the resolved on-disk git directory so fetch/push can open the real remote ODB.
pub fn try_resolve_ext_upload_pack_git_dir(ext_url: &str) -> Option<PathBuf> {
    for svc in ["git-upload-pack", "git-receive-pack"] {
        if let Some(p) = try_resolve_ext_service_git_dir(ext_url, svc) {
            return Some(p);
        }
    }
    None
}

fn try_resolve_ext_service_git_dir(ext_url: &str, parse_service: &str) -> Option<PathBuf> {
    let spec = parse_remote_ext_url_with_service(ext_url, parse_service).ok()?;
    let (prog, child_args, _argv_ns) = resolve_ext_child_argv(&spec);
    let want_cmd = if parse_service.contains("upload") {
        "upload-pack"
    } else {
        "receive-pack"
    };
    if !is_grit_subcommand_child(&prog, &child_args, want_cmd) || child_args.len() != 2 {
        return None;
    }
    let mut repo = PathBuf::from(&child_args[1]);
    if repo.as_os_str() == "." {
        repo = std::env::current_dir()
            .and_then(|p| p.canonicalize())
            .unwrap_or(repo);
    } else if repo.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            repo = cwd.join(&repo);
        }
    }
    let git_dir = if repo.file_name().is_some_and(|n| n == ".git") {
        repo.clone()
    } else if repo.join(".git").is_dir() {
        repo.join(".git")
    } else {
        repo.clone()
    };
    fs::canonicalize(&git_dir).ok()
}

fn write_git_daemon_request(
    w: &mut impl Write,
    service: &str,
    repo_path: &str,
    vhost: Option<&str>,
) -> Result<()> {
    let mut inner: Vec<u8> = Vec::new();
    inner.extend_from_slice(service.as_bytes());
    inner.push(b' ');
    inner.extend_from_slice(repo_path.as_bytes());
    inner.push(0);
    if let Some(h) = vhost {
        inner.extend_from_slice(b"host=");
        inner.extend_from_slice(h.as_bytes());
        inner.push(0);
    }
    pkt_line::write_packet_raw(w, &inner).context("write ext:: git:// request")?;
    w.flush().ok();
    Ok(())
}

/// Fetch via `ext::` helper: spawn the user's command with stdin/stdout as the git wire, then run
/// the same upload-pack negotiation as local fetch.
///
/// `service` is typically `git-upload-pack` for fetch/clone.
pub fn fetch_via_ext_skipping(
    local_git_dir: &Path,
    ext_url: &str,
    service: &str,
    refspecs: &[String],
    mirror_preserve_storage_paths: bool,
    compute_wants: impl FnOnce(&[(String, ObjectId)]) -> anyhow::Result<Vec<ObjectId>>,
) -> Result<(
    Vec<(String, ObjectId)>,
    Vec<(String, ObjectId)>,
    Vec<(String, ObjectId)>,
    Option<String>,
    Option<ObjectId>,
)> {
    let spec = parse_remote_ext_url(ext_url)?;
    let (prog, child_args, argv_ns) = resolve_ext_child_argv(&spec);
    let mut child = if is_grit_subcommand_child(&prog, &child_args, "upload-pack") {
        let mut repo = PathBuf::from(&child_args[1]);
        if repo.as_os_str() == "." {
            repo = std::env::current_dir()
                .and_then(|p| p.canonicalize())
                .unwrap_or(repo);
        } else if repo.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                repo = cwd.join(&repo);
            }
        }
        fetch_transport::spawn_upload_pack_with_proto(None, &repo, 0, argv_ns.as_deref())
            .with_context(|| {
                format!(
                    "failed to spawn upload-pack for ext:: (repo {})",
                    repo.display()
                )
            })?
    } else {
        let mut cmd = Command::new(&prog);
        cmd.args(&child_args)
            .env("GIT_EXT_SERVICE", service)
            .env("GIT_EXT_SERVICE_NOPREFIX", service_noprefix(service))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd.env_remove("GIT_PROTOCOL");
        cmd.spawn().with_context(|| {
            format!(
                "failed to spawn ext:: command {} {:?}",
                prog.display(),
                child_args
            )
        })?
    };

    let mut stdin = child.stdin.take().context("ext:: stdin")?;
    let mut stdout = child.stdout.take().context("ext:: stdout")?;

    if let Some(ref repo_path) = spec.git_repo_path {
        write_git_daemon_request(&mut stdin, service, repo_path, spec.git_vhost.as_deref())?;
    }

    let (advertised, head_symref) = fetch_transport::read_advertisement(&mut stdout)?;
    let wants = compute_wants(&advertised)?;
    if wants.is_empty() {
        if refspecs.is_empty() && advertised.is_empty() {
            drop(stdin);
            let _ = fetch_transport::drain_child_stdout_to_eof(&mut stdout);
            let status = child.wait()?;
            if !status.success() {
                bail!("ext:: helper exited with {}", status);
            }
            return Ok((Vec::new(), Vec::new(), Vec::new(), head_symref, None));
        }
        if refspecs.is_empty() {
            drop(stdin);
            let _ = fetch_transport::drain_child_stdout_to_eof(&mut stdout);
            let status = child.wait()?;
            if !status.success() {
                bail!("ext:: helper exited with {}", status);
            }
            let remote_heads: Vec<_> = advertised
                .iter()
                .filter(|(n, _)| n.starts_with("refs/heads/"))
                .cloned()
                .collect();
            let remote_tags: Vec<_> = advertised
                .iter()
                .filter(|(n, _)| n.starts_with("refs/tags/"))
                .cloned()
                .collect();
            let extra_ns: Vec<_> = if mirror_preserve_storage_paths {
                advertised
                    .iter()
                    .filter(|(n, _)| n.starts_with("refs/namespaces/"))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            let head_advertised_oid = advertised
                .iter()
                .find(|(n, _)| n == "HEAD")
                .map(|(_, o)| *o);
            return Ok((
                remote_heads,
                remote_tags,
                extra_ns,
                head_symref,
                head_advertised_oid,
            ));
        }
        bail!("nothing to fetch (advertised {} ref(s))", advertised.len());
    }

    let remote_heads: Vec<_> = advertised
        .iter()
        .filter(|(n, _)| n.starts_with("refs/heads/"))
        .cloned()
        .collect();
    let remote_tags: Vec<_> = advertised
        .iter()
        .filter(|(n, _)| n.starts_with("refs/tags/"))
        .cloned()
        .collect();
    let extra_ns: Vec<_> = if mirror_preserve_storage_paths {
        advertised
            .iter()
            .filter(|(n, _)| n.starts_with("refs/namespaces/"))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let head_advertised_oid = advertised
        .iter()
        .find(|(n, _)| n == "HEAD")
        .map(|(_, o)| *o);

    let pack_buf = fetch_transport::fetch_upload_pack_negotiate_pack_bytes_with_streams(
        local_git_dir,
        &advertised,
        &mut stdin,
        &mut stdout,
        &wants,
    )?;

    let status = child.wait()?;
    if !status.success() {
        bail!("ext:: helper exited with {}", status);
    }

    if pack_buf.len() < 12 || &pack_buf[0..4] != b"PACK" {
        bail!("did not receive a pack file from ext:: transport");
    }

    let odb = Odb::new(&local_git_dir.join("objects"));
    if pack_buf.len() > 12 {
        unpack_objects(&mut pack_buf.as_slice(), &odb, &UnpackOptions::default())?;
    }

    Ok((
        remote_heads,
        remote_tags,
        extra_ns,
        head_symref,
        head_advertised_oid,
    ))
}

/// Run `git-upload-pack` over an `ext::` URL and return the v0 ref advertisement (no fetch).
pub fn read_ext_upload_pack_advertisement(
    ext_url: &str,
) -> Result<(Vec<(String, ObjectId)>, Option<String>)> {
    let spec = parse_remote_ext_url(ext_url)?;
    let (prog, child_args, argv_ns) = resolve_ext_child_argv(&spec);
    let service = "git-upload-pack";
    let mut child = if is_grit_subcommand_child(&prog, &child_args, "upload-pack") {
        let mut repo = PathBuf::from(&child_args[1]);
        if repo.as_os_str() == "." {
            repo = std::env::current_dir()
                .and_then(|p| p.canonicalize())
                .unwrap_or(repo);
        } else if repo.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                repo = cwd.join(&repo);
            }
        }
        fetch_transport::spawn_upload_pack_with_proto(None, &repo, 0, argv_ns.as_deref())
            .with_context(|| {
                format!(
                    "failed to spawn upload-pack for ext:: ls-remote (repo {})",
                    repo.display()
                )
            })?
    } else {
        let mut cmd = Command::new(&prog);
        cmd.args(&child_args)
            .env("GIT_EXT_SERVICE", service)
            .env("GIT_EXT_SERVICE_NOPREFIX", service_noprefix(service))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd.env_remove("GIT_PROTOCOL");
        cmd.spawn().with_context(|| {
            format!(
                "failed to spawn ext:: command {} {:?}",
                prog.display(),
                child_args
            )
        })?
    };

    let mut stdin = child.stdin.take().context("ext:: ls-remote stdin")?;
    let mut stdout = child.stdout.take().context("ext:: ls-remote stdout")?;

    if let Some(ref repo_path) = spec.git_repo_path {
        write_git_daemon_request(&mut stdin, service, repo_path, spec.git_vhost.as_deref())?;
    }

    let (advertised, head_symref) = fetch_transport::read_advertisement(&mut stdout)?;
    drop(stdin);
    let _ = fetch_transport::drain_child_stdout_to_eof(&mut stdout);
    let status = child.wait()?;
    if !status.success() {
        bail!("ext:: upload-pack exited with {}", status);
    }
    Ok((advertised, head_symref))
}

/// One ref update for [`push_via_ext`] (`new_oid` = `None` means delete on the remote).
pub struct ExtPushUpdate<'a> {
    pub remote_ref: &'a str,
    pub old_oid: Option<ObjectId>,
    pub new_oid: Option<ObjectId>,
}

/// Push via `ext::` URL: spawn the helper with `git-receive-pack`, send ref commands + pack.
pub fn push_via_ext(
    repo: &Repository,
    ext_url: &str,
    updates: &[ExtPushUpdate<'_>],
    force: bool,
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    for u in updates {
        if let (Some(old), Some(new)) = (u.old_oid, u.new_oid) {
            if old != new
                && !force
                && !u.remote_ref.starts_with("refs/tags/")
                && !is_ancestor(repo, old, new)?
            {
                bail!(
                    "non-fast-forward update to '{}' rejected (use --force to override)",
                    u.remote_ref
                );
            }
        }
    }

    let spec = parse_remote_ext_url_with_service(ext_url, "git-receive-pack")?;
    let (prog, child_args, argv_ns) = resolve_ext_child_argv(&spec);
    let service = "git-receive-pack";
    let mut child = if is_grit_subcommand_child(&prog, &child_args, "receive-pack") {
        let mut rpath = PathBuf::from(&child_args[1]);
        if rpath.as_os_str() == "." {
            rpath = std::env::current_dir()
                .and_then(|p| p.canonicalize())
                .unwrap_or(rpath);
        } else if rpath.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                rpath = cwd.join(&rpath);
            }
        }
        fetch_transport::spawn_receive_pack_with_proto(None, &rpath, 0, argv_ns.as_deref())
            .with_context(|| {
                format!(
                    "failed to spawn receive-pack for ext:: (repo {})",
                    rpath.display()
                )
            })?
    } else {
        let mut cmd = Command::new(&prog);
        cmd.args(&child_args)
            .env("GIT_EXT_SERVICE", service)
            .env("GIT_EXT_SERVICE_NOPREFIX", service_noprefix(service))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd.env_remove("GIT_PROTOCOL");
        cmd.spawn().with_context(|| {
            format!(
                "failed to spawn ext:: command {} {:?}",
                prog.display(),
                child_args
            )
        })?
    };

    let mut stdin = child.stdin.take().context("ext:: push stdin")?;
    let mut stdout = child.stdout.take().context("ext:: push stdout")?;

    if let Some(ref repo_path) = spec.git_repo_path {
        write_git_daemon_request(&mut stdin, service, repo_path, spec.git_vhost.as_deref())?;
    }

    let (extra_have, _server_sideband, advertised_oids) =
        send_pack::read_advertisement(&mut stdout)?;

    let caps = "report-status report-status-v2 quiet object-format=sha1";
    let mut first_cmd = true;
    let zero = "0".repeat(40);
    for u in updates {
        let old_hex = u
            .old_oid
            .map(|o| o.to_hex())
            .unwrap_or_else(|| zero.clone());
        let new_hex = u
            .new_oid
            .map(|o| o.to_hex())
            .unwrap_or_else(|| zero.clone());
        let pkt = if first_cmd {
            first_cmd = false;
            format!("{old_hex} {new_hex} {}\0{caps}\n", u.remote_ref)
        } else {
            format!("{old_hex} {new_hex} {}\n", u.remote_ref)
        };
        send_pack::write_pkt_line(&mut stdin, pkt.as_bytes())?;
    }
    pkt_line::write_flush(&mut stdin)?;
    stdin.flush().ok();

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
        .context("spawn pack-objects for ext:: push")?;
    {
        let mut pack_stdin = pack_child.stdin.take().context("pack-objects stdin")?;
        let mut fed: HashSet<ObjectId> = HashSet::new();
        let new_tips: HashSet<ObjectId> = updates.iter().filter_map(|u| u.new_oid).collect();
        for oid in send_pack::peel_advertised_commits(repo, &advertised_oids) {
            if new_tips.contains(&oid) {
                continue;
            }
            if fed.insert(oid) {
                writeln!(pack_stdin, "^{}", oid.to_hex())?;
            }
        }
        for h in &extra_have {
            if fed.insert(*h) {
                writeln!(pack_stdin, "^{}", h.to_hex())?;
            }
        }
        for u in updates {
            if let Some(old) = u.old_oid {
                if fed.insert(old) {
                    writeln!(pack_stdin, "^{}", old.to_hex())?;
                }
            }
            if let Some(new) = u.new_oid {
                writeln!(pack_stdin, "{}", new.to_hex())?;
            }
        }
        pack_stdin.flush().context("flush pack-objects stdin")?;
    }

    let pack_output = pack_child
        .wait_with_output()
        .context("wait for pack-objects (ext:: push)")?;
    if !pack_output.status.success() {
        bail!("pack-objects failed with status {}", pack_output.status);
    }
    stdin.write_all(&pack_output.stdout)?;
    stdin.flush()?;

    drop(stdin);
    let mut output = Vec::new();
    stdout.read_to_end(&mut output)?;
    let status = child.wait()?;
    let out_str = String::from_utf8_lossy(&output);
    if out_str.contains("ng ") {
        bail!("remote rejected ref(s) during ext:: push");
    }
    if !status.success() {
        bail!("receive-pack failed (ext:: push)");
    }

    Ok(())
}
