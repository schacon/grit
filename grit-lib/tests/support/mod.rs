//! Shared integration-test helpers (refs backend harness, etc.).

mod attributes_harness;
mod config_oracle;
mod diff_harness;
mod history_harness;
mod refs_harness;
mod refstore_conformance;
mod revparse_oracle;
mod revwalk_fixtures;

pub use attributes_harness::*;
pub use config_oracle::*;
pub use diff_harness::*;
pub use history_harness::{
    assert_grit_commits_match_git, assert_rev_list_commits_match_git, build_commit_dag,
    commit_file, commit_labeled_with_dates, expand_revision_tokens, expand_token_for_rev_list, git,
    git_commit_graph_write, git_ls_files_stage, git_ls_files_stage0_map, git_merge_base,
    git_merge_base_is_ancestor, git_rev_list, git_rev_list_lines, git_rev_list_object_set,
    git_rev_list_with_boundary, git_rev_parse, git_rev_parse_verify, git_version,
    git_version_at_least, grit_rev_list_commits, grit_rev_list_commits_in_repo,
    grit_rev_list_object_set, open_grit_repo, ordering_from_git_args, rev_parse_many,
    skip_unless_git_at_least, symmetric_rev_list_specs, HistoryRepo,
};
pub use refs_harness::*;
pub use refstore_conformance::*;
pub use revparse_oracle::*;
pub use revwalk_fixtures::{setup_path_simplify_history, setup_treesame_history};
