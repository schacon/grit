//! UTF-8 NFC path normalization for macOS-style filesystems (`core.precomposeUnicode`).
//!
//! When the filesystem treats NFD and NFC spellings as the same path, Git stores paths in
//! precomposed (NFC) form. This module implements the same normalization using ICU.

use icu_normalizer::ComposingNormalizerBorrowed;
use std::borrow::Cow;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Return true if `s` contains any non-ASCII UTF-8 byte.
#[must_use]
pub fn has_non_ascii_utf8(s: &str) -> bool {
    s.as_bytes().iter().any(|b| *b & 0x80 != 0)
}

/// Normalize a single path segment (no `/`) to NFC when it contains non-ASCII UTF-8.
#[must_use]
pub fn precompose_utf8_segment(s: &str) -> Cow<'_, str> {
    if !has_non_ascii_utf8(s) {
        return Cow::Borrowed(s);
    }
    let normalized = ComposingNormalizerBorrowed::new_nfc().normalize(s);
    if normalized == s {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(normalized.into_owned())
    }
}

/// Normalize every `/`-separated segment of `path` to NFC.
#[must_use]
pub fn precompose_utf8_path(path: &str) -> Cow<'_, str> {
    if !path.as_bytes().iter().any(|b| *b & 0x80 != 0) {
        return Cow::Borrowed(path);
    }
    let mut buf = String::with_capacity(path.len());
    for (i, seg) in path.split('/').enumerate() {
        if i > 0 {
            buf.push('/');
        }
        let c = precompose_utf8_segment(seg);
        buf.push_str(c.as_ref());
    }
    if buf == path {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(buf)
    }
}

/// Update `s` in place when it is valid UTF-8 and NFC differs from the current spelling.
pub fn precompose_os_string_utf8_path(s: &mut OsString, enabled: bool) {
    if !enabled {
        return;
    }
    let Some(utf8) = s.to_str() else {
        return;
    };
    let normalized = precompose_utf8_path(utf8).into_owned();
    if normalized != utf8 {
        *s = OsString::from(normalized);
    }
}

/// Probe whether creating a file under `git_dir` with an NFC filename makes the NFD spelling
/// visible as the same path (macOS / HFS+ style).
///
/// Matches Git's `probe_utf8_pathname_composition` / `UTF8_NFD_TO_NFC` test prerequisite.
pub fn probe_filesystem_normalizes_nfd_to_nfc(git_dir: &Path) -> std::io::Result<bool> {
    const NFC: &str = "\u{00e4}";
    const NFD: &str = "\u{0061}\u{0308}";
    let nfc_path: PathBuf = git_dir.join(NFC);
    let _ = std::fs::remove_file(&nfc_path);
    {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&nfc_path)?;
        f.write_all(b"x")?;
    }
    let nfd_path = git_dir.join(NFD);
    let aliases = nfd_path.exists();
    let _ = std::fs::remove_file(&nfc_path);
    Ok(aliases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precompose_nfd_filename_to_nfc() {
        // Matches t3910: Adiarnfc = UTF-8 \303\204 (U+00C4), Adiarnfd = A + U+0308.
        let nfd = format!("f.{}\u{0308}", 'A');
        let nfc = format!("f.\u{00c4}");
        assert_eq!(precompose_utf8_path(&nfd).as_ref(), nfc.as_str());
    }
}
