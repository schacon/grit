//! `grit annotate` — historical alias for `grit blame`.

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit annotate` (identical to `blame`).
#[derive(Debug, ClapArgs)]
#[command(about = "Annotate file lines with revision info (alias for blame)")]
pub struct Args {
    #[arg(short = 'L', action = clap::ArgAction::Append)]
    pub line_range: Vec<String>,

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

    #[arg(
        short = 'C',
        value_name = "score",
        num_args = 0..=1,
        default_missing_value = "",
        action = clap::ArgAction::Append
    )]
    pub copy_detection: Vec<String>,

    #[arg(
        short = 'M',
        value_name = "score",
        num_args = 0..=1,
        default_missing_value = "",
        action = clap::ArgAction::Append
    )]
    pub move_detection: Vec<String>,

    #[arg(short = 'f', long = "show-name")]
    pub show_name: bool,

    #[arg(long = "abbrev")]
    pub abbrev: Option<usize>,

    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    #[arg(long = "root")]
    pub root: bool,

    #[arg(long = "reverse")]
    pub reverse: bool,

    #[arg(long = "first-parent")]
    pub first_parent: bool,

    #[arg(long = "diff-algorithm")]
    pub diff_algorithm: Option<String>,

    #[arg(long = "minimal")]
    pub minimal: bool,

    #[arg(long = "textconv")]
    pub textconv: bool,

    #[arg(long = "no-textconv")]
    pub no_textconv: bool,

    #[arg(long = "contents", value_name = "file")]
    pub contents: Option<String>,

    #[arg()]
    pub args: Vec<String>,
}

pub fn run(mut args: Args) -> Result<()> {
    // Upstream annotate-tests use `-h <rev>` as the starting revision (not `git blame --help`).
    let mut rev_from_h: Option<String> = None;
    let mut out_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.args.len() {
        if args.args[i] == "-h" && i + 1 < args.args.len() && rev_from_h.is_none() {
            rev_from_h = Some(args.args[i + 1].clone());
            i += 2;
        } else {
            out_args.push(args.args[i].clone());
            i += 1;
        }
    }
    args.args = out_args;
    if let Some(r) = rev_from_h {
        args.args.insert(0, r);
    }

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
        move_detection: args.move_detection,
        show_name: args.show_name,
        abbrev: args.abbrev,
        no_abbrev: args.no_abbrev,
        root: args.root,
        reverse: args.reverse,
        first_parent: args.first_parent,
        diff_algorithm: args.diff_algorithm,
        minimal: args.minimal,
        textconv: args.textconv,
        no_textconv: args.no_textconv,
        contents: args.contents,
        progress: false,
        annotate_output: true,
        args: args.args,
    })
}
