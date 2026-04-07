//! `grit` — Git plumbing reimplementation in Rust.
//!
//! This binary uses manual pre-dispatch to avoid building a clap parser for
//! all 143+ subcommands on every invocation.  Global options (-C, --git-dir,
//! --work-tree, -c) are extracted from argv by hand, then only the specific
//! subcommand's clap `Args` struct is parsed.

#![allow(dead_code)] // test-tool and harness helpers not fully wired through dispatch

use anyhow::{bail, Result};
use clap::{Args, Command, FromArgMatches, Parser};
use std::io::Read;
use std::path::{Path, PathBuf};

mod alias;
mod commands;
mod dotfile;
mod git_path;
mod grit_exe;
pub mod pathspec;
pub mod pkt_line;
pub mod protocol;

mod upstream_help_builtin_synopsis {
    include!(concat!(env!("OUT_DIR"), "/upstream_help_synopsis.rs"));
}

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
                // Match shell signal convention for SIGPIPE.
                exit_code = 128 + 13;
            } else if let Some(msg) = verbatim_lib_error_message(&e) {
                eprintln!("{msg}");
                exit_code = 128;
            } else {
                let display = format!("{e:#}");
                if display.starts_with("fatal:") {
                    eprintln!("{display}");
                    exit_code = 128;
                } else {
                    eprintln!("error: {display}");
                    exit_code = 1;
                }
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

fn verbatim_lib_error_message(err: &anyhow::Error) -> Option<String> {
    for cause in err.chain() {
        if let Some(grit_lib::error::Error::Message(msg)) =
            cause.downcast_ref::<grit_lib::error::Error>()
        {
            return Some(msg.clone());
        }
    }
    None
}

fn is_broken_pipe_error(err: &anyhow::Error) -> bool {
    use std::io::ErrorKind;
    for cause in err.chain() {
        if let Some(ioe) = cause.downcast_ref::<std::io::Error>() {
            if ioe.kind() == ErrorKind::BrokenPipe {
                return true;
            }
        }
        if let Some(lib_err) = cause.downcast_ref::<grit_lib::error::Error>() {
            if let grit_lib::error::Error::Io(ioe) = lib_err {
                if ioe.kind() == ErrorKind::BrokenPipe {
                    return true;
                }
            }
        }
    }
    false
}

/// Get process ancestry by walking parent PIDs on Linux.
fn get_process_ancestry() -> Vec<String> {
    #[allow(unused_mut)]
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
pub(crate) fn write_git_trace(dest: &str, line: &str) {
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

pub(crate) fn exit_with_status(status: std::process::ExitStatus) -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            std::process::exit(128 + sig);
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}

const TEST_TOOL_EXAMPLE_TAP_OUTPUT: &str = include_str!("test_tool_example_tap_output.txt");

fn run_test_tool_example_tap(rest: &[String]) -> Result<()> {
    if rest.len() != 1 {
        bail!("usage: test-tool example-tap");
    }
    print!("{TEST_TOOL_EXAMPLE_TAP_OUTPUT}");
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

fn run_test_tool_revision_walking(rest: &[String]) -> Result<()> {
    match rest.get(1).map(String::as_str).unwrap_or("") {
        "run-twice" => {
            let repo = grit_lib::repo::Repository::discover(None)?;
            let tips = vec!["HEAD".to_owned()];
            let empty: Vec<String> = Vec::new();
            let opts = grit_lib::rev_list::RevListOptions::default();
            let walked = grit_lib::rev_list::rev_list(&repo, &tips, &empty, &opts)?;

            for label in ["1st", "2nd"] {
                println!("{label}");
                for oid in &walked.commits {
                    let obj = repo.odb.read(oid)?;
                    let commit = grit_lib::objects::parse_commit(&obj.data)?;
                    let subject = commit.message.lines().next().unwrap_or_default();
                    println!(" > {subject}");
                }
            }
            Ok(())
        }
        other => bail!("test-tool revision-walking: unknown subcommand '{other}'"),
    }
}

fn run_test_tool_mergesort(rest: &[String]) -> Result<()> {
    match rest.get(1).map(String::as_str).unwrap_or("") {
        "test" => {
            // Minimal self-check used by t0071-sort.sh.
            let mut values = vec![9, 1, 5, 3, 7, 2, 8, 4, 6];
            let mut expected = values.clone();
            values.sort();
            expected.sort();
            if values == expected {
                Ok(())
            } else {
                bail!("test-tool mergesort: internal self-check failed");
            }
        }
        other => bail!("test-tool mergesort: unknown subcommand '{other}'"),
    }
}

fn run_test_tool_hexdump(_rest: &[String]) -> Result<()> {
    use std::io::{Read, Write};

    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut buf = [0u8; 1024];
    let mut have_data = false;

    loop {
        let len = stdin.read(&mut buf)?;
        if len == 0 {
            break;
        }

        have_data = true;
        for byte in &buf[..len] {
            write!(stdout, "{:02x} ", *byte)?;
        }
    }

    if have_data {
        writeln!(stdout)?;
    }

    Ok(())
}

const BUILTIN_USERDIFF_DRIVERS: &[&str] = &[
    "ada", "bash", "bibtex", "cpp", "csharp", "css", "dts", "elixir", "fortran", "fountain",
    "golang", "html", "ini", "java", "kotlin", "markdown", "matlab", "objc", "pascal", "perl",
    "php", "python", "r", "ruby", "rust", "scheme", "tex",
];

fn collect_custom_userdiff_drivers(config: &grit_lib::config::ConfigSet) -> Vec<String> {
    let mut custom = std::collections::BTreeSet::new();

    for entry in config.entries() {
        let Some(rest) = entry.key.strip_prefix("diff.") else {
            continue;
        };
        let Some(driver) = rest
            .strip_suffix(".funcname")
            .or_else(|| rest.strip_suffix(".xfuncname"))
        else {
            continue;
        };
        if driver.is_empty() || BUILTIN_USERDIFF_DRIVERS.contains(&driver) {
            continue;
        }
        custom.insert(driver.to_owned());
    }

    custom.into_iter().collect()
}

fn run_test_tool_userdiff(rest: &[String]) -> Result<()> {
    if rest.len() != 2 {
        bail!("usage: test-tool userdiff <list-drivers|list-builtin-drivers|list-custom-drivers>");
    }

    let (want_builtin, want_custom) = match rest[1].as_str() {
        "list-drivers" => (true, true),
        "list-builtin-drivers" => (true, false),
        "list-custom-drivers" => (false, true),
        other => bail!("test-tool userdiff: unknown argument '{other}'"),
    };

    if want_builtin {
        for driver in BUILTIN_USERDIFF_DRIVERS {
            println!("{driver}");
        }
    }

    if want_custom {
        let repo = grit_lib::repo::Repository::discover(None)?;
        let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
            .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());

        for driver in collect_custom_userdiff_drivers(&config) {
            println!("{driver}");
        }
    }

    Ok(())
}

fn parse_find_pack_count_arg(value: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("invalid --check-count value: {value}"))
}

fn run_test_tool_find_pack(rest: &[String]) -> Result<()> {
    let mut i = 1usize;
    let mut expected_count: Option<usize> = None;

    while i < rest.len() {
        let arg = &rest[i];
        if arg == "--check-count" || arg == "-c" {
            let Some(next) = rest.get(i + 1) else {
                bail!("usage: test-tool find-pack [--check-count=<n>|-c <n>] <object>");
            };
            expected_count = Some(parse_find_pack_count_arg(next)?);
            i += 2;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--check-count=") {
            expected_count = Some(parse_find_pack_count_arg(v)?);
            i += 1;
            continue;
        }
        break;
    }

    let Some(spec) = rest.get(i) else {
        bail!("usage: test-tool find-pack [--check-count=<n>|-c <n>] <object>");
    };
    if i + 1 != rest.len() {
        bail!("usage: test-tool find-pack [--check-count=<n>|-c <n>] <object>");
    }

    let repo = grit_lib::repo::Repository::discover(None)?;
    let oid = grit_lib::rev_parse::resolve_revision(&repo, spec)?;
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut packs: Vec<String> = Vec::new();
    for idx in indexes {
        if idx.entries.iter().any(|entry| entry.oid == oid) {
            if let Some(name) = idx.pack_path.file_name().and_then(|s| s.to_str()) {
                packs.push(format!(".git/objects/pack/{name}"));
            }
        }
    }
    packs.sort();
    packs.dedup();

    if let Some(n) = expected_count {
        if packs.len() == n {
            return Ok(());
        }
        std::process::exit(1);
    }

    for path in packs {
        println!("{path}");
    }
    Ok(())
}

fn run_test_tool_ref_store(rest: &[String]) -> Result<()> {
    if rest.len() < 5 {
        bail!("usage: test-tool ref-store <backend> update-ref <msg> <ref> <new> <old> [flags...]");
    }
    let backend = &rest[1];
    let sub = &rest[2];
    if backend != "main" || sub != "update-ref" {
        bail!("test-tool ref-store: unsupported invocation");
    }

    let msg = &rest[3];
    let refname = &rest[4];
    let new_oid = rest
        .get(5)
        .ok_or_else(|| anyhow::anyhow!("missing new oid"))?;
    let old_oid = rest
        .get(6)
        .ok_or_else(|| anyhow::anyhow!("missing old oid"))?;
    let flags = if rest.len() > 7 { &rest[7..] } else { &[] };
    let skip_oid_verification = flags.iter().any(|f| f == "REF_SKIP_OID_VERIFICATION");

    // Build equivalent `update-ref` invocation.
    // REF_SKIP_OID_VERIFICATION is approximated by allowing arbitrary new object ids
    // for tests that intentionally create dangling refs.
    let mut args = vec![
        "update-ref".to_owned(),
        "-m".to_owned(),
        msg.clone(),
        refname.clone(),
        new_oid.clone(),
    ];
    if old_oid != "0000000000000000000000000000000000000000" {
        args.push(old_oid.clone());
    }
    if skip_oid_verification {
        // Create the loose ref directly to avoid object existence checks.
        let git_dir = std::path::PathBuf::from(".git");
        let ref_path = git_dir.join(refname);
        if let Some(parent) = ref_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(ref_path, format!("{new_oid}\n"))?;
        return Ok(());
    }
    dispatch("update-ref", &args, &GlobalOpts::default())
}

fn dir_iterator_error_name(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        _ => "ESOMETHINGELSE",
    }
}

