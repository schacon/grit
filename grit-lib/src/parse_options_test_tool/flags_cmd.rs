//! `test-tool parse-options-flags` — mirrors `cmd__parse_options_flags` / `parse_options_flags__cmd`.

use super::parse_options_cmd::ParseOptionsToolError;

const KEEP_DASHDASH: u32 = 1 << 0;
const STOP_AT_NON_OPTION: u32 = 1 << 1;
const KEEP_ARGV0: u32 = 1 << 2;
const KEEP_UNKNOWN_OPT: u32 = 1 << 3;
const NO_INTERNAL_HELP: u32 = 1 << 4;

/// `test-tool parse-options-flags` — mirrors `cmd__parse_options_flags`.
pub fn run_parse_options_flags(args: &[String]) -> Result<i32, ParseOptionsToolError> {
    if args.len() < 2 {
        return Err(ParseOptionsToolError::Fatal(
            "error: 'cmd' is mandatory\nusage: test-tool parse-options-flags [flag-options] cmd [options]\n"
                .to_string(),
        ));
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
            "--subcommand-optional" => test_flags |= 1 << 7,
            _ => {
                return Err(ParseOptionsToolError::Fatal(format!(
                    "error: unknown option `{a}'\n"
                )));
            }
        }
        i += 1;
    }
    if args.get(i).map(|s| s.as_str()) != Some("cmd") {
        return Err(ParseOptionsToolError::Fatal(
            "error: 'cmd' is mandatory\nusage: test-tool parse-options-flags [flag-options] cmd [options]\n"
                .to_string(),
        ));
    }
    parse_flags_cmd_inner(&args[i..], test_flags)
}

fn parse_int_opt(s: &str) -> Result<i32, ParseOptionsToolError> {
    s.parse().map_err(|_| {
        ParseOptionsToolError::Fatal("error: option `opt' expects a numerical value\n".to_string())
    })
}

fn parse_flags_cmd_inner(argv: &[String], flags: u32) -> Result<i32, ParseOptionsToolError> {
    if argv.is_empty() || argv[0] != "cmd" {
        return Err(ParseOptionsToolError::Fatal(
            "error: 'cmd' is mandatory\nusage: test-tool parse-options-flags [flag-options] cmd [options]\n"
                .to_string(),
        ));
    }

    let keep_dashdash = flags & KEEP_DASHDASH != 0;
    let stop_at_non = flags & STOP_AT_NON_OPTION != 0;
    let keep_argv0 = flags & KEEP_ARGV0 != 0;
    let keep_unknown = flags & KEEP_UNKNOWN_OPT != 0;
    let no_internal_help = flags & NO_INTERNAL_HELP != 0;
    let internal_help = !no_internal_help;

    let total_after_cmd = argv.len().saturating_sub(1);
    let mut opt: i32 = 0;
    let mut i = 1usize;
    let mut out: Vec<String> = Vec::new();
    if keep_argv0 {
        out.push("cmd".to_string());
    }

    while i < argv.len() {
        let arg = &argv[i];

        if internal_help && total_after_cmd == 1 && arg == "-h" {
            return Err(ParseOptionsToolError::Help);
        }

        if arg == "-" || !arg.starts_with('-') {
            if stop_at_non {
                out.extend(argv[i..].iter().cloned());
                break;
            }
            out.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            if keep_dashdash {
                out.push("--".to_string());
                i += 1;
                out.extend(argv[i..].iter().cloned());
            } else {
                i += 1;
                out.extend(argv[i..].iter().cloned());
            }
            break;
        }

        if arg == "--end-of-options" {
            if !keep_unknown {
                i += 1;
                out.extend(argv[i..].iter().cloned());
            } else {
                out.push(arg.clone());
                i += 1;
                out.extend(argv[i..].iter().cloned());
            }
            break;
        }

        if arg.starts_with("--") {
            let name = arg.strip_prefix("--").unwrap_or(arg.as_str());
            if internal_help && (name == "help" || name == "help-all") {
                return Err(ParseOptionsToolError::Help);
            }
            if let Some(rest) = name.strip_prefix("opt=") {
                opt = parse_int_opt(rest)?;
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
                opt = parse_int_opt(v)?;
                i += 1;
                continue;
            }
            if keep_unknown {
                out.push(arg.clone());
                i += 1;
                continue;
            }
            let key = name.split('=').next().unwrap_or(name);
            return Err(ParseOptionsToolError::Fatal(format!(
                "error: unknown option `{key}'\nusage: <...> cmd [options]\n"
            )));
        }

        // short options: -o, -oN, -h, -u2, ...
        let body = &arg[1..];
        if let Some(rest) = body.strip_prefix('o') {
            if rest.is_empty() {
                i += 1;
                let v = argv.get(i).ok_or_else(|| {
                    ParseOptionsToolError::Fatal("error: switch `o' requires a value\n".to_string())
                })?;
                opt = parse_int_opt(v)?;
                i += 1;
            } else {
                opt = parse_int_opt(rest)?;
                i += 1;
            }
            continue;
        }
        if (body == "h" || (internal_help && body.starts_with('h'))) && internal_help {
            return Err(ParseOptionsToolError::Help);
        }
        if no_internal_help && (body == "h" || body.starts_with('h')) {
            if keep_unknown {
                out.push(arg.clone());
                i += 1;
                continue;
            }
            return Err(ParseOptionsToolError::Fatal(
                "error: unknown switch `h'\nusage: <...> cmd [options]\n".to_string(),
            ));
        }
        if keep_unknown {
            out.push(arg.clone());
            i += 1;
            continue;
        }
        let c = body.chars().next().unwrap_or('?');
        return Err(ParseOptionsToolError::Fatal(format!(
            "error: unknown switch `{c}'\nusage: <...> cmd [options]\n"
        )));
    }

    println!("opt: {opt}");
    for (idx, a) in out.iter().enumerate() {
        println!("arg {:02}: {a}", idx);
    }
    Ok(0)
}
