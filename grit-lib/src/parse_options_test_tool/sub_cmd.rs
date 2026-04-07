//! `test-tool parse-subcommand` — mirrors `cmd__parse_subcommand` / `parse_subcommand__cmd`.

use super::parse_options_cmd::ParseOptionsToolError;

const KEEP_DASHDASH: u32 = 1 << 0;
const STOP_AT_NON_OPTION: u32 = 1 << 1;
const KEEP_ARGV0: u32 = 1 << 2;
const KEEP_UNKNOWN_OPT: u32 = 1 << 3;
const NO_INTERNAL_HELP: u32 = 1 << 4;
const SUBCOMMAND_OPTIONAL: u32 = 1 << 7;

#[derive(Clone, Copy)]
enum Subcmd {
    One,
    Two,
}

/// `test-tool parse-subcommand` — mirrors `cmd__parse_subcommand`.
pub fn run_parse_subcommand(args: &[String]) -> Result<i32, ParseOptionsToolError> {
    if args.len() < 2 {
        return Err(fatal_need_subcommand());
    }
    let mut i = 1usize;
    let mut test_flags: u32 = 0;
    while i < args.len() {
        let a = &args[i];
        if !a.starts_with("--") {
            break;
        }
        match a.as_str() {
            "--keep-dashdash" => test_flags |= KEEP_DASHDASH,
            "--stop-at-non-option" => test_flags |= STOP_AT_NON_OPTION,
            "--keep-argv0" => test_flags |= KEEP_ARGV0,
            "--keep-unknown-opt" => test_flags |= KEEP_UNKNOWN_OPT,
            "--no-internal-help" => test_flags |= NO_INTERNAL_HELP,
            "--subcommand-optional" => test_flags |= SUBCOMMAND_OPTIONAL,
            _ => {
                return Err(ParseOptionsToolError::Fatal(format!(
                    "error: unknown option `{a}`\n"
                )));
            }
        }
        i += 1;
    }
    if args.get(i).map(|s| s.as_str()) != Some("cmd") {
        return Err(fatal_need_subcommand());
    }

    if (test_flags & STOP_AT_NON_OPTION) != 0 {
        return Err(ParseOptionsToolError::Bug(
            "BUG: parse-options.c:767: subcommands are incompatible with PARSE_OPT_STOP_AT_NON_OPTION\n"
                .to_string(),
        ));
    }
    if (test_flags & KEEP_UNKNOWN_OPT) != 0 && (test_flags & SUBCOMMAND_OPTIONAL) == 0 {
        return Err(ParseOptionsToolError::Bug(
            "BUG: parse-options.c:770: subcommands are incompatible with PARSE_OPT_KEEP_UNKNOWN_OPT unless in combination with PARSE_OPT_SUBCOMMAND_OPTIONAL\n"
                .to_string(),
        ));
    }
    if (test_flags & KEEP_DASHDASH) != 0 && (test_flags & SUBCOMMAND_OPTIONAL) == 0 {
        return Err(ParseOptionsToolError::Bug(
            "BUG: parse-options.c:772: subcommands are incompatible with PARSE_OPT_KEEP_DASHDASH unless in combination with PARSE_OPT_SUBCOMMAND_OPTIONAL\n"
                .to_string(),
        ));
    }

    parse_subcommand_inner(&args[i..], test_flags)
}

fn fatal_need_subcommand() -> ParseOptionsToolError {
    ParseOptionsToolError::Fatal(
        "error: need a subcommand\nusage: test-tool parse-subcommand [flag-options] cmd <subcommand>\n"
            .to_string(),
    )
}

fn parse_int(s: &str) -> Result<i32, ParseOptionsToolError> {
    s.parse().map_err(|_| {
        ParseOptionsToolError::Fatal("error: option `opt' expects a numerical value\n".to_string())
    })
}

