//! `grit http-push` — push objects to a remote repository via HTTP(S).
//!
//! This command is a compatibility entry point that forwards to the native
//! `grit push` implementation with an explicit HTTP(S) URL remote.
//!
//!     grit http-push <URL> [<REF>...]

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit http-push`.
#[derive(Debug, ClapArgs)]
#[command(about = "Push objects over HTTP/DAV to another repository")]
pub struct Args {
    /// URL of the remote repository.
    #[arg(value_name = "URL")]
    pub url: String,

    /// Refs to push.
    #[arg(value_name = "REF")]
    pub refs: Vec<String>,

    /// Report what would be pushed without actually doing it.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Verbose output.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Run `grit http-push` by forwarding to `grit push`.
pub fn run(args: Args) -> Result<()> {
    crate::commands::push::run(crate::commands::push::Args {
        no_ipv4: false,
        no_ipv6: false,
        remote: Some(args.url),
        refspecs: args.refs,
        force: false,
        no_force: false,
        tags: false,
        dry_run: args.dry_run,
        delete: false,
        set_upstream: false,
        force_with_lease: None,
        force_if_includes: false,
        no_force_if_includes: false,
        atomic: false,
        push_option: Vec::new(),
        porcelain: false,
        all: false,
        branches: false,
        mirror: false,
        quiet: !args.verbose,
        no_verify: false,
        recurse_submodules: Vec::new(),
        no_recurse_submodules: true,
        signed: None,
        no_signed: false,
        follow_tags: false,
        no_follow_tags: false,
        prune: false,
        verbose: args.verbose,
        progress: false,
        no_progress: false,
        receive_pack: None,
        upload_pack: None,
    })
}