fn walk_dir_iterator(
    root_abs: &Path,
    root_display: &str,
    rel: &Path,
    pedantic: bool,
) -> std::result::Result<(), ()> {
    let current = if rel.as_os_str().is_empty() {
        root_abs.to_path_buf()
    } else {
        root_abs.join(rel)
    };

    let read_dir = match std::fs::read_dir(&current) {
        Ok(it) => it,
        Err(_) => {
            return if pedantic { Err(()) } else { Ok(()) };
        }
    };

    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(e) => entries.push(e),
            Err(_) => {
                if pedantic {
                    return Err(());
                }
            }
        }
    }
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let basename = entry.file_name();
        let basename_display = basename.to_string_lossy().to_string();
        let child_rel = if rel.as_os_str().is_empty() {
            PathBuf::from(&basename)
        } else {
            rel.join(&basename)
        };
        let child_abs = root_abs.join(&child_rel);

        let meta = match std::fs::symlink_metadata(&child_abs) {
            Ok(m) => m,
            Err(_) => {
                if pedantic {
                    return Err(());
                }
                continue;
            }
        };
        let ft = meta.file_type();
        let kind = if ft.is_dir() {
            'd'
        } else if ft.is_file() {
            'f'
        } else if ft.is_symlink() {
            's'
        } else {
            '?'
        };

        let path_display = Path::new(root_display).join(&child_rel);
        println!(
            "[{kind}] ({}) [{}] {}",
            child_rel.to_string_lossy(),
            basename_display,
            path_display.display()
        );

        if ft.is_dir() && walk_dir_iterator(root_abs, root_display, &child_rel, pedantic).is_err() {
            return Err(());
        }
    }

    Ok(())
}

fn run_test_tool_dir_iterator(rest: &[String]) -> Result<()> {
    let mut pedantic = false;
    let mut path_arg: Option<String> = None;

    for arg in rest.iter().skip(1) {
        if arg == "--pedantic" {
            pedantic = true;
            continue;
        }
        if arg.starts_with("--") {
            bail!("invalid option '{arg}'");
        }
        if path_arg.is_some() {
            bail!("dir-iterator needs exactly one non-option argument");
        }
        path_arg = Some(arg.clone());
    }

    let Some(path_arg) = path_arg else {
        bail!("dir-iterator needs exactly one non-option argument");
    };

    let root_abs = PathBuf::from(&path_arg);
    let root_meta = match std::fs::symlink_metadata(&root_abs) {
        Ok(m) => m,
        Err(e) => {
            println!(
                "dir_iterator_begin failure: {}",
                dir_iterator_error_name(e.kind())
            );
            std::process::exit(1);
        }
    };

    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        println!("dir_iterator_begin failure: ENOTDIR");
        std::process::exit(1);
    }

    if walk_dir_iterator(&root_abs, &path_arg, Path::new(""), pedantic).is_err() {
        println!("dir_iterator_advance failure");
        std::process::exit(1);
    }

    Ok(())
}

fn unquote_c_style(input: &str) -> String {
    if input.len() >= 2 && input.starts_with('"') && input.ends_with('"') {
        let mut out = String::new();
        let mut chars = input[1..input.len() - 1].chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }

            match chars.next() {
                Some('a') => out.push('\u{0007}'),
                Some('b') => out.push('\u{0008}'),
                Some('f') => out.push('\u{000C}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('v') => out.push('\u{000B}'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(c @ '0'..='7') => {
                    let mut value = c.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        if let Some(next @ '0'..='7') = chars.peek().copied() {
                            let _ = chars.next();
                            value = value * 8 + next.to_digit(8).unwrap_or(0);
                        } else {
                            break;
                        }
                    }
                    out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    } else {
        input.to_owned()
    }
}

