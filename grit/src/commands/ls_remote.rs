//! `grit ls-remote` — list references from a local repository path.
//!
//! Only the **local path** transport is supported.  Network URLs are not
//! handled in v1.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::ls_remote::{ls_remote, Options, RefEntry};
use grit_lib::objects::ObjectId;
use grit_lib::repo::Repository;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::pkt_line;

/// Arguments for `grit ls-remote`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show only branches (`refs/heads/`).
    #[arg(long = "heads")]
    pub heads: bool,

    /// Show only tags (`refs/tags/`).
    #[arg(long = "tags")]
    pub tags: bool,

    /// Exclude pseudo-refs (HEAD) and peeled tag `^{}` entries.
    #[arg(long = "refs")]
    pub refs_only: bool,

    /// Show the symbolic ref that HEAD points to.
    #[arg(long = "symref")]
    pub symref: bool,

    /// Path to git-upload-pack on the remote host.
    #[arg(long = "upload-pack", alias = "exec")]
    pub upload_pack: Option<String>,

    /// Quiet: suppress output, only set the exit status.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Path to the local repository (bare or non-bare).
    #[arg(value_name = "REPOSITORY")]
    pub repository: PathBuf,

    /// Optional ref patterns; only matching refs are printed.
    #[arg(value_name = "PATTERN", num_args = 0..)]
    pub patterns: Vec<String>,
}

/// Run `grit ls-remote`.
///
/// Opens the repository at `args.repository`, enumerates its references
/// according to the supplied flags, and prints them to stdout as
/// `<oid>\t<refname>` lines, with HEAD first.
///
/// Exits with status 1 when no refs match (same behaviour as `git ls-remote`).
pub fn run(args: Args) -> Result<()> {
    // If the repository argument is a configured remote name, resolve its URL
    let effective_path = resolve_remote_or_path(&args.repository);

    // Check if the path is a bundle file
    if is_bundle_file(&effective_path) {
        return run_bundle_ls_remote(&effective_path, &args);
    }

    let repo_path_str = effective_path.to_string_lossy();
    if repo_path_str.starts_with("ext::") {
        if args
            .upload_pack
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            bail!("ls-remote: --upload-pack is not supported with ext:: URLs");
        }
        return run_ls_remote_ext(repo_path_str.as_ref(), &args);
    }
    if repo_path_str.starts_with("git://") && crate::file_upload_pack_v2::client_wants_protocol_v2()
    {
        return crate::file_upload_pack_v2::ls_remote_git_v2(repo_path_str.as_ref(), &args);
    }
    let is_file_url = repo_path_str.starts_with("file://");
    if is_file_url && crate::file_upload_pack_v2::client_wants_protocol_v2() {
        let path = PathBuf::from(
            repo_path_str
                .strip_prefix("file://")
                .unwrap_or(repo_path_str.as_ref()),
        );
        let upload = args.upload_pack.as_deref().filter(|s| !s.is_empty());
        return crate::file_upload_pack_v2::ls_remote_file_v2(&path, upload, &args);
    }

    if let Some(upload_pack) = args.upload_pack.as_deref() {
        if !upload_pack.is_empty() {
            return run_ls_remote_via_upload_pack(&effective_path, upload_pack, &args);
        }
    }

    let repo = open_local_repo(&effective_path)?;

    let opts = Options {
        heads: args.heads,
        tags: args.tags,
        refs_only: args.refs_only,
        symref: args.symref,
        patterns: args.patterns,
    };

    let refs_git_dir = common_git_dir_or_self(&repo.git_dir);
    let entries = ls_remote(&refs_git_dir, &repo.odb, &opts)?;

    if entries.is_empty() {
        // git ls-remote exits 0 even when no refs match patterns
        return Ok(());
    }

    if args.quiet {
        return Ok(());
    }

    for entry in &entries {
        if let Some(target) = &entry.symref_target {
            println!("ref: {target}\t{}", entry.name);
        }
        println!("{}\t{}", entry.oid, entry.name);
    }

    Ok(())
}

