//! Git-compatible `parse-options` surface used by `test-tool parse-options` and related
//! helpers (`parse-options-flags`, `parse-subcommand`). Matches `git/t/helper/test-parse-options.c`.

mod flags_cmd;
mod git_number;
mod parse_options_cmd;
mod sub_cmd;

pub use flags_cmd::run_parse_options_flags;
pub use parse_options_cmd::{run_parse_options, ParseOptionsToolError};
pub use sub_cmd::run_parse_subcommand;
