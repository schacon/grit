//! `grit fast-export` — export repository as a fast-import stream.
//!
//! Uses system `git fast-export` for core stream generation, then augments
//! commit-signature commands for `--signed-commits=<mode>` compatibility on
//! older system Git versions.

use crate::commands::git_passthrough;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::{Command, Stdio};

/// Arguments for `grit fast-export`.
#[derive(Debug, ClapArgs)]
#[command(about = "Export repository as fast-import stream")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignedCommitsMode {
    Strip,
    Verbatim,
    WarnVerbatim,
    WarnStrip,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignedTagsMode {
    Strip,
    WarnStrip,
}

#[derive(Debug, Clone)]
struct ExportSignature {
    algo: String,
    format: String,
    payload: String,
}

/// Run `grit fast-export`.
pub fn run(args: Args) -> Result<()> {
    let (signed_mode, mut remaining_args) = extract_signed_commits_mode(&args.args)?;
    let signed_tags_mode = extract_signed_tags_mode(&mut remaining_args);
    remaining_args = rewrite_end_of_options_args(remaining_args);

    // Normalize newer option names for compatibility with older system Git.
    for arg in &mut remaining_args {
        if arg == "--signed-tags=warn-verbatim" {
            *arg = "--signed-tags=warn".to_string();
        }
    }

    // No transformation requested: pure passthrough.
    if signed_mode.is_none() && signed_tags_mode.is_none() {
        return git_passthrough::run("fast-export", &remaining_args);
    }

    let user_requested_original_ids = remaining_args.iter().any(|a| a == "--show-original-ids");
    if signed_mode.is_some() && !user_requested_original_ids {
        remaining_args.push("--show-original-ids".to_owned());
    }

    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let output = Command::new(&git_bin)
        .arg("fast-export")
        .args(&remaining_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to execute {}", git_bin.to_string_lossy()))?;

    if !output.stderr.is_empty() {
        let _ = io::stderr().write_all(&output.stderr);
    }
    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let mut transformed = output.stdout;

    if let Some(mode) = signed_tags_mode {
        transformed = transform_tag_signatures(&transformed, mode)?;
    }

    if let Some(mode) = signed_mode {
        let repo = Repository::discover(None).context("not a git repository")?;
        transformed = transform_export_stream(&transformed, &repo, mode, user_requested_original_ids)?;
    }

    io::stdout().write_all(&transformed)?;
    Ok(())
}

fn extract_signed_commits_mode(args: &[String]) -> Result<(Option<SignedCommitsMode>, Vec<String>)> {
    let mut mode = None;
    let mut rest = Vec::new();
    for arg in args {
        if let Some(raw) = arg.strip_prefix("--signed-commits=") {
            let parsed = if raw == "strip" {
                SignedCommitsMode::Strip
            } else if raw == "verbatim" {
                SignedCommitsMode::Verbatim
            } else if raw == "warn-verbatim" {
                SignedCommitsMode::WarnVerbatim
            } else if raw == "warn-strip" {
                SignedCommitsMode::WarnStrip
            } else if raw == "abort" {
                SignedCommitsMode::Abort
            } else if raw == "strip-if-invalid" {
                bail!(
                    "'strip-if-invalid' is not a valid mode for git fast-export with --signed-commits=<mode>"
                );
            } else if raw.starts_with("sign-if-invalid") {
                bail!(
                    "'sign-if-invalid' is not a valid mode for git fast-export with --signed-commits=<mode>"
                );
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

fn extract_signed_tags_mode(args: &mut [String]) -> Option<SignedTagsMode> {
    let mut mode = None;
    for arg in args {
        if let Some(raw) = arg.strip_prefix("--signed-tags=") {
            match raw {
                "strip" => {
                    mode = Some(SignedTagsMode::Strip);
                    *arg = "--signed-tags=verbatim".to_string();
                }
                "warn-strip" => {
                    mode = Some(SignedTagsMode::WarnStrip);
                    *arg = "--signed-tags=verbatim".to_string();
                }
                _ => {}
            }
        }
    }
    mode
}

fn rewrite_end_of_options_args(args: Vec<String>) -> Vec<String> {
    let repo = Repository::discover(None).ok();
    let mut seen_end = false;
    let mut out = Vec::with_capacity(args.len());

    for arg in args {
        if arg == "--end-of-options" {
            seen_end = true;
            continue;
        }
        if seen_end && arg.starts_with('-') {
            if let Some(repo) = &repo {
                if let Some(rewritten) = disambiguate_leading_dash_revision(repo, &arg) {
                    out.push(rewritten);
                    continue;
                }
            }
        }
        out.push(arg);
    }
    out
}

fn disambiguate_leading_dash_revision(repo: &Repository, arg: &str) -> Option<String> {
    let candidates = [
        arg.to_string(),
        format!("refs/heads/{arg}"),
        format!("refs/tags/{arg}"),
        format!("refs/remotes/{arg}"),
    ];
    for candidate in candidates {
        if refs::resolve_ref(&repo.git_dir, &candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn transform_tag_signatures(input: &[u8], mode: SignedTagsMode) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0usize;

    while let Some(line) = read_line(input, &mut pos) {
        if line.starts_with(b"tag ") {
            out.extend_from_slice(line);
            while let Some(tag_line) = read_line(input, &mut pos) {
                out.extend_from_slice(tag_line);
                if let Some(data_len) = parse_data_len(tag_line) {
                    let payload = read_exact(input, &mut pos, data_len)?;
                    let payload_text = String::from_utf8_lossy(payload).to_string();
                    let (stripped, had_signature) = strip_tag_signature_blocks(&payload_text);
                    if had_signature {
                        if matches!(mode, SignedTagsMode::WarnStrip) {
                            eprintln!("warning: stripping signed tag payload");
                        }
                        out.truncate(out.len().saturating_sub(tag_line.len()));
                        out.extend_from_slice(format!("data {}\n", stripped.len()).as_bytes());
                        out.extend_from_slice(stripped.as_bytes());
                    } else {
                        out.extend_from_slice(payload);
                    }
                    if pos < input.len() && input[pos] == b'\n' {
                        out.push(b'\n');
                        pos += 1;
                    }
                    continue;
                }
                if tag_line == b"\n" {
                    break;
                }
            }
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

    Ok(out)
}

fn strip_tag_signature_blocks(payload: &str) -> (String, bool) {
    let mut out = Vec::new();
    let mut in_block = false;
    let mut had_signature = false;
    let had_trailing_nl = payload.ends_with('\n');

    for line in payload.lines() {
        if !in_block
            && line.starts_with("-----BEGIN ")
            && (line.contains("SIGNATURE") || line.contains("SIGNED MESSAGE"))
        {
            in_block = true;
            had_signature = true;
            continue;
        }
        if in_block {
            if line.starts_with("-----END ")
                && (line.contains("SIGNATURE") || line.contains("SIGNED MESSAGE"))
            {
                in_block = false;
            }
            continue;
        }
        out.push(line);
    }

    let mut compact = Vec::new();
    let mut prev_blank = false;
    for line in out {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        compact.push(line);
        prev_blank = blank;
    }

    let mut result = compact.join("\n");
    if had_trailing_nl {
        result.push('\n');
    }
    (result, had_signature)
}

fn transform_export_stream(
    input: &[u8],
    repo: &Repository,
    mode: SignedCommitsMode,
    keep_original_ids: bool,
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() + 1024);
    let mut pos = 0usize;

    while let Some(line) = read_line(input, &mut pos) {
        if line.starts_with(b"commit ") {
            out.extend_from_slice(line);
            transform_commit_block(input, &mut pos, &mut out, repo, mode, keep_original_ids)?;
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
    Ok(out)
}

fn transform_commit_block(
    input: &[u8],
    pos: &mut usize,
    out: &mut Vec<u8>,
    repo: &Repository,
    mode: SignedCommitsMode,
    keep_original_ids: bool,
) -> Result<()> {
    let mut original_oid: Option<ObjectId> = None;
    let mut emitted_signatures = false;

    while let Some(line) = read_line(input, pos) {
        if line == b"\n" {
            if !emitted_signatures {
                maybe_emit_commit_signatures(out, repo, original_oid.as_ref(), mode)?;
            }
            out.extend_from_slice(line);
            return Ok(());
        }

        if let Some(oid_hex) = line.strip_prefix(b"original-oid ") {
            let oid_text = String::from_utf8_lossy(oid_hex).trim().to_owned();
            if let Ok(oid) = ObjectId::from_hex(&oid_text) {
                original_oid = Some(oid);
            }
            if keep_original_ids {
                out.extend_from_slice(line);
            }
            continue;
        }

        if !emitted_signatures && line.starts_with(b"data ") {
            maybe_emit_commit_signatures(out, repo, original_oid.as_ref(), mode)?;
            emitted_signatures = true;
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

    Ok(())
}

fn maybe_emit_commit_signatures(
    out: &mut Vec<u8>,
    repo: &Repository,
    oid: Option<&ObjectId>,
    mode: SignedCommitsMode,
) -> Result<()> {
    let Some(oid) = oid else {
        return Ok(());
    };
    let signatures = read_commit_signatures(repo, oid)?;
    if signatures.is_empty() {
        return Ok(());
    }

    match mode {
        SignedCommitsMode::Abort => {
            bail!(
                "encountered signed commit {}; use --signed-commits=<mode> to handle it",
                oid
            );
        }
        SignedCommitsMode::WarnVerbatim => {
            eprintln!("warning: exporting a commit signature for commit {}", oid);
        }
        SignedCommitsMode::WarnStrip => {
            eprintln!("warning: stripping a commit signature from commit {}", oid);
        }
        SignedCommitsMode::Strip | SignedCommitsMode::Verbatim => {}
    }

    if matches!(
        mode,
        SignedCommitsMode::Verbatim | SignedCommitsMode::WarnVerbatim
    ) {
        for sig in signatures {
            out.extend_from_slice(format!("gpgsig {} {}\n", sig.algo, sig.format).as_bytes());
            out.extend_from_slice(format!("data {}\n", sig.payload.len()).as_bytes());
            out.extend_from_slice(sig.payload.as_bytes());
            out.push(b'\n');
        }
    }
    Ok(())
}

fn read_commit_signatures(repo: &Repository, oid: &ObjectId) -> Result<Vec<ExportSignature>> {
    let obj = repo.odb.read(oid)?;
    let mut signatures = Vec::new();
    let mut pos = 0usize;

    while pos < obj.data.len() {
        let line_end = obj.data[pos..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|p| pos + p)
            .unwrap_or(obj.data.len());
        let line = &obj.data[pos..line_end];
        pos = line_end.saturating_add(1);

        if line.is_empty() {
            break;
        }

        let (prefix, algo) = if line.starts_with(b"gpgsig ") {
            (b"gpgsig ".as_slice(), "sha1")
        } else if line.starts_with(b"gpgsig-sha256 ") {
            (b"gpgsig-sha256 ".as_slice(), "sha256")
        } else {
            continue;
        };

        let mut payload = line[prefix.len()..].to_vec();
        while pos < obj.data.len() {
            let cont_end = obj.data[pos..]
                .iter()
                .position(|b| *b == b'\n')
                .map(|p| pos + p)
                .unwrap_or(obj.data.len());
            let cont_line = &obj.data[pos..cont_end];
            if cont_line.first().copied() != Some(b' ') {
                break;
            }
            payload.push(b'\n');
            payload.extend_from_slice(&cont_line[1..]);
            pos = cont_end.saturating_add(1);
        }

        let payload = String::from_utf8_lossy(&payload).to_string();
        signatures.push(ExportSignature {
            algo: algo.to_owned(),
            format: detect_signature_format(&payload).to_owned(),
            payload,
        });
    }

    Ok(signatures)
}

fn detect_signature_format(payload: &str) -> &'static str {
    if let Some(rest) = payload.strip_prefix("GRITSIGV1 ") {
        if let Some(fmt) = rest.split_whitespace().next() {
            return match fmt {
                "ssh" => "ssh",
                "x509" => "x509",
                _ => "openpgp",
            };
        }
    }
    if payload.contains("SSH SIGNATURE") {
        "ssh"
    } else if payload.contains("X509") || payload.contains("CMS") {
        "x509"
    } else {
        "openpgp"
    }
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
    let line = std::str::from_utf8(line).ok()?.trim_end();
    line.strip_prefix("data ")?.parse::<usize>().ok()
}

fn read_exact<'a>(input: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8]> {
    if *pos + len > input.len() {
        bail!("fast-export stream ended unexpectedly while reading data payload");
    }
    let slice = &input[*pos..*pos + len];
    *pos += len;
    Ok(slice)
}