fn run_ls_remote_ext(url: &str, args: &Args) -> Result<()> {
    let git_dir = Repository::discover(None).ok().map(|r| r.git_dir);
    crate::protocol::check_protocol_allowed("ext", git_dir.as_deref())?;
    let (advertised, head_symref) = crate::ext_transport::read_ext_upload_pack_advertisement(url)?;
    let mut entries: Vec<RefEntry> = Vec::new();
    for (refname, oid) in advertised {
        if refname == "HEAD" && !args.symref {
            if args.patterns.is_empty() {
                continue;
            }
            if !grit_lib::ls_remote::ref_matches_ls_remote_patterns("HEAD", &args.patterns) {
                continue;
            }
        }
        let raw = if refname == "HEAD" && args.symref {
            if let Some(sr) = head_symref.as_deref() {
                format!("{}\t{}\0symref=HEAD:{}\n", oid.to_hex(), refname, sr)
            } else {
                format!("{}\t{}\n", oid.to_hex(), refname)
            }
        } else {
            format!("{}\t{}\n", oid.to_hex(), refname)
        };
        entries.extend(parse_v0_ref_advertisement_line(&raw, args)?);
    }
    entries.sort_by(|a, b| {
        let rank = |n: &str| if n == "HEAD" { 0 } else { 1 };
        match rank(&a.name).cmp(&rank(&b.name)) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            o => o,
        }
    });
    if entries.is_empty() {
        return Ok(());
    }
    if args.quiet {
        return Ok(());
    }
    for entry in &entries {
        if let Some(target) = &entry.symref_target {
            println!("ref: {target}\t{}", entry.name);
        }
        println!("{}\t{}", entry.oid, entry.name);
    }
    Ok(())
}

fn run_ls_remote_via_upload_pack(repo_path: &Path, upload_pack: &str, args: &Args) -> Result<()> {
    let repo = open_local_repo(repo_path)?;
    let repo_dir = repo
        .work_tree
        .clone()
        .unwrap_or_else(|| repo.git_dir.clone());

    let default_hash = std::env::var("GIT_DEFAULT_HASH").unwrap_or_else(|_| "sha1".to_owned());

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(upload_pack)
        .arg(repo_dir.to_string_lossy().as_ref())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run upload-pack via sh -c '{upload_pack}'"))?;

    let mut stdin = child.stdin.take().context("upload-pack stdin")?;
    let mut stdout = child.stdout.take().context("upload-pack stdout")?;
    let mut stderr = child.stderr.take().context("upload-pack stderr")?;

    let stderr_buf = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stderr.read_to_end(&mut v);
        v
    });

    let entries = read_ls_remote_upload_pack_output(&mut stdin, &mut stdout, &default_hash, args)?;
    drop(stdin);

    let status = child.wait()?;
    let err_bytes = stderr_buf.join().unwrap_or_default();
    if !err_bytes.is_empty() {
        let _ = std::io::stderr().write_all(&err_bytes);
    }
    if !status.success() {
        bail!(
            "upload-pack exited with status {}",
            status.code().unwrap_or(-1)
        );
    }
    if entries.is_empty() {
        return Ok(());
    }
    if args.quiet {
        return Ok(());
    }
    for entry in &entries {
        if let Some(target) = &entry.symref_target {
            println!("ref: {target}\t{}", entry.name);
        }
        println!("{}\t{}", entry.oid, entry.name);
    }
    Ok(())
}

fn read_ls_remote_upload_pack_output(
    stdin: &mut impl Write,
    stdout: &mut impl Read,
    object_format: &str,
    args: &Args,
) -> Result<Vec<RefEntry>> {
    let first = match pkt_line::read_packet(stdout).context("read upload-pack first packet")? {
        None => bail!("unexpected EOF from upload-pack"),
        Some(p) => p,
    };

    match first {
        pkt_line::Packet::Data(ref line) if line == "version 2" => {
            skip_rest_of_v2_capability_advertisement(stdout)?;
            write_v2_ls_refs_request(stdin, object_format, args)?;
            stdin.flush()?;
            let mut buf = Vec::new();
            stdout
                .take(512 * 1024)
                .read_to_end(&mut buf)
                .context("read v2 ls-refs response")?;
            parse_v2_ls_refs_output(&buf, args)
        }
        pkt_line::Packet::Data(line) => {
            let mut entries = parse_v0_ref_advertisement_line(&line, args)?;
            loop {
                match pkt_line::read_packet(stdout)? {
                    None => break,
                    Some(pkt_line::Packet::Flush) => break,
                    Some(pkt_line::Packet::Data(l)) => {
                        entries.extend(parse_v0_ref_advertisement_line(&l, args)?);
                    }
                    Some(other) => bail!("unexpected packet in v0 ref advertisement: {other:?}"),
                }
            }
            Ok(entries)
        }
        other => bail!("unexpected first packet from upload-pack: {other:?}"),
    }
}

