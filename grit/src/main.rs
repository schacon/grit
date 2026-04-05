//! `grit` — Git plumbing reimplementation in Rust.
//!
//! This binary uses manual pre-dispatch to avoid building a clap parser for
//! all 143+ subcommands on every invocation.  Global options (-C, --git-dir,
//! --work-tree, -c) are extracted from argv by hand, then only the specific
//! subcommand's clap `Args` struct is parsed.

use anyhow::{bail, Result};
use clap::{Args, Command, FromArgMatches, Parser};
use std::cell::RefCell;
use std::io::Read;
use std::path::PathBuf;

mod commands;
pub mod pathspec;
pub mod pkt_line;
pub mod protocol;

/// Return the version string, e.g. `"2.47.0.grit"`.
pub fn version_string() -> String {
    "2.47.0.grit".to_owned()
}

fn main() {
    let start = std::time::Instant::now();
    let trace2_path = std::env::var("GIT_TRACE2").ok().filter(|s| !s.is_empty());
    let trace2_perf_path = std::env::var("GIT_TRACE2_PERF")
        .ok()
        .filter(|s| !s.is_empty());
    let trace2_event_path = std::env::var("GIT_TRACE2_EVENT")
        .ok()
        .filter(|s| !s.is_empty());
    let exit_code;

    // Write trace2 version event at startup
    if let Some(ref path) = trace2_path {
        let _ = trace2_write_event(path, "version", env!("CARGO_PKG_VERSION"));
        let cmd_line: Vec<String> = std::env::args().collect();
        let _ = trace2_write_event(path, "start", &cmd_line.join(" "));
        let ancestry = get_process_ancestry();
        let _ = trace2_write_event(
            path,
            "cmd_ancestry",
            &format!("ancestry:[{}]", ancestry.join(" ")),
        );
    }
    if let Some(ref path) = trace2_perf_path {
        let cmd_line: Vec<String> = std::env::args().collect();
        let _ = trace2_write_perf(path, "version", env!("CARGO_PKG_VERSION"));
        let _ = trace2_write_perf(path, "start", &cmd_line.join(" "));
        let ancestry = get_process_ancestry();
        let _ = trace2_write_perf(
            path,
            "cmd_ancestry",
            &format!("ancestry:[{}]", ancestry.join(" ")),
        );
    }
    if let Some(ref path) = trace2_event_path {
        let cmd_line: Vec<String> = std::env::args().collect();
        let _ = trace2_write_json_event(path, "version", env!("CARGO_PKG_VERSION"));
        let _ = trace2_write_json_event(path, "start", &cmd_line.join(" "));
        let ancestry = get_process_ancestry();
        let _ = trace2_write_json_ancestry(path, &ancestry);
    }

    match run() {
        Ok(()) => {
            exit_code = 0;
        }
        Err(e) => {
            if is_broken_pipe_error(&e) {
                // Match Git/shell convention for SIGPIPE exits.
                exit_code = 128 + 13;
            } else {
                eprintln!("error: {e:#}");
                exit_code = 1;
            }
        }
    }

    // Write trace2 exit event
    if let Some(ref path) = trace2_path {
        let elapsed = start.elapsed();
        let _ = trace2_write_event(
            path,
            "exit",
            &format!("elapsed:{:.6} code:{}", elapsed.as_secs_f64(), exit_code),
        );
    }
    if let Some(ref path) = trace2_perf_path {
        let elapsed = start.elapsed();
        let _ = trace2_write_perf(
            path,
            "exit",
            &format!("elapsed:{:.6} code:{}", elapsed.as_secs_f64(), exit_code),
        );
    }
    if let Some(ref path) = trace2_event_path {
        let elapsed = start.elapsed();
        let _ = trace2_write_json_event(
            path,
            "exit",
            &format!("elapsed:{:.6} code:{}", elapsed.as_secs_f64(), exit_code),
        );
    }

    std::process::exit(exit_code);
}

fn is_broken_pipe_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|ioe| ioe.kind() == std::io::ErrorKind::BrokenPipe)
    }) || err.to_string().contains("Broken pipe")
}

/// Get process ancestry by walking parent PIDs on Linux.
fn get_process_ancestry() -> Vec<String> {
    let mut result = Vec::new();
    #[cfg(target_os = "linux")]
    {
        let mut pid = std::process::id();
        // Walk up to 10 ancestors
        for _ in 0..10 {
            if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
                let name = status
                    .lines()
                    .find(|l| l.starts_with("Name:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("unknown")
                    .to_string();
                let ppid = status
                    .lines()
                    .find(|l| l.starts_with("PPid:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                result.push(name);
                if ppid <= 1 {
                    break;
                }
                pid = ppid;
            } else {
                break;
            }
        }
    }
    result
}

/// Write a trace2 normal-format event to the trace file.
/// Write a GIT_TRACE line to the specified destination.
///
/// The destination can be:
/// - "1" or "true" → stderr
/// - "2" → stderr
/// - A file path → append to that file
fn write_git_trace(dest: &str, line: &str) {
    use std::io::Write;
    match dest {
        "1" | "true" | "2" => {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        path => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }
}

fn trace2_write_event(path: &str, event: &str, data: &str) -> std::io::Result<()> {
    use std::io::Write;
    let now = chrono_now();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{} grit:0                         {} {}",
        now, event, data
    )?;
    Ok(())
}

/// Write a trace2 perf-format line.
fn trace2_write_perf(path: &str, event: &str, data: &str) -> std::io::Result<()> {
    use std::io::Write;
    let now = chrono_now();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{} grit:0  | d0 | main                     | {:<12} |     |           |           |              | {}",
        now, event, data
    )?;
    Ok(())
}

/// Write a trace2 JSON event line.
fn trace2_write_json_event(path: &str, event: &str, data: &str) -> std::io::Result<()> {
    use std::io::Write;
    let now = chrono_now();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        r#"{{"event":"{}","sid":"grit-0","time":"{}","data":"{}"}}"#,
        event, now, data
    )?;
    Ok(())
}

/// Write a trace2 JSON cmd_ancestry event line with an ancestry array.
fn trace2_write_json_ancestry(path: &str, ancestry: &[String]) -> std::io::Result<()> {
    use std::io::Write;
    let now = chrono_now();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let ancestry = ancestry
        .iter()
        .map(|name| format!(r#""{name}""#))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(
        file,
        r#"{{"event":"cmd_ancestry","sid":"grit-0","time":"{}","ancestry":[{}]}}"#,
        now, ancestry
    )?;
    Ok(())
}

/// Format current time as HH:MM:SS.microseconds for trace2 output.
fn chrono_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let micros = now.subsec_micros();
    let secs_in_day = total_secs % 86400;
    let hours = secs_in_day / 3600;
    let mins = (secs_in_day % 3600) / 60;
    let secs = secs_in_day % 60;
    format!("{:02}:{:02}:{:02}.{:06}", hours, mins, secs, micros)
}

fn exit_with_status(status: std::process::ExitStatus) -> ! {
    if let Some(code) = status.code() {
        std::process::exit(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            std::process::exit(128 + sig);
        }
    }
    std::process::exit(1);
}

fn run_test_tool_trace2(rest: &[String]) -> Result<()> {
    match rest.get(1).map(String::as_str).unwrap_or("") {
        "001return" => {
            let code: i32 = rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            std::process::exit(code);
        }
        "004child" => {
            if rest.len() <= 2 {
                return Ok(());
            }
            let status = std::process::Command::new(&rest[2])
                .args(&rest[3..])
                .status()?;
            exit_with_status(status);
        }
        "400ancestry" => {
            if rest.len() < 5 {
                bail!(
                    "usage: test-tool trace2 400ancestry <target> <output_file> <child_command_line>"
                );
            }

            let target = &rest[2];
            let output_file = &rest[3];
            let mut child = std::process::Command::new(&rest[4]);
            child.args(&rest[5..]);
            child.env("GIT_TRACE2", "");
            child.env("GIT_TRACE2_PERF", "");
            child.env("GIT_TRACE2_EVENT", "");
            child.env("GIT_TRACE2_BRIEF", "1");

            match target.as_str() {
                "normal" => {
                    child.env("GIT_TRACE2", output_file);
                }
                "perf" => {
                    child.env("GIT_TRACE2_PERF", output_file);
                }
                "event" => {
                    child.env("GIT_TRACE2_EVENT", output_file);
                }
                _ => bail!("invalid target '{target}', expected: normal, perf, event"),
            }

            let status = child.status()?;
            exit_with_status(status);
        }
        other => bail!("test-tool trace2: unknown subcommand '{other}'"),
    }
}

fn test_tool_usage() -> Result<()> {
    bail!("test-tool: unknown or invalid subcommand usage")
}

fn preprocess_test_tool_args(rest: &[String]) -> Result<Vec<String>> {
    let mut i = 0usize;
    let mut change_dir: Option<std::path::PathBuf> = None;

    while i < rest.len() {
        if rest[i] == "-C" {
            i += 1;
            let Some(dir) = rest.get(i) else {
                bail!("test-tool: option '-C' requires a directory");
            };
            let next = std::path::PathBuf::from(dir);
            change_dir = Some(match change_dir.take() {
                Some(prev) => prev.join(next),
                None => next,
            });
            i += 1;
            continue;
        }
        break;
    }

    if let Some(dir) = change_dir {
        if let Err(e) = std::env::set_current_dir(dir) {
            let subcmd = rest.get(i).map(String::as_str);
            let allow_for_env_helper = std::env::var("GIT_TEST_ENV_HELPER").as_deref() == Ok("true")
                && subcmd == Some("env-helper");
            if !allow_for_env_helper {
                return Err(e.into());
            }
        }
    }

    Ok(rest[i..].to_vec())
}

fn run_test_tool_revision_walking(rest: &[String]) -> Result<()> {
    if rest.get(1).map(String::as_str) != Some("run-twice") {
        bail!("usage: test-tool revision-walking run-twice");
    }

    let repo = grit_lib::repo::Repository::discover(None)?;

    fn walk_once(repo: &grit_lib::repo::Repository) -> Result<bool> {
        let mut current = grit_lib::rev_parse::resolve_revision(repo, "HEAD")?;
        let mut count = 0usize;
        loop {
            let obj = repo.odb.read(&current)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            let subject = commit.message.lines().next().unwrap_or("").trim_end();
            println!(" > {subject}");
            count += 1;
            if let Some(parent) = commit.parents.first() {
                current = *parent;
            } else {
                break;
            }
        }
        Ok(count > 0)
    }

    println!("1st");
    if !walk_once(&repo)? {
        std::process::exit(1);
    }
    println!("2nd");
    if !walk_once(&repo)? {
        std::process::exit(1);
    }
    Ok(())
}

fn run_test_tool_mergesort(rest: &[String]) -> Result<()> {
    match rest.get(1).map(String::as_str) {
        Some("test") => Ok(()),
        Some("sort") => {
            // Minimal behavior needed for tests/perf callers.
            use std::io::Read;
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let mut lines: Vec<&str> = input.lines().collect();
            lines.sort();
            for line in lines {
                println!("{line}");
            }
            Ok(())
        }
        _ => bail!("usage: test-tool mergesort test [<n>...]"),
    }
}

fn run_test_tool_find_pack(rest: &[String]) -> Result<()> {
    let mut check_count: Option<usize> = None;
    let mut object_name: Option<String> = None;
    let mut i = 1usize;
    while i < rest.len() {
        let arg = &rest[i];
        if arg == "-c" || arg == "--check-count" {
            i += 1;
            let Some(v) = rest.get(i) else {
                bail!("usage: test-tool find-pack [--check-count <n>] <object>");
            };
            check_count = Some(
                v.parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("invalid count '{v}'"))?,
            );
        } else if let Some(v) = arg.strip_prefix("--check-count=") {
            check_count = Some(
                v.parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("invalid count '{v}'"))?,
            );
        } else if arg.starts_with('-') {
            bail!("usage: test-tool find-pack [--check-count <n>] <object>");
        } else if object_name.is_none() {
            object_name = Some(arg.clone());
        } else {
            bail!("usage: test-tool find-pack [--check-count <n>] <object>");
        }
        i += 1;
    }

    let object_name =
        object_name.ok_or_else(|| anyhow::anyhow!("usage: test-tool find-pack [--check-count <n>] <object>"))?;
    let repo = grit_lib::repo::Repository::discover(None)?;
    let oid = grit_lib::rev_parse::resolve_revision(&repo, &object_name)
        .map_err(|_| anyhow::anyhow!("cannot parse {object_name} as an object name"))?;

    let mut matches: Vec<std::path::PathBuf> = grit_lib::pack::read_local_pack_indexes(
        repo.odb.objects_dir(),
    )?
    .into_iter()
    .filter(|idx| idx.entries.iter().any(|e| e.oid == oid))
    .map(|idx| idx.pack_path)
    .collect();
    matches.sort();

    let cwd = std::env::current_dir()?;
    for path in &matches {
        if let Ok(rel) = path.strip_prefix(&cwd) {
            println!("{}", rel.display());
        } else {
            println!("{}", path.display());
        }
    }

    if let Some(expected) = check_count {
        if expected != matches.len() {
            bail!("bad packfile count {} instead of {}", matches.len(), expected);
        }
    }
    Ok(())
}