fn parse_subcommand_inner(argv: &[String], flags: u32) -> Result<i32, ParseOptionsToolError> {
    if argv.is_empty() || argv[0] != "cmd" {
        return Err(fatal_need_subcommand());
    }

    let keep_dashdash = flags & KEEP_DASHDASH != 0;
    let keep_argv0 = flags & KEEP_ARGV0 != 0;
    let keep_unknown = flags & KEEP_UNKNOWN_OPT != 0;
    let subcommand_optional = flags & SUBCOMMAND_OPTIONAL != 0;
    let internal_help = flags & NO_INTERNAL_HELP == 0;

    let total_after_cmd = argv.len().saturating_sub(1);
    let mut opt: i32 = 0;
    let mut i = 1usize;

    while i < argv.len() {
        let arg = &argv[i];

        if internal_help && total_after_cmd == 1 && arg == "-h" {
            return Err(ParseOptionsToolError::Help);
        }

        if total_after_cmd == 1 && arg == "--git-completion-helper" {
            println!("subcmd-one subcmd-two --opt= --no-opt");
            return Ok(0);
        }

        if arg == "-" || !arg.starts_with('-') {
            return match_dashless(arg, opt, &argv[i..], subcommand_optional, keep_argv0);
        }

        if arg == "--" {
            if keep_dashdash {
                return dispatch_subcmd_one(opt, &argv[i..], keep_argv0);
            }
            i += 1;
            return finish_after_double_dash(opt, &argv[i..], subcommand_optional, keep_argv0);
        }

        if arg == "--end-of-options" {
            if !keep_unknown {
                i += 1;
                return finish_after_double_dash(opt, &argv[i..], subcommand_optional, keep_argv0);
            }
            return dispatch_subcmd_one(opt, &argv[i..], keep_argv0);
        }

        if arg.starts_with("--") {
            let name = arg.strip_prefix("--").unwrap_or(arg.as_str());
            if internal_help && (name == "help" || name == "help-all") {
                return Err(ParseOptionsToolError::Help);
            }
            if let Some(rest) = name.strip_prefix("opt=") {
                opt = parse_int(rest)?;
                i += 1;
                continue;
            }
            if name == "opt" {
                i += 1;
                let v = argv.get(i).ok_or_else(|| {
                    ParseOptionsToolError::Fatal(
                        "error: option `opt' requires a value\n".to_string(),
                    )
                })?;
                opt = parse_int(v)?;
                i += 1;
                continue;
            }
            if subcommand_optional && keep_unknown {
                return dispatch_subcmd_one(opt, &argv[i..], keep_argv0);
            }
            return Err(ParseOptionsToolError::Fatal(format!(
                "error: unknown option `{name}`\nusage: test-tool parse-subcommand [flag-options] cmd <subcommand>\n"
            )));
        }

        let body = &arg[1..];
        if body.starts_with('o') {
            let rest = &body[1..];
            if rest.is_empty() {
                i += 1;
                let v = argv.get(i).ok_or_else(|| {
                    ParseOptionsToolError::Fatal("error: switch `o' requires a value\n".to_string())
                })?;
                opt = parse_int(v)?;
                i += 1;
            } else {
                opt = parse_int(rest)?;
                i += 1;
            }
            continue;
        }
        if internal_help && (body == "h" || body.starts_with('h')) {
            return Err(ParseOptionsToolError::Help);
        }
        if subcommand_optional && keep_unknown {
            return dispatch_subcmd_one(opt, &argv[i..], keep_argv0);
        }
        let c = body.chars().next().unwrap_or('?');
        return Err(ParseOptionsToolError::Fatal(format!(
            "error: unknown switch `{c}`\nusage: test-tool parse-subcommand [flag-options] cmd <subcommand>\n"
        )));
    }

    if !subcommand_optional {
        return Err(fatal_need_subcommand());
    }
    dispatch_subcmd_one(opt, &[], keep_argv0)
}

fn finish_after_double_dash(
    opt: i32,
    rest: &[String],
    subcommand_optional: bool,
    keep_argv0: bool,
) -> Result<i32, ParseOptionsToolError> {
    if subcommand_optional {
        dispatch_subcmd_one(opt, rest, keep_argv0)
    } else {
        Err(fatal_need_subcommand())
    }
}

fn match_dashless(
    arg: &str,
    opt: i32,
    rest: &[String],
    subcommand_optional: bool,
    keep_argv0: bool,
) -> Result<i32, ParseOptionsToolError> {
    match arg {
        "subcmd-one" => dispatch_subcmd(Subcmd::One, opt, rest, keep_argv0),
        "subcmd-two" => dispatch_subcmd(Subcmd::Two, opt, rest, keep_argv0),
        _ => {
            if subcommand_optional {
                dispatch_subcmd_one(opt, rest, keep_argv0)
            } else {
                Err(ParseOptionsToolError::Fatal(format!(
                    "error: unknown subcommand: `{arg}`\nusage: test-tool parse-subcommand [flag-options] cmd <subcommand>\n"
                )))
            }
        }
    }
}

fn dispatch_subcmd(
    sub: Subcmd,
    opt: i32,
    rest: &[String],
    keep_argv0: bool,
) -> Result<i32, ParseOptionsToolError> {
    println!("opt: {opt}");
    match sub {
        Subcmd::One => {
            println!("fn: subcmd_one");
            print_args_maybe_keep_argv0(rest, keep_argv0);
        }
        Subcmd::Two => {
            println!("fn: subcmd_two");
            print_args_maybe_keep_argv0(rest, keep_argv0);
        }
    }
    Ok(0)
}

fn dispatch_subcmd_one(
    opt: i32,
    rest: &[String],
    keep_argv0: bool,
) -> Result<i32, ParseOptionsToolError> {
    println!("opt: {opt}");
    println!("fn: subcmd_one");
    print_args_maybe_keep_argv0(rest, keep_argv0);
    Ok(0)
}

fn print_args_maybe_keep_argv0(rest: &[String], keep_argv0: bool) {
    if keep_argv0 {
        println!("arg {:02}: cmd", 0);
        for (idx, a) in rest.iter().enumerate() {
            println!("arg {:02}: {a}", idx + 1);
        }
    } else {
        for (idx, a) in rest.iter().enumerate() {
            println!("arg {:02}: {a}", idx);
        }
    }
}
