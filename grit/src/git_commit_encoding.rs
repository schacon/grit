//! Map Git `i18n.commitEncoding` / commit `encoding` header names to codecs.
//!
//! Git's `ISO-8859-1` is strict Latin-1; `encoding_rs` maps that label to Windows-1252.

use encoding_rs::Encoding;

fn is_iso_8859_1(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1"
    )
}

fn decode_latin1(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        s.push(char::from_u32(u32::from(b)).unwrap_or('\u{FFFD}'));
    }
    s
}

fn encode_latin1_lossy(unicode: &str) -> Vec<u8> {
    unicode
        .chars()
        .map(|c| {
            let cp = u32::from(c);
            if cp <= 0xFF {
                cp as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Git stores the commit message body with a trailing newline when non-empty.
#[must_use]
pub fn ensure_body_trailing_newline(mut bytes: Vec<u8>) -> Vec<u8> {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes
}

/// Resolve an encoding label the way Git uses it in config and commit objects.
///
/// Git accepts names like `eucJP` that `encoding_rs::Encoding::for_label` does not recognize.
/// ISO-8859-1 is handled separately (strict Latin-1).
#[must_use]
pub fn resolve(label: &str) -> Option<&'static Encoding> {
    let t = label.trim();
    if t.is_empty() || is_iso_8859_1(t) {
        return None;
    }
    let normalized = t.replace('_', "-");
    let lower = normalized.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "eucjp" => "euc-jp",
        "cp932" | "mskanji" | "sjis" => "shift_jis",
        _ => normalized.as_str(),
    };
    Encoding::for_label(mapped.as_bytes()).or_else(|| Encoding::for_label(t.as_bytes()))
}

/// Encode `unicode` for storage in a commit using Git's encoding name.
#[must_use]
pub fn encode_unicode(label: &str, unicode: &str) -> Option<Vec<u8>> {
    let t = label.trim();
    let raw = if is_iso_8859_1(t) {
        encode_latin1_lossy(unicode)
    } else {
        let enc = resolve(t)?;
        let (cow, _, _) = enc.encode(unicode);
        cow.into_owned()
    };
    Some(ensure_body_trailing_newline(raw))
}

/// Prepare a commit message for storage per `i18n.commitEncoding` (or equivalent).
///
/// When the configured encoding is not UTF-8, returns [`Some`] raw bytes for the body
/// and sets `encoding` in the commit object; otherwise UTF-8 is stored without an
/// `encoding` header.
#[must_use]
pub fn finalize_stored_commit_message(
    message: String,
    commit_encoding: Option<&str>,
) -> (String, Option<String>, Option<Vec<u8>>) {
    let is_utf8 = match commit_encoding {
        None => true,
        Some(e) => e.eq_ignore_ascii_case("utf-8") || e.eq_ignore_ascii_case("utf8"),
    };
    if is_utf8 {
        return (message, None, None);
    }
    let Some(label) = commit_encoding.filter(|s| !s.trim().is_empty()) else {
        return (message, None, None);
    };
    let Some(raw) = encode_unicode(label, &message) else {
        return (message, None, None);
    };
    (message, Some(label.to_owned()), Some(raw))
}

/// Decode `=?charset?q?...?=` encoded-words in an email display name (before `<`).
///
/// Used when applying patches: `git format-patch` emits RFC 2047 in `From:`; the stored
/// commit author should be the decoded Unicode form.
#[must_use]
pub fn decode_rfc2047_mailbox_from_line(from: &str) -> String {
    let from = from.trim();
    let Some(lt) = from.find('<') else {
        return decode_rfc2047_encoded_words(from);
    };
    let name = from[..lt].trim();
    let tail = &from[lt..];
    let decoded = decode_rfc2047_encoded_words(name);
    if decoded.is_empty() {
        tail.trim_start().to_string()
    } else {
        format!("{decoded} {tail}")
    }
}

fn decode_rfc2047_encoded_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("=?") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        // `=?charset?Q?payload?=` — cannot use naive `?=` because `?Q?` contains `?=`.
        let Some(d1) = rest.find('?') else {
            out.push_str("=?");
            out.push_str(rest);
            return out;
        };
        let charset = &rest[..d1];
        let after_cs = &rest[d1 + 1..];
        let Some(d2) = after_cs.find('?') else {
            out.push_str("=?");
            out.push_str(rest);
            return out;
        };
        let encoding = after_cs[..d2].to_ascii_lowercase();
        let after_enc = &after_cs[d2 + 1..];
        let Some(end) = after_enc.find("?=") else {
            out.push_str("=?");
            out.push_str(rest);
            return out;
        };
        let payload = &after_enc[..end];
        rest = &after_enc[end + 2..];
        if encoding == "q" {
            let bytes = decode_quoted_printable_soft(payload);
            out.push_str(&decode_bytes(Some(charset), &bytes));
        } else if encoding == "b" {
            if let Ok(bytes) = base64_decode_rfc2047(payload) {
                out.push_str(&decode_bytes(Some(charset), &bytes));
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_quoted_printable_soft(payload: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = payload.as_bytes().iter().copied().peekable();
    while let Some(b) = it.next() {
        if b == b'_' {
            out.push(b' ');
        } else if b == b'=' {
            let h1 = it.next();
            let h2 = it.next();
            if let (Some(a), Some(c)) = (h1, h2) {
                if let (Some(hi), Some(lo)) = (hex_nibble(a), hex_nibble(c)) {
                    out.push((hi << 4) | lo);
                    continue;
                }
            }
            out.push(b'=');
        } else {
            out.push(b);
        }
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn base64_decode_rfc2047(input: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in input.as_bytes() {
        if byte == b'=' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let val = TABLE.iter().position(|&c| c == byte).ok_or(())?;
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(output)
}

/// Unicode commit message body for display (e.g. `format-patch`): uses `raw_message` when set.
#[must_use]
pub fn commit_message_unicode_for_display(
    encoding: Option<&str>,
    message: &str,
    raw_message: Option<&[u8]>,
) -> String {
    if let Some(raw) = raw_message {
        decode_bytes(encoding, raw)
    } else {
        message.to_owned()
    }
}

/// Decode `bytes` using Git's encoding name, or lossy UTF-8 if unknown.
#[must_use]
pub fn decode_bytes(label: Option<&str>, bytes: &[u8]) -> String {
    if let Some(l) = label {
        if is_iso_8859_1(l) {
            return decode_latin1(bytes);
        }
        if let Some(enc) = resolve(l) {
            let (cow, _) = enc.decode_without_bom_handling(bytes);
            return cow.into_owned();
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}