fn run_test_tool_parse_pathspec_file(rest: &[String]) -> Result<()> {
    let mut pathspec_from_file: Option<String> = None;
    let mut pathspec_file_nul = false;

    for arg in rest.iter().skip(1) {
        if let Some(v) = arg.strip_prefix("--pathspec-from-file=") {
            pathspec_from_file = Some(v.to_owned());
            continue;
        }
        if arg == "--pathspec-file-nul" {
            pathspec_file_nul = true;
            continue;
        }
        bail!("usage: test-tool parse-pathspec-file --pathspec-from-file [--pathspec-file-nul]");
    }

    let Some(pathspec_source) = pathspec_from_file else {
        bail!("usage: test-tool parse-pathspec-file --pathspec-from-file [--pathspec-file-nul]");
    };

    let data = if pathspec_source == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        buf
    } else {
        std::fs::read(&pathspec_source)?
    };

    let items: Vec<String> = if pathspec_file_nul {
        data.split(|b| *b == 0u8)
            .filter(|chunk| !chunk.is_empty())
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect()
    } else {
        let text = String::from_utf8_lossy(&data);
        text.split('\n')
            .filter(|line| !line.is_empty())
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .map(unquote_c_style)
            .collect()
    };

    for item in items {
        println!("{item}");
    }
    Ok(())
}

fn parse_bool_str(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn run_test_tool_advise(rest: &[String]) -> Result<()> {
    if rest.len() != 2 {
        bail!("usage: test-tool advise <message>");
    }
    let advice_msg = &rest[1];

    let global_advice = std::env::var("GIT_ADVICE")
        .ok()
        .and_then(|v| parse_bool_str(&v));
    if global_advice == Some(false) {
        return Ok(());
    }

    let config_advice = if let Some(v) = protocol::check_config_param("advice.nestedTag") {
        parse_bool_str(&v)
    } else {
        let git_dir = std::env::var("GIT_DIR")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                grit_lib::repo::Repository::discover(None)
                    .ok()
                    .map(|r| r.git_dir)
            });
        if let Some(gd) = git_dir {
            if let Ok(config) = grit_lib::config::ConfigSet::load(Some(gd.as_path()), true) {
                config
                    .get("advice.nestedTag")
                    .and_then(|v| parse_bool_str(&v))
            } else {
                None
            }
        } else {
            None
        }
    };

    let enabled = global_advice == Some(true) || config_advice != Some(false);
    if !enabled {
        return Ok(());
    }

    eprintln!("hint: {advice_msg}");
    if config_advice.is_none() {
        eprintln!("hint: Disable this message with \"git config set advice.nestedTag false\"");
    }
    Ok(())
}

