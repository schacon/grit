//! `grit sh-i18n` — shell internationalization support.
//!
//! Helper for shell scripts that need i18n.  In practice this is sourced
//! by shell scripts and provides `gettext` / `eval_gettext` functions.
//! Our stub simply passes through the input text unmodified (no translation).
//!
//!     grit sh-i18n [<text>]

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit sh-i18n`.
#[derive(Debug, ClapArgs)]
#[command(about = "Shell script i18n support (stub — passes through untranslated)")]
pub struct Args {
    /// Text to translate (passed through as-is).
    #[arg(value_name = "TEXT", num_args = 0.., trailing_var_arg = true)]
    pub text: Vec<String>,
}

/// Run `grit sh-i18n`.
pub fn run(args: Args) -> Result<()> {
    if !args.text.is_empty() {
        println!("{}", args.text.join(" "));
    }
    Ok(())
}
