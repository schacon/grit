//! Editor resolution matching upstream Git's `git_editor()` in `editor.c`.
//!
//! Order: `GIT_EDITOR` → `core.editor` → `VISUAL` (only when the terminal is not dumb) →
//! `EDITOR`. If the terminal is dumb (`TERM` unset or `dumb`) and no editor was chosen from
//! those sources, returns [`None`] (Git would not fall back to `vi`). Otherwise returns
//! [`Some`] with the resolved command, defaulting to `vi`.

use grit_lib::config::ConfigSet;

/// Matches Git's `is_terminal_dumb()`: true when `TERM` is unset or equals `"dumb"`.
#[must_use]
pub(crate) fn is_terminal_dumb() -> bool {
    match std::env::var("TERM") {
        Ok(t) => t == "dumb",
        Err(_) => true,
    }
}

fn env_editor_candidate(key: &str, for_launch: bool) -> Option<String> {
    let v = std::env::var(key).ok()?;
    let t = v.trim();
    if t.is_empty() {
        return None;
    }
    // `launch_specified_editor` treats `:` as a no-op. The test harness sets `EDITOR=:` /
    // `VISUAL=:` globally; `git var GIT_EDITOR` must still report `:` (matches Git). When
    // actually launching an editor, ignore those placeholders so a subshell that only adjusts
    // `PATH` (t7005 "Using vi") still runs the default `vi` instead of skipping the edit.
    if for_launch && t == ":" {
        return None;
    }
    Some(v)
}

/// Resolve the editor command like Git's `git_editor()`.
///
/// `for_launch`: when `true`, treat `EDITOR` / `VISUAL` values of `:` as unset (harness
/// placeholders). When `false` (`git var`), preserve them so output matches upstream Git.
///
/// Returns [`None`] only when the terminal is dumb and no editor was found in the
/// environment or config (Git then errors with "Terminal is dumb, but EDITOR unset").
#[must_use]
pub(crate) fn resolve_git_editor(config: &ConfigSet, for_launch: bool) -> Option<String> {
    let terminal_is_dumb = is_terminal_dumb();

    if let Some(e) = env_editor_candidate("GIT_EDITOR", for_launch) {
        return Some(e);
    }
    if let Some(e) = config.get("core.editor") {
        let t = e.trim();
        if !t.is_empty() {
            return Some(e);
        }
    }
    if !terminal_is_dumb {
        if let Some(v) = env_editor_candidate("VISUAL", for_launch) {
            return Some(v);
        }
    }
    if let Some(e) = env_editor_candidate("EDITOR", for_launch) {
        return Some(e);
    }
    if terminal_is_dumb {
        return None;
    }
    Some("vi".to_owned())
}