/// Consume pkt-lines after the initial `version 2` line until `0000` flush.
fn skip_rest_of_v2_capability_advertisement(r: &mut impl Read) -> Result<()> {
    loop {
        match pkt_line::read_packet(r).context("read v2 capability packet")? {
            None => bail!("unexpected EOF in v2 capability advertisement"),
            Some(pkt_line::Packet::Flush) => return Ok(()),
            Some(pkt_line::Packet::Data(_)) => {}
            Some(other) => bail!("unexpected packet in v2 capability advertisement: {other:?}"),
        }
    }
}

fn symref_head_target_from_caps(caps: &str) -> Option<String> {
    for word in caps.split_whitespace() {
        if let Some(t) = word.strip_prefix("symref=HEAD:") {
            return Some(t.to_owned());
        }
    }
    None
}

/// Parse one v0/v1 ref advertisement pkt-line (`<oid>\t<ref>[\0<capabilities>]`).
fn parse_v0_ref_advertisement_line(raw: &str, args: &Args) -> Result<Vec<RefEntry>> {
    let (payload, caps_opt) = match raw.split_once('\0') {
        Some((p, c)) => (p, Some(c)),
        None => (raw, None),
    };
    let (oid_hex, refname) = payload
        .split_once('\t')
        .or_else(|| payload.split_once(' '))
        .ok_or_else(|| anyhow::anyhow!("malformed v0 ref advertisement: {raw}"))?;
    let refname = refname.split('\0').next().unwrap_or(refname).trim();
    let oid = ObjectId::from_hex(oid_hex.trim())
        .with_context(|| format!("bad oid in v0 ref advertisement: {oid_hex}"))?;

    if args.heads && refname != "HEAD" && !refname.starts_with("refs/heads/") {
        return Ok(vec![]);
    }
    if args.tags && !refname.starts_with("refs/tags/") {
        return Ok(vec![]);
    }
    if args.refs_only && (refname == "HEAD" || refname.ends_with("^{}")) {
        return Ok(vec![]);
    }
    if !grit_lib::ls_remote::ref_matches_ls_remote_patterns(refname, &args.patterns) {
        return Ok(vec![]);
    }

    let symref_target = if args.symref && refname == "HEAD" {
        caps_opt.and_then(symref_head_target_from_caps)
    } else {
        None
    };

    Ok(vec![RefEntry {
        name: refname.to_owned(),
        oid,
        symref_target,
    }])
}

fn write_v2_ls_refs_request(w: &mut impl Write, object_format: &str, args: &Args) -> Result<()> {
    pkt_line::write_line(w, "command=ls-refs")?;
    pkt_line::write_line(w, &format!("object-format={object_format}"))?;
    pkt_line::write_delim(w)?;
    if args.symref {
        pkt_line::write_line(w, "symrefs")?;
    }
    if !args.refs_only {
        pkt_line::write_line(w, "peel")?;
    }
    if args.heads {
        pkt_line::write_line(w, "ref-prefix refs/heads/")?;
    }
    if args.tags {
        pkt_line::write_line(w, "ref-prefix refs/tags/")?;
    }
    for p in &args.patterns {
        pkt_line::write_line(w, &format!("ref-prefix {p}"))?;
    }
    pkt_line::write_flush(w)?;
    Ok(())
}

