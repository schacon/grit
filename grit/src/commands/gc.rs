//! `grit gc` command.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;

/// Arguments for `grit gc`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit gc`.
pub fn run(args: Args) -> Result<()> {
    // When preciousObjects is set, gc should not prune or repack destructively.
    // We pass --no-prune to the underlying git gc to avoid touching precious objects.
    if let Ok(repo) = Repository::discover(None) {
        let config =
            grit_lib::config::ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_default();
        if config
            .get_bool("extensions.preciousObjects")
            .and_then(|r| r.ok())
            .unwrap_or(false)
        {
            let mut safe_args = args.args.clone();
            if !safe_args.contains(&"--no-prune".to_string()) {
                safe_args.push("--no-prune".to_string());
            }
            return git_passthrough::run("gc", &safe_args);
        }
    }
    git_passthrough::run("gc", &args.args)
}
