//! `grit annotate` — historical alias for `grit blame`.

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit annotate` (identical to `blame`).
#[derive(Debug, ClapArgs)]
#[command(about = "Annotate file lines with revision info (alias for blame)")]
pub struct Args {
    #[arg(short = 'L')]
    pub line_range: Option<String>,

    #[arg(short = 'l')]
    pub long_hash: bool,

    #[arg(short = 's')]
    pub suppress: bool,

    #[arg(short = 'e', long = "show-email")]
    pub email: bool,

    #[arg(short = 'p', long = "porcelain")]
    pub porcelain: bool,

    #[arg(long = "line-porcelain")]
    pub line_porcelain: bool,

    #[arg(long = "ignore-rev")]
    pub ignore_rev: Vec<String>,

    #[arg(long = "ignore-revs-file")]
    pub ignore_revs_file: Vec<String>,

    #[arg(long = "color-lines")]
    pub color_lines: bool,

    #[arg(long = "color-by-age")]
    pub color_by_age: bool,

    #[arg(short = 'C', action = clap::ArgAction::Count)]
    pub copy_detection: u8,

    #[arg(short = 'f', long = "show-name")]
    pub show_name: bool,

    #[arg(long = "abbrev")]
    pub abbrev: Option<usize>,

    #[arg()]
    pub args: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    // Delegate to blame with the same arguments
    super::blame::run(super::blame::Args {
        line_range: args.line_range,
        long_hash: args.long_hash,
        suppress: args.suppress,
        email: args.email,
        porcelain: args.porcelain,
        line_porcelain: args.line_porcelain,
        ignore_rev: args.ignore_rev,
        ignore_revs_file: args.ignore_revs_file,
        color_lines: args.color_lines,
        color_by_age: args.color_by_age,
        copy_detection: args.copy_detection,
        show_name: args.show_name,
        abbrev: args.abbrev,
        args: args.args,
    })
}
