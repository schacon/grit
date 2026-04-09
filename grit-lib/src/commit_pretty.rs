//! Human-oriented commit one-line formats shared by porcelain commands.

use crate::objects::ObjectId;

/// Abbreviate `oid` to at most `abbrev_len` hex characters (minimum 4, maximum 40).
///
/// # Parameters
///
/// - `oid` — full commit object id.
/// - `abbrev_len` — desired abbreviation length (clamped to 4..=40 and to the hex length).
#[must_use]
pub fn abbrev_hex(oid: &ObjectId, abbrev_len: usize) -> String {
    let hex = oid.to_hex();
    let n = abbrev_len.clamp(4, 40).min(hex.len());
    hex[..n].to_owned()
}

fn parse_tz_offset_seconds(offset: &str) -> i64 {
    if offset.len() < 5 {
        return 0;
    }
    let sign = if offset.starts_with('-') { -1i64 } else { 1i64 };
    let hours: i64 = offset[1..3].parse().unwrap_or(0);
    let minutes: i64 = offset[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Format the author/committer date as `YYYY-MM-DD` in the commit's local timezone.
///
/// Matches Git's `DATE_SHORT` mode used by `--pretty=reference` (e.g. `2005-04-07`).
#[must_use]
pub fn format_short_date_from_ident(ident: &str) -> String {
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() < 2 {
        return ident.to_owned();
    }
    let ts_str = parts[1];
    let offset_str = parts[0];
    let Ok(ts) = ts_str.parse::<i64>() else {
        return ident.to_owned();
    };
    let offset_secs = parse_tz_offset_seconds(offset_str);
    let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(ts + offset_secs) else {
        return ident.to_owned();
    };
    let format = time::format_description::parse("[year]-[month]-[day]");
    let Ok(fmt) = format else {
        return ident.to_owned();
    };
    dt.format(&fmt).unwrap_or_else(|_| ident.to_owned())
}

/// One-line `reference` format: `abbrev (subject, YYYY-MM-DD)`.
///
/// Matches upstream `git show -s --pretty=reference` / sequencer `refer_to_commit` output.
///
/// # Parameters
///
/// - `subject_first_line` — first line of the commit message (no trailing newline).
/// - `committer_ident` — raw `committer` header line (`Name <email> epoch tz`).
/// - `abbrev_len` — abbreviation length for the hash (typically 7).
#[must_use]
pub fn format_reference_line(
    oid: &ObjectId,
    subject_first_line: &str,
    committer_ident: &str,
    abbrev_len: usize,
) -> String {
    let abbrev = abbrev_hex(oid, abbrev_len);
    let date = format_short_date_from_ident(committer_ident);
    format!("{abbrev} ({subject_first_line}, {date})")
}
