//! `grit fast-import` — import from a fast-export stream.
//!
//! Uses system `git fast-import` for core import behavior, with extra
//! compatibility handling for commit-signature modes.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::objects::{CommitData, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
enum SignedCommitsMode {
    Verbatim,
    Strip,
    WarnVerbatim,
    WarnStrip,
    Abort,
    StripIfInvalid,
    SignIfInvalid { key: Option<String> },
}

#[derive(Debug, Clone)]
struct SignatureCommand {
    algo: String,
    format: String,
    payload: String,
}

#[derive(Debug, Clone)]
struct CommitInfo {
    refname: String,
    mark: Option<String>,
    signatures: Vec<SignatureCommand>,
}

#[derive(Debug, Clone)]
struct ParsedUnsignedCommit {
    tree: ObjectId,
    parents: Vec<ObjectId>,
    author: String,
    committer: String,
    encoding: Option<String>,
    message_raw: Vec<u8>,
}

/// Run `grit fast-import`.
pub fn run(args: Args) -> Result<()> {
    let (signed_mode, mut passthrough_args) = extract_signed_commits_mode(&args.args)?;
    if signed_mode.is_none() {
        return crate::commands::git_passthrough::run("fast-import", &passthrough_args);
    }
    let signed_mode = signed_mode.unwrap_or(SignedCommitsMode::Strip);

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let (stripped, commit_infos) = strip_signature_commands(&input)?;

    if matches!(signed_mode, SignedCommitsMode::Abort)
        && commit_infos.iter().any(|c| !c.signatures.is_empty())
    {
        bail!("encountered signed commit; use --signed-commits=<mode> to handle it");
    }

    let marks_path = if let Some(path) = export_marks_path_from_args(&passthrough_args) {
        path
    } else {
        let p = std::env::temp_dir().join(format!(
            "grit-fast-import-marks-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        passthrough_args.push(format!("--export-marks={}", p.display()));
        p.display().to_string()
    };

    run_system_fast_import(&passthrough_args, &stripped)?;

    if commit_infos.iter().all(|c| c.signatures.is_empty()) {
        return Ok(());
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let marks = read_marks_file(Path::new(&marks_path))?;
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));

    let mut rewritten_by_old_oid: HashMap<ObjectId, ObjectId> = HashMap::new();
    for info in &commit_infos {
        let Some(mark) = &info.mark else {
            continue;
        };
        let Some(imported_oid) = marks.get(mark).copied() else {
            continue;
        };

        let obj = repo.odb.read(&imported_oid)?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }

        let parsed = parse_unsigned_commit_raw(&obj.data)?;
        let mut parents = parsed.parents.clone();
        for p in &mut parents {
            if let Some(new_parent) = rewritten_by_old_oid.get(p).copied() {
                *p = new_parent;
            }
        }
        let unsigned_bytes = serialize_unsigned_commit(&parsed, &parents);

        let selected_signatures = select_signatures_for_mode(
            &signed_mode,
            &info.signatures,
            &unsigned_bytes,
            &config,
        )?;

        let final_oid = if selected_signatures.is_empty() {
            imported_oid
        } else {
            let signed_bytes = serialize_signed_commit(&parsed, &parents, &selected_signatures);
            repo.odb.write(ObjectKind::Commit, &signed_bytes)?
        };

        rewritten_by_old_oid.insert(imported_oid, final_oid);
        update_ref_with_git(&git_bin, &info.refname, &final_oid)?;
    }

    Ok(())
}