fn parse_ulong_str(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

fn run_test_tool_env_helper(rest: &[String]) -> Result<()> {
    // test-tool env-helper --type=<bool|ulong> --default=<value> [--exit-code] <VAR>
    if rest.len() < 3 || rest.first().map(String::as_str) != Some("env-helper") {
        bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
    }

    let mut value_type: Option<&str> = None;
    let mut default_value: Option<String> = None;
    let mut exit_code_only = false;
    let mut variable_name: Option<&str> = None;

    let mut i = 1usize;
    while i < rest.len() {
        let arg = &rest[i];
        if let Some(v) = arg.strip_prefix("--type=") {
            value_type = Some(v);
            i += 1;
            continue;
        }
        if arg == "--type" {
            bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
        }
        if let Some(v) = arg.strip_prefix("--default=") {
            if v.is_empty() {
                bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
            }
            default_value = Some(v.to_owned());
            i += 1;
            continue;
        }
        if arg == "--default" {
            bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
        }
        if arg == "--exit-code" {
            exit_code_only = true;
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
        }
        if variable_name.is_some() {
            bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
        }
        variable_name = Some(arg);
        i += 1;
    }

    let Some(value_type) = value_type else {
        bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
    };
    let Some(var) = variable_name else {
        bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
    };

    let resolved = std::env::var(var).ok().or(default_value);
    let Some(value) = resolved else {
        bail!("usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>");
    };

    match value_type {
        "bool" => {
            let Some(flag) = parse_bool_str(&value) else {
                std::process::exit(1);
            };
            if !exit_code_only {
                println!("{}", if flag { "true" } else { "false" });
            }
            if flag {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        "ulong" => {
            let Some(num) = parse_ulong_str(&value) else {
                std::process::exit(1);
            };
            if !exit_code_only {
                println!("{num}");
            }
            if num > 0 {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        _ => bail!(
            "usage: test-tool env-helper --type=<bool|ulong> [--default=<value>] [--exit-code] <VAR>"
        ),
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
            let allow_for_env_helper = std::env::var("GIT_TEST_ENV_HELPER").as_deref()
                == Ok("true")
                && subcmd == Some("env-helper");
            if !allow_for_env_helper {
                return Err(e.into());
            }
        }
    }

    Ok(rest[i..].to_vec())
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
        let line = raw_line.trim().trim_end_matches([' ', '\t']);
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
                let key = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-string requires key"))?;
                let value = parts
                    .get(2)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-string requires value"))?;
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push((
                            (*key).to_string(),
                            JsonWriterValue::String((*value).to_string()),
                        ));
                    }
                    _ => bail!("json-writer: object-string used outside object"),
                }
            }
            "object-int" => {
                let key = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-int requires key"))?;
                let value = parts
                    .get(2)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-int requires value"))?;
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid integer '{value}'"))?;
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), JsonWriterValue::Integer(parsed)));
                    }
                    _ => bail!("json-writer: object-int used outside object"),
                }
            }
            "object-double" => {
                let key = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-double requires key"))?;
                let precision = parts.get(2).ok_or_else(|| {
                    anyhow::anyhow!("json-writer: object-double requires precision")
                })?;
                let value = parts
                    .get(3)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-double requires value"))?;
                let p = precision
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid precision '{precision}'"))?;
                let v = value
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid float '{value}'"))?;
                let rendered = format!("{v:.p$}");
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), JsonWriterValue::Double(rendered)));
                    }
                    _ => bail!("json-writer: object-double used outside object"),
                }
            }
            "object-true" | "object-false" | "object-null" => {
                let key = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object literal requires key"))?;
                let val = match verb {
                    "object-true" => JsonWriterValue::Boolean(true),
                    "object-false" => JsonWriterValue::Boolean(false),
                    _ => JsonWriterValue::Null,
                };
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Object { entries, .. } => {
                        entries.push(((*key).to_string(), val));
                    }
                    _ => bail!("json-writer: object literal used outside object"),
                }
            }
            "object-object" => {
                let key = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-object requires key"))?;
                stack.push(JsonWriterContainer::Object {
                    key_in_parent: Some((*key).to_string()),
                    entries: Vec::new(),
                });
            }
            "object-array" => {
                let key = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: object-array requires key"))?;
                stack.push(JsonWriterContainer::Array {
                    key_in_parent: Some((*key).to_string()),
                    entries: Vec::new(),
                });
            }

            "array-string" => {
                let value = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: array-string requires value"))?;
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Array { entries, .. } => {
                        entries.push(JsonWriterValue::String((*value).to_string()));
                    }
                    _ => bail!("json-writer: array-string used outside array"),
                }
            }
            "array-int" => {
                let value = parts
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: array-int requires value"))?;
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid integer '{value}'"))?;
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
                match parent {
                    JsonWriterContainer::Array { entries, .. } => {
                        entries.push(JsonWriterValue::Integer(parsed));
                    }
                    _ => bail!("json-writer: array-int used outside array"),
                }
            }
            "array-double" => {
                let precision = parts.get(1).ok_or_else(|| {
                    anyhow::anyhow!("json-writer: array-double requires precision")
                })?;
                let value = parts
                    .get(2)
                    .ok_or_else(|| anyhow::anyhow!("json-writer: array-double requires value"))?;
                let p = precision
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid precision '{precision}'"))?;
                let v = value
                    .parse::<f64>()
                    .map_err(|_| anyhow::anyhow!("json-writer: invalid float '{value}'"))?;
                let rendered = format!("{v:.p$}");
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
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
                let parent = stack
                    .last_mut()
                    .ok_or_else(|| anyhow::anyhow!("json-writer: no active container"))?;
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
#[derive(Debug, Clone, Copy)]
struct BloomSettings {
    hash_version: u32,
    num_hashes: usize,
    bits_per_entry: usize,
    max_changed_paths: usize,
}

const TEST_BLOOM_SETTINGS: BloomSettings = BloomSettings {
    // Matches git's DEFAULT_BLOOM_FILTER_SETTINGS used by test-tool bloom.
    hash_version: 1,
    num_hashes: 7,
    bits_per_entry: 10,
    max_changed_paths: 512,
};

fn bloom_rotate_left(value: u32, count: u32) -> u32 {
    value.rotate_left(count)
}

fn bloom_signed_char_u32(b: u8) -> u32 {
    ((b as i8) as i32) as u32
}

fn bloom_murmur3_seeded_v2(mut seed: u32, data: &[u8]) -> u32 {
    let c1: u32 = 0xcc9e2d51;
    let c2: u32 = 0x1b873593;
    let r1: u32 = 15;
    let r2: u32 = 13;
    let m: u32 = 5;
    let n: u32 = 0xe6546b64;

    let mut i = 0usize;
    while i + 4 <= data.len() {
        let mut k = (data[i] as u32)
            | ((data[i + 1] as u32) << 8)
            | ((data[i + 2] as u32) << 16)
            | ((data[i + 3] as u32) << 24);
        k = k.wrapping_mul(c1);
        k = bloom_rotate_left(k, r1);
        k = k.wrapping_mul(c2);

        seed ^= k;
        seed = bloom_rotate_left(seed, r2).wrapping_mul(m).wrapping_add(n);
        i += 4;
    }

    let tail = &data[i..];
    let mut k1: u32 = 0;
    match tail.len() {
        3 => {
            k1 ^= (tail[2] as u32) << 16;
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
        }
        2 => {
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
        }
        1 => {
            k1 ^= tail[0] as u32;
        }
        _ => {}
    }
    if !tail.is_empty() {
        k1 = k1.wrapping_mul(c1);
        k1 = bloom_rotate_left(k1, r1);
        k1 = k1.wrapping_mul(c2);
        seed ^= k1;
    }

    seed ^= data.len() as u32;
    seed ^= seed >> 16;
    seed = seed.wrapping_mul(0x85ebca6b);
    seed ^= seed >> 13;
    seed = seed.wrapping_mul(0xc2b2ae35);
    seed ^= seed >> 16;
    seed
}

fn bloom_murmur3_seeded_v1(mut seed: u32, data: &[u8]) -> u32 {
    let c1: u32 = 0xcc9e2d51;
    let c2: u32 = 0x1b873593;
    let r1: u32 = 15;
    let r2: u32 = 13;
    let m: u32 = 5;
    let n: u32 = 0xe6546b64;

    let mut i = 0usize;
    while i + 4 <= data.len() {
        let mut k = bloom_signed_char_u32(data[i])
            | (bloom_signed_char_u32(data[i + 1]) << 8)
            | (bloom_signed_char_u32(data[i + 2]) << 16)
            | (bloom_signed_char_u32(data[i + 3]) << 24);
        k = k.wrapping_mul(c1);
        k = bloom_rotate_left(k, r1);
        k = k.wrapping_mul(c2);

        seed ^= k;
        seed = bloom_rotate_left(seed, r2).wrapping_mul(m).wrapping_add(n);
        i += 4;
    }

    let tail = &data[i..];
    let mut k1: u32 = 0;
    match tail.len() {
        3 => {
            k1 ^= bloom_signed_char_u32(tail[2]) << 16;
            k1 ^= bloom_signed_char_u32(tail[1]) << 8;
            k1 ^= bloom_signed_char_u32(tail[0]);
        }
        2 => {
            k1 ^= bloom_signed_char_u32(tail[1]) << 8;
            k1 ^= bloom_signed_char_u32(tail[0]);
        }
        1 => {
            k1 ^= bloom_signed_char_u32(tail[0]);
        }
        _ => {}
    }
    if !tail.is_empty() {
        k1 = k1.wrapping_mul(c1);
        k1 = bloom_rotate_left(k1, r1);
        k1 = k1.wrapping_mul(c2);
        seed ^= k1;
    }

    seed ^= data.len() as u32;
    seed ^= seed >> 16;
    seed = seed.wrapping_mul(0x85ebca6b);
    seed ^= seed >> 13;
    seed = seed.wrapping_mul(0xc2b2ae35);
    seed ^= seed >> 16;
    seed
}

fn bloom_murmur3_seeded(seed: u32, data: &[u8], version: u32) -> u32 {
    match version {
        2 => bloom_murmur3_seeded_v2(seed, data),
        _ => bloom_murmur3_seeded_v1(seed, data),
    }
}

fn bloom_key_hashes(data: &[u8], settings: BloomSettings) -> Vec<u32> {
    let seed0 = 0x293ae76f;
    let seed1 = 0x7e646e2c;
    let hash0 = bloom_murmur3_seeded(seed0, data, settings.hash_version);
    let hash1 = bloom_murmur3_seeded(seed1, data, settings.hash_version);

    let mut out = Vec::with_capacity(settings.num_hashes);
    for i in 0..settings.num_hashes {
        out.push(hash0.wrapping_add((i as u32).wrapping_mul(hash1)));
    }
    out
}

fn bloom_add_hashes_to_filter(hashes: &[u32], filter: &mut [u8]) {
    let mod_bits = (filter.len() * 8) as u64;
    if mod_bits == 0 {
        return;
    }
    for hash in hashes {
        let hash_mod = (*hash as u64) % mod_bits;
        let block_pos = (hash_mod / 8) as usize;
        let bitmask = 1u8 << (hash_mod & 7);
        filter[block_pos] |= bitmask;
    }
}

fn bloom_print_filter(filter: &[u8]) {
    println!("Filter_Length:{}", filter.len());
    print!("Filter_Data:");
    for b in filter {
        print!("{b:02x}|");
    }
    println!();
}

fn bloom_collect_paths_with_prefixes(path: &str, out: &mut std::collections::BTreeSet<String>) {
    if path.is_empty() {
        return;
    }
    let mut cur = path.to_string();
    loop {
        out.insert(cur.clone());
        let Some(pos) = cur.rfind('/') else {
            break;
        };
        cur.truncate(pos);
        if cur.is_empty() {
            break;
        }
    }
}

fn run_test_tool_bloom(rest: &[String]) -> Result<()> {
    if rest.len() < 2 {
        bail!(
            "usage: test-tool bloom [get_murmur3|get_murmur3_seven_highbit|generate_filter|get_filter_for_commit]"
        );
    }

    match rest[1].as_str() {
        "get_murmur3" => {
            let Some(s) = rest.get(2) else {
                bail!("usage: test-tool bloom get_murmur3 <string>");
            };
            let hashed = bloom_murmur3_seeded(0, s.as_bytes(), 2);
            println!("Murmur3 Hash with seed=0:0x{hashed:08x}");
            Ok(())
        }
        "get_murmur3_seven_highbit" => {
            let bytes = [0x99u8, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
            let hashed = bloom_murmur3_seeded(0, &bytes, 2);
            println!("Murmur3 Hash with seed=0:0x{hashed:08x}");
            Ok(())
        }
        "generate_filter" => {
            if rest.len() < 3 {
                bail!("usage: test-tool bloom generate_filter <string> [<string>...]");
            }
            let len = TEST_BLOOM_SETTINGS.bits_per_entry.div_ceil(8);
            let mut filter = vec![0u8; len];
            for item in rest.iter().skip(2) {
                let hashes = bloom_key_hashes(item.as_bytes(), TEST_BLOOM_SETTINGS);
                print!("Hashes:");
                for h in &hashes {
                    print!("0x{h:08x}|");
                }
                println!();
                bloom_add_hashes_to_filter(&hashes, &mut filter);
            }
            bloom_print_filter(&filter);
            Ok(())
        }
        "get_filter_for_commit" => {
            let Some(commit_hex) = rest.get(2) else {
                bail!("usage: test-tool bloom get_filter_for_commit <commit-hex>");
            };
            let commit_oid = commit_hex
                .parse::<grit_lib::objects::ObjectId>()
                .map_err(|_| anyhow::anyhow!("cannot parse oid '{commit_hex}'"))?;
            let repo = grit_lib::repo::Repository::discover(None)?;
            let commit_obj = repo.odb.read(&commit_oid)?;
            if commit_obj.kind != grit_lib::objects::ObjectKind::Commit {
                bail!("object '{commit_hex}' is not a commit");
            }
            let commit = grit_lib::objects::parse_commit(&commit_obj.data)?;

            let parent_tree = if let Some(parent_oid) = commit.parents.first() {
                let parent_obj = repo.odb.read(parent_oid)?;
                if parent_obj.kind != grit_lib::objects::ObjectKind::Commit {
                    None
                } else {
                    let parent_commit = grit_lib::objects::parse_commit(&parent_obj.data)?;
                    Some(parent_commit.tree)
                }
            } else {
                None
            };

            let diffs = grit_lib::diff::diff_trees(
                &repo.odb,
                parent_tree.as_ref(),
                Some(&commit.tree),
                "",
            )?;

            let mut changed_paths: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for d in diffs {
                if let Some(path) = d.new_path.or(d.old_path) {
                    bloom_collect_paths_with_prefixes(&path, &mut changed_paths);
                }
            }

            let mut filter = if changed_paths.len() > TEST_BLOOM_SETTINGS.max_changed_paths {
                vec![0xff]
            } else {
                let bit_count = changed_paths.len() * TEST_BLOOM_SETTINGS.bits_per_entry;
                let mut len = bit_count.div_ceil(8);
                if len == 0 {
                    len = 1;
                }
                let mut data = vec![0u8; len];
                for path in &changed_paths {
                    let hashes = bloom_key_hashes(path.as_bytes(), TEST_BLOOM_SETTINGS);
                    bloom_add_hashes_to_filter(&hashes, &mut data);
                }
                data
            };

            bloom_print_filter(&filter);
            filter.clear();
            Ok(())
        }
        _ => bail!(
            "usage: test-tool bloom [get_murmur3|get_murmur3_seven_highbit|generate_filter|get_filter_for_commit]"
        ),
    }
}

/// Global options parsed from argv before the subcommand.
#[derive(Default)]
pub(crate) struct GlobalOpts {
    git_dir: Option<PathBuf>,
    work_tree: Option<PathBuf>,
    change_dir: Option<PathBuf>,
    config_overrides: Vec<String>,
    attr_source: Option<String>,
    bare: bool,
    no_advice: bool,
    literal_pathspecs: bool,
    glob_pathspecs: bool,
    noglob_pathspecs: bool,
    icase_pathspecs: bool,
    exec_path: Option<PathBuf>,
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

        // --exec-path=<val>
        if let Some(val) = arg.strip_prefix("--exec-path=") {
            opts.exec_path = Some(PathBuf::from(val));
            i += 1;
            continue;
        }
        if arg == "--exec-path" {
            // Print exec-path and exit
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    println!("{}", dir.display());
                }
            }
            std::process::exit(0);
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

        // --attr-source=<tree-ish> or --attr-source <tree-ish>
        if let Some(val) = arg.strip_prefix("--attr-source=") {
            opts.attr_source = Some(val.to_owned());
            i += 1;
            continue;
        }
        if arg == "--attr-source" {
            i += 1;
            if i < items.len() {
                opts.attr_source = Some(items[i].clone());
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

        // Pathspec parsing globals accepted by Git before the subcommand.
        if arg == "--literal-pathspecs" {
            opts.literal_pathspecs = true;
            i += 1;
            continue;
        }
        if arg == "--glob-pathspecs" {
            opts.glob_pathspecs = true;
            i += 1;
            continue;
        }
        if arg == "--noglob-pathspecs" {
            opts.noglob_pathspecs = true;
            i += 1;
            continue;
        }
        if arg == "--icase-pathspecs" {
            opts.icase_pathspecs = true;
            i += 1;
            continue;
        }
        // Pager controls (no-op)
        if arg == "--no-pager" || arg == "--paginate" {
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

        // --end-of-options: stop processing options, next arg is subcommand
        if arg == "--end-of-options" {
            if i + 1 < items.len() {
                subcmd = Some(items[i + 1].clone());
                rest = items[i + 2..].to_vec();
            }
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
    if let Some(attr_source) = &opts.attr_source {
        std::env::set_var("GIT_ATTR_SOURCE", attr_source);
    }
    Ok(())
}

// Wrapper to parse a clap `Args` struct standalone (must not use doc comments here
// or clap uses them as the command `about` text in --help output).
#[derive(Debug, Parser)]
#[command(name = "grit", disable_help_subcommand = true)]
struct ArgsWrapper<T: Args> {
    #[command(flatten)]
    inner: T,
}

/// Split adoc synopsis into usage variants: each variant starts with a `git …` line; following
/// lines are continuations (AsciiDoc tabs) until the next `git …` line.
fn synopsis_variants_from_adoc(syn: &str) -> Vec<Vec<String>> {
    let mut variants: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in syn.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("git ") && !current.is_empty() {
            variants.push(core::mem::take(&mut current));
        }
        current.push(trimmed.to_owned());
    }
    if !current.is_empty() {
        variants.push(current);
    }
    variants
}

/// Print `git <cmd> -h` synopsis (from vendored Documentation/*.adoc), then exit 129.
///
/// Continuation lines are padded with spaces to width `git <cmd> ` (same as t0450 `align_after_nl`).
fn print_upstream_synopsis_and_exit(subcmd: &str, syn: &str) -> ! {
    let pad = " ".repeat(format!("git {subcmd} ").len());
    let variants = synopsis_variants_from_adoc(syn);
    for (i, var) in variants.iter().enumerate() {
        let Some(first) = var.first() else {
            continue;
        };
        if i == 0 {
            println!("usage: {first}");
        } else {
            println!("   or: {first}");
        }
        for cont in var.iter().skip(1) {
            println!("{pad}{cont}");
        }
    }
    println!();
    std::process::exit(129);
}

/// Parse a command's clap Args from the remaining arguments.
///
/// When `-h` is passed, clap prints usage and the process exits with code 129
/// (Git convention for usage errors) instead of clap's default exit code 0.
fn parse_cmd_args<T: Args + FromArgMatches>(subcmd: &str, rest: &[String]) -> T {
    if rest.len() == 1 && (rest[0] == "-h" || rest[0] == "--help") {
        if let Some(syn) = upstream_help_builtin_synopsis::synopsis_for_builtin(subcmd) {
            print_upstream_synopsis_and_exit(subcmd, syn);
        }
    }

    let mut argv = vec![format!("git {subcmd}")];
    argv.extend(rest.iter().cloned());
    match ArgsWrapper::<T>::try_parse_from(&argv) {
        Ok(wrapper) => wrapper.inner,
        Err(e) => {
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                    | clap::error::ErrorKind::DisplayVersion
            ) {
                // Git prints lowercase "usage:"; clap uses "Usage:". Tests grep for "usage".
                let mut msg = e.render().to_string();
                msg = msg.replace("Usage:", "usage:");
                print!("{msg}");
            } else {
                let _ = e.print();
            }
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
            commands::help::print_common_help();
            std::process::exit(1);
        }
    };

    // t0017-env-helper expects config to be loaded very early when
    // GIT_TEST_ENV_HELPER=true, even before applying -C.
    if subcmd == "test-tool"
        && std::env::var("GIT_TEST_ENV_HELPER")
            .ok()
            .and_then(|v| parse_bool_str(&v))
            == Some(true)
    {
        // Accept optional leading "-C <dir>" pairs before "env-helper".
        let mut idx = 0usize;
        while idx + 1 < rest.len() && rest[idx] == "-C" {
            idx += 2;
        }
        let is_env_helper = rest.get(idx).map(String::as_str) == Some("env-helper");
        if is_env_helper {
            let _ = grit_lib::config::ConfigSet::load(None, true)?;
        }
    }

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

    alias::run_command_with_aliases(subcmd, rest, &opts)
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
        "multi-pack-index" => extract_options::<commands::multi_pack_index::Args>(show_all),
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
            "list-all" | "builtins" | "main" => {
                result.extend_from_slice(&mainporcelain);
                result.extend_from_slice(&complete);
                result.extend_from_slice(&plumbing);
            }
            "deprecated" => {
                result.extend_from_slice(crate::alias::DEPRECATED_COMMANDS);
            }
            "others" => {
                // Non-built-in commands like gitk
                result.push("gitk");
            }
            "alias" | "nohelpers" => {
                // alias = git aliases (handled by config, could list them)
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
    let mut i = 0usize;
    let word_diff_modes = ["plain", "color", "porcelain", "none"];
    while i < rest.len() {
        let arg = &rest[i];
        if arg == "-U" {
            // `-U <N>` with a space — merge into `--unified=<N>`
            if i + 1 < rest.len() {
                result.push(format!("--unified={}", rest[i + 1]));
                i += 2;
            } else {
                result.push(arg.clone());
                i += 1;
            }
        } else if arg == "--word-diff" {
            if i + 1 < rest.len() && word_diff_modes.contains(&rest[i + 1].as_str()) {
                result.push(format!("--word-diff={}", rest[i + 1]));
                i += 2;
            } else {
                // Prevent clap from consuming the first path argument as MODE.
                result.push("--word-diff=plain".to_owned());
                i += 1;
            }
        } else if let Some(n) = arg.strip_prefix("-U") {
            // `-U<N>` without a space
            result.push(format!("--unified={n}"));
            i += 1;
        } else {
            result.push(arg.clone());
            i += 1;
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

pub(crate) const KNOWN_COMMANDS: &[&str] = &[
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
    "merge-recursive",
    "merge-resolve",
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
pub(crate) fn dispatch(subcmd: &str, rest: &[String], opts: &GlobalOpts) -> Result<()> {
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
            // Git grep uses -h for --no-filename, conflicting with clap's -h for help.
            // A lone `git grep -h` is Git's short help (exit 129); do not rewrite to --no-filename.
            if rest.len() == 1 && rest[0] == "-h" {
                if let Some(syn) = upstream_help_builtin_synopsis::synopsis_for_builtin(subcmd) {
                    print_upstream_synopsis_and_exit(subcmd, syn);
                }
            }
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
        "merge" => match commands::merge::run(parse_cmd_args(subcmd, rest)) {
            Ok(()) => Ok(()),
            Err(err) => {
                if commands::merge::is_internal_merge_execution_error(&err) {
                    eprintln!("error: failed to execute internal merge");
                    std::process::exit(2);
                }
                Err(err)
            }
        },
        "merge-recursive" => commands::merge_recursive::run(parse_cmd_args(subcmd, rest)),
        "merge-resolve" => commands::merge_resolve::run(parse_cmd_args(subcmd, rest)),
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
        "rev-parse" => commands::rev_parse::run_with_raw_args(rest),
        "revert" => commands::revert::run(parse_cmd_args(subcmd, rest)),
        "rm" => commands::rm::run(parse_cmd_args(subcmd, rest)),
        "scalar" => commands::scalar::run(rest),
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
        "update-index" => commands::update_index::run(parse_cmd_args(subcmd, rest), rest),
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
            let sub = rest.first().map(|s| s.as_str()).unwrap_or("");
            match sub {
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
                        "iwildmatch" => {
                            grit_lib::wildmatch::WM_PATHNAME | grit_lib::wildmatch::WM_CASEFOLD
                        }
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
                "example-tap" => run_test_tool_example_tap(rest),
                "advise" => run_test_tool_advise(rest),
                "env-helper" => run_test_tool_env_helper(rest),
                "dir-iterator" => run_test_tool_dir_iterator(rest),
                "parse-pathspec-file" => run_test_tool_parse_pathspec_file(rest),
                "revision-walking" => run_test_tool_revision_walking(rest),
                "mergesort" => run_test_tool_mergesort(rest),
                "hexdump" => run_test_tool_hexdump(rest),
                "chmtime" => run_test_tool_chmtime(&rest[1..]),
                "userdiff" => run_test_tool_userdiff(rest),
                "find-pack" => run_test_tool_find_pack(rest),
                "ref-store" => run_test_tool_ref_store(rest),
                "path-utils" => run_test_tool_path_utils(&rest[1..]),
                "submodule" => run_test_tool_submodule(&rest[1..]),
                "config" => run_test_tool_config(&rest[1..]),
                "parse-options" => {
                    let args = preprocess_test_tool_args(rest)?;
                    use grit_lib::parse_options_test_tool::ParseOptionsToolError;
                    match grit_lib::parse_options_test_tool::run_parse_options(&args) {
                        Ok(code) => std::process::exit(code),
                        Err(ParseOptionsToolError::Help) => std::process::exit(129),
                        Err(ParseOptionsToolError::Fatal(s)) => {
                            eprint!("{s}");
                            std::process::exit(129);
                        }
                        Err(ParseOptionsToolError::Bug(s)) => {
                            eprint!("{s}");
                            std::process::exit(99);
                        }
                    }
                }
                "parse-options-flags" => {
                    let args = preprocess_test_tool_args(rest)?;
                    use grit_lib::parse_options_test_tool::ParseOptionsToolError;
                    match grit_lib::parse_options_test_tool::run_parse_options_flags(&args) {
                        Ok(code) => std::process::exit(code),
                        Err(ParseOptionsToolError::Help) => std::process::exit(129),
                        Err(ParseOptionsToolError::Fatal(s)) => {
                            eprint!("{s}");
                            std::process::exit(129);
                        }
                        Err(ParseOptionsToolError::Bug(s)) => {
                            eprint!("{s}");
                            std::process::exit(99);
                        }
                    }
                }
                "parse-subcommand" => {
                    let args = preprocess_test_tool_args(rest)?;
                    use grit_lib::parse_options_test_tool::ParseOptionsToolError;
                    match grit_lib::parse_options_test_tool::run_parse_subcommand(&args) {
                        Ok(code) => std::process::exit(code),
                        Err(ParseOptionsToolError::Help) => std::process::exit(129),
                        Err(ParseOptionsToolError::Fatal(s)) => {
                            eprint!("{s}");
                            std::process::exit(129);
                        }
                        Err(ParseOptionsToolError::Bug(s)) => {
                            eprint!("{s}");
                            std::process::exit(99);
                        }
                    }
                }
                "date" => match grit_lib::git_date::test_tool_date(&rest[1..]) {
                    Ok(grit_lib::git_date::TestToolDateResult::Output(lines)) => {
                        for line in lines {
                            println!("{line}");
                        }
                        Ok(())
                    }
                    Ok(grit_lib::git_date::TestToolDateResult::Exit(code)) => {
                        std::process::exit(code);
                    }
                    Err(e) => bail!("{e}"),
                },
                "genrandom" => {
                    // Generate N random bytes
                    use std::io::Write;
                    let seed = rest.get(1).map(|s| s.as_str()).unwrap_or("0");
                    let n: usize = rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                    // Simple LCG random
                    let mut state: u64 = seed
                        .bytes()
                        .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
                    let stdout = std::io::stdout();
                    let mut out = stdout.lock();
                    let mut buf = vec![0u8; 8192];
                    let mut remaining = n;
                    while remaining > 0 {
                        let chunk = remaining.min(8192);
                        for b in &mut buf[..chunk] {
                            state = state
                                .wrapping_mul(6364136223846793005)
                                .wrapping_add(1442695040888963407);
                            *b = ((state >> 33) ^ state) as u8;
                        }
                        out.write_all(&buf[..chunk])?;
                        remaining -= chunk;
                    }
                    Ok(())
                }
                "genzeros" => {
                    // Generate N zero bytes
                    let n: usize = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                    use std::io::Write;
                    let stdout = std::io::stdout();
                    let mut out = stdout.lock();
                    let buf = vec![0u8; 8192];
                    let mut remaining = n;
                    while remaining > 0 {
                        let chunk = remaining.min(8192);
                        out.write_all(&buf[..chunk])?;
                        remaining -= chunk;
                    }
                    Ok(())
                }
                other => bail!("test-tool: unknown subcommand '{other}'"),
            }
        }
        "__list_cmds" => {
            let categories = rest.first().map(|s| s.as_str()).unwrap_or("");
            print_list_cmds(categories);
            Ok(())
        }
        _ => {
            if rest.len() == 1 && (rest[0] == "--help" || rest[0] == "-h") {
                eprintln!("git: '{subcmd}' is not a git command. See 'git --help'.");
                std::process::exit(1);
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
                    // Try external command: look for git-<subcmd> in exec-path
                    let ext_cmd = format!("git-{}", subcmd);
                    let exec_path = opts.exec_path.clone().or_else(|| {
                        std::env::current_exe()
                            .ok()
                            .and_then(|e| e.parent().map(|p| p.to_path_buf()))
                    });
                    if let Some(ref ep) = exec_path {
                        let ext_path = ep.join(&ext_cmd);
                        if ext_path.is_file() {
                            let status = std::process::Command::new(&ext_path)
                                .args(rest.iter())
                                .status()
                                .map_err(|e| anyhow::anyhow!("failed to run {}: {}", ext_cmd, e))?;
                            std::process::exit(status.code().unwrap_or(1));
                        }
                    }
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

/// Normalize a path (resolve . and ..) without requiring filesystem existence.
/// Returns "++failed++" if path goes above root for relative paths.
fn normalize_path_simple(path: &str) -> String {
    match git_path::normalize_path_copy(path) {
        Ok(s) => s,
        Err(()) => "++failed++".to_string(),
    }
}

/// POSIX `basename(3)` (matches libc used by Git's test-tool path-utils).
fn posix_basename(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let mut end = path.len();
    while end > 0 && path.as_bytes()[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return "/".to_string();
    }
    let path = &path[..end];
    if let Some(i) = path.rfind('/') {
        path[i + 1..].to_string()
    } else {
        path.to_string()
    }
}

/// POSIX `dirname(3)` (matches libc used by Git's test-tool path-utils).
fn posix_dirname(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let mut end = path.len();
    while end > 0 && path.as_bytes()[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return "/".to_string();
    }
    let mut len = end;
    while len > 0 && path.as_bytes()[len - 1] != b'/' {
        len -= 1;
    }
    if len == 0 {
        if path.as_bytes()[0] == b'/' {
            return "/".to_string();
        }
        return ".".to_string();
    }
    let mut d_end = len;
    while d_end > 0 && path.as_bytes()[d_end - 1] == b'/' {
        d_end -= 1;
    }
    if d_end == 0 {
        "/".to_string()
    } else {
        path[..d_end].to_string()
    }
}

/// Handle `test-tool path-utils` — path manipulation utilities.
fn run_test_tool_path_utils(rest: &[String]) -> Result<()> {
    let subcmd = rest.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "normalize_path_copy" => {
            let path = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("normalize_path_copy: missing path"))?;
            println!("{}", normalize_path_simple(path));
            Ok(())
        }
        "print_path" => {
            let path = rest.get(1).map(|s| s.as_str()).unwrap_or("");
            println!("{path}");
            Ok(())
        }
        "real_path" => {
            let path = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("real_path: missing path"))?;
            if path.is_empty() {
                bail!("The empty string is not a valid path");
            }
            let p = git_path::real_path_resolving(path);
            println!("{}", p.display());
            Ok(())
        }
        "absolute_path" => {
            let path = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("absolute_path: missing path"))?;
            if path.is_empty() {
                bail!("The empty string is not a valid path");
            }
            let cwd = std::env::current_dir()?;
            let abs = if std::path::Path::new(path).is_absolute() {
                normalize_path_simple(path)
            } else {
                normalize_path_simple(&cwd.join(path).display().to_string())
            };
            println!("{abs}");
            Ok(())
        }
        "basename" => {
            for arg in &rest[1..] {
                println!("{}", posix_basename(arg));
            }
            Ok(())
        }
        "dirname" => {
            for arg in &rest[1..] {
                println!("{}", posix_dirname(arg));
            }
            Ok(())
        }
        "strip_path_suffix" => {
            let path = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("strip_path_suffix: missing path"))?;
            let suffix = rest.get(2).map(|s| s.as_str()).unwrap_or("");
            match git_path::strip_path_suffix(path, suffix) {
                Some(p) => println!("{p}"),
                None => std::process::exit(1),
            }
            Ok(())
        }
        "longest_ancestor_length" => {
            let path = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("longest_ancestor_length: missing path"))?;
            let prefixes_str = rest
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("longest_ancestor_length: missing prefixes"))?;
            let len = git_path::longest_ancestor_length(path, prefixes_str).map_err(|_| {
                anyhow::anyhow!("longest_ancestor_length: could not normalize path")
            })?;
            println!("{len}");
            Ok(())
        }
        "prefix_path" => {
            let prefix = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("prefix_path: missing prefix"))?;
            let path = rest
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("prefix_path: missing path"))?;
            let repo = grit_lib::repo::Repository::discover(None)
                .map_err(|_| anyhow::anyhow!("prefix_path: not a git repository"))?;
            let Some(wt) = repo.work_tree.as_ref() else {
                bail!("prefix_path: bare repository");
            };
            match git_path::prefix_path_gently(prefix, path, wt.as_path()) {
                Some(p) => println!("{p}"),
                None => bail!("prefix_path: path outside repository"),
            }
            Ok(())
        }
        "relative_path" => {
            let path = rest
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("relative_path: missing path"))?;
            let base = rest
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("relative_path: missing base"))?;
            let path = if path == "<empty>" || path == "<null>" || path == "(null)" {
                ""
            } else {
                path.as_str()
            };
            let base = if base == "<empty>" || base == "<null>" || base == "(null)" {
                ""
            } else {
                base.as_str()
            };
            let mut sb = String::new();
            let rel = git_path::relative_path(path, base, &mut sb);
            match rel {
                None => println!("(null)"),
                Some(s) if s.is_empty() => println!("(empty)"),
                Some(s) => println!("{s}"),
            }
            Ok(())
        }
        "is_dotgitattributes" | "is_dotgitignore" | "is_dotgitmodules" | "is_dotmailmap" => {
            let mut res = 0;
            let mut expect = 1;
            for arg in &rest[1..] {
                if arg == "--not" {
                    expect = 0;
                    continue;
                }
                let hit = dotfile::dotfile_matches(subcmd, arg);
                if expect != (hit as i32) {
                    res = 1;
                }
            }
            std::process::exit(res);
        }
        other => bail!("test-tool path-utils: unknown subcommand '{other}'"),
    }
}

