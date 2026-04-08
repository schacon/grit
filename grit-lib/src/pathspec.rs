//! Git-style pathspec matching for command-line path arguments.
//!
//! Wildcards use [`crate::wildmatch`] in Git’s default (non-`:(glob)`) mode:
//! `*` may match across `/` unless `**` pathname rules apply inside the pattern.

use crate::wildmatch::{wildmatch, WM_CASEFOLD, WM_PATHNAME};

/// Optional path metadata for literal pathspecs with a trailing `/`.
///
/// Git treats `dir/` as “directory or git submodule only”: a regular file `dir`
/// does not match, but a tree entry `dir` or gitlink `dir` does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathspecMatchContext {
    /// The index/tree entry is a directory (mode `040000`).
    pub is_directory: bool,
    /// The entry is a git submodule / gitlink (`160000`).
    pub is_git_submodule: bool,
}

/// Returns whether `path` matches the pathspec `spec` with default (file) context.
///
/// For pathspecs ending in `/`, a path equal to the prefix matches only when
/// [PathspecMatchContext] indicates a directory or submodule; see
/// [`matches_pathspec_with_context`].
#[must_use]
pub fn matches_pathspec(spec: &str, path: &str) -> bool {
    matches_pathspec_with_context(spec, path, PathspecMatchContext::default())
}

/// Like [`matches_pathspec`], but uses `ctx` for trailing-`/` literal pathspecs.
#[must_use]
pub fn matches_pathspec_with_context(spec: &str, path: &str, ctx: PathspecMatchContext) -> bool {
    let trimmed = spec.strip_prefix("./").unwrap_or(spec);
    if trimmed == "." || trimmed.is_empty() {
        return true;
    }

    if trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[') {
        let flags = if trimmed.contains("**") {
            WM_PATHNAME
        } else {
            0
        };
        if wildmatch(trimmed.as_bytes(), path.as_bytes(), flags) {
            return true;
        }
        // `tree-walk.c` match_entry: a directory matches a pattern whose next
        // character after the entry name is `/` (e.g. `path1/f*` matches tree
        // entry `path1` before recursion).
        if (ctx.is_directory || ctx.is_git_submodule)
            && !path.is_empty()
            && trimmed.len() > path.len()
            && trimmed.as_bytes().get(path.len()) == Some(&b'/')
            && trimmed.starts_with(path)
        {
            return true;
        }
        return false;
    }

    if let Some(prefix) = trimmed.strip_suffix('/') {
        if path.starts_with(&format!("{prefix}/")) {
            return true;
        }
        if path == prefix {
            return ctx.is_directory || ctx.is_git_submodule;
        }
        return false;
    }

    path == trimmed || path.starts_with(&format!("{trimmed}/"))
}

/// Parse a Git mode string (e.g. `100644`, `040000`) into a [`PathspecMatchContext`].
#[must_use]
pub fn context_from_mode_octal(mode: &str) -> PathspecMatchContext {
    let Ok(bits) = u32::from_str_radix(mode, 8) else {
        return PathspecMatchContext::default();
    };
    context_from_mode_bits(bits)
}

/// Classify a raw Git mode (e.g. from an index or tree entry) for pathspec matching.
#[must_use]
pub fn context_from_mode_bits(mode: u32) -> PathspecMatchContext {
    let ty = mode & 0o170000;
    PathspecMatchContext {
        is_directory: ty == 0o040000,
        is_git_submodule: ty == 0o160000,
    }
}

#[must_use]
/// Returns wildmatch flags for `:(icase)` / `:(glob)`-style patterns when those
/// appear as explicit magic (not used by default CLI pathspecs).
pub fn wildmatch_flags_icase_glob(icase: bool, glob: bool) -> u32 {
    let mut f = if glob { WM_PATHNAME } else { 0 };
    if icase {
        f |= WM_CASEFOLD;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_prefix_and_exact() {
        assert!(matches_pathspec("path1", "path1/file1"));
        assert!(matches_pathspec_with_context(
            "path1/",
            "path1/file1",
            PathspecMatchContext::default()
        ));
        assert!(matches_pathspec("file0", "file0"));
        assert!(!matches_pathspec("path", "path1/file1"));
    }

    #[test]
    fn wildcards_cross_slash_by_default() {
        assert!(matches_pathspec("f*", "file0"));
        assert!(matches_pathspec("*file1", "path1/file1"));
        assert!(matches_pathspec_with_context(
            "path1/f*",
            "path1",
            PathspecMatchContext {
                is_directory: true,
                ..Default::default()
            }
        ));
        assert!(matches_pathspec("path1/*file1", "path1/file1"));
    }

    #[test]
    fn trailing_slash_directory_only() {
        assert!(!matches_pathspec_with_context(
            "file0/",
            "file0",
            PathspecMatchContext::default()
        ));
        assert!(matches_pathspec_with_context(
            "file0/",
            "file0",
            PathspecMatchContext {
                is_directory: true,
                ..Default::default()
            }
        ));
        assert!(matches_pathspec_with_context(
            "submod/",
            "submod",
            PathspecMatchContext {
                is_git_submodule: true,
                ..Default::default()
            }
        ));
    }
}
