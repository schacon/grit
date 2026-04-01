//! `grit mktag` — read a tag object from stdin, validate strictly, write to ODB.
//!
//! Stricter than `hash-object -t tag`: validates the tag format, verifies the
//! referenced object exists, and checks that the `type` field matches the actual
//! object type.
//!
//! # Format
//!
//! ```text
//! object <sha1>
//! type <typename>
//! tag <tagname>
//! tagger <name> <email> <timestamp> <timezone>
//!
//! [optional message]
//! ```

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::Read;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::repo::Repository;

/// Arguments for `grit mktag`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Disable strict checking (strict mode is on by default).
    #[arg(long = "no-strict", overrides_with = "strict")]
    pub no_strict: bool,

    /// Enable strict checking (default).
    #[arg(long = "strict", overrides_with = "no_strict")]
    pub strict: bool,
}

/// Policy for the `fsck.extraHeaderEntry` config option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtraHeaderPolicy {
    /// Extra entries are always an error regardless of strict mode.
    Error,
    /// Extra entries warn in non-strict mode and fail in strict mode.
    Warn,
    /// Extra entries are silently ignored.
    Ignore,
}

impl ExtraHeaderPolicy {
    fn from_config_value(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "error" => Self::Error,
            "ignore" => Self::Ignore,
            _ => Self::Warn,
        }
    }
}

/// Run `grit mktag`.
pub fn run(args: Args) -> Result<()> {
    let strict = !args.no_strict;

    let repo = Repository::discover(None).context("not a git repository")?;
    let extra_header_policy = load_extra_header_policy(&repo.git_dir);

    let mut data = Vec::new();
    std::io::stdin()
        .read_to_end(&mut data)
        .context("could not read from stdin")?;

    let (tagged_oid, tagged_kind) = validate_tag_format(&data, strict, extra_header_policy)?;

    verify_tagged_object(&repo, &tagged_oid, tagged_kind)?;

    let oid = repo
        .odb
        .write(ObjectKind::Tag, &data)
        .context("unable to write tag file")?;

    println!("{oid}");
    Ok(())
}

/// Read `fsck.extraHeaderEntry` from the repository config.
fn load_extra_header_policy(git_dir: &Path) -> ExtraHeaderPolicy {
    ConfigSet::load(Some(git_dir), true)
        .ok()
        .and_then(|cfg| cfg.get("fsck.extraheaderentry"))
        .map(|v| ExtraHeaderPolicy::from_config_value(&v))
        .unwrap_or(ExtraHeaderPolicy::Warn)
}

/// Verify the tagged object exists in the ODB and has the expected type.
fn verify_tagged_object(
    repo: &Repository,
    oid: &ObjectId,
    expected_kind: ObjectKind,
) -> Result<()> {
    let obj = repo
        .odb
        .read(oid)
        .map_err(|_| anyhow::anyhow!("could not read tagged object '{oid}'"))?;

    if obj.kind != expected_kind {
        bail!(
            "object '{oid}' tagged as '{expected_kind}', but is a '{}' type",
            obj.kind
        );
    }
    Ok(())
}

/// Read the next line from `data` starting at `*pos`.
///
/// Returns `(line_bytes_without_newline, had_newline)`, or `None` at end of data.
fn next_line<'a>(data: &'a [u8], pos: &mut usize) -> Option<(&'a [u8], bool)> {
    if *pos >= data.len() {
        return None;
    }
    let start = *pos;
    match data[start..].iter().position(|&b| b == b'\n') {
        Some(nl) => {
            *pos = start + nl + 1;
            Some((&data[start..start + nl], true))
        }
        None => {
            *pos = data.len();
            Some((&data[start..], false))
        }
    }
}

/// Build an fsck error value (always fatal).
fn fsck_err(code: &str, msg: &str) -> anyhow::Error {
    eprintln!("error: tag input does not pass fsck: {code}: {msg}");
    anyhow::anyhow!("tag on stdin did not pass our strict fsck check")
}