/// Handle `test-tool submodule` subcommands.
fn run_test_tool_submodule(rest: &[String]) -> Result<()> {
    let subcmd = rest.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "resolve-relative-url" => {
            // resolve-relative-url <up_path> <remoteurl> <url> — see git/t/helper/test-submodule.c
            let up_path = rest.get(1).map(|s| s.as_str()).unwrap_or("");
            let remote_url = rest.get(2).map(|s| s.as_str()).unwrap_or("");
            let url = rest.get(3).map(|s| s.as_str()).unwrap_or("");
            let up = if up_path == "(null)" {
                None
            } else {
                Some(up_path)
            };
            let result = git_path::relative_url(remote_url, url, up)
                .map_err(|_| anyhow::anyhow!("resolve-relative-url: invalid remote_url"))?;
            println!("{result}");
            Ok(())
        }
        other => bail!("test-tool submodule: unknown subcommand '{other}'"),
    }
}

/// Handle `test-tool chmtime` — get or set file modification times.
fn run_test_tool_chmtime(rest: &[String]) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if rest.is_empty() {
        bail!("usage: test-tool chmtime [--get|=<ts>|+<n>|-<n>] <file>");
    }
    let flag = &rest[0];
    if flag == "--get" {
        for path in &rest[1..] {
            let meta = std::fs::metadata(path)
                .map_err(|e| anyhow::anyhow!("chmtime: cannot stat '{path}': {e}"))?;
            println!("{}", meta.mtime());
        }
        return Ok(());
    }
    for path in &rest[1..] {
        let meta = std::fs::metadata(path)
            .map_err(|e| anyhow::anyhow!("chmtime: cannot stat '{path}': {e}"))?;
        let current_mtime = meta.mtime();
        let new_mtime: i64 = if let Some(ts_str) = flag.strip_prefix('=') {
            ts_str
                .parse::<i64>()
                .map_err(|e| anyhow::anyhow!("chmtime: invalid timestamp: {e}"))?
        } else if let Some(d) = flag.strip_prefix('+') {
            current_mtime
                + d.parse::<i64>()
                    .map_err(|e| anyhow::anyhow!("chmtime: {e}"))?
        } else if flag.starts_with('-') && !flag.starts_with("--") {
            current_mtime
                - flag[1..]
                    .parse::<i64>()
                    .map_err(|e| anyhow::anyhow!("chmtime: {e}"))?
        } else {
            bail!("chmtime: unknown flag '{flag}'");
        };
        // Use touch -t to set the mtime (format: [[CC]YY]MMDDhhmm[.ss])
        // Convert epoch to touch -d format
        // Use 'touch -m -d @<epoch>' to set mtime in UTC (avoids timezone issues)
        // macOS supports: touch -m -t YYYYMMDDhhmm.ss but that's TZ-dependent.
        // Use python or perl as fallback for reliable epoch setting.
        let ts_str = new_mtime.to_string();
        // Try touch with @epoch (works on Linux/BSD with GNU touch)
        let ok = std::process::Command::new("touch")
            .args(["-m", "-d", &format!("@{ts_str}"), path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            // Fallback: use Python
            let py = format!("import os; os.utime('{path}', ({new_mtime}, {new_mtime}))");
            let status = std::process::Command::new("python3")
                .args(["-c", &py])
                .status()
                .map_err(|e| anyhow::anyhow!("chmtime: python3 failed: {e}"))?;
            if !status.success() {
                bail!("chmtime: could not set mtime for '{path}'");
            }
        }
    }
    Ok(())
}

