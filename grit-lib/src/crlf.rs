//! CRLF / EOL conversion and clean/smudge filter support.
//!
//! This module handles line-ending conversion when staging files (`git add`)
//! and checking out files (`git checkout`, `read-tree -u`, `checkout-index`).
//!
//! Config knobs:
//!   - `core.autocrlf` (true / input / false)
//!   - `core.eol` (lf / crlf / native)
//!   - `core.safecrlf` (true / warn / false)
//!
//! Gitattributes:
//!   - `text` / `text=auto` / `-text` / `binary`
//!   - `eol=lf` / `eol=crlf`
//!   - `filter=<name>` (with `filter.<name>.clean` / `filter.<name>.smudge`)
//!   - `ident` keyword expansion

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::ConfigSet;

/// What `core.autocrlf` is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCrlf {
    True,
    Input,
    False,
}

/// What `core.eol` is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreEol {
    Lf,
    Crlf,
    Native,
}

/// What `core.safecrlf` is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeCrlf {
    True,
    Warn,
    False,
}

/// Per-file text attribute from .gitattributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAttr {
    /// `text` — always treat as text.
    Set,
    /// `text=auto` — auto-detect.
    Auto,
    /// `-text` or `binary` — never convert.
    Unset,
    /// No text attribute specified.
    Unspecified,
}

/// Per-file eol attribute from .gitattributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolAttr {
    Lf,
    Crlf,
    Unspecified,
}

/// Legacy `crlf` gitattribute (deprecated in Git; still honored for EOL conversion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrlfLegacyAttr {
    #[default]
    Unspecified,
    /// `-crlf` — disable CRLF conversion.
    Unset,
    /// `crlf=input` — normalize to LF in the object database; no CRLF on checkout.
    Input,
    /// Bare `crlf` (set) — force CRLF on checkout for text files.
    Crlf,
}

/// Per-file merge attribute from .gitattributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeAttr {
    /// No merge attribute specified.
    Unspecified,
    /// `-merge` — treat as binary/non-text merge.
    Unset,
    /// `merge=<driver>` — use named merge driver.
    Driver(String),
}

/// Per-file attributes relevant to conversion.
#[derive(Debug, Clone)]
pub struct FileAttrs {
    pub text: TextAttr,
    pub eol: EolAttr,
    /// Diff driver name from `diff=<driver>` attribute.
    pub diff_driver: Option<String>,
    pub filter_clean: Option<String>,
    pub filter_smudge: Option<String>,
    /// Whether `filter.<name>.required` is set for the active smudge filter.
    pub filter_smudge_required: bool,
    pub ident: bool,
    pub merge: MergeAttr,
    pub conflict_marker_size: Option<String>,
    /// Working tree encoding (e.g. "utf-16") — content is converted to UTF-8 on add.
    pub working_tree_encoding: Option<String>,
    /// Legacy `crlf` / `-crlf` / `crlf=input` from `.gitattributes`.
    pub crlf_legacy: CrlfLegacyAttr,
}

impl Default for FileAttrs {
    fn default() -> Self {
        FileAttrs {
            text: TextAttr::Unspecified,
            eol: EolAttr::Unspecified,
            diff_driver: None,
            filter_clean: None,
            filter_smudge: None,
            filter_smudge_required: false,
            ident: false,
            merge: MergeAttr::Unspecified,
            conflict_marker_size: None,
            working_tree_encoding: None,
            crlf_legacy: CrlfLegacyAttr::Unspecified,
        }
    }
}

/// Global conversion settings derived from config.
#[derive(Debug, Clone)]
pub struct ConversionConfig {
    pub autocrlf: AutoCrlf,
    pub eol: CoreEol,
    pub safecrlf: SafeCrlf,
}

impl ConversionConfig {
    /// Load conversion settings from a ConfigSet.
    pub fn from_config(config: &ConfigSet) -> Self {
        let autocrlf = match config.get("core.autocrlf") {
            Some(v) => match v.to_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => AutoCrlf::True,
                "input" => AutoCrlf::Input,
                _ => AutoCrlf::False,
            },
            None => AutoCrlf::False,
        };

