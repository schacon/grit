//! `grit serve-v2` — protocol v2 server.
//!
//! Implements the server side of Git protocol v2 for testing.
//! Supports capability advertisement, ls-refs, fetch, object-info,
//! and bundle-uri commands.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{self, ObjectKind};
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::io::{self, Write};
use std::path::Path;

use crate::pkt_line;

/// Arguments for `grit serve-v2`.
#[derive(Debug, ClapArgs)]
#[command(about = "Protocol v2 server (test helper)")]
pub struct Args {
    /// Advertise capabilities and exit.
    #[arg(long)]
    pub advertise_capabilities: bool,

    /// Stateless RPC mode: read one request from stdin, respond, exit.
    #[arg(long)]
    pub stateless_rpc: bool,
}

/// Known commands and their feature strings.
struct ServerCaps {
    agent: String,
    object_format: String,
    advertise_object_info: bool,
    advertise_bundle_uri: bool,
}

impl ServerCaps {
    fn load(git_dir: &Path) -> Self {
        let version = crate::version_string();
        let agent = format!("agent=git/{version}-");

        let advertise_object_info = read_config_bool(git_dir, "transfer.advertiseObjectInfo");
        let advertise_bundle_uri = read_config_bool(git_dir, "uploadpack.advertiseBundleURIs");

        Self {
            agent,
            object_format: "sha1".to_owned(),
            advertise_object_info,
            advertise_bundle_uri,
        }
    }

    /// Write the capability advertisement to `w` in pkt-line format.
    fn advertise(&self, w: &mut impl Write) -> io::Result<()> {
        pkt_line::write_line(w, "version 2")?;
        pkt_line::write_line(w, &self.agent)?;
        pkt_line::write_line(w, "ls-refs=unborn")?;
        pkt_line::write_line(w, "fetch=shallow wait-for-done")?;
        pkt_line::write_line(w, "server-option")?;
        pkt_line::write_line(w, &format!("object-format={}", self.object_format))?;
        if self.advertise_object_info {
            pkt_line::write_line(w, "object-info")?;
        }
        if self.advertise_bundle_uri {
            pkt_line::write_line(w, "bundle-uri")?;
        }
        pkt_line::write_flush(w)?;
        w.flush()
    }

    fn is_valid_command(&self, cmd: &str) -> bool {
        match cmd {
            "ls-refs" | "fetch" => true,
            "object-info" if self.advertise_object_info => true,
            "bundle-uri" if self.advertise_bundle_uri => true,
            _ => false,
        }
    }

    fn is_valid_capability(&self, cap: &str) -> bool {
        // Capabilities that may appear in a request
        cap.starts_with("agent=")
            || cap.starts_with("object-format=")
            || cap.starts_with("server-option=")
    }
}

pub fn run(args: Args) -> Result<()> {
    let git_dir = discover_git_dir()?;
    let caps = ServerCaps::load(&git_dir);

    if args.advertise_capabilities {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        caps.advertise(&mut out)?;
        return Ok(());
    }

    if args.stateless_rpc {
        return stateless_rpc(&git_dir, &caps);
    }

    // Default: advertise + serve loop
    let stdout = io::stdout();
    let mut out = stdout.lock();
    caps.advertise(&mut out)?;
    drop(out);
    stateless_rpc(&git_dir, &caps)
}

fn stateless_rpc(git_dir: &Path, caps: &ServerCaps) -> Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();

    // Read the request header (up to flush packet).
    let (header_lines, terminator) = pkt_line::read_until_flush_or_delim(&mut input)?;

    // Empty request (or EOF): just exit silently.
    if header_lines.is_empty() {
        if terminator.is_some() {
            // Got a flush with no content — that's fine for stateless-rpc
            return Ok(());
        }
        // EOF with no data
        return Ok(());
    }

    // Parse command and capabilities from header lines.
    let mut command: Option<String> = None;
    let mut client_object_format: Option<String> = None;

    for line in &header_lines {
        if let Some(cmd) = line.strip_prefix("command=") {
            // Reject command=value=extra
            if cmd.contains('=') {
                bail!("invalid command '{cmd}'");
            }
            command = Some(cmd.to_owned());
        } else if let Some(fmt) = line.strip_prefix("object-format=") {
            client_object_format = Some(fmt.to_owned());
        } else if caps.is_valid_capability(line) {
            // valid capability, ignore
        } else {
            bail!("unknown capability '{line}'");
        }
    }

    let cmd = match command {
        Some(c) => c,
        None => bail!("no command requested"),
    };

    // Validate object-format if provided
    if let Some(ref fmt) = client_object_format {
        if fmt != &caps.object_format {
            bail!(
                "mismatched object format: client={fmt}, server={}",
                caps.object_format
            );
        }
    }

    // Check that the command is valid
    if !caps.is_valid_command(&cmd) {
        eprintln!("fatal: invalid command '{cmd}'");
        std::process::exit(128);
    }

    // Read arguments section (after delimiter, up to flush).
    let args = if terminator == Some(pkt_line::Packet::Delim) {
        let (arg_lines, _) = pkt_line::read_until_flush_or_delim(&mut input)?;
        arg_lines
    } else {
        Vec::new()
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match cmd.as_str() {
        "ls-refs" => cmd_ls_refs(git_dir, &args, &mut out)?,
        "fetch" => cmd_fetch(git_dir, &args, &mut out)?,
        "object-info" => cmd_object_info(git_dir, &args, &mut out)?,
        "bundle-uri" => bail!("bundle-uri requires uploadpack.advertiseBundleURIs=true"),
        _ => bail!("invalid command '{cmd}'"),
    }

    out.flush()?;
    Ok(())
}

