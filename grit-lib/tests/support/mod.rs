//! Shared integration-test helpers (refs backend harness, etc.).

mod attributes_harness;
mod config_oracle;
mod history_harness;
mod refs_harness;
mod refstore_conformance;
mod revwalk_fixtures;

pub use attributes_harness::*;
pub use config_oracle::*;
pub use history_harness::{
    assert_grit_commits_match_git, build_commit_dag, commit_file, git_ls_files_stage,
    git_ls_files_stage0_map, git_rev_list, git_rev_list_object_set, git_rev_list_with_boundary,
    git_rev_parse, git_version, git_version_at_least, grit_rev_list_object_set,
    ordering_from_git_args, rev_parse_many, skip_unless_git_at_least, HistoryRepo,
};
pub use refs_harness::*;
pub use refstore_conformance::*;
pub use revwalk_fixtures::{setup_path_simplify_history, setup_treesame_history};