        let eol = match config.get("core.eol") {
            Some(v) => match v.to_lowercase().as_str() {
                "crlf" => CoreEol::Crlf,
                "lf" => CoreEol::Lf,
                "native" => CoreEol::Native,
                _ => CoreEol::Native,
            },
            None => CoreEol::Native,
        };

        let safecrlf = match config.get("core.safecrlf") {
            Some(v) => match v.to_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => SafeCrlf::True,
                "warn" => SafeCrlf::Warn,
                _ => SafeCrlf::False,
            },
            None => SafeCrlf::False,
        };

        ConversionConfig {
            autocrlf,
            eol,
            safecrlf,
        }
    }
}

/// A parsed .gitattributes rule.
#[derive(Debug, Clone)]
pub struct AttrRule {
    pattern: String,
    attrs: Vec<(String, String)>, // (name, value) where value is "set"/"unset"/specific value
}

/// Load .gitattributes from the worktree root.
pub fn load_gitattributes(work_tree: &Path) -> Vec<AttrRule> {
    let mut rules = Vec::new();

    let root_attrs = work_tree.join(".gitattributes");
    if let Ok(content) = std::fs::read_to_string(&root_attrs) {
        parse_gitattributes(&content, &mut rules);
    }

    let info_attrs = work_tree.join(".git/info/attributes");
    if let Ok(content) = std::fs::read_to_string(&info_attrs) {
        parse_gitattributes(&content, &mut rules);
    }

    rules
}

/// Parse gitattributes content into attribute rules.
///
/// This is useful when attributes are sourced from non-worktree inputs
/// (for example, tree objects selected by `--attr-source`).
#[must_use]
pub fn parse_gitattributes_content(content: &str) -> Vec<AttrRule> {
    let mut rules = Vec::new();
    parse_gitattributes(content, &mut rules);
    rules
}

/// Load .gitattributes from the index (for use during checkout when
/// the worktree file may not yet exist).
pub fn load_gitattributes_from_index(
    index: &crate::index::Index,
    odb: &crate::odb::Odb,
) -> Vec<AttrRule> {
    let mut rules = Vec::new();

    // Look for .gitattributes in the index (stage 0)
    if let Some(entry) = index.get(b".gitattributes", 0) {
        if let Ok(obj) = odb.read(&entry.oid) {
            if let Ok(content) = String::from_utf8(obj.data) {
                parse_gitattributes(&content, &mut rules);
            }
        }
    }

    rules
}

/// Load `.gitattributes` rules that apply to `rel_path`, including root and
/// nested `dir/.gitattributes` along parent directories (Git-consistent order:
/// root first, then each ancestor directory; later rules win in [`get_file_attrs`]).
///
/// Reads from the working tree when present, otherwise from a stage-0 index entry.
pub fn load_gitattributes_for_checkout(
    work_tree: &Path,
    rel_path: &str,
    index: &crate::index::Index,
    odb: &crate::odb::Odb,
) -> Vec<AttrRule> {
    let mut rules = load_gitattributes(work_tree);

    // Root `.gitattributes` may exist only in the index while the worktree file
    // is missing (e.g. t0020 in-tree attributes after `rm -rf .gitattributes`).
    if !work_tree.join(".gitattributes").exists() {
        if let Some(entry) = index.get(b".gitattributes", 0) {
            if let Ok(obj) = odb.read(&entry.oid) {
                if let Ok(content) = String::from_utf8(obj.data) {
                    parse_gitattributes(&content, &mut rules);
                }
            }
        }
    }

    let path = Path::new(rel_path);
    if let Some(parent) = path.parent() {
        let mut accum = PathBuf::new();
        for comp in parent.components() {
            accum.push(comp);
            let ga_rel = accum.join(".gitattributes");
            let wt_ga = work_tree.join(&ga_rel);
            if let Ok(content) = std::fs::read_to_string(&wt_ga) {
                parse_gitattributes(&content, &mut rules);
            } else {
                let key = path_to_index_bytes(&ga_rel);
                if let Some(entry) = index.get(&key, 0) {
                    if let Ok(obj) = odb.read(&entry.oid) {
                        if let Ok(content) = String::from_utf8(obj.data) {
                            parse_gitattributes(&content, &mut rules);
                        }
                    }
                }
            }
        }
    }

    rules
}