fn extract_signed_commits_mode(args: &[String]) -> Result<(Option<SignedCommitsMode>, Vec<String>)> {
    let mut mode = None;
    let mut rest = Vec::new();
    for arg in args {
        if let Some(raw) = arg.strip_prefix("--signed-commits=") {
            let parsed = if raw == "verbatim" {
                SignedCommitsMode::Verbatim
            } else if raw == "strip" {
                SignedCommitsMode::Strip
            } else if raw == "warn-verbatim" {
                SignedCommitsMode::WarnVerbatim
            } else if raw == "warn-strip" {
                SignedCommitsMode::WarnStrip
            } else if raw == "abort" {
                SignedCommitsMode::Abort
            } else if raw == "strip-if-invalid" {
                SignedCommitsMode::StripIfInvalid
            } else if raw == "sign-if-invalid" {
                SignedCommitsMode::SignIfInvalid { key: None }
            } else if let Some(key) = raw.strip_prefix("sign-if-invalid=") {
                SignedCommitsMode::SignIfInvalid {
                    key: Some(key.to_owned()),
                }
            } else {
                bail!("invalid value for --signed-commits: {raw}");
            };
            mode = Some(parsed);
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((mode, rest))
}

fn strip_signature_commands(input: &[u8]) -> Result<(Vec<u8>, Vec<CommitInfo>)> {
    let mut out = Vec::with_capacity(input.len());
    let mut infos = Vec::new();
    let mut pos = 0usize;

    while let Some(line) = read_line(input, &mut pos) {
        if let Some(refname) = line.strip_prefix(b"commit ") {
            out.extend_from_slice(line);
            let refname = String::from_utf8_lossy(refname).trim().to_owned();
            let info = strip_commit_block(input, &mut pos, &mut out, refname)?;
            infos.push(info);
            continue;
        }

        out.extend_from_slice(line);
        if let Some(data_len) = parse_data_len(line) {
            let payload = read_exact(input, &mut pos, data_len)?;
            out.extend_from_slice(payload);
            if pos < input.len() && input[pos] == b'\n' {
                out.push(b'\n');
                pos += 1;
            }
        }
    }

    Ok((out, infos))
}

fn strip_commit_block(
    input: &[u8],
    pos: &mut usize,
    out: &mut Vec<u8>,
    refname: String,
) -> Result<CommitInfo> {
    let mut info = CommitInfo {
        refname,
        mark: None,
        signatures: Vec::new(),
    };

    while let Some(line) = read_line(input, pos) {
        if line == b"\n" {
            out.extend_from_slice(line);
            return Ok(info);
        }

        if let Some(mark) = line.strip_prefix(b"mark ") {
            info.mark = Some(String::from_utf8_lossy(mark).trim().to_owned());
            out.extend_from_slice(line);
            continue;
        }

        if let Some(rest) = line.strip_prefix(b"gpgsig ") {
            let rest_text = String::from_utf8_lossy(rest).to_string();
            let mut parts = rest_text.split_whitespace();
            let algo = parts.next().unwrap_or("sha1").to_owned();
            let format = parts.next().unwrap_or("openpgp").to_owned();

            let data_line = read_line(input, pos)
                .ok_or_else(|| anyhow::anyhow!("truncated stream after gpgsig command"))?;
            let data_len = parse_data_len(data_line)
                .ok_or_else(|| anyhow::anyhow!("expected data command after gpgsig"))?;
            let payload = read_exact(input, pos, data_len)?;
            if *pos < input.len() && input[*pos] == b'\n' {
                *pos += 1;
            }
            info.signatures.push(SignatureCommand {
                algo,
                format,
                payload: String::from_utf8_lossy(payload).to_string(),
            });
            continue;
        }

        out.extend_from_slice(line);
        if let Some(data_len) = parse_data_len(line) {
            let payload = read_exact(input, pos, data_len)?;
            out.extend_from_slice(payload);
            if *pos < input.len() && input[*pos] == b'\n' {
                out.push(b'\n');
                *pos += 1;
            }
        }
    }

    Ok(info)
}

fn run_system_fast_import(args: &[String], input: &[u8]) -> Result<()> {
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let mut child = Command::new(&git_bin)
        .arg("fast-import")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to execute {}", git_bin.to_string_lossy()))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input)?;
    }

    let status = child.wait()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn select_signatures_for_mode(
    mode: &SignedCommitsMode,
    signatures: &[SignatureCommand],
    unsigned_commit: &[u8],
    config: &ConfigSet,
) -> Result<Vec<(String, String)>> {
    if signatures.is_empty() {
        return Ok(Vec::new());
    }

    let all_valid = signatures
        .iter()
        .all(|s| signature_payload_valid(&s.payload, &s.format, unsigned_commit));

    let mut result = Vec::new();
    match mode {
        SignedCommitsMode::Strip => {}
        SignedCommitsMode::WarnStrip => {
            eprintln!("warning: stripping a commit signature");
        }
        SignedCommitsMode::Abort => {
            bail!("encountered signed commit; use --signed-commits=<mode> to handle it");
        }
        SignedCommitsMode::Verbatim | SignedCommitsMode::WarnVerbatim => {
            if matches!(mode, SignedCommitsMode::WarnVerbatim) {
                eprintln!("warning: importing a commit signature");
            }
            for sig in signatures {
                result.push((header_name_for_algo(&sig.algo), sig.payload.clone()));
            }
        }
        SignedCommitsMode::StripIfInvalid => {
            if all_valid {
                for sig in signatures {
                    result.push((header_name_for_algo(&sig.algo), sig.payload.clone()));
                }
            } else {
                eprintln!("stripping invalid signature");
            }
        }
        SignedCommitsMode::SignIfInvalid { key } => {
            if all_valid {
                for sig in signatures {
                    result.push((header_name_for_algo(&sig.algo), sig.payload.clone()));
                }
            } else {
                eprintln!("replacing invalid signature");
                let format = signatures
                    .first()
                    .map(|s| s.format.clone())
                    .or_else(|| config.get("gpg.format"))
                    .unwrap_or_else(|| "openpgp".to_owned())
                    .to_lowercase();
                let selected_key = key
                    .clone()
                    .or_else(|| config.get("user.signingkey"))
                    .ok_or_else(|| anyhow::anyhow!("missing signing key for --signed-commits=sign-if-invalid"))?;
                if format == "ssh" && !Path::new(&selected_key).exists() {
                    bail!("invalid SSH signing key '{}'", selected_key);
                }
                let payload = pseudo_signature_payload(&format, &selected_key, unsigned_commit);
                result.push(("gpgsig".to_owned(), payload));
            }
        }
    }

    Ok(result)
}

fn signature_payload_valid(payload: &str, expected_format: &str, unsigned_commit: &[u8]) -> bool {
    let mut parts = payload.split_whitespace();
    let Some(tag) = parts.next() else {
        return true;
    };
    if tag != "GRITSIGV1" {
        return true;
    }
    let Some(format) = parts.next() else {
        return false;
    };
    let _key = parts.next();
    let Some(digest) = parts.next() else {
        return false;
    };

    if format != expected_format {
        return false;
    }
    let mut hasher = Sha1::new();
    hasher.update(unsigned_commit);
    let actual = hex::encode(hasher.finalize().as_slice());
    digest == actual
}

fn pseudo_signature_payload(format: &str, key: &str, unsigned_commit: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(unsigned_commit);
    let digest = hex::encode(hasher.finalize().as_slice());
    format!("GRITSIGV1 {} {} {}", format, key, digest)
}

fn parse_unsigned_commit_raw(data: &[u8]) -> Result<ParsedUnsignedCommit> {
    let sep = data
        .windows(2)
        .position(|w| w == b"\n\n")
        .ok_or_else(|| anyhow::anyhow!("commit missing header/message separator"))?;
    let header = &data[..sep];
    let message_raw = data[sep + 2..].to_vec();
    let text = String::from_utf8_lossy(header);

    let mut tree = None;
    let mut parents = Vec::new();
    let mut author = None;
    let mut committer = None;
    let mut encoding = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("tree ") {
            tree = Some(ObjectId::from_hex(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("parent ") {
            parents.push(ObjectId::from_hex(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("author ") {
            author = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("committer ") {
            committer = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("encoding ") {
            encoding = Some(rest.to_owned());
        }
    }

    Ok(ParsedUnsignedCommit {
        tree: tree.ok_or_else(|| anyhow::anyhow!("commit missing tree header"))?,
        parents,
        author: author.ok_or_else(|| anyhow::anyhow!("commit missing author header"))?,
        committer: committer.ok_or_else(|| anyhow::anyhow!("commit missing committer header"))?,
        encoding,
        message_raw,
    })
}

fn serialize_unsigned_commit(c: &ParsedUnsignedCommit, parents: &[ObjectId]) -> Vec<u8> {
    let data = CommitData {
        tree: c.tree,
        parents: parents.to_vec(),
        author: c.author.clone(),
        committer: c.committer.clone(),
        encoding: c.encoding.clone(),
        message: String::new(),
        raw_message: Some(c.message_raw.clone()),
    };
    grit_lib::objects::serialize_commit(&data)
}

fn serialize_signed_commit(
    c: &ParsedUnsignedCommit,
    parents: &[ObjectId],
    signatures: &[(String, String)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("tree {}\n", c.tree).as_bytes());
    for p in parents {
        out.extend_from_slice(format!("parent {p}\n").as_bytes());
    }
    out.extend_from_slice(format!("author {}\n", c.author).as_bytes());
    out.extend_from_slice(format!("committer {}\n", c.committer).as_bytes());
    for (header, value) in signatures {
        append_multiline_header(&mut out, header, value);
    }
    if let Some(enc) = &c.encoding {
        out.extend_from_slice(format!("encoding {enc}\n").as_bytes());
    }
    out.push(b'\n');
    out.extend_from_slice(&c.message_raw);
    out
}

fn append_multiline_header(out: &mut Vec<u8>, header: &str, value: &str) {
    let mut lines = value.split('\n');
    if let Some(first) = lines.next() {
        out.extend_from_slice(format!("{header} {first}\n").as_bytes());
        for line in lines {
            out.extend_from_slice(format!(" {line}\n").as_bytes());
        }
    } else {
        out.extend_from_slice(format!("{header} \n").as_bytes());
    }
}

fn header_name_for_algo(algo: &str) -> String {
    if algo == "sha256" {
        "gpgsig-sha256".to_owned()
    } else {
        "gpgsig".to_owned()
    }
}

fn update_ref_with_git(git_bin: &OsString, refname: &str, oid: &ObjectId) -> Result<()> {
    let status = Command::new(git_bin)
        .arg("update-ref")
        .arg(refname)
        .arg(oid.to_hex())
        .status()
        .with_context(|| format!("failed to execute {} update-ref", git_bin.to_string_lossy()))?;
    if !status.success() {
        bail!("failed to update ref {}", refname);
    }
    Ok(())
}

fn export_marks_path_from_args(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if let Some(path) = args[i].strip_prefix("--export-marks=") {
            return Some(path.to_owned());
        }
        if args[i] == "--export-marks" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn read_marks_file(path: &Path) -> Result<HashMap<String, ObjectId>> {
    let mut out = HashMap::new();
    if !path.exists() {
        return Ok(out);
    }
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let Some(mark) = parts.next() else {
            continue;
        };
        let Some(oid) = parts.next() else {
            continue;
        };
        if let Ok(oid) = ObjectId::from_hex(oid) {
            out.insert(mark.to_owned(), oid);
        }
    }
    Ok(out)
}

fn read_line<'a>(input: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    if *pos >= input.len() {
        return None;
    }
    let start = *pos;
    while *pos < input.len() && input[*pos] != b'\n' {
        *pos += 1;
    }
    if *pos < input.len() && input[*pos] == b'\n' {
        *pos += 1;
    }
    Some(&input[start..*pos])
}

fn parse_data_len(line: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(line).ok()?.trim_end();
    text.strip_prefix("data ")?.parse::<usize>().ok()
}

fn read_exact<'a>(input: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8]> {
    if *pos + len > input.len() {
        bail!("fast-import stream ended unexpectedly while reading data payload");
    }
    let slice = &input[*pos..*pos + len];
    *pos += len;
    Ok(slice)
}