pub(crate) fn parse_v2_ls_refs_output(data: &[u8], args: &Args) -> Result<Vec<RefEntry>> {
    let mut cursor = Cursor::new(data);
    let mut entries: Vec<RefEntry> = Vec::new();

    loop {
        let pkt = match pkt_line::read_packet(&mut cursor)? {
            None => break,
            Some(pkt_line::Packet::Flush) => break,
            Some(pkt_line::Packet::Data(line)) => line,
            Some(other) => bail!("unexpected pkt-line in ls-refs response: {other:?}"),
        };

        let (name, oid, peeled, symref_target) = parse_ls_refs_v2_line(&pkt)?;
        if !grit_lib::ls_remote::ref_matches_ls_remote_patterns(&name, &args.patterns) {
            continue;
        }

        entries.push(RefEntry {
            name: name.clone(),
            oid,
            symref_target: symref_target.clone(),
        });

        if let Some(poid) = peeled {
            if !args.refs_only {
                entries.push(RefEntry {
                    name: format!("{name}^{{}}"),
                    oid: poid,
                    symref_target: None,
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        let rank = |n: &str| if n == "HEAD" { 0 } else { 1 };
        match rank(&a.name).cmp(&rank(&b.name)) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            o => o,
        }
    });

    Ok(entries)
}

/// Parse one `ls-refs` v2 data line: `<oid> <ref> [peeled:<hex>] [symref-target:<path>]`.
pub(crate) fn parse_ls_refs_v2_line(
    line: &str,
) -> Result<(String, ObjectId, Option<ObjectId>, Option<String>)> {
    const PEEL: &str = " peeled:";
    const SYM: &str = " symref-target:";

    let (oid_hex, after_oid) = line
        .split_once(' ')
        .ok_or_else(|| anyhow::anyhow!("bad ls-refs line: {line}"))?;
    let oid =
        ObjectId::from_hex(oid_hex).with_context(|| format!("bad oid in ls-refs: {oid_hex}"))?;

    let mut peeled = None;
    let mut symref_target = None;

    if let Some(pos) = after_oid.find(PEEL) {
        let name = after_oid[..pos].to_owned();
        let tail = &after_oid[pos + PEEL.len()..];
        let hex_end = tail.find(' ').unwrap_or(tail.len());
        let ph = &tail[..hex_end];
        peeled = Some(ObjectId::from_hex(ph).with_context(|| format!("bad peeled oid: {ph}"))?);
        let rest = tail[hex_end..].trim_start();
        if let Some(s) = rest.strip_prefix("symref-target:") {
            symref_target = Some(s.to_owned());
        }
        return Ok((name, oid, peeled, symref_target));
    }

    if let Some(pos) = after_oid.find(SYM) {
        let name = after_oid[..pos].to_owned();
        let target = after_oid[pos + SYM.len()..].to_owned();
        symref_target = Some(target);
        return Ok((name, oid, peeled, symref_target));
    }

    Ok((after_oid.to_owned(), oid, None, None))
}

/// Open a local repository given a user-supplied path.
///
/// Tries `path` directly (bare repository or an explicit `.git` directory),
/// and falls back to `path/.git` for a standard non-bare working directory.
///
/// # Errors
///
/// Returns an error when neither location looks like a valid git repository.
fn open_local_repo(path: &Path) -> Result<Repository> {
    // Strip file:// URL scheme if present
    let effective_path = {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            path.to_path_buf()
        }
    };
    let path = &effective_path;

    // Bare repository or explicit git-dir directory.
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }

    // Explicit gitfile path (e.g. ".../foo/.git" where ".git" is a file).
    if path.is_file() {
        if let Ok(git_dir) = resolve_gitdir_from_gitfile_path(path) {
            return Ok(Repository::open(&git_dir, path.parent())?);
        }
    }

    // Standard working-tree repository path.
    let dot_git = path.join(".git");
    if dot_git.is_file() {
        if let Ok(git_dir) = resolve_gitdir_from_gitfile_path(&dot_git) {
            return Ok(Repository::open(&git_dir, Some(path))?);
        }
    }
    if dot_git.is_dir() {
        return Ok(Repository::open(&dot_git, Some(path))?);
    }

    Repository::open(&dot_git, Some(path)).with_context(|| {
        format!(
            "'{}' does not appear to be a git repository: {}",
            path.display(),
            "not a git repository (or any of the parent directories)"
        )
    })
}

