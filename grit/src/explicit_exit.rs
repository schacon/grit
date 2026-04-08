//! Errors that request a specific process exit code (Git-compatible).

use std::fmt;

/// Carries a non-default exit code for the grit binary (e.g. merge pre-flight).
#[derive(Debug)]
pub struct ExplicitExit {
    pub code: i32,
    pub message: String,
}

impl fmt::Display for ExplicitExit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExplicitExit {}