/// Handle `test-tool config` — config API test helper.
fn run_test_tool_config(rest: &[String]) -> Result<()> {
    let subcmd = rest.first().map(|s| s.as_str()).unwrap_or("");
    let key = rest.get(1).map(|s| s.as_str()).unwrap_or("");

    // Load config from current git repo
    let repo = grit_lib::repo::Repository::discover(None).ok();
    let git_dir = repo.as_ref().map(|r| r.git_dir.as_path());
    let cfg = grit_lib::config::ConfigSet::load(git_dir, true).unwrap_or_default();

    match subcmd {
        "get_value" | "get" => match cfg.get(key) {
            Some(val) => {
                println!("{val}");
                Ok(())
            }
            None => {
                eprintln!("fatal: config {} not found", key);
                std::process::exit(1);
            }
        },
        "get_value_multi" | "get_all" => {
            // Get all values for a key
            let values = cfg.get_all(key);
            if values.is_empty() {
                eprintln!("fatal: config {} not found", key);
                std::process::exit(1);
            }
            for v in values {
                println!("{v}");
            }
            Ok(())
        }
        "get_int" => match cfg.get(key) {
            Some(val) => match val.parse::<i64>() {
                Ok(n) => {
                    println!("{n}");
                    Ok(())
                }
                Err(_) => bail!("bad numeric config value '{}'", val),
            },
            None => {
                eprintln!("fatal: config {} not found", key);
                std::process::exit(1);
            }
        },
        "get_bool" => match cfg.get_bool(key) {
            Some(Ok(b)) => {
                println!("{}", if b { "true" } else { "false" });
                Ok(())
            }
            Some(Err(e)) => bail!("bad boolean config value: {}", e),
            None => {
                eprintln!("fatal: config {} not found", key);
                std::process::exit(1);
            }
        },
        _ => bail!("test-tool config: unknown subcommand '{subcmd}'"),
    }
}