fn resolve_gitdir_from_gitfile_path(gitfile_path: &Path) -> Result<PathBuf> {
    let content = std::fs::read_to_string(gitfile_path).with_context(|| {
        format!(
            "'{}' does not appear to be a git repository: {}",
            gitfile_path.display(),
            "not a git repository (or any of the parent directories)"
        )
    })?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("gitdir:") {
            let rel = rest.trim();
            if rel.is_empty() {
                break;
            }
            let candidate = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                gitfile_path.parent().unwrap_or(Path::new(".")).join(rel)
            };
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "'{}' does not appear to be a git repository: not a git repository (or any of the parent directories)",
        gitfile_path.display()
    )
}

/// If the repository argument matches a configured remote name, resolve to its URL.
/// Otherwise return the original path.
fn resolve_remote_or_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();

    // Remote config takes precedence over filesystem paths, even when the
    // remote name itself contains slashes.
    if let Ok(repo) = Repository::discover(None) {
        let config_path = repo.git_dir.join("config");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Some(url) = parse_remote_url(&content, &path_str) {
                return PathBuf::from(url);
            }
        }
    }

    path.to_path_buf()
}

/// Check if a path looks like a git bundle file (starts with v2 bundle header).
fn is_bundle_file(path: &Path) -> bool {
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = [0u8; 20];
        if let Ok(n) = std::io::Read::read(&mut f, &mut buf) {
            return buf[..n].starts_with(b"# v2 git bundle");
        }
    }
    false
}

/// Run ls-remote against a bundle file.
fn run_bundle_ls_remote(path: &Path, args: &Args) -> Result<()> {
    let data = std::fs::read(path)
        .with_context(|| format!("could not read bundle '{}'.", path.display()))?;
    let refs = parse_bundle_refs(&data)?;

    if refs.is_empty() {
        return Ok(());
    }

    if args.quiet {
        return Ok(());
    }

    for (refname, oid) in &refs {
        if args.heads && !refname.starts_with("refs/heads/") {
            continue;
        }
        if args.tags && !refname.starts_with("refs/tags/") {
            continue;
        }
        if !args.patterns.is_empty() {
            let matched = args
                .patterns
                .iter()
                .any(|p| refname.contains(p) || refname.ends_with(p));
            if !matched {
                continue;
            }
        }
        println!("{oid}\t{refname}");
    }
    Ok(())
}

/// Parse refs from a v2 bundle header.
fn parse_bundle_refs(data: &[u8]) -> Result<Vec<(String, grit_lib::objects::ObjectId)>> {
    let header_line = b"# v2 git bundle\n";
    if !data.starts_with(header_line) {
        anyhow::bail!("not a v2 git bundle");
    }
    let mut pos = header_line.len();
    let mut refs = Vec::new();
    loop {
        let eol = data[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| pos + i)
            .ok_or_else(|| anyhow::anyhow!("truncated bundle header"))?;
        let line = &data[pos..eol];
        if line.is_empty() {
            break;
        }
        let line_str = std::str::from_utf8(line)?;
        if line_str.starts_with('-') {
            pos = eol + 1;
            continue;
        }
        if let Some((hex, refname)) = line_str.split_once(' ') {
            let oid = grit_lib::objects::ObjectId::from_hex(hex)
                .map_err(|e| anyhow::anyhow!("bad oid in bundle: {e}"))?;
            refs.push((refname.to_string(), oid));
        }
        pos = eol + 1;
    }
    Ok(refs)
}

fn parse_remote_url(config: &str, remote_name: &str) -> Option<String> {
    let section_header = format!("[remote \"{remote_name}\"]");
    let mut in_section = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section_header;
            continue;
        }
        if in_section {
            if let Some(value) = trimmed.strip_prefix("url") {
                let value = value.trim_start();
                if let Some(value) = value.strip_prefix('=') {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

fn common_git_dir_or_self(git_dir: &Path) -> PathBuf {
    let commondir_path = git_dir.join("commondir");
    let Ok(raw) = std::fs::read_to_string(commondir_path) else {
        return git_dir.to_path_buf();
    };
    let rel = raw.trim();
    if rel.is_empty() {
        return git_dir.to_path_buf();
    }
    let candidate = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    candidate.canonicalize().unwrap_or(candidate)
}
