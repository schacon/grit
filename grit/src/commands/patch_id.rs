//! `grit patch-id` — compute unique IDs for patches.
//!
//! Reads unified diff text from stdin (e.g. the output of `git log -p` or
//! `git diff-tree --patch --stdin`) and prints one `<patch-id> <commit-id>`
//! line per patch encountered.

use anyhow::Result;
use clap::Args as ClapArgs;
use grit_lib::patch_ids::{compute_patch_ids_from_text, PatchIdMode};
use std::io::{self, Read, Write};

/// Arguments for `grit patch-id`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Compute unique IDs for patches",
    override_usage = "grit patch-id [--stable | --unstable | --verbatim]"
)]
pub struct Args {
    /// Use the stable patch-ID algorithm (file order is irrelevant).
    #[arg(long, conflicts_with_all = ["unstable", "verbatim"])]
    pub stable: bool,

    /// Use the unstable patch-ID algorithm, compatible with Git 1.9 and older.
    #[arg(long, conflicts_with_all = ["stable", "verbatim"])]
    pub unstable: bool,

    /// Do not strip whitespace; implies --stable.
    #[arg(long, conflicts_with_all = ["stable", "unstable"])]
    pub verbatim: bool,
}

/// Run the `patch-id` command.
///
/// Reads all of stdin, computes patch-IDs for each commit found in the diff
/// stream, and writes one `<patch-id> <commit-id>` line per commit to stdout.
///
/// # Errors
///
/// Returns an error if reading stdin or writing stdout fails.
pub fn run(args: Args) -> Result<()> {
    let mode = resolve_mode(&args);

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let pairs = compute_patch_ids_from_text(&input, mode);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for (patch_id, commit_id) in pairs {
        writeln!(out, "{} {}", patch_id.to_hex(), commit_id.to_hex())?;
    }

    Ok(())
}

/// Determine the [`PatchIdMode`] from CLI flags and git config.
///
/// CLI flags take precedence over config; config defaults to unstable.
fn resolve_mode(args: &Args) -> PatchIdMode {
    if args.verbatim {
        return PatchIdMode::Verbatim;
    }
    if args.stable {
        return PatchIdMode::Stable;
    }
    if args.unstable {
        return PatchIdMode::Unstable;
    }

    // Read patchid.stable / patchid.verbatim from config.
    let git_dir = grit_lib::repo::Repository::discover(None)
        .ok()
        .map(|r| r.git_dir);

    if let Some(ref dir) = git_dir {
        if let Ok(config) = grit_lib::config::ConfigSet::load(Some(dir.as_path()), false) {
            if config
                .get("patchid.verbatim")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                return PatchIdMode::Verbatim;
            }
            if config
                .get("patchid.stable")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                return PatchIdMode::Stable;
            }
        }
    }

    PatchIdMode::Unstable
}
