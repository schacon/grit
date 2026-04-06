//! `grit repack` command.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;

/// Arguments for `grit repack`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit repack`.
pub fn run(args: Args) -> Result<()> {
    // Check for preciousObjects: destructive repacks (-a, -A, -d) are forbidden.
    if let Ok(repo) = Repository::discover(None) {
        let config =
            grit_lib::config::ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_default();
        if config
            .get_bool("extensions.preciousObjects")
            .and_then(|r| r.ok())
            .unwrap_or(false)
        {
            let destructive = args.args.iter().any(|a| {
                a == "-a"
                    || a == "-A"
                    || a == "-d"
                    || a == "--pack-loose-unreachable"
                    || a.starts_with("-a")
                    || a.starts_with("-A")
            });
            if destructive {
                anyhow::bail!("fatal: cannot repack in a repository with precious objects");
            }
        }
    }
    git_passthrough::run("repack", &args.args)
}
