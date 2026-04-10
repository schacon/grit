//! `grit imap-send` — stub compatible with tests that only assert empty-input behavior.
//!
//! Full IMAP delivery is not implemented. When `imap.host` is configured and stdin is empty,
//! Git prints `nothing to send` to stderr and exits **1** (t1517-outside-repo).

use anyhow::Result;
use grit_lib::config::ConfigSet;
use std::io::{self, Read};

/// Entry point: argv after `imap-send`.
pub fn run_from_argv(rest: &[String]) -> Result<()> {
    if rest.len() == 1 {
        let a = rest[0].as_str();
        if matches!(a, "-h" | "--help" | "--help-all") {
            print_imap_send_usage();
            if a == "--help" {
                return Ok(());
            }
            std::process::exit(129);
        }
    }

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let config = ConfigSet::load(None, true).unwrap_or_default();
    let host = config
        .get("imap.host")
        .or_else(|| config.get("IMAP.host"))
        .filter(|s| !s.is_empty());

    if host.is_none() {
        eprintln!("no imap store specified");
        std::process::exit(1);
    }

    if input.is_empty() {
        eprintln!("nothing to send");
        std::process::exit(1);
    }

    eprintln!("grit: imap-send with non-empty input is not implemented");
    std::process::exit(1);
}

fn print_imap_send_usage() {
    print!(
        "\
usage: git imap-send [-v] [-q] [--[no-]curl] < <mbox>

    -v, --[no-]verbose    be more verbose
    -q, --[no-]quiet      be more quiet
    --[no-]curl           use libcurl to communicate with the IMAP server

"
    );
}
