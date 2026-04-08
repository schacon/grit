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