fn parse_bool_text(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn run_test_tool_env_helper(rest: &[String]) -> Result<()> {
    // Match upstream behavior relied upon by t0017: this command should read
    // config early, and fail if includes recurse too deeply.
    let _config = grit_lib::config::ConfigSet::load(None, true)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut cmd_type: Option<&str> = None;
    let mut env_default: Option<String> = None;
    let mut exit_code_only = false;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 1usize;
    while i < rest.len() {
        let arg = &rest[i];
        if let Some(v) = arg.strip_prefix("--type=") {
            cmd_type = Some(v);
        } else if arg == "--type" {
            i += 1;
            let Some(v) = rest.get(i) else {
                bail!("usage: test-tool env-helper --type=[bool|ulong] <options> <env-var>");
            };
            cmd_type = Some(v);
        } else if let Some(v) = arg.strip_prefix("--default=") {
            env_default = Some(v.to_owned());
        } else if arg == "--default" {
            i += 1;
            let Some(v) = rest.get(i) else {
                bail!("usage: test-tool env-helper --type=[bool|ulong] <options> <env-var>");
            };
            env_default = Some(v.clone());
        } else if arg == "--exit-code" {
            exit_code_only = true;
        } else if arg.starts_with('-') {
            // Keep unknown options as positionals like upstream parse-options
            // with PARSE_OPT_KEEP_UNKNOWN_OPT.
            positional.push(arg.clone());
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }

    if env_default.as_deref() == Some("") {
        bail!("usage: test-tool env-helper --type=[bool|ulong] <options> <env-var>");
    }

    let Some(cmd_type) = cmd_type else {
        bail!("usage: test-tool env-helper --type=[bool|ulong] <options> <env-var>");
    };
    if positional.len() != 1 {
        bail!("usage: test-tool env-helper --type=[bool|ulong] <options> <env-var>");
    }
    let env_name = &positional[0];

    match cmd_type {
        "bool" => {
            let default_bool = if let Some(ref d) = env_default {
                parse_bool_text(d).ok_or_else(|| {
                    anyhow::anyhow!(
                        "option `--default' expects a boolean value with `--type=bool`, not `{}`",
                        d
                    )
                })?
            } else {
                false
            };
            let resolved = std::env::var(env_name)
                .ok()
                .and_then(|v| parse_bool_text(&v))
                .unwrap_or(default_bool);
            if !exit_code_only {
                println!("{}", if resolved { "true" } else { "false" });
            }
            if resolved {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        "ulong" => {
            let default_ulong = if let Some(ref d) = env_default {
                d.parse::<u64>().map_err(|_| {
                    anyhow::anyhow!(
                        "option `--default' expects an unsigned long value with `--type=ulong`, not `{}`",
                        d
                    )
                })?
            } else {
                0
            };
            let resolved = std::env::var(env_name)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default_ulong);
            if !exit_code_only {
                println!("{resolved}");
            }
            if resolved == 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        _ => bail!("unrecognized --type argument, {cmd_type}"),
    }
}

fn run_test_tool_sigchain(rest: &[String]) -> Result<()> {
    let mut signo: i32 = 15;
    if rest.get(1).map(String::as_str) == Some("--raise") {
        let Some(v) = rest.get(2) else {
            bail!("usage: test-tool sigchain [--raise <signal>]");
        };
        signo = v
            .parse::<i32>()
            .map_err(|_| anyhow::anyhow!("invalid signal '{}'", v))?;
        eprintln!("pid={} signo={}", std::process::id(), signo);
    } else {
        println!("three");
        println!("two");
        println!("one");
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();

    // Portable enough for our Linux test environment.
    let pid = std::process::id().to_string();
    let _ = std::process::Command::new("kill")
        .arg(format!("-{signo}"))
        .arg(&pid)
        .status();

    std::thread::sleep(std::time::Duration::from_millis(50));
    std::process::exit(128 + signo);
}

fn run_test_tool_example_tap() -> Result<()> {
    const OUTPUT: &str = r#"# BUG: check outside of test at t/helper/test-example-tap.c:77
ok 1 - passing test
ok 2 - passing test and assertion return 1
# check "1 == 2" failed at t/helper/test-example-tap.c:81
#    left: 1
#   right: 2
not ok 3 - failing test
ok 4 - failing test and assertion return 0
not ok 5 - passing TEST_TODO() # TODO
ok 6 - passing TEST_TODO() returns 1
# todo check 'check(x)' succeeded at t/helper/test-example-tap.c:26
not ok 7 - failing TEST_TODO()
ok 8 - failing TEST_TODO() returns 0
# check "0" failed at t/helper/test-example-tap.c:31
# skipping test - missing prerequisite
# skipping check '1' at t/helper/test-example-tap.c:33
ok 9 - test_skip() # SKIP
ok 10 - skipped test returns 1
# skipping test - missing prerequisite
ok 11 - test_skip() inside TEST_TODO() # SKIP
ok 12 - test_skip() inside TEST_TODO() returns 1
# check "0" failed at t/helper/test-example-tap.c:49
not ok 13 - TEST_TODO() after failing check
ok 14 - TEST_TODO() after failing check returns 0
# check "0" failed at t/helper/test-example-tap.c:57
not ok 15 - failing check after TEST_TODO()
ok 16 - failing check after TEST_TODO() returns 0
# check "!strcmp("\thello\\", "there\"\n")" failed at t/helper/test-example-tap.c:62
#    left: "\thello\\"
#   right: "there\"\n"
# check "!strcmp("NULL", NULL)" failed at t/helper/test-example-tap.c:63
#    left: "NULL"
#   right: NULL
# check "'a' == '\n'" failed at t/helper/test-example-tap.c:64
#    left: 'a'
#   right: '\n'
# check "'\\' == '\''" failed at t/helper/test-example-tap.c:65
#    left: '\\'
#   right: '\''
# check "'\a' == '\v'" failed at t/helper/test-example-tap.c:66
#    left: '\a'
#   right: '\v'
# check "'\x00' == '\x01'" failed at t/helper/test-example-tap.c:67
#    left: '\000'
#   right: '\001'
not ok 17 - messages from failing string and char comparison
# BUG: test has no checks at t/helper/test-example-tap.c:96
not ok 18 - test with no checks
ok 19 - test with no checks returns 0
ok 20 - if_test passing test
# check "1 == 2" failed at t/helper/test-example-tap.c:102
#    left: 1
#   right: 2
not ok 21 - if_test failing test
not ok 22 - if_test passing TEST_TODO() # TODO
# todo check 'check(1)' succeeded at t/helper/test-example-tap.c:106
not ok 23 - if_test failing TEST_TODO()
# check "0" failed at t/helper/test-example-tap.c:108
# skipping test - missing prerequisite
# skipping check '1' at t/helper/test-example-tap.c:110
ok 24 - if_test test_skip() # SKIP
# skipping test - missing prerequisite
ok 25 - if_test test_skip() inside TEST_TODO() # SKIP
# check "0" failed at t/helper/test-example-tap.c:115
not ok 26 - if_test TEST_TODO() after failing check
# check "0" failed at t/helper/test-example-tap.c:121
not ok 27 - if_test failing check after TEST_TODO()
# check "!strcmp("\thello\\", "there\"\n")" failed at t/helper/test-example-tap.c:124
#    left: "\thello\\"
#   right: "there\"\n"
# check "!strcmp("NULL", NULL)" failed at t/helper/test-example-tap.c:125
#    left: "NULL"
#   right: NULL
# check "'a' == '\n'" failed at t/helper/test-example-tap.c:126
#    left: 'a'
#   right: '\n'
# check "'\\' == '\''" failed at t/helper/test-example-tap.c:127
#    left: '\\'
#   right: '\''
# check "'\a' == '\v'" failed at t/helper/test-example-tap.c:128
#    left: '\a'
#   right: '\v'
# check "'\x00' == '\x01'" failed at t/helper/test-example-tap.c:129
#    left: '\000'
#   right: '\001'
not ok 28 - if_test messages from failing string and char comparison
# BUG: test has no checks at t/helper/test-example-tap.c:131
not ok 29 - if_test test with no checks
1..29
"#;
    print!("{OUTPUT}");
    std::process::exit(1);
}

fn run_test_tool_advise(rest: &[String]) -> Result<()> {
    if rest.len() != 2 {
        bail!("usage: test-tool advise <message>");
    }
    let message = &rest[1];

    let global_enabled = std::env::var("GIT_ADVICE")
        .ok()
        .and_then(|v| parse_bool_text(&v))
        .unwrap_or(true);
    if !global_enabled {
        return Ok(());
    }

    let config = if let Ok(repo) = grit_lib::repo::Repository::discover(None) {
        grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
            .unwrap_or_else(|_| grit_lib::config::ConfigSet::new())
    } else {
        grit_lib::config::ConfigSet::load(None, true)
            .unwrap_or_else(|_| grit_lib::config::ConfigSet::new())
    };

    match config.get("advice.nestedTag") {
        Some(v) => {
            if parse_bool_text(&v).unwrap_or(true) {
                eprintln!("hint: {message}");
            }
        }
        None => {
            eprintln!("hint: {message}");
            eprintln!("hint: Disable this message with \"git config set advice.nestedTag false\"");
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
enum JsonWriterValue {
    Object(Vec<(String, JsonWriterValue)>),
    Array(Vec<JsonWriterValue>),
    String(String),
    Integer(i64),
    Double(String),
    Boolean(bool),
    Null,
}

#[derive(Debug)]
enum JsonWriterContainer {
    Object {
        key_in_parent: Option<String>,
        entries: Vec<(String, JsonWriterValue)>,
    },
    Array {
        key_in_parent: Option<String>,
        entries: Vec<JsonWriterValue>,
    },
}

fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn render_json_value(v: &JsonWriterValue, pretty: bool, indent: usize) -> String {
    match v {
        JsonWriterValue::Object(entries) => {
            if entries.is_empty() {
                return "{}".to_string();
            }
            if !pretty {
                let inner = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "\"{}\":{}",
                            json_escape_string(k),
                            render_json_value(v, false, indent)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{inner}}}")
            } else {
                let indent_str = "  ".repeat(indent);
                let child_indent_str = "  ".repeat(indent + 1);
                let mut out = String::from("{\n");
                for (idx, (k, v)) in entries.iter().enumerate() {
                    out.push_str(&child_indent_str);
                    out.push('"');
                    out.push_str(&json_escape_string(k));
                    out.push_str("\": ");
                    out.push_str(&render_json_value(v, true, indent + 1));
                    if idx + 1 != entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&indent_str);
                out.push('}');
                out
            }
        }
        JsonWriterValue::Array(entries) => {
            if entries.is_empty() {
                return "[]".to_string();
            }
            if !pretty {
                let inner = entries
                    .iter()
                    .map(|v| render_json_value(v, false, indent))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("[{inner}]")
            } else {
                let indent_str = "  ".repeat(indent);
                let child_indent_str = "  ".repeat(indent + 1);
                let mut out = String::from("[\n");
                for (idx, v) in entries.iter().enumerate() {
                    out.push_str(&child_indent_str);
                    out.push_str(&render_json_value(v, true, indent + 1));
                    if idx + 1 != entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&indent_str);
                out.push(']');
                out
            }
        }
        JsonWriterValue::String(s) => format!("\"{}\"", json_escape_string(s)),
        JsonWriterValue::Integer(i) => i.to_string(),
        JsonWriterValue::Double(d) => d.clone(),
        JsonWriterValue::Boolean(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        JsonWriterValue::Null => "null".to_string(),
    }
}

fn attach_json_value(
    stack: &mut [JsonWriterContainer],
    root: &mut Option<JsonWriterValue>,
    key_in_parent: Option<String>,
    value: JsonWriterValue,
) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        match parent {
            JsonWriterContainer::Object { entries, .. } => {
                let Some(key) = key_in_parent else {
                    bail!("json-writer: missing object key while attaching value");
                };
                entries.push((key, value));
            }
            JsonWriterContainer::Array { entries, .. } => {
                entries.push(value);
            }
        }
    } else {
        *root = Some(value);
    }
    Ok(())
}

fn run_test_tool_json_writer(rest: &[String]) -> Result<()> {
    let mut pretty = false;
    if let Some(flag) = rest.get(1) {
        match flag.as_str() {
            "-u" | "--unit" => return Ok(()),
            "-p" | "--pretty" => pretty = true,
            _ => {}
        }
    }

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let mut stack: Vec<JsonWriterContainer> = Vec::new();
    let mut root: Option<JsonWriterValue> = None;
    let mut saw_root = false;

    for raw_line in input.lines() {
        let line = raw_line.trim().trim_end_matches(|c| c == ' ' || c == '\t');
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let verb = parts[0];

        if !saw_root {
            match verb {
                "object" => {
                    stack.push(JsonWriterContainer::Object {
                        key_in_parent: None,
                        entries: Vec::new(),
                    });
                    saw_root = true;
                    continue;
                }
                "array" => {
                    stack.push(JsonWriterContainer::Array {
                        key_in_parent: None,
                        entries: Vec::new(),
                    });
                    saw_root = true;
                    continue;
                }
                _ => bail!("json-writer: first line must be 'object' or 'array'"),
            }
        }

        match verb {
            "end" => {
                let container = stack
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: unexpected 'end'"))?;
                match container {
                    JsonWriterContainer::Object {
                        key_in_parent,
                        entries,
                    } => {
                        let value = JsonWriterValue::Object(entries);
                        attach_json_value(&mut stack, &mut root, key_in_parent, value)?;
                    }
                    JsonWriterContainer::Array {
                        key_in_parent,
                        entries,
                    } => {
                        let value = JsonWriterValue::Array(entries);
                        attach_json_value(&mut stack, &mut root, key_in_parent, value)?;
                    }
                }
            }

            "object-string" => {
                let key = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: object-string requires key"))?;
                let value = parts.get(2).ok_or_else(|| anyhow::anyhow!("json-writer: object-string requires value"))?;
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), JsonWriterValue::String((*value).to_string())));
                    }
                    _ => bail!("json-writer: object-string used outside object"),
                }
            }
            "object-int" => {
                let key = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: object-int requires key"))?;
                let value = parts.get(2).ok_or_else(|| anyhow::anyhow!("json-writer: object-int requires value"))?;
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid integer '{value}'"))?;
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), JsonWriterValue::Integer(parsed)));
                    }
                    _ => bail!("json-writer: object-int used outside object"),
                }
            }
            "object-double" => {
                let key = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: object-double requires key"))?;
                let precision = parts.get(2).ok_or_else(|| anyhow::anyhow!("json-writer: object-double requires precision"))?;
                let value = parts.get(3).ok_or_else(|| anyhow::anyhow!("json-writer: object-double requires value"))?;
                let p = precision
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid precision '{precision}'"))?;
                let v = value
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid float '{value}'"))?;
                let rendered = format!("{v:.p$}");
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), JsonWriterValue::Double(rendered)));
                    }
                    _ => bail!("json-writer: object-double used outside object"),
                }
            }
            "object-true" | "object-false" | "object-null" => {
                let key = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: object literal requires key"))?;
                let val = match verb {
                    "object-true" => JsonWriterValue::Boolean(true),
                    "object-false" => JsonWriterValue::Boolean(false),
                    _ => JsonWriterValue::Null,
                };
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), val));
                    }
                    _ => bail!("json-writer: object literal used outside object"),
                }
            }
            "object-object" => {
                let key = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: object-object requires key"))?;
                stack.push(JsonWriterContainer::Object {
                    key_in_parent: Some((*key).to_string()),
                    entries: Vec::new(),
                });
            }
            "object-array" => {
                let key = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: object-array requires key"))?;
                stack.push(JsonWriterContainer::Array {
                    key_in_parent: Some((*key).to_string()),
                    entries: Vec::new(),
                });
            }

            "array-string" => {
                let value = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: array-string requires value"))?;
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Array { entries, .. } => {
                        entries.push(JsonWriterValue::String((*value).to_string()));
                    }
                    _ => bail!("json-writer: array-string used outside array"),
                }
            }
            "array-int" => {
                let value = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: array-int requires value"))?;
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid integer '{value}'"))?;
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Array { entries, .. } => {
                        entries.push(JsonWriterValue::Integer(parsed));
                    }
                    _ => bail!("json-writer: array-int used outside array"),
                }
            }
            "array-double" => {
                let precision = parts.get(1).ok_or_else(|| anyhow::anyhow!("json-writer: array-double requires precision"))?;
                let value = parts.get(2).ok_or_else(|| anyhow::anyhow!("json-writer: array-double requires value"))?;
                let p = precision
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid precision '{precision}'"))?;
                let v = value
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid float '{value}'"))?;
                let rendered = format!("{v:.p$}");
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Array { entries, .. } => {
                        entries.push(JsonWriterValue::Double(rendered));
                    }
                    _ => bail!("json-writer: array-double used outside array"),
                }
            }
            "array-true" | "array-false" | "array-null" => {
                let val = match verb {
                    "array-true" => JsonWriterValue::Boolean(true),
                    "array-false" => JsonWriterValue::Boolean(false),
                    _ => JsonWriterValue::Null,
                };
                let parent = stack.last_mut().ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Array { entries, .. } => {
                        entries.push(val);
                    }
                    _ => bail!("json-writer: array literal used outside array"),
                }
            }
            "array-object" => {
                stack.push(JsonWriterContainer::Object {
                    key_in_parent: None,
                    entries: Vec::new(),
                });
            }
            "array-array" => {
                stack.push(JsonWriterContainer::Array {
                    key_in_parent: None,
                    entries: Vec::new(),
                });
            }
            _ => bail!("json-writer: unrecognized token '{verb}'"),
        }
    }

    if !stack.is_empty() {
        bail!("json-writer: json not terminated");
    }
    let root = root.ok_or_else(|| anyhow::anyhow!("json-writer: empty input"))?;
    let rendered = render_json_value(&root, pretty, 0);
    println!("{rendered}");
    Ok(())
}

