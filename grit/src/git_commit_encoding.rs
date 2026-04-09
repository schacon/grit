//! Re-export commit encoding helpers from `grit-lib` (used by merge/rebase paths), plus
//! `i18n.commitEncoding` storage and RFC 2047 helpers for format-patch / am.

pub use grit_lib::commit_encoding::*;

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