/// Emit an fsck diagnostic.
///
/// `is_warning` determines whether the message is a warn-level issue (only
/// fatal in strict mode) or an error-level issue (always fatal).
fn fsck_diag(code: &str, msg: &str, is_warning: bool, strict: bool) -> Result<()> {
    if !is_warning || strict {
        eprintln!("error: tag input does not pass fsck: {code}: {msg}");
        bail!("tag on stdin did not pass our strict fsck check");
    }
    eprintln!("warning: tag input does not pass fsck: {code}: {msg}");
    Ok(())
}

/// Parse a 40-hex-character object ID.
fn parse_object_id(hex: &[u8]) -> Result<ObjectId> {
    let s = std::str::from_utf8(hex).map_err(|_| anyhow::anyhow!("invalid OID encoding"))?;
    s.parse::<ObjectId>()
        .map_err(|_| anyhow::anyhow!("invalid OID: {s}"))
}

/// Validate the tagger identity value (everything after `"tagger "`).
///
/// Expected format: `<name> <email> <unix-timestamp> <[+-]HHMM>`
fn validate_tagger_ident(value: &[u8], strict: bool) -> Result<()> {
    // Locate the opening '<' for the email field.
    let lt = match value.iter().position(|&b| b == b'<') {
        Some(p) => p,
        None => return fsck_diag("badEmail", "missing '<' in tagger email", true, strict),
    };

    // Locate the closing '>' on the same (already-stripped) line.
    let gt = match value[lt..].iter().position(|&b| b == b'>') {
        Some(p) => lt + p,
        None => return fsck_diag("badEmail", "missing '>' in tagger email", true, strict),
    };

    // After '>' must come a space then the timestamp.
    let after = &value[gt + 1..];
    if after.is_empty() || after[0] != b' ' {
        return Err(fsck_err("badDate", "missing space after email in tagger"));
    }

    let rest = match std::str::from_utf8(&after[1..]) {
        Ok(s) => s,
        Err(_) => return Err(fsck_err("badDate", "invalid encoding after tagger email")),
    };

    let mut parts = rest.split_whitespace();

    // Timestamp must be a decimal integer.
    let timestamp = match parts.next() {
        Some(t) => t,
        None => return Err(fsck_err("badDate", "missing timestamp in tagger")),
    };
    if timestamp.is_empty() || !timestamp.bytes().all(|b| b.is_ascii_digit()) {
        return Err(fsck_err("badDate", "invalid timestamp in tagger"));
    }

    // Timezone must be exactly `[+-][0-9]{4}`.
    let timezone = match parts.next() {
        Some(tz) => tz,
        None => return Err(fsck_err("badDate", "missing timezone in tagger")),
    };
    let tz = timezone.as_bytes();
    if tz.len() != 5
        || (tz[0] != b'+' && tz[0] != b'-')
        || !tz[1..].iter().all(|b| b.is_ascii_digit())
    {
        return Err(fsck_err("badTimezone", "invalid timezone in tagger"));
    }

    Ok(())
}

/// Check whether an extra header entry should fail or warn.
fn check_extra_header(strict: bool, policy: ExtraHeaderPolicy) -> Result<()> {
    match policy {
        ExtraHeaderPolicy::Ignore => Ok(()),
        ExtraHeaderPolicy::Warn => fsck_diag(
            "extraHeaderEntry",
            "extra header entry in tag",
            true,
            strict,
        ),
        ExtraHeaderPolicy::Error => Err(fsck_err(
            "extraHeaderEntry",
            "extra header entry not allowed",
        )),
    }
}