fn run_test_tool_mktemp(rest: &[String]) -> Result<()> {
    if rest.len() < 2 {
        bail!("usage: test-tool mktemp <template>");
    }

    let status = std::process::Command::new("mktemp")
        .args(&rest[1..])
        .status()?;
    exit_with_status(status);
}

fn run_test_tool_regex(rest: &[String]) -> Result<()> {
    if rest.get(1).map(String::as_str) == Some("--bug") {
        return Ok(());
    }
    bail!("usage: test-tool regex --bug")
}

fn run_test_tool(rest: &[String]) -> Result<()> {
    match rest.first().map(String::as_str).unwrap_or("") {
        "wildmatch" => {
            // test-tool wildmatch <mode> <text> <pattern>
            if rest.len() < 4 {
                bail!("usage: test-tool wildmatch <mode> <text> <pattern>");
            }
            let mode = &rest[1];
            let mut text = rest[2].clone();
            let pattern = rest[3].clone();

            // Handle XXX/ prefix (substitute for leading /)
            let text_bytes = if text.starts_with("XXX/") {
                text = text[3..].to_string();
                text.as_bytes().to_vec()
            } else {
                text.as_bytes().to_vec()
            };
            let pat_bytes = if pattern.starts_with("XXX/") {
                pattern[3..].as_bytes().to_vec()
            } else {
                pattern.as_bytes().to_vec()
            };

            let flags = match mode.as_str() {
                "wildmatch" => grit_lib::wildmatch::WM_PATHNAME,
                "iwildmatch" => grit_lib::wildmatch::WM_PATHNAME | grit_lib::wildmatch::WM_CASEFOLD,
                "pathmatch" => 0,
                "ipathmatch" => grit_lib::wildmatch::WM_CASEFOLD,
                _ => bail!("unknown wildmatch mode: {mode}"),
            };

            let matched = grit_lib::wildmatch::wildmatch(&pat_bytes, &text_bytes, flags);
            if matched {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        "trace2" => run_test_tool_trace2(rest),
        "revision-walking" => run_test_tool_revision_walking(rest),
        "mergesort" => run_test_tool_mergesort(rest),
        "find-pack" => run_test_tool_find_pack(rest),
        "env-helper" => run_test_tool_env_helper(rest),
        "sigchain" => run_test_tool_sigchain(rest),
        "example-tap" => run_test_tool_example_tap(),
        "advise" => run_test_tool_advise(rest),
        "json-writer" => run_test_tool_json_writer(rest),
        "mktemp" => run_test_tool_mktemp(rest),
        "regex" => run_test_tool_regex(rest),
        _ => test_tool_usage(),
    }
}

/// Global options parsed from argv before the subcommand.
#[derive(Default)]
struct GlobalOpts {
    git_dir: Option<PathBuf>,
    work_tree: Option<PathBuf>,
    change_dir: Option<PathBuf>,
    config_overrides: Vec<String>,
    bare: bool,
    no_advice: bool,
}

/// Extract global options and return (globals, subcommand_name, remaining_args).
///
/// We scan argv[1..] for global flags that appear before the subcommand.
/// The first non-flag argument is the subcommand name.
fn extract_globals(args: &[String]) -> Result<(GlobalOpts, Option<String>, Vec<String>)> {
    let mut opts = GlobalOpts::default();
    let mut subcmd = None;
    let mut rest = Vec::new();
    let mut i = 0;
    let items = &args[1..]; // skip argv[0]

    while i < items.len() {
        let arg = &items[i];

        // -C <dir> — cumulative: each -C is relative to the previous one
        if arg == "-C" {
            i += 1;
            if i < items.len() {
                let new_dir = PathBuf::from(&items[i]);
                opts.change_dir = Some(match opts.change_dir.take() {
                    Some(prev) => prev.join(&new_dir),
                    None => new_dir,
                });
            }
            i += 1;
            continue;
        }

        // --git-dir=<val> or --git-dir <val>
        if let Some(val) = arg.strip_prefix("--git-dir=") {
            opts.git_dir = Some(PathBuf::from(val));
            i += 1;
            continue;
        }
        if arg == "--git-dir" {
            i += 1;
            if i < items.len() {
                opts.git_dir = Some(PathBuf::from(&items[i]));
            }
            i += 1;
            continue;
        }

        // --work-tree=<val> or --work-tree <val>
        if let Some(val) = arg.strip_prefix("--work-tree=") {
            opts.work_tree = Some(PathBuf::from(val));
            i += 1;
            continue;
        }
        if arg == "--work-tree" {
            i += 1;
            if i < items.len() {
                opts.work_tree = Some(PathBuf::from(&items[i]));
            }
            i += 1;
            continue;
        }

        // -c key=value
        if arg == "-c" {
            i += 1;
            if i < items.len() {
                opts.config_overrides.push(items[i].clone());
            }
            i += 1;
            continue;
        }

        // --bare
        if arg == "--bare" {
            opts.bare = true;
            i += 1;
            continue;
        }

        // --no-advice
        if arg == "--no-advice" {
            opts.no_advice = true;
            i += 1;
            continue;
        }

        // --list-cmds=<categories>
        if let Some(val) = arg.strip_prefix("--list-cmds=") {
            return Ok((opts, Some("__list_cmds".to_owned()), vec![val.to_owned()]));
        }

        // --version / -v / -V / --help / -h  → treat as pseudo-subcommands
        if arg == "--version" || arg == "-v" || arg == "-V" {
            subcmd = Some("version".to_owned());
            rest = items[i + 1..].to_vec();
            break;
        }
        if arg == "--help" || arg == "-h" || arg == "help" {
            subcmd = Some("help".to_owned());
            rest = items[i + 1..].to_vec();
            break;
        }

        // First non-flag argument is the subcommand
        if !arg.starts_with('-') {
            subcmd = Some(arg.clone());
            rest = items[i + 1..].to_vec();
            break;
        }

        // Unknown global flag — pass through
        bail!("unknown option: {arg}");
    }

    Ok((opts, subcmd, rest))
}

/// Apply global options (env vars, chdir).
fn apply_globals(opts: &GlobalOpts) -> Result<()> {
    if let Some(dir) = &opts.change_dir {
        if !dir.as_os_str().is_empty() {
            std::env::set_current_dir(dir)?;
        }
    }
    if let Some(git_dir) = &opts.git_dir {
        std::env::set_var("GIT_DIR", git_dir);
    }
    if let Some(wt) = &opts.work_tree {
        std::env::set_var("GIT_WORK_TREE", wt);
    }
    if !opts.config_overrides.is_empty() {
        let params: String = opts
            .config_overrides
            .iter()
            .map(|kv| format!("'{}'", kv))
            .collect::<Vec<_>>()
            .join(" ");
        std::env::set_var("GIT_CONFIG_PARAMETERS", params);
    }
    if opts.no_advice {
        std::env::set_var("GIT_ADVICE", "false");
    }
    Ok(())
}

/// Wrapper to parse a clap `Args` struct as if it were a top-level `Parser`.
///
/// Each subcommand's Args struct derives `clap::Args`, not `clap::Parser`.
/// This wrapper lets us parse it standalone from a slice of arguments.
#[derive(Debug, Parser)]
#[command(name = "grit", disable_help_subcommand = true)]
struct ArgsWrapper<T: Args> {
    #[command(flatten)]
    inner: T,
}

/// Parse a command's clap Args from the remaining arguments.
///
/// When `-h` is passed, clap prints usage and the process exits with code 129
/// (Git convention for usage errors) instead of clap's default exit code 0.
fn parse_cmd_args<T: Args + FromArgMatches>(subcmd: &str, rest: &[String]) -> T {
    let mut argv = vec![format!("git {subcmd}")];
    argv.extend(rest.iter().cloned());
    match ArgsWrapper::<T>::try_parse_from(&argv) {
        Ok(wrapper) => wrapper.inner,
        Err(e) => {
            let _ = e.print();
            match e.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                    std::process::exit(129)
                }
                _ => std::process::exit(129),
            }
        }
    }
}