fn path_to_index_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

fn parse_gitattributes(content: &str, rules: &mut Vec<AttrRule>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let pattern = match parts.next() {
            Some(p) => p.to_owned(),
            None => continue,
        };

        let mut attrs = Vec::new();
        for part in parts {
            if part == "binary" {
                attrs.push(("text".to_owned(), "unset".to_owned()));
                attrs.push(("diff".to_owned(), "unset".to_owned()));
            } else if let Some(rest) = part.strip_prefix('-') {
                attrs.push((rest.to_owned(), "unset".to_owned()));
            } else if let Some((key, val)) = part.split_once('=') {
                attrs.push((key.to_owned(), val.to_owned()));
            } else {
                attrs.push((part.to_owned(), "set".to_owned()));
            }
        }

        if !attrs.is_empty() {
            rules.push(AttrRule { pattern, attrs });
        }
    }
}

fn config_bool_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1"
    )
}

/// Get file attributes for a given path from .gitattributes rules and config.
pub fn get_file_attrs(rules: &[AttrRule], rel_path: &str, config: &ConfigSet) -> FileAttrs {
    let mut fa = FileAttrs::default();

    // Walk rules; last match wins for each attribute.
    for rule in rules {
        if pattern_matches(&rule.pattern, rel_path) {
            for (name, value) in &rule.attrs {
                match name.as_str() {
                    "text" => {
                        fa.text = match value.as_str() {
                            "set" => TextAttr::Set,
                            "unset" => TextAttr::Unset,
                            "auto" => TextAttr::Auto,
                            _ => TextAttr::Unspecified,
                        };
                    }
                    "eol" => {
                        fa.eol = match value.as_str() {
                            "lf" => EolAttr::Lf,
                            "crlf" => EolAttr::Crlf,
                            _ => EolAttr::Unspecified,
                        };
                    }
                    "filter" => {
                        if value == "unset" {
                            fa.filter_clean = None;
                            fa.filter_smudge = None;
                            fa.filter_smudge_required = false;
                        } else {
                            let clean_key = format!("filter.{value}.clean");
                            let smudge_key = format!("filter.{value}.smudge");
                            let req_key = format!("filter.{value}.required");
                            fa.filter_clean = config.get(&clean_key);
                            fa.filter_smudge = config.get(&smudge_key);
                            fa.filter_smudge_required =
                                config.get(&req_key).is_some_and(|v| config_bool_truthy(&v));
                        }
                    }
                    "diff" => {
                        if value == "unset" {
                            fa.diff_driver = None;
                        } else if !value.is_empty() && value != "set" {
                            fa.diff_driver = Some(value.clone());
                        }
                    }
                    "ident" => {
                        fa.ident = value == "set";
                    }
                    "merge" => {
                        fa.merge = match value.as_str() {
                            "unset" => MergeAttr::Unset,
                            "set" => MergeAttr::Unspecified,
                            other => MergeAttr::Driver(other.to_string()),
                        };
                    }
                    "conflict-marker-size" => {
                        if value == "unset" {
                            fa.conflict_marker_size = None;
                        } else {
                            fa.conflict_marker_size = Some(value.clone());
                        }
                    }
                    "working-tree-encoding" => {
                        if value != "unset" && !value.is_empty() {
                            fa.working_tree_encoding = Some(value.clone());
                        }
                    }
                    "crlf" => {
                        fa.crlf_legacy = match value.as_str() {
                            "unset" => CrlfLegacyAttr::Unset,
                            "input" => CrlfLegacyAttr::Input,
                            "set" => CrlfLegacyAttr::Crlf,
                            _ => CrlfLegacyAttr::Unspecified,
                        };
                    }
                    _ => {}
                }
            }
        }
    }

    fa
}