/// Handle the `ls-refs` command.
fn cmd_ls_refs(git_dir: &Path, args: &[String], out: &mut impl Write) -> Result<()> {
    let mut prefixes: Vec<String> = Vec::new();
    let mut peel = false;
    let mut symrefs = false;

    for arg in args {
        if let Some(prefix) = arg.strip_prefix("ref-prefix ") {
            prefixes.push(prefix.to_owned());
        } else if arg == "peel" {
            peel = true;
        } else if arg == "symrefs" {
            symrefs = true;
        } else if arg == "unborn" {
            // Accepted but we don't send unborn HEAD
        } else {
            bail!("unexpected line: '{arg}'");
        }
    }

    // If too many prefixes (>= 65536), ignore them all (list everything).
    let use_prefixes = prefixes.len() < 65536;

    // Collect all refs.
    let mut entries: Vec<RefInfo> = Vec::new();

    // HEAD
    if let Ok(head_oid) = refs::resolve_ref(git_dir, "HEAD") {
        let symref_target = if symrefs {
            refs::read_symbolic_ref(git_dir, "HEAD").ok().flatten()
        } else {
            None
        };
        entries.push(RefInfo {
            name: "HEAD".to_owned(),
            oid: head_oid,
            symref_target,
            peeled: None,
        });
    }

    // All refs under refs/
    for prefix in &["refs/heads/", "refs/tags/", "refs/remotes/", "refs/notes/"] {
        if let Ok(ref_list) = refs::list_refs(git_dir, prefix) {
            for (name, oid) in ref_list {
                let mut info = RefInfo {
                    name: name.clone(),
                    oid,
                    symref_target: None,
                    peeled: None,
                };
                if symrefs {
                    info.symref_target = refs::read_symbolic_ref(git_dir, &name).ok().flatten();
                }
                if peel && name.starts_with("refs/tags/") {
                    info.peeled = peel_to_commit(git_dir, &oid);
                }
                entries.push(info);
            }
        }
    }

    // Filter by prefix
    if use_prefixes && !prefixes.is_empty() {
        entries.retain(|e| prefixes.iter().any(|p| e.name.starts_with(p)));
    }

    // Sort by ref name
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    // Write output
    for entry in &entries {
        let mut line = format!("{} {}", entry.oid.to_hex(), entry.name);
        if let Some(ref peeled) = entry.peeled {
            line.push_str(&format!(" peeled:{}", peeled.to_hex()));
        }
        if let Some(ref target) = entry.symref_target {
            line.push_str(&format!(" symref-target:{target}"));
        }
        pkt_line::write_line(out, &line)?;
    }
    pkt_line::write_flush(out)?;
    Ok(())
}

struct RefInfo {
    name: String,
    oid: grit_lib::objects::ObjectId,
    symref_target: Option<String>,
    peeled: Option<grit_lib::objects::ObjectId>,
}

/// Peel a tag to its target object. Returns None if not an annotated tag.
fn peel_to_commit(
    git_dir: &Path,
    oid: &grit_lib::objects::ObjectId,
) -> Option<grit_lib::objects::ObjectId> {
    let repo = Repository::open(git_dir, None).ok()?;
    let obj = repo.odb.read(oid).ok()?;
    if obj.kind == ObjectKind::Tag {
        let tag = objects::parse_tag(&obj.data).ok()?;
        Some(tag.object)
    } else {
        None
    }
}