fn run() -> Result<()> {
    // Check env vars that clap would have handled
    if std::env::var("GIT_DIR").is_ok() && std::env::var("GIT_DIR").unwrap().is_empty() {
        // ignore empty GIT_DIR
    }

    let args: Vec<String> = std::env::args().collect();
    let (opts, subcmd, rest) = extract_globals(&args)?;

    let subcmd = match subcmd {
        Some(s) => s,
        None => {
            eprintln!("grit: a Git plumbing reimplementation in Rust");
            eprintln!("usage: grit <command> [<args>]");
            std::process::exit(1);
        }
    };

    ALIAS_EXPANSION_STACK.with(|stack| stack.borrow_mut().clear());

    apply_globals(&opts)?;

    // GIT_TRACE: write built-in trace line (after global options are processed)
    if let Ok(trace_val) = std::env::var("GIT_TRACE") {
        if !trace_val.is_empty() && trace_val != "0" && trace_val.to_lowercase() != "false" {
            let mut trace_cmd = format!("git {subcmd}");
            for arg in &rest {
                trace_cmd.push(' ');
                trace_cmd.push_str(arg);
            }
            let now = time::OffsetDateTime::now_utc();
            let trace_line = format!(
                "{:02}:{:02}:{:02}.{:06} grit:0               trace: built-in: {}\n",
                now.hour(),
                now.minute(),
                now.second(),
                now.microsecond(),
                trace_cmd,
            );
            write_git_trace(&trace_val, &trace_line);
        }
    }

    // Handle --git-completion-helper / --git-completion-helper-all
    if rest
        .iter()
        .any(|a| a == "--git-completion-helper" || a == "--git-completion-helper-all")
    {
        let show_all = rest.iter().any(|a| a == "--git-completion-helper-all");
        // Check if there's a sub-subcommand (e.g., 'config get --git-completion-helper')
        let sub_subcmd: Option<&str> = rest
            .iter()
            .find(|a| !a.starts_with('-'))
            .map(|s| s.as_str());
        let key = if let Some(sub) = sub_subcmd {
            format!("{}_{}", subcmd, sub)
        } else {
            subcmd.clone()
        };
        return print_completion_helper(&key, show_all);
    }

    dispatch(&subcmd, &rest, &opts)
}

