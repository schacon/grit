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
//! tagger <name> <email> <unix-timestamp> <timezone>
//!
//! [optional message]
//! ```

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::io::Read;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::fsck_standalone::{fsck_tag_mktag_trailer_from, parse_tag_for_mktag, FsckError};
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

    /// Stop option parsing (Git compatibility; `mktag` reads only from stdin).
    #[arg(long = "end-of-options", hide = true)]
    pub end_of_options: bool,
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

    let (tagged_oid, tagged_kind) = validate_mktag_input(&data, strict, extra_header_policy)?;

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
    // Match `git mktag` / `odb_read_object(..., OBJECT_INFO_LOOKUP_REPLACE)`: load the tagged
    // object with replace refs applied, then ensure the resulting object's kind matches the tag's
    // `type` header.
    let obj = repo
        .read_replaced(oid)
        .map_err(|_| anyhow::anyhow!("fatal: could not read tagged object '{oid}'"))?;

    if obj.kind != expected_kind {
        bail!(
            "fatal: object '{oid}' tagged as '{expected_kind}', but is a '{}' type",
            obj.kind
        );
    }
    Ok(())
}

fn mktag_fsck_error(e: &FsckError) -> anyhow::Error {
    eprintln!(
        "error: tag input does not pass fsck: {}: {}",
        e.id, e.detail
    );
    anyhow::anyhow!("tag on stdin did not pass our strict fsck check")
}

fn validate_mktag_input(
    data: &[u8],
    strict: bool,
    extra_header_policy: ExtraHeaderPolicy,
) -> Result<(ObjectId, ObjectKind)> {
    let mut warn = |e: &FsckError| {
        eprintln!(
            "warning: tag input does not pass fsck: {}: {}",
            e.id, e.detail
        );
    };
    let (tagged_oid, tagged_kind, after_tagger, check_trailer) =
        parse_tag_for_mktag(data, strict, &mut warn).map_err(|e| mktag_fsck_error(&e))?;

    if check_trailer {
        match fsck_tag_mktag_trailer_from(data, after_tagger) {
            Ok(()) => {}
            Err(e) if e.id == "extraHeaderEntry" => match extra_header_policy {
                ExtraHeaderPolicy::Ignore => {}
                ExtraHeaderPolicy::Warn => {
                    if strict {
                        return Err(mktag_fsck_error(&e));
                    }
                    eprintln!(
                        "warning: tag input does not pass fsck: {}: {}",
                        e.id, e.detail
                    );
                }
                ExtraHeaderPolicy::Error => return Err(mktag_fsck_error(&e)),
            },
            Err(e) => return Err(mktag_fsck_error(&e)),
        }
    }

    Ok((tagged_oid, tagged_kind))
}