/// Handle the `fetch` command — validate arguments.
fn cmd_fetch(_git_dir: &Path, args: &[String], out: &mut impl Write) -> Result<()> {
    // Validate arguments
    for arg in args {
        match arg.as_str() {
            "thin-pack" | "no-progress" | "include-tag" | "ofs-delta" | "wait-for-done" => {}
            s if s.starts_with("want ") => {}
            s if s.starts_with("have ") => {}
            s if s.starts_with("shallow ") => {}
            s if s.starts_with("deepen ") => {}
            s if s.starts_with("deepen-since ") => {}
            s if s.starts_with("deepen-not ") => {}
            "deepen-relative" => {}
            "done" => {}
            s if s.starts_with("want-ref ") => {}
            s if s.starts_with("filter ") => {}
            other => bail!("unexpected line: '{other}'"),
        }
    }
    // For now, just send an empty response
    pkt_line::write_flush(out)?;
    Ok(())
}

/// Handle the `object-info` command.
fn cmd_object_info(git_dir: &Path, args: &[String], out: &mut impl Write) -> Result<()> {
    let repo = Repository::open(git_dir, None).with_context(|| "could not open repository")?;

    let mut want_size = false;
    let mut oids: Vec<grit_lib::objects::ObjectId> = Vec::new();

    for arg in args {
        if arg == "size" {
            want_size = true;
        } else if let Some(hex) = arg.strip_prefix("oid ") {
            let oid: grit_lib::objects::ObjectId =
                hex.parse().with_context(|| format!("invalid oid: {hex}"))?;
            oids.push(oid);
        }
    }

    if want_size {
        pkt_line::write_line(out, "size")?;
    }

    for oid in &oids {
        let obj = repo.odb.read(oid)?;
        if want_size {
            pkt_line::write_line(out, &format!("{} {}", oid.to_hex(), obj.data.len()))?;
        }
    }

    pkt_line::write_flush(out)?;
    Ok(())
}

/// Read a boolean config value.
fn read_config_bool(git_dir: &Path, key: &str) -> bool {
    // Check environment-based config overrides first
    if let Some(val) = check_env_config(key) {
        return matches!(val.to_lowercase().as_str(), "true" | "yes" | "1");
    }
    // Check repo config
    let config_path = git_dir.join("config");
    if let Ok(contents) = std::fs::read_to_string(&config_path) {
        if let Some(val) = parse_config_value(&contents, key) {
            return matches!(val.to_lowercase().as_str(), "true" | "yes" | "1");
        }
    }
    false
}

/// Check GIT_CONFIG_COUNT/KEY_N/VALUE_N for a given key.
fn check_env_config(key: &str) -> Option<String> {
    let count: usize = std::env::var("GIT_CONFIG_COUNT").ok()?.parse().ok()?;
    for i in 0..count {
        let k = std::env::var(format!("GIT_CONFIG_KEY_{i}")).ok()?;
        if k.eq_ignore_ascii_case(key) {
            return std::env::var(format!("GIT_CONFIG_VALUE_{i}")).ok();
        }
    }
    None
}

/// Simple config file parser: find the last value for a key like "section.key"
/// or "section.subsection.key".
fn parse_config_value(contents: &str, key: &str) -> Option<String> {
    let parts: Vec<&str> = key.splitn(2, '.').collect();
    if parts.len() != 2 {
        return None;
    }
    let section = parts[0];
    let var_name = parts[1];

    let mut in_section = false;
    let mut result = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Parse section header
            let header = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
            in_section = header.eq_ignore_ascii_case(section);
        } else if in_section {
            if let Some((k, v)) = trimmed.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                if k.eq_ignore_ascii_case(var_name) {
                    result = Some(v.to_owned());
                }
            }
        }
    }
    result
}

/// Discover the git directory from the current working directory.
fn discover_git_dir() -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;

    // Check GIT_DIR env
    if let Ok(dir) = std::env::var("GIT_DIR") {
        let p = std::path::Path::new(&dir);
        if p.is_absolute() {
            return Ok(p.to_path_buf());
        }
        return Ok(cwd.join(p));
    }

    // Check if cwd is a bare repo
    if cwd.join("HEAD").exists() && cwd.join("objects").exists() {
        return Ok(cwd.clone());
    }

    // Check .git
    let git_dir = cwd.join(".git");
    if git_dir.is_dir() {
        return Ok(git_dir);
    }
    // .git might be a file (worktree)
    if git_dir.is_file() {
        let contents = std::fs::read_to_string(&git_dir)?;
        if let Some(path) = contents.strip_prefix("gitdir: ") {
            let path = path.trim();
            let p = std::path::Path::new(path);
            if p.is_absolute() {
                return Ok(p.to_path_buf());
            }
            return Ok(cwd.join(p));
        }
    }

    // Walk up
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => bail!("not a git repository (or any parent)"),
        }
    }
}