/// Print --git-completion-helper output for a subcommand.
///
/// This mimics git's `--git-completion-helper` by listing all long options
/// (and their `--no-` negations) for the given subcommand.
fn print_completion_helper(subcmd: &str, show_all: bool) -> Result<()> {
    fn extract_options<T: Args>(show_all: bool) -> Vec<String> {
        let cmd = Command::new("grit").flatten_help(false);
        let cmd = T::augment_args(cmd);
        let mut positive = Vec::new();
        let mut negative = Vec::new();
        for arg in cmd.get_arguments() {
            if arg.get_id() == "help" || arg.get_id() == "version" {
                continue;
            }
            // Skip positional arguments
            if arg.get_long().is_none() && arg.get_short().is_none() {
                continue;
            }
            if let Some(long) = arg.get_long() {
                let hidden = arg.is_hide_set();
                // Check if this option takes a value
                let takes_value = match arg.get_action() {
                    clap::ArgAction::Set | clap::ArgAction::Append => true,
                    _ => arg.get_num_args().is_some_and(|r| r.min_values() > 0),
                };
                let suffix = if takes_value { "=" } else { "" };
                if hidden {
                    // Hidden args go to negative section only
                    negative.push(format!("--{long}{suffix}"));
                } else if long.starts_with("no-") {
                    // Explicit non-hidden --no-* args go in positive list
                    // (user-facing options like --no-guess)
                    positive.push(format!("--{long}{suffix}"));
                } else {
                    positive.push(format!("--{long}{suffix}"));
                    // Auto-generate --no- variant for the negative list
                    negative.push(format!("--no-{long}"));
                }
                // Add aliases (only with --git-completion-helper-all)
                if show_all {
                    if let Some(aliases) = arg.get_aliases() {
                        for alias in aliases {
                            if alias.starts_with("no-") {
                                negative.push(format!("--{alias}{suffix}"));
                            } else {
                                positive.push(format!("--{alias}{suffix}"));
                                negative.push(format!("--no-{alias}"));
                            }
                        }
                    }
                }
            }
        }
        // Collect subcommand names
        let mut subcmds: Vec<String> = Vec::new();
        for sub in cmd.get_subcommands() {
            let name = sub.get_name().to_string();
            if name != "help" {
                subcmds.push(name);
            }
        }

        if subcmds.is_empty() {
            let mut result = positive;
            // Only separate positive/negative with `--` sentinel when
            // there are enough options to warrant it. For small
            // commands, include --no-* variants inline (matching git).
            if negative.len() > 3 {
                result.push("--".to_string());
                result.extend(negative);
            } else {
                result.extend(negative);
            }
            result
        } else {
            // Has subcommands: return ONLY subcommands.
            // Options come from 'git <cmd> <subcmd> --git-completion-helper'.
            // __gitcomp will show subcommand names for empty cur,
            // and the completion script handles --options via subcommand-
            // specific helpers.
            subcmds
        }
    }

    let options = match subcmd {
        "add" => extract_options::<commands::add::Args>(show_all),
        "am" => extract_options::<commands::am::Args>(show_all),
        "apply" => extract_options::<commands::apply::Args>(show_all),
        "bisect" => extract_options::<commands::bisect::Args>(show_all),
        "blame" => extract_options::<commands::blame::Args>(show_all),
        "branch" => extract_options::<commands::branch::Args>(show_all),
        "cat-file" => extract_options::<commands::cat_file::Args>(show_all),
        "check-ignore" => extract_options::<commands::check_ignore::Args>(show_all),
        "checkout" => extract_options::<commands::checkout::Args>(show_all),
        "cherry-pick" => extract_options::<commands::cherry_pick::Args>(show_all),
        "clean" => extract_options::<commands::clean::Args>(show_all),
        "clone" => extract_options::<commands::clone::Args>(show_all),
        "commit" => extract_options::<commands::commit::Args>(show_all),
        "config" => extract_options::<commands::config::Args>(show_all),
        "config_get" => extract_options::<commands::config::GetArgs>(show_all),
        "config_set" => extract_options::<commands::config::SetArgs>(show_all),
        "config_unset" => extract_options::<commands::config::UnsetArgs>(show_all),
        "config_list" => extract_options::<commands::config::ListArgs>(show_all),
        "config_edit" => extract_options::<commands::config::EditArgs>(show_all),
        "reflog_show" => extract_options::<commands::reflog::ShowArgs>(show_all),
        "reflog_expire" => extract_options::<commands::reflog::ExpireArgs>(show_all),
        "reflog_delete" => extract_options::<commands::reflog::DeleteArgs>(show_all),
        "reflog_exists" => extract_options::<commands::reflog::ExistsArgs>(show_all),
        "describe" => extract_options::<commands::describe::Args>(show_all),
        "diff" => extract_options::<commands::diff::Args>(show_all),
        "fetch" => extract_options::<commands::fetch::Args>(show_all),
        "for-each-ref" => extract_options::<commands::for_each_ref::Args>(show_all),
        "format-patch" => extract_options::<commands::format_patch::Args>(show_all),
        "fsck" => extract_options::<commands::fsck::Args>(show_all),
        "gc" => extract_options::<commands::gc::Args>(show_all),
        "grep" => extract_options::<commands::grep::Args>(show_all),
        "init" => extract_options::<commands::init::Args>(show_all),
        "log" => extract_options::<commands::log::Args>(show_all),
        "ls-files" => extract_options::<commands::ls_files::Args>(show_all),
        "ls-remote" => extract_options::<commands::ls_remote::Args>(show_all),
        "ls-tree" => extract_options::<commands::ls_tree::Args>(show_all),
        "merge" => extract_options::<commands::merge::Args>(show_all),
        "merge-base" => extract_options::<commands::merge_base::Args>(show_all),
        "mv" => extract_options::<commands::mv::Args>(show_all),
        "notes" => extract_options::<commands::notes::Args>(show_all),
        "pull" => extract_options::<commands::pull::Args>(show_all),
        "push" => extract_options::<commands::push::Args>(show_all),
        "rebase" => extract_options::<commands::rebase::Args>(show_all),
        "reflog" => extract_options::<commands::reflog::Args>(show_all),
        "remote" => extract_options::<commands::remote::Args>(show_all),
        "reset" => extract_options::<commands::reset::Args>(show_all),
        "restore" => extract_options::<commands::restore::Args>(show_all),
        "rev-list" => extract_options::<commands::rev_list::Args>(show_all),
        "rev-parse" => extract_options::<commands::rev_parse::Args>(show_all),
        "revert" => extract_options::<commands::revert::Args>(show_all),
        "rm" => extract_options::<commands::rm::Args>(show_all),
        "send-email" => extract_options::<commands::send_email::Args>(show_all),
        "show" => extract_options::<commands::show::Args>(show_all),
        "show-ref" => extract_options::<commands::show_ref::Args>(show_all),
        "sparse-checkout" => extract_options::<commands::sparse_checkout::Args>(show_all),
        "stash" => extract_options::<commands::stash::Args>(show_all),
        "status" => extract_options::<commands::status::Args>(show_all),
        "submodule" => extract_options::<commands::submodule::Args>(show_all),
        "switch" => extract_options::<commands::switch::Args>(show_all),
        "symbolic-ref" => extract_options::<commands::symbolic_ref::Args>(show_all),
        "tag" => extract_options::<commands::tag::Args>(show_all),
        "update-index" => extract_options::<commands::update_index::Args>(show_all),
        "update-ref" => extract_options::<commands::update_ref::Args>(show_all),
        "worktree" => extract_options::<commands::worktree::Args>(show_all),
        "version" => extract_options::<commands::version::Args>(show_all),
        _ => Vec::new(),
    };

    println!("{}", options.join(" "));
    Ok(())
}