/// Simple gitattributes pattern matching.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    if !pattern.contains('/') {
        // Match against basename only
        let basename = path.rsplit('/').next().unwrap_or(path);
        glob_matches(pattern, basename)
    } else {
        glob_matches(pattern, path)
    }
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_bytes(pat: &[u8], text: &[u8]) -> bool {
    match (pat.first(), text.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            let pat_rest = pat
                .iter()
                .position(|&b| b != b'*')
                .map_or(&pat[pat.len()..], |i| &pat[i..]);
            if pat_rest.is_empty() {
                return true;
            }
            for i in 0..=text.len() {
                if glob_match_bytes(pat_rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        (Some(&b'?'), Some(_)) => glob_match_bytes(&pat[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob_match_bytes(&pat[1..], &text[1..]),
        _ => false,
    }
}

/// Returns true if the data looks binary (contains NUL bytes in the first 8000 bytes).
pub fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8000);
    data[..check_len].contains(&0)
}

/// Returns true if data contains any CRLF sequences.
pub fn has_crlf(data: &[u8]) -> bool {
    data.windows(2).any(|w| w == b"\r\n")
}

/// Returns true if data contains any lone LF (not preceded by CR).
pub fn has_lone_lf(data: &[u8]) -> bool {
    for i in 0..data.len() {
        if data[i] == b'\n' && (i == 0 || data[i - 1] != b'\r') {
            return true;
        }
    }
    false
}

/// Returns true if data contains a bare CR not followed by LF (Git `text_stat.lonecr`).
fn has_lone_cr(data: &[u8]) -> bool {
    for i in 0..data.len() {
        if data[i] == b'\r' && (i + 1 >= data.len() || data[i + 1] != b'\n') {
            return true;
        }
    }
    false
}

/// Git `convert.c` `will_convert_lf_to_crlf` for `CRLF_AUTO` / `CRLF_AUTO_INPUT` / `CRLF_AUTO_CRLF`:
/// if the blob already has CRLF pairs or lone CRs, do not convert lone LFs to CRLF on checkout.
fn auto_crlf_should_smudge_lf_to_crlf(data: &[u8]) -> bool {
    if !has_lone_lf(data) {
        return false;
    }
    if has_lone_cr(data) || has_crlf(data) {
        return false;
    }
    if is_binary(data) {
        return false;
    }
    true
}

/// Returns true if ALL line endings are CRLF (no lone LF).
pub fn is_all_crlf(data: &[u8]) -> bool {
    has_crlf(data) && !has_lone_lf(data)
}

/// Returns true if ALL line endings are LF (no CRLF).
pub fn is_all_lf(data: &[u8]) -> bool {
    has_lone_lf(data) && !has_crlf(data)
}

// ---------------------------------------------------------------------------
// Input (add / clean) direction
// ---------------------------------------------------------------------------

/// Convert data for storage in the index/object database (the "clean" direction).
///
/// This handles:
/// 1. Clean filter execution
/// 2. CRLF → LF conversion based on config + attributes
/// 3. safecrlf checking
///
/// Returns `Ok(data)` on success, or an error if safecrlf rejects it.
pub fn convert_to_git(
    data: &[u8],
    rel_path: &str,
    conv: &ConversionConfig,
    file_attrs: &FileAttrs,
) -> Result<Vec<u8>, String> {
    let mut buf = data.to_vec();

    // 1. Run clean filter if configured
    if let Some(ref clean_cmd) = file_attrs.filter_clean {
        buf = run_filter(clean_cmd, &buf, rel_path)
            .map_err(|e| format!("clean filter failed: {e}"))?;
    }

    // 2. Determine if we should do CRLF→LF conversion
    let would_convert = would_convert_on_input(conv, file_attrs, &buf);

    // 3. safecrlf check — always check if conversion is configured,
    // even if no actual conversion is needed for this particular file.
    if would_convert {
        check_safecrlf_input(conv, &buf, rel_path)?;
    }

    // 4. Actually convert CRLF → LF if the file has CRLFs
    if would_convert && has_crlf(&buf) {
        buf = crlf_to_lf(&buf);
    }

    Ok(buf)
}

/// Decide whether CRLF/LF conversion is configured for this file on input.
/// Returns true if the file *would* be subject to conversion (even if no
/// actual bytes need changing).
fn would_convert_on_input(conv: &ConversionConfig, attrs: &FileAttrs, data: &[u8]) -> bool {
    match attrs.crlf_legacy {
        CrlfLegacyAttr::Unset => return false,
        CrlfLegacyAttr::Input => {
            if is_binary(data) {
                return false;
            }
            return true;
        }
        CrlfLegacyAttr::Crlf => {
            if attrs.text == TextAttr::Unset {
                return false;
            }
            if is_binary(data) {
                return false;
            }
            return true;
        }
        CrlfLegacyAttr::Unspecified => {}
    }

    // If text is explicitly unset (-text or binary), never convert
    if attrs.text == TextAttr::Unset {
        return false;
    }

    // If eol attr is set, this implies text mode
    if attrs.eol != EolAttr::Unspecified {
        if attrs.text == TextAttr::Auto && is_binary(data) {
            return false;
        }
        return true;
    }

    // If text is explicitly set, always convert
    if attrs.text == TextAttr::Set {
        return true;
    }

    if attrs.text == TextAttr::Auto {
        if is_binary(data) {
            return false;
        }
        return true;
    }

    // No text attribute: fall back to core.autocrlf
    match conv.autocrlf {
        AutoCrlf::True | AutoCrlf::Input => {
            if is_binary(data) {
                return false;
            }
            true
        }
        AutoCrlf::False => false,
    }
}

/// Git-compatible stderr when `core.safecrlf` is `warn` (clean direction, CRLF→LF).
fn eprint_safecrlf_warn_crlf_to_lf(rel_path: &str) {
    eprintln!(
        "warning: in the working copy of '{rel_path}', CRLF will be replaced by LF the next time Git touches it"
    );
}

/// Git-compatible stderr when `core.safecrlf` is `warn` (clean direction, LF→CRLF).
fn eprint_safecrlf_warn_lf_to_crlf(rel_path: &str) {
    eprintln!(
        "warning: in the working copy of '{rel_path}', LF will be replaced by CRLF the next time Git touches it"
    );
}

/// Check safecrlf constraints on input.
fn check_safecrlf_input(
    conv: &ConversionConfig,
    data: &[u8],
    rel_path: &str,
) -> Result<(), String> {
    if conv.safecrlf == SafeCrlf::False {
        return Ok(());
    }

    if is_binary(data) {
        return Ok(());
    }

    let mixed = has_crlf(data) && has_lone_lf(data);

    // Mixed line endings: clean would change some lines; unsafe for both autocrlf modes.
    if mixed {
        if conv.autocrlf == AutoCrlf::Input {
            let msg = format!("fatal: CRLF would be replaced by LF in {rel_path}");
            if conv.safecrlf == SafeCrlf::True {
                return Err(msg);
            }
            eprint_safecrlf_warn_crlf_to_lf(rel_path);
            return Ok(());
        }
        if conv.autocrlf == AutoCrlf::True {
            let msg = format!("fatal: LF would be replaced by CRLF in {rel_path}");
            if conv.safecrlf == SafeCrlf::True {
                return Err(msg);
            }
            eprint_safecrlf_warn_lf_to_crlf(rel_path);
            return Ok(());
        }
    }

    // safecrlf with autocrlf=input: reject if file is all CRLF
    // (the conversion would be irreversible — CRLF→LF, but checkout won't
    // add CR back because autocrlf=input only strips on input)
    if conv.autocrlf == AutoCrlf::Input && is_all_crlf(data) {
        let msg = format!("fatal: CRLF would be replaced by LF in {rel_path}");
        if conv.safecrlf == SafeCrlf::True {
            return Err(msg);
        }
        eprint_safecrlf_warn_crlf_to_lf(rel_path);
        return Ok(());
    }

    // safecrlf with autocrlf=true: reject if file is all LF
    // (LF→LF on input, then LF→CRLF on checkout changes the file)
    if conv.autocrlf == AutoCrlf::True && is_all_lf(data) {
        let msg = format!("fatal: LF would be replaced by CRLF in {rel_path}");
        if conv.safecrlf == SafeCrlf::True {
            return Err(msg);
        }
        eprint_safecrlf_warn_lf_to_crlf(rel_path);
        return Ok(());
    }

    Ok(())
}

/// Replace CRLF with LF.
pub fn crlf_to_lf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 1 < data.len() && data[i] == b'\r' && data[i + 1] == b'\n' {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

/// Replace lone LF with CRLF.
pub fn lf_to_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 10);
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\n' && (i == 0 || data[i - 1] != b'\r') {
            out.push(b'\r');
            out.push(b'\n');
        } else {
            out.push(data[i]);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Output (checkout / smudge) direction
// ---------------------------------------------------------------------------

/// Convert data from the object database for writing to the working tree
/// (the "smudge" direction).
///
/// This handles:
/// 1. LF → CRLF conversion based on config + attributes
/// 2. Smudge filter execution
/// 3. Ident keyword expansion
pub fn convert_to_worktree(
    data: &[u8],
    rel_path: &str,
    conv: &ConversionConfig,
    file_attrs: &FileAttrs,
    oid_hex: Option<&str>,
) -> std::io::Result<Vec<u8>> {
    let mut buf = data.to_vec();

    // 1. Ident expansion
    if file_attrs.ident {
        if let Some(oid) = oid_hex {
            buf = expand_ident(&buf, oid);
        }
    }

    // 2. Smudge filter (before EOL conversion) — matches Git's checkout pipeline
    if let Some(ref smudge_cmd) = file_attrs.filter_smudge {
        match run_filter(smudge_cmd, &buf, rel_path) {
            Ok(filtered) => buf = filtered,
            Err(e) => {
                if file_attrs.filter_smudge_required {
                    return Err(e);
                }
            }
        }
    }

    // 3. LF→CRLF for working tree
    let should_convert = should_convert_to_crlf(conv, file_attrs, &buf);
    if should_convert {
        buf = lf_to_crlf(&buf);
    }

    Ok(buf)
}

/// Decide whether to convert LF→CRLF on output.
fn should_convert_to_crlf(conv: &ConversionConfig, attrs: &FileAttrs, data: &[u8]) -> bool {
    match attrs.crlf_legacy {
        CrlfLegacyAttr::Unset | CrlfLegacyAttr::Input => return false,
        CrlfLegacyAttr::Crlf => {
            if attrs.text == TextAttr::Unset {
                return false;
            }
            // Legacy `crlf` (set) forces CRLF on checkout (even for paths Git
            // would otherwise treat as binary; see t0020 "t* crlf" + `three`).
            return true;
        }
        CrlfLegacyAttr::Unspecified => {}
    }

    // If text is explicitly unset, never convert
    if attrs.text == TextAttr::Unset {
        return false;
    }

    // If there's an explicit eol attribute
    if attrs.eol != EolAttr::Unspecified {
        if attrs.text == TextAttr::Auto && is_binary(data) {
            return false;
        }
        if attrs.eol != EolAttr::Crlf {
            return false;
        }
        // `text=auto` + `eol=crlf` → Git `CRLF_AUTO_CRLF` (safe mixed handling).
        if attrs.text == TextAttr::Auto {
            return auto_crlf_should_smudge_lf_to_crlf(data);
        }
        // Explicit `eol=crlf` with `text` set, etc. → `CRLF_TEXT_CRLF` (always normalize).
        return true;
    }

    // If text is explicitly set, use eol config
    if attrs.text == TextAttr::Set {
        return output_eol_is_crlf(conv);
    }

    if attrs.text == TextAttr::Auto {
        if is_binary(data) {
            return false;
        }
        if !output_eol_is_crlf(conv) {
            return false;
        }
        return auto_crlf_should_smudge_lf_to_crlf(data);
    }

    // No text attribute: fall back to core.autocrlf
    match conv.autocrlf {
        AutoCrlf::True => {
            if is_binary(data) {
                return false;
            }
            auto_crlf_should_smudge_lf_to_crlf(data)
        }
        AutoCrlf::Input | AutoCrlf::False => false,
    }
}

/// Whether the output EOL should be CRLF based on config.
fn output_eol_is_crlf(conv: &ConversionConfig) -> bool {
    // autocrlf=true overrides core.eol
    if conv.autocrlf == AutoCrlf::True {
        return true;
    }
    match conv.eol {
        CoreEol::Crlf => true,
        CoreEol::Lf => false,
        CoreEol::Native => {
            // On Unix, native is LF
            cfg!(windows)
        }
    }
}

/// Expand `$Id$` → `$Id: <oid>$` in data.
fn expand_ident(data: &[u8], oid: &str) -> Vec<u8> {
    let needle = b"$Id$";
    let replacement = format!("$Id: {oid} $");
    let mut out = Vec::with_capacity(data.len() + 60);
    let mut i = 0;
    while i < data.len() {
        if i + needle.len() <= data.len() && &data[i..i + needle.len()] == needle {
            out.extend_from_slice(replacement.as_bytes());
            i += needle.len();
        } else if i + 4 <= data.len() && &data[i..i + 4] == b"$Id:" {
            // Already expanded — replace existing expansion
            if let Some(end) = data[i + 4..].iter().position(|&b| b == b'$') {
                out.extend_from_slice(replacement.as_bytes());
                i += 4 + end + 1;
            } else {
                out.push(data[i]);
                i += 1;
            }
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

/// Collapse `$Id: ... $` back to `$Id$`.
pub fn collapse_ident(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 4 <= data.len() && &data[i..i + 4] == b"$Id:" {
            if let Some(end) = data[i + 4..].iter().position(|&b| b == b'$') {
                out.extend_from_slice(b"$Id$");
                i += 4 + end + 1;
                continue;
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Run a filter command, piping data through stdin→stdout.
fn run_filter(cmd: &str, data: &[u8], _rel_path: &str) -> Result<Vec<u8>, std::io::Error> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    use std::io::Write;
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(data)?;
    }
    drop(child.stdin.take());

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "filter command exited with status {}",
            output.status
        )));
    }

    Ok(output.stdout)
}

// Re-export AttrRule type is internal, but we expose the vec through load_gitattributes.
// The public API uses the opaque Vec from load_gitattributes + get_file_attrs.

/// Opaque type alias for loaded gitattributes rules.
pub type GitAttributes = Vec<AttrRule>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crlf_to_lf() {
        assert_eq!(crlf_to_lf(b"hello\r\nworld\r\n"), b"hello\nworld\n");
        assert_eq!(crlf_to_lf(b"hello\nworld\n"), b"hello\nworld\n");
        assert_eq!(crlf_to_lf(b"hello\r\n"), b"hello\n");
    }

    #[test]
    fn test_lf_to_crlf() {
        assert_eq!(lf_to_crlf(b"hello\nworld\n"), b"hello\r\nworld\r\n");
        assert_eq!(lf_to_crlf(b"hello\r\nworld\r\n"), b"hello\r\nworld\r\n");
    }

    #[test]
    fn test_has_crlf() {
        assert!(has_crlf(b"hello\r\nworld"));
        assert!(!has_crlf(b"hello\nworld"));
    }

    #[test]
    fn smudge_mixed_line_endings_unchanged_with_autocrlf_true() {
        let mut blob = Vec::new();
        for part in [
            b"Oh\n".as_slice(),
            b"here\n",
            b"is\n",
            b"CRLF\r\n",
            b"in\n",
            b"text\n",
        ] {
            blob.extend_from_slice(part);
        }
        let conv = ConversionConfig {
            autocrlf: AutoCrlf::True,
            eol: CoreEol::Lf,
            safecrlf: SafeCrlf::False,
        };
        let attrs = FileAttrs::default();
        let out = convert_to_worktree(&blob, "mixed", &conv, &attrs, None).unwrap();
        assert_eq!(out, blob);
    }

    #[test]
    fn smudge_lf_only_gets_crlf_with_autocrlf_true() {
        let blob = b"a\nb\n";
        let conv = ConversionConfig {
            autocrlf: AutoCrlf::True,
            eol: CoreEol::Lf,
            safecrlf: SafeCrlf::False,
        };
        let attrs = FileAttrs::default();
        let out = convert_to_worktree(blob, "x", &conv, &attrs, None).unwrap();
        assert_eq!(out, b"a\r\nb\r\n");
    }

    #[test]
    fn test_is_binary() {
        assert!(is_binary(b"hello\0world"));
        assert!(!is_binary(b"hello world"));
    }

    #[test]
    fn test_expand_collapse_ident() {
        let data = b"$Id$";
        let expanded = expand_ident(data, "abc123");
        assert_eq!(expanded, b"$Id: abc123 $");
        let collapsed = collapse_ident(&expanded);
        assert_eq!(collapsed, b"$Id$");
    }
}