/// Validate the raw bytes of a tag object and return `(tagged_oid, tagged_kind)`.
fn validate_tag_format(
    data: &[u8],
    strict: bool,
    extra_header_policy: ExtraHeaderPolicy,
) -> Result<(ObjectId, ObjectKind)> {
    let mut pos = 0usize;

    // ── 1. "object <sha1>" ───────────────────────────────────────────────────
    let (line, has_nl) = match next_line(data, &mut pos) {
        Some(l) => l,
        None => return Err(fsck_err("missingObject", "tag is missing 'object' header")),
    };
    if !has_nl {
        return Err(fsck_err(
            "unterminatedHeader",
            "tag header has no terminating newline",
        ));
    }
    let sha1_hex = match line.strip_prefix(b"object ") {
        Some(rest) => rest,
        None => return Err(fsck_err("missingObject", "tag is missing 'object' header")),
    };
    let tagged_oid = parse_object_id(sha1_hex)
        .map_err(|_| fsck_err("badObjectSha1", "invalid 'object' SHA-1"))?;

    // ── 2. "type <typename>" ─────────────────────────────────────────────────
    let (line, has_nl) = match next_line(data, &mut pos) {
        Some(l) => l,
        None => return Err(fsck_err("missingTypeEntry", "tag is missing 'type' header")),
    };
    if !has_nl {
        return Err(fsck_err(
            "unterminatedHeader",
            "tag header has no terminating newline",
        ));
    }
    let type_str = match line.strip_prefix(b"type ") {
        Some(rest) => rest,
        None => return Err(fsck_err("missingTypeEntry", "tag is missing 'type' header")),
    };
    let tagged_kind = ObjectKind::from_bytes(type_str)
        .map_err(|_| fsck_err("badType", "invalid 'type' value in tag"))?;

    // ── 3. "tag <name>" ──────────────────────────────────────────────────────
    let (line, has_nl) = match next_line(data, &mut pos) {
        Some(l) => l,
        None => return Err(fsck_err("missingTagEntry", "tag is missing 'tag' header")),
    };
    if !has_nl {
        return Err(fsck_err(
            "unterminatedHeader",
            "tag header has no terminating newline",
        ));
    }
    let tag_name = match line.strip_prefix(b"tag ") {
        Some(rest) if !rest.is_empty() => rest,
        _ => return Err(fsck_err("missingTagEntry", "tag is missing 'tag' header")),
    };
    if tag_name.contains(&b'\t') {
        fsck_diag(
            "badTagName",
            "tag name contains control characters",
            true,
            strict,
        )?;
    }

    // ── 4. "tagger <ident>" ──────────────────────────────────────────────────
    match next_line(data, &mut pos) {
        None => {
            // EOF right after tag name — no tagger, no body.
            fsck_diag(
                "missingTaggerEntry",
                "tag is missing 'tagger' entry",
                true,
                strict,
            )?;
            return Ok((tagged_oid, tagged_kind));
        }
        Some(([], _)) => {
            // Blank separator with no tagger line.
            fsck_diag(
                "missingTaggerEntry",
                "tag is missing 'tagger' entry",
                true,
                strict,
            )?;
            return Ok((tagged_oid, tagged_kind));
        }
        Some((line, has_nl)) => {
            if !has_nl {
                return Err(fsck_err(
                    "unterminatedHeader",
                    "tag header has no terminating newline",
                ));
            }
            if let Some(tagger_value) = line.strip_prefix(b"tagger ") {
                if tagger_value.is_empty() {
                    fsck_diag(
                        "missingTaggerEntry",
                        "tag is missing 'tagger' entry",
                        true,
                        strict,
                    )?;
                } else {
                    validate_tagger_ident(tagger_value, strict)?;
                }
            } else if line == b"tagger" {
                // "tagger" with no space+value.
                fsck_diag(
                    "missingTaggerEntry",
                    "tag is missing 'tagger' entry",
                    true,
                    strict,
                )?;
            } else {
                // Unrecognised line where tagger was expected.
                fsck_diag(
                    "missingTaggerEntry",
                    "tag is missing 'tagger' entry",
                    true,
                    strict,
                )?;
                check_extra_header(strict, extra_header_policy)?;
            }
        }
    }

    // ── 5. Extra header lines before the blank separator ─────────────────────
    loop {
        match next_line(data, &mut pos) {
            None => break,
            Some(([], _)) => break,
            Some((_, has_nl)) => {
                if !has_nl {
                    return Err(fsck_err(
                        "unterminatedHeader",
                        "tag header has no terminating newline",
                    ));
                }
                check_extra_header(strict, extra_header_policy)?;
            }
        }
    }

    Ok((tagged_oid, tagged_kind))
}