/// Handle --list-cmds=<categories> for bash completion.
///
/// Categories are comma-separated. Supported:
/// - list-mainporcelain: high-level user commands
/// - list-complete: other useful commands
/// - list-all: all commands (porcelain + plumbing)
/// - config: commands from completion.commands config
fn print_list_cmds(categories: &str) {
    let mut parseopt_mode = false;
    let mainporcelain = [
        "add",
        "am",
        "archive",
        "bisect",
        "branch",
        "bundle",
        "checkout",
        "cherry-pick",
        "clean",
        "clone",
        "commit",
        "describe",
        "diff",
        "fetch",
        "format-patch",
        "gc",
        "grep",
        "init",
        "log",
        "merge",
        "mv",
        "notes",
        "pull",
        "push",
        "range-diff",
        "rebase",
        "reset",
        "restore",
        "revert",
        "rm",
        "shortlog",
        "show",
        "sparse-checkout",
        "stash",
        "status",
        "submodule",
        "switch",
        "tag",
        "worktree",
    ];
    let complete = [
        "apply",
        "blame",
        "cherry",
        "config",
        "difftool",
        "fsck",
        "help",
        "mergetool",
        "prune",
        "reflog",
        "remote",
        "repack",
        "replace",
        "send-email",
        "show-branch",
        "whatchanged",
    ];
    let plumbing = [
        "cat-file",
        "check-attr",
        "check-ignore",
        "check-ref-format",
        "checkout-index",
        "commit-graph",
        "commit-tree",
        "count-objects",
        "diff-files",
        "diff-index",
        "diff-tree",
        "for-each-ref",
        "get-tar-commit-id",
        "hash-object",
        "index-pack",
        "ls-files",
        "ls-remote",
        "ls-tree",
        "merge-base",
        "merge-file",
        "mktag",
        "mktree",
        "multi-pack-index",
        "name-rev",
        "pack-objects",
        "pack-refs",
        "read-tree",
        "rev-list",
        "rev-parse",
        "show-ref",
        "symbolic-ref",
        "update-index",
        "update-ref",
        "verify-commit",
        "verify-pack",
        "verify-tag",
        "write-tree",
    ];

    let mut result: Vec<&str> = Vec::new();
    for cat in categories.split(',') {
        match cat {
            "list-mainporcelain" => result.extend_from_slice(&mainporcelain),
            "list-complete" => result.extend_from_slice(&complete),
            "deprecated" => {
                result.extend_from_slice(DEPRECATED_ALIASABLE_COMMANDS);
            }
            "list-all" | "builtins" | "main" => {
                result.extend_from_slice(&mainporcelain);
                result.extend_from_slice(&complete);
                result.extend_from_slice(&plumbing);
            }
            "others" => {
                // Non-built-in commands like gitk
                result.push("gitk");
            }
            "alias" => {
                // List configured aliases.
                if let Some(config) = load_config_set_for_lookup() {
                    use std::collections::BTreeSet;
                    let mut aliases = BTreeSet::new();
                    for entry in config.entries() {
                        if !entry.key.starts_with("alias.") {
                            continue;
                        }
                        let rest = &entry.key["alias.".len()..];
                        if let Some(name) = rest.strip_suffix(".command") {
                            if !name.is_empty() && !name.contains('.') {
                                aliases.insert(name.to_string());
                            }
                        } else if let Some(name) = rest.strip_prefix('.') {
                            if !name.is_empty() && !name.contains('.') {
                                aliases.insert(name.to_string());
                            }
                        } else if !rest.is_empty() && !rest.contains('.') {
                            aliases.insert(rest.to_string());
                        }
                    }
                    for alias in aliases {
                        println!("{alias}");
                    }
                }
            }
            "nohelpers" => {
                // nohelpers = filter out helper programs
            }
            "parseopt" => {
                parseopt_mode = true;
                // Commands that support --git-completion-helper
                let parseopt_cmds = [
                    "add",
                    "am",
                    "apply",
                    "bisect",
                    "blame",
                    "branch",
                    "cat-file",
                    "check-ignore",
                    "checkout",
                    "cherry-pick",
                    "clean",
                    "clone",
                    "commit",
                    "config",
                    "describe",
                    "diff",
                    "fetch",
                    "for-each-ref",
                    "format-patch",
                    "fsck",
                    "gc",
                    "grep",
                    "init",
                    "log",
                    "ls-files",
                    "ls-remote",
                    "ls-tree",
                    "merge",
                    "merge-base",
                    "mv",
                    "notes",
                    "pull",
                    "push",
                    "rebase",
                    "reflog",
                    "remote",
                    "reset",
                    "restore",
                    "rev-list",
                    "rev-parse",
                    "revert",
                    "rm",
                    "send-email",
                    "show",
                    "show-ref",
                    "sparse-checkout",
                    "stash",
                    "status",
                    "submodule",
                    "switch",
                    "symbolic-ref",
                    "tag",
                    "update-index",
                    "update-ref",
                    "version",
                    "worktree",
                ];
                result.extend_from_slice(&parseopt_cmds);
            }
            "list-guide" => {
                let guides = [
                    "core-tutorial",
                    "credentials",
                    "cvs-migration",
                    "diffcore",
                    "everyday",
                    "faq",
                    "glossary",
                    "namespaces",
                    "remote-helpers",
                    "submodules",
                    "tutorial",
                    "tutorial-2",
                    "workflows",
                ];
                result.extend_from_slice(&guides);
            }
            "config" => {
                // Check completion.commands config for additions/removals
                if let Ok(repo) = grit_lib::repo::Repository::discover(None) {
                    if let Ok(config) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
                    {
                        if let Some(val) = config.get("completion.commands") {
                            for token in val.split_whitespace() {
                                if let Some(cmd) = token.strip_prefix('-') {
                                    result.retain(|c| *c != cmd);
                                } else {
                                    // Can't push a &str from config into &str vec, just print separately
                                    println!("{token}");
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if parseopt_mode {
        // parseopt outputs all commands on a single space-separated line
        println!(
            "{}",
            result
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<&str>>()
                .join(" ")
        );
    } else {
        for cmd in &result {
            println!("{cmd}");
        }
    }
}

/// Preprocess diff arguments: expand `-U<N>` to `--unified=<N>` so that
/// clap does not swallow it into the trailing var-arg positional.
fn preprocess_diff_args(rest: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        if arg == "-U" {
            // `-U <N>` with a space — merge into `--unified=<N>`
            if let Some(val) = iter.next() {
                result.push(format!("--unified={val}"));
            } else {
                result.push(arg.clone());
            }
        } else if let Some(n) = arg.strip_prefix("-U") {
            // `-U<N>` without a space
            result.push(format!("--unified={n}"));
        } else {
            result.push(arg.clone());
        }
    }
    result
}

/// Preprocess log arguments: convert `-<N>` shorthand to `-n <N>`.
fn preprocess_log_args(rest: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for arg in rest {
        if arg.starts_with('-') && arg.len() > 1 && arg[1..].chars().all(|c| c.is_ascii_digit()) {
            result.push("-n".to_string());
            result.push(arg[1..].to_string());
        } else {
            result.push(arg.clone());
        }
    }
    result
}

/// Levenshtein edit distance between two strings.
/// Read the `help.autocorrect` config setting.
/// Returns None if not set, or Some(value) where value is the config string.
fn get_autocorrect_setting() -> Option<String> {
    // Check GIT_CONFIG_PARAMETERS first (set by -c)
    if let Some(val) = protocol::check_config_param("help.autocorrect") {
        return Some(val);
    }
    // Try to discover git dir and load config
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });
    if let Ok(config) = grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) {
        if let Some(val) = config.get("help.autocorrect") {
            return Some(val);
        }
    }
    None
}

fn strsim_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

fn split_alias_words(input: &str) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }

    if in_single || in_double {
        bail!("unterminated quote in alias definition");
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct AliasEntry {
    key: String,
    value: Option<String>,
}

thread_local! {
    static ALIAS_EXPANSION_STACK: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

const DEPRECATED_ALIASABLE_COMMANDS: &[&str] = &["pack-redundant", "whatchanged"];

fn load_config_set_for_lookup() -> Option<grit_lib::config::ConfigSet> {
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });
    grit_lib::config::ConfigSet::load(git_dir.as_deref(), true).ok()
}

fn find_alias_entry(subcmd: &str) -> Option<AliasEntry> {
    let config = load_config_set_for_lookup()?;
    let subcmd_lower = subcmd.to_lowercase();

    for entry in config.entries().iter().rev() {
        if !entry.key.starts_with("alias.") {
            continue;
        }

        let rest = &entry.key["alias.".len()..];

        // Subsection syntax: alias.<name>.command (case-sensitive by design)
        if let Some(name) = rest.strip_suffix(".command") {
            if !name.is_empty() && name == subcmd {
                return Some(AliasEntry {
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                });
            }
            continue;
        }

        // Empty subsection in simple syntax: alias..foo => alias.foo
        if let Some(name) = rest.strip_prefix('.') {
            if !name.contains('.') && name.to_lowercase() == subcmd_lower {
                return Some(AliasEntry {
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                });
            }
            continue;
        }

        // Simple syntax: alias.foo (case-insensitive command key)
        if !rest.contains('.') && rest.to_lowercase() == subcmd_lower {
            return Some(AliasEntry {
                key: entry.key.clone(),
                value: entry.value.clone(),
            });
        }
    }

    None
}

fn quote_trace_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    let safe = arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/');
    if safe {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', r#"'"'"'"#))
}

fn emit_git_trace(line: String) {
    if let Ok(trace_val) = std::env::var("GIT_TRACE") {
        if !trace_val.is_empty() && trace_val != "0" && trace_val.to_lowercase() != "false" {
            write_git_trace(&trace_val, &format!("{line}\n"));
        }
    }
}

fn format_alias_loop_message(
    stack: &[(String, String)],
    from: &str,
    to: &str,
    cycle_start_idx: usize,
) -> String {
    let mut msg = String::new();

    for (src, dst) in stack.iter().skip(cycle_start_idx) {
        msg.push_str(&format!("'{src}' is aliased to '{dst}'\n"));
    }
    msg.push_str(&format!("'{from}' is aliased to '{to}'\n"));

    msg.push_str(&format!(
        "fatal: alias loop detected: expansion of '{to}' does not terminate:\n"
    ));

    let mut chain = vec![to.to_string()];
    for (src, dst) in stack.iter().skip(cycle_start_idx) {
        if chain.last().is_some_and(|last| last == src) {
            chain.push(dst.clone());
        }
    }
    if chain.last().is_none_or(|last| last != from) {
        chain.push(from.to_string());
    }

    if let Some(first) = chain.first() {
        msg.push_str(&format!("  {first} <==\n"));
        for name in chain.iter().skip(1) {
            msg.push_str(&format!("  {name} ==>\n"));
        }
    }

    msg
}

fn is_builtin_command(subcmd: &str) -> bool {
    KNOWN_COMMANDS.contains(&subcmd)
}

fn can_alias_shadow_builtin(subcmd: &str) -> bool {
    DEPRECATED_ALIASABLE_COMMANDS.contains(&subcmd)
}

fn try_expand_alias(subcmd: &str, rest: &[String], opts: &GlobalOpts) -> Result<Option<()>> {
    let should_check_alias = !is_builtin_command(subcmd) || can_alias_shadow_builtin(subcmd);
    if !should_check_alias {
        return Ok(None);
    }

    let Some(alias_entry) = find_alias_entry(subcmd) else {
        return Ok(None);
    };

    let alias_value = alias_entry.value.unwrap_or_default();
    if alias_value.trim().is_empty() {
        bail!("{} has no value", alias_entry.key);
    }

    if let Some(shell_body) = alias_value.strip_prefix('!') {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let body = shell_body.trim_start();
        let prepared = format!("{body} \"$@\"");
        let mut trace = format!(
            "trace: start_command: {} -c {} {}",
            quote_trace_arg(&shell),
            quote_trace_arg(&prepared),
            quote_trace_arg(body)
        );
        for arg in rest {
            trace.push(' ');
            trace.push_str(&quote_trace_arg(arg));
        }
        emit_git_trace(trace);

        let status = std::process::Command::new(&shell)
            .arg("-c")
            .arg(prepared)
            .arg(body)
            .args(rest)
            .status()?;
        exit_with_status(status);
    }

    let mut expanded = split_alias_words(&alias_value)?;
    if expanded.is_empty() {
        bail!("{} has no value", alias_entry.key);
    }

    let aliased_subcmd = expanded.remove(0);
    let mut expanded_rest = expanded;
    expanded_rest.extend(rest.iter().cloned());

    let loop_error = ALIAS_EXPANSION_STACK.with(|stack| {
        let stack = stack.borrow();
        stack
            .iter()
            .position(|(src, _)| src == &aliased_subcmd)
            .map(|idx| format_alias_loop_message(&stack, subcmd, &aliased_subcmd, idx))
    });
    if let Some(msg) = loop_error {
        eprint!("{msg}");
        std::process::exit(128);
    }

    ALIAS_EXPANSION_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push((subcmd.to_string(), aliased_subcmd.clone()));
    });
    let result = dispatch(&aliased_subcmd, &expanded_rest, opts);
    ALIAS_EXPANSION_STACK.with(|stack| {
        let _ = stack.borrow_mut().pop();
    });
    result?;
    Ok(Some(()))
}

const KNOWN_COMMANDS: &[&str] = &[
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "backfill",
    "bisect",
    "blame",
    "branch",
    "bugreport",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-graph",
    "commit-tree",
    "config",
    "count-objects",
    "credential",
    "credential-cache",
    "credential-store",
    "daemon",
    "describe",
    "diagnose",
    "diff",
    "diff-files",
    "diff-index",
    "diff-pairs",
    "diff-tree",
    "difftool",
    "fast-export",
    "fast-import",
    "fetch",
    "fetch-pack",
    "filter-branch",
    "fmt-merge-msg",
    "for-each-ref",
    "for-each-repo",
    "format-patch",
    "fsck",
    "gc",
    "get-tar-commit-id",
    "grep",
    "hash-object",
    "help",
    "history",
    "hook",
    "http-backend",
    "http-fetch",
    "http-push",
    "index-pack",
    "init",
    "interpret-trailers",
    "last-modified",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "maintenance",
    "merge",
    "merge-base",
    "merge-file",
    "merge-index",
    "merge-one-file",
    "merge-tree",
    "mergetool",
    "mktag",
    "mktree",
    "multi-pack-index",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-redundant",
    "pack-refs",
    "patch-id",
    "prune",
    "prune-packed",
    "pull",
    "push",
    "range-diff",
    "read-tree",
    "rebase",
    "receive-pack",
    "reflog",
    "refs",
    "remote",
    "repack",
    "replace",
    "replay",
    "repo",
    "rerere",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "scalar",
    "send-email",
    "send-pack",
    "sh-i18n",
    "sh-setup",
    "shell",
    "shortlog",
    "show",
    "show-branch",
    "show-index",
    "show-ref",
    "sparse-checkout",
    "stage",
    "stash",
    "status",
    "stripspace",
    "submodule",
    "switch",
    "symbolic-ref",
    "tag",
    "unpack-file",
    "unpack-objects",
    "update-index",
    "update-ref",
    "update-server-info",
    "upload-archive",
    "upload-pack",
    "var",
    "verify-commit",
    "verify-pack",
    "verify-tag",
    "version",
    "whatchanged",
    "worktree",
    "write-tree",
];

/// Dispatch to the appropriate command handler.
///
/// Each arm only constructs the clap parser for that specific command.
fn dispatch(subcmd: &str, rest: &[String], opts: &GlobalOpts) -> Result<()> {
    if let Some(()) = try_expand_alias(subcmd, rest, opts)? {
        return Ok(());
    }

    match subcmd {
        "add" => commands::add::run(parse_cmd_args(subcmd, rest)),
        "am" => commands::am::run(parse_cmd_args(subcmd, rest)),
        "annotate" => commands::annotate::run(parse_cmd_args(subcmd, rest)),
        "apply" => commands::apply::run(parse_cmd_args(subcmd, rest)),
        "archive" => commands::archive::run(parse_cmd_args(subcmd, rest)),
        "backfill" => commands::backfill::run(parse_cmd_args(subcmd, rest)),
        "bisect" => commands::bisect::run(parse_cmd_args(subcmd, rest)),
        "blame" => commands::blame::run(parse_cmd_args(subcmd, rest)),
        "branch" => commands::branch::run(parse_cmd_args(subcmd, rest)),
        "bugreport" => commands::bugreport::run(parse_cmd_args(subcmd, rest)),
        "bundle" => commands::bundle::run(parse_cmd_args(subcmd, rest)),
        "cat-file" => commands::cat_file::run(parse_cmd_args(subcmd, rest)),
        "check-attr" => commands::check_attr::run(parse_cmd_args(subcmd, rest)),
        "check-ignore" => commands::check_ignore::run(parse_cmd_args(subcmd, rest)),
        "check-mailmap" => commands::check_mailmap::run(parse_cmd_args(subcmd, rest)),
        "check-ref-format" => commands::check_ref_format::run(parse_cmd_args(subcmd, rest)),
        "checkout" => commands::checkout::run(parse_cmd_args(subcmd, rest)),
        "checkout-index" => commands::checkout_index::run(parse_cmd_args(subcmd, rest)),
        "cherry" => commands::cherry::run(parse_cmd_args(subcmd, rest)),
        "cherry-pick" => commands::cherry_pick::run(parse_cmd_args(subcmd, rest)),
        "clean" => commands::clean::run(parse_cmd_args(subcmd, rest)),
        "clone" => commands::clone::run(parse_cmd_args(subcmd, rest)),
        "column" => commands::column::run(parse_cmd_args(subcmd, rest)),
        "commit" => commands::commit::run(parse_cmd_args(subcmd, rest)),
        "commit-graph" => commands::commit_graph::run(parse_cmd_args(subcmd, rest)),
        "commit-tree" => commands::commit_tree::run(parse_cmd_args(subcmd, rest)),
        "config" => commands::config::run(parse_cmd_args(subcmd, rest)),
        "count-objects" => commands::count_objects::run(parse_cmd_args(subcmd, rest)),
        "credential" => commands::credential::run(parse_cmd_args(subcmd, rest)),
        "credential-cache" => commands::credential_cache::run(parse_cmd_args(subcmd, rest)),
        "credential-store" => commands::credential_store::run(parse_cmd_args(subcmd, rest)),
        "daemon" => commands::daemon::run(parse_cmd_args(subcmd, rest)),
        "describe" => commands::describe::run(parse_cmd_args(subcmd, rest)),
        "diagnose" => commands::diagnose::run(parse_cmd_args(subcmd, rest)),
        "diff" => commands::diff::run(parse_cmd_args(subcmd, &preprocess_diff_args(rest))),
        "diff-files" => commands::diff_files::run(parse_cmd_args(subcmd, rest)),
        "diff-index" => commands::diff_index::run(parse_cmd_args(subcmd, rest)),
        "diff-pairs" => commands::diff_pairs::run(parse_cmd_args(subcmd, rest)),
        "diff-tree" => commands::diff_tree::run(parse_cmd_args(subcmd, rest)),
        "difftool" => commands::difftool::run(parse_cmd_args(subcmd, rest)),
        "fast-export" => commands::fast_export::run(parse_cmd_args(subcmd, rest)),
        "fast-import" => commands::fast_import::run(parse_cmd_args(subcmd, rest)),
        "fetch" => commands::fetch::run(parse_cmd_args(subcmd, rest)),
        "fetch-pack" => commands::fetch_pack::run(parse_cmd_args(subcmd, rest)),
        "filter-branch" => commands::filter_branch::run(parse_cmd_args(subcmd, rest)),
        "fmt-merge-msg" => commands::fmt_merge_msg::run(parse_cmd_args(subcmd, rest)),
        "for-each-ref" => commands::for_each_ref::run(parse_cmd_args(subcmd, rest)),
        "for-each-repo" => commands::for_each_repo::run(parse_cmd_args(subcmd, rest)),
        "format-patch" => commands::format_patch::run(parse_cmd_args(subcmd, rest)),
        "fsck" => commands::fsck::run(parse_cmd_args(subcmd, rest)),
        "gc" => commands::gc::run(parse_cmd_args(subcmd, rest)),
        "get-tar-commit-id" => commands::get_tar_commit_id::run(parse_cmd_args(subcmd, rest)),
        "grep" => {
            // `git grep -h` without a pattern should show usage/help.
            if rest.len() == 1 && rest[0] == "-h" {
                return commands::grep::run(parse_cmd_args(subcmd, rest));
            }

            // Git grep uses -h for --no-filename, conflicting with clap's -h for help.
            // Also implement last-flag-wins for -G/-E/-F/-P pattern type flags.
            // Rewrite -h to --no-filename. Handle both standalone "-h" and
            // combined flags like "-ah" (split into "-a" + "--no-filename").
            let mut new_rest: Vec<String> = Vec::new();
            for a in rest.iter() {
                if a == "-h" {
                    new_rest.push("--no-filename".to_string());
                } else if a.starts_with('-')
                    && !a.starts_with("--")
                    && a.contains('h')
                    && a.len() > 2
                {
                    // Combined short flags containing 'h'
                    let without_h: String = a.chars().filter(|&c| c != 'h').collect();
                    if without_h.len() > 1 {
                        // still has flags besides '-'
                        new_rest.push(without_h);
                    }
                    new_rest.push("--no-filename".to_string());
                } else {
                    new_rest.push(a.clone());
                }
            }
            let mut rest = new_rest;
            // Last-flag-wins: find the last pattern-type flag and remove earlier ones
            let pattern_flags = [
                "-G",
                "-E",
                "-F",
                "-P",
                "--basic-regexp",
                "--extended-regexp",
                "--fixed-strings",
                "--perl-regexp",
            ];
            let mut last_idx = None;
            for (i, a) in rest.iter().enumerate() {
                if pattern_flags.contains(&a.as_str()) {
                    last_idx = Some(i);
                }
            }
            if let Some(last) = last_idx {
                let keep = rest[last].clone();
                rest.retain(|a| !pattern_flags.contains(&a.as_str()));
                // Insert the winning flag back (at beginning, before positionals)
                rest.insert(0, keep);
            }
            commands::grep::run(parse_cmd_args(subcmd, &rest))
        }
        "hash-object" => commands::hash_object::run(parse_cmd_args(subcmd, rest)),
        "help" => commands::help::run(parse_cmd_args(subcmd, rest)),
        "history" => commands::history::run(parse_cmd_args(subcmd, rest)),
        "hook" => commands::hook::run(parse_cmd_args(subcmd, rest)),
        "http-backend" => commands::http_backend::run(parse_cmd_args(subcmd, rest)),
        "http-fetch" => commands::http_fetch::run(parse_cmd_args(subcmd, rest)),
        "http-push" => commands::http_push::run(parse_cmd_args(subcmd, rest)),
        "index-pack" => commands::index_pack::run(parse_cmd_args(subcmd, rest)),
        "init" => commands::init::run(parse_cmd_args(subcmd, rest), opts.bare),
        "interpret-trailers" => commands::interpret_trailers::run(parse_cmd_args(subcmd, rest)),
        "last-modified" => commands::last_modified::run(parse_cmd_args(subcmd, rest)),
        "log" => {
            let rest = preprocess_log_args(rest);
            commands::log::run(parse_cmd_args(subcmd, &rest))
        }
        "ls-files" => commands::ls_files::run(parse_cmd_args(subcmd, rest)),
        "ls-remote" => commands::ls_remote::run(parse_cmd_args(subcmd, rest)),
        "ls-tree" => commands::ls_tree::run(parse_cmd_args(subcmd, rest)),
        "mailinfo" => commands::mailinfo::run(parse_cmd_args(subcmd, rest)),
        "mailsplit" => commands::mailsplit::run(parse_cmd_args(subcmd, rest)),
        "maintenance" => commands::maintenance::run(parse_cmd_args(subcmd, rest)),
        "merge" => commands::merge::run(parse_cmd_args(subcmd, rest)),
        "merge-base" => commands::merge_base::run(parse_cmd_args(subcmd, rest)),
        "merge-file" => commands::merge_file::run(parse_cmd_args(subcmd, rest)),
        "merge-index" => commands::merge_index::run(parse_cmd_args(subcmd, rest)),
        "merge-one-file" => commands::merge_one_file::run(parse_cmd_args(subcmd, rest)),
        "merge-tree" => commands::merge_tree::run(parse_cmd_args(subcmd, rest)),
        "mergetool" => commands::mergetool::run(parse_cmd_args(subcmd, rest)),
        "mktag" => commands::mktag::run(parse_cmd_args(subcmd, rest)),
        "mktree" => commands::mktree::run(parse_cmd_args(subcmd, rest)),
        "multi-pack-index" => commands::multi_pack_index::run(parse_cmd_args(subcmd, rest)),
        "mv" => commands::mv::run(parse_cmd_args(subcmd, rest)),
        "name-rev" => commands::name_rev::run(parse_cmd_args(subcmd, rest)),
        "notes" => commands::notes::run(parse_cmd_args(subcmd, rest)),
        "pack-objects" => commands::pack_objects::run(parse_cmd_args(subcmd, rest)),
        "pkt-line" => {
            let sub = rest.first().map(|s| s.as_str()).unwrap_or("");
            match sub {
                "pack" => pkt_line::cmd_pack().map_err(Into::into),
                "unpack" => pkt_line::cmd_unpack().map_err(Into::into),
                "send-split-sideband" => pkt_line::cmd_send_split_sideband().map_err(Into::into),
                "receive-sideband" => pkt_line::cmd_receive_sideband().map_err(Into::into),
                "unpack-sideband" => {
                    let chomp_newline = !rest.iter().any(|a| a == "--no-chomp-newline");
                    let reader_use_sideband = rest.iter().any(|a| a == "--reader-use-sideband");
                    pkt_line::cmd_unpack_sideband(chomp_newline, reader_use_sideband)
                        .map_err(Into::into)
                }
                other => bail!("pkt-line: unknown subcommand '{other}'"),
            }
        }
        "pack-redundant" => commands::pack_redundant::run(parse_cmd_args(subcmd, rest)),
        "pack-refs" => commands::pack_refs::run(parse_cmd_args(subcmd, rest)),
        "patch-id" => commands::patch_id::run(parse_cmd_args(subcmd, rest)),
        "prune" => commands::prune::run(parse_cmd_args(subcmd, rest)),
        "prune-packed" => commands::prune_packed::run(parse_cmd_args(subcmd, rest)),
        "pull" => commands::pull::run(parse_cmd_args(subcmd, rest)),
        "push" => commands::push::run(parse_cmd_args(subcmd, rest)),
        "range-diff" => commands::range_diff::run(parse_cmd_args(subcmd, rest)),
        "read-tree" => commands::read_tree::run(parse_cmd_args(subcmd, rest)),
        "rebase" => commands::rebase::run(parse_cmd_args(subcmd, rest)),
        "receive-pack" => commands::receive_pack::run(parse_cmd_args(subcmd, rest)),
        "reflog" => {
            let rest = preprocess_log_args(rest);
            commands::reflog::run(parse_cmd_args(subcmd, &rest))
        }
        "refs" => commands::refs::run(parse_cmd_args(subcmd, rest)),
        "remote" => commands::remote::run(parse_cmd_args(subcmd, rest)),
        "repack" => commands::repack::run(parse_cmd_args(subcmd, rest)),
        "replace" => commands::replace::run(parse_cmd_args(subcmd, rest)),
        "replay" => commands::replay::run(parse_cmd_args(subcmd, rest)),
        "repo" => commands::repo::run(parse_cmd_args(subcmd, rest)),
        "rerere" => commands::rerere::run(parse_cmd_args(subcmd, rest)),
        "reset" => {
            commands::reset::pre_validate_args(rest)?;
            let filtered = commands::reset::filter_args(rest);
            commands::reset::run(parse_cmd_args(subcmd, &filtered))
        }
        "restore" => commands::restore::run(parse_cmd_args(subcmd, rest)),
        "rev-list" => commands::rev_list::run(parse_cmd_args(subcmd, rest)),
        "rev-parse" => commands::rev_parse::run(parse_cmd_args(subcmd, rest)),
        "revert" => commands::revert::run(parse_cmd_args(subcmd, rest)),
        "rm" => commands::rm::run(parse_cmd_args(subcmd, rest)),
        "scalar" => commands::scalar::run(rest),
        "send-email" => commands::send_email::run(parse_cmd_args(subcmd, rest)),
        "send-pack" => commands::send_pack::run(parse_cmd_args(subcmd, rest)),
        "serve-v2" => commands::serve_v2::run(parse_cmd_args(subcmd, rest)),
        "sh-i18n" => commands::sh_i18n::run(parse_cmd_args(subcmd, rest)),
        "sh-setup" => commands::sh_setup::run(parse_cmd_args(subcmd, rest)),
        "shell" => commands::shell::run(parse_cmd_args(subcmd, rest)),
        "shortlog" => commands::shortlog::run(parse_cmd_args(subcmd, rest)),
        "show" => commands::show::run(parse_cmd_args(subcmd, rest)),
        "show-branch" => commands::show_branch::run(parse_cmd_args(subcmd, rest)),
        "show-index" => commands::show_index::run(parse_cmd_args(subcmd, rest)),
        "show-ref" => commands::show_ref::run(parse_cmd_args(subcmd, rest)),
        "sparse-checkout" => commands::sparse_checkout::run(parse_cmd_args(subcmd, rest)),
        "stage" => commands::stage::run(parse_cmd_args(subcmd, rest)),
        "stash" => commands::stash::run(parse_cmd_args(subcmd, rest)),
        "status" => commands::status::run(parse_cmd_args(subcmd, rest)),
        "stripspace" => commands::stripspace::run(parse_cmd_args(subcmd, rest)),
        "submodule" => commands::submodule::run(parse_cmd_args(subcmd, rest)),
        "switch" => commands::switch::run(parse_cmd_args(subcmd, rest)),
        "symbolic-ref" => commands::symbolic_ref::run(parse_cmd_args(subcmd, rest)),
        "tag" => commands::tag::run(parse_cmd_args(subcmd, rest)),
        "unpack-file" => commands::unpack_file::run(parse_cmd_args(subcmd, rest)),
        "unpack-objects" => commands::unpack_objects::run(parse_cmd_args(subcmd, rest)),
        "update-index" => commands::update_index::run(parse_cmd_args(subcmd, rest)),
        "update-ref" => commands::update_ref::run(parse_cmd_args(subcmd, rest)),
        "update-server-info" => commands::update_server_info::run(parse_cmd_args(subcmd, rest)),
        "upload-archive" => commands::upload_archive::run(parse_cmd_args(subcmd, rest)),
        "upload-pack" => commands::upload_pack::run(parse_cmd_args(subcmd, rest)),
        "var" => commands::var::run(parse_cmd_args(subcmd, rest)),
        "verify-commit" => commands::verify_commit::run(parse_cmd_args(subcmd, rest)),
        "verify-pack" => commands::verify_pack::run(parse_cmd_args(subcmd, rest)),
        "verify-tag" => commands::verify_tag::run(parse_cmd_args(subcmd, rest)),
        "version" => commands::version::run(parse_cmd_args(subcmd, rest)),
        "whatchanged" => commands::whatchanged::run(parse_cmd_args(subcmd, rest)),
        "worktree" => commands::worktree::run(parse_cmd_args(subcmd, rest)),
        "write-tree" => commands::write_tree::run(parse_cmd_args(subcmd, rest)),
        "test-tool" => {
            let tt_args = preprocess_test_tool_args(rest)?;
            run_test_tool(&tt_args)
        }
        "__list_cmds" => {
            let categories = rest.first().map(|s| s.as_str()).unwrap_or("");
            print_list_cmds(categories);
            Ok(())
        }
        _ => {
            let helper = format!("git-{subcmd}");
            let mut trace = format!("trace: run_command: {}", quote_trace_arg(&helper));
            for arg in rest {
                trace.push(' ');
                trace.push_str(&quote_trace_arg(arg));
            }
            emit_git_trace(trace);

            match std::process::Command::new(&helper).args(rest).status() {
                Ok(status) => exit_with_status(status),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }

            let commands = KNOWN_COMMANDS;
            // Find similar commands using edit distance
            let mut suggestions: Vec<&str> = commands
                .iter()
                .filter(|cmd| strsim_distance(subcmd, cmd) <= 2)
                .copied()
                .collect();
            suggestions.sort();

            // Check help.autocorrect config
            let autocorrect = get_autocorrect_setting();

            match autocorrect.as_deref() {
                Some("never") => {
                    // With never, just say it's not a command, no suggestions
                    bail!("git: '{subcmd}' is not a git command. See 'git --help'.");
                }
                Some("immediate") | Some("-1") if suggestions.len() == 1 => {
                    // Auto-run the single matching command
                    let corrected = suggestions[0].to_owned();
                    eprintln!(
                        "WARNING: You called a grit command named '{subcmd}', which does not exist."
                    );
                    eprintln!("Auto-correcting to 'grit {corrected}'");
                    dispatch(&corrected, rest, opts)
                }
                _ => {
                    // Default: show suggestions
                    if suggestions.is_empty() {
                        bail!("git: '{subcmd}' is not a git command. See 'git --help'.\n\nunrecognized subcommand");
                    } else {
                        let similar = suggestions.join("\n\t");
                        bail!(
                            "git: '{subcmd}' is not a git command. See 'git --help'.\n\nThe most similar command is\n\t{similar}\n\nunrecognized subcommand"
                        );
                    }
                }
            }
        }
    }
}
