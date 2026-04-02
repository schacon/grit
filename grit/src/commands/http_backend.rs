//! `grit http-backend` — CGI program for smart HTTP transport.
//!
//! Implements the server side of the Git smart HTTP protocol as a CGI
//! program.  Currently a stub that accepts the CGI environment but
//! returns an error response.
//!
//!     grit http-backend

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit http-backend`.
#[derive(Debug, ClapArgs)]
#[command(about = "Server side implementation of Git over HTTP")]
pub struct Args {
    /// Stateless RPC mode (for smart HTTP).
    #[arg(long = "stateless-rpc")]
    pub stateless_rpc: bool,
}

/// Run `grit http-backend`.
pub fn run(_args: Args) -> Result<()> {
    // In a real implementation this would read CGI environment variables
    // (PATH_INFO, QUERY_STRING, REQUEST_METHOD, etc.) and serve Git
    // smart HTTP protocol responses.
    eprintln!("fatal: http-backend is not yet implemented in grit");
    // Return proper CGI error
    println!("Status: 501 Not Implemented");
    println!("Content-Type: text/plain");
    println!();
    println!("grit http-backend: not yet implemented");
    Ok(())
}
