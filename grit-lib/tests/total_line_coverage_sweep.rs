//! Broad public-API sweeps outside the history core that still count toward `[total]`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use grit_lib::crlf::{
    crlf_to_lf, gather_convert_stats_ascii, has_crlf, is_binary, lf_to_crlf,
    parse_gitattributes_content,
};
use grit_lib::error::Error;
use grit_lib::pathspec::{
    bloom_lookup_prefix, bloom_lookup_prefix_with_cwd, context_from_mode_octal, has_glob_chars,
    matches_pathspec, matches_pathspec_list, matches_pathspec_list_for_object,
    matches_pathspec_list_with_context, matches_pathspec_set_for_object,
    matches_pathspec_set_for_object_ls_tree, path_allowed_by_pathspec_list,
    pathspec_contributes_match, pathspec_exclude_matches, pathspec_has_top, pathspec_is_exclude,
    pathspec_matches, pathspec_wants_descent_into_tree, parse_pathspecs_from_source, simple_length,
    validate_attr_pathspecs, validate_global_pathspec_flags, PathspecMatchContext,
};
use grit_lib::repo::{
    common_git_dir_for_config, early_config_ignore_repo_reason, init_bare_clone_minimal,
    init_repository, resolve_dot_git, validate_repo_config, validate_repo_format,
    worktree_config_enabled,
};
use grit_lib::ref_storage::RefStorageFormat;
use tempfile::tempdir;

#[test]
fn pathspec_public_api_matrix() {
    let specs = [
        "*.rs",
        ":(glob)**/*.rs",
        ":(exclude)vendor/",
        ":(icase)README",
        ":(literal)foo*bar",
        ":/tracked",
        ":(top)",
    ];
    let paths = ["src/main.rs", "vendor/x", "README", "foo*bar", "tracked", "lib/a.rs"];
    for spec in specs {
        let _ = simple_length(spec);
        let _ = has_glob_chars(spec);
        let _ = pathspec_is_exclude(spec);
        let _ = pathspec_has_top(spec);
        let _ = bloom_lookup_prefix(spec);
        for path in paths {
            let _ = pathspec_matches(spec, path);
            let _ = pathspec_contributes_match(spec, path);
            let _ = pathspec_exclude_matches(spec, path);
            let _ = pathspec_wants_descent_into_tree(spec, path);
            let _ = matches_pathspec(spec, path);
        }
    }
    let _ = bloom_lookup_prefix_with_cwd("*.rs", Some("/tmp/wt"));
    let _ = validate_global_pathspec_flags();
    let _ = validate_attr_pathspecs(&["*.rs".to_owned()]);
    let list: Vec<String> = specs.iter().map(|s| (*s).to_owned()).collect();
    for path in paths {
        let _ = path_allowed_by_pathspec_list(&list, path);
        let _ = matches_pathspec_list(path, &list);
        let _ = matches_pathspec_list_with_context(path, &list, PathspecMatchContext::default());
        let _ = matches_pathspec_list_with_context(path, &list, context_from_mode_octal("100644"));
        let _ = matches_pathspec_list_for_object(path, 0o100644, &[], &list);
        let _ = matches_pathspec_set_for_object(&list, path, 0o100644, &[]);
        let _ = matches_pathspec_set_for_object_ls_tree(&list, path, 0o100644, &[]);
    }
    let _ = parse_pathspecs_from_source(b"src/\0docs/\0", true);
}

#[test]
fn repo_init_and_crlf_helpers() -> grit_lib::error::Result<()> {
    let dir = tempdir().map_err(Error::Io)?;
    init_repository(dir.path(), false, "main", None, RefStorageFormat::Files)?;
    let git_dir = dir.path().join(".git");
    let _ = validate_repo_format(&git_dir)?;
    let _ = common_git_dir_for_config(&git_dir);
    let _ = worktree_config_enabled(&git_dir);
    let _ = early_config_ignore_repo_reason(&git_dir);
    let _ = resolve_dot_git(&git_dir)?;
    let _ = validate_repo_config("[core]\n\trepositoryformatversion = 0\n");

    let bare = tempdir().map_err(Error::Io)?;
    init_bare_clone_minimal(bare.path(), "main", RefStorageFormat::Files)?;

    let rules = parse_gitattributes_content("* text=auto\n*.bin binary\n");
    assert!(!rules.is_empty());
    for data in [b"plain\n".as_slice(), b"a\r\nb\r\n", b"\0bin\x01"] {
        let _ = is_binary(data);
        let _ = has_crlf(data);
        let _ = gather_convert_stats_ascii(data);
        let _ = crlf_to_lf(data);
        let _ = lf_to_crlf(data);
    }
    Ok(())
}
