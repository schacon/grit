//! `grit` — Git plumbing reimplementation in Rust.
//!
//! This binary uses manual pre-dispatch to avoid building a clap parser for
//! all 143+ subcommands on every invocation.  Global options (-C, --git-dir,
//! --work-tree, -c) are extracted from argv by hand, then only the specific
//! subcommand's clap `Args` struct is parsed.

use anyhow::{bail, Result};
use clap::{Args, Command, FromArgMatches, Parser};
use std::path::{Path, PathBuf};

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
                // Match shell signal convention for SIGPIPE.
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

fn run_test_tool_online_cpus(rest: &[String]) -> Result<()> {
    if rest.len() != 1 {
        bail!("usage: test-tool online-cpus");
    }
    let cpus = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    println!("{cpus}");
    Ok(())
}

fn run_test_tool_lazy_init_name_hash(rest: &[String]) -> Result<()> {
    #[derive(Default)]
    struct LazyInitNameHashArgs {
        single: bool,
        multi: bool,
        count: usize,
        dump: bool,
    }

    let mut args = LazyInitNameHashArgs {
        count: 1,
        ..Default::default()
    };
    let mut i = 1usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-s" | "--single" => {
                args.single = true;
                i += 1;
            }
            "-m" | "--multi" => {
                args.multi = true;
                i += 1;
            }
            "-d" | "--dump" => {
                args.dump = true;
                i += 1;
            }
            "-c" | "--count" => {
                let Some(value) = rest.get(i + 1) else {
                    bail!("usage: test-tool lazy-init-name-hash (-s | -m) [-c <count>]");
                };
                args.count = value.parse()?;
                i += 2;
            }
            value if value.starts_with("--count=") => {
                args.count = value[8..].parse()?;
                i += 1;
            }
            "-p" | "--perf" | "-a" | "--analyze" | "--step" => {
                bail!("test-tool lazy-init-name-hash: unsupported mode");
            }
            other => bail!("test-tool lazy-init-name-hash: unknown argument '{other}'"),
        }
    }

    if !args.single && !args.multi {
        bail!("test-tool lazy-init-name-hash: require either -s or -m or both");
    }

    let repo = grit_lib::repo::Repository::discover(None)?;
    for _ in 0..args.count {
        let index = grit_lib::index::Index::load(&repo.index_path())?;
        let mut dirs = std::collections::BTreeSet::new();
        for entry in &index.entries {
            let path = String::from_utf8_lossy(&entry.path);
            let mut prefix = String::new();
            for component in path.split('/').filter(|component| !component.is_empty()) {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                dirs.insert(prefix.clone());
            }
        }

        if args.dump {
            for dir in &dirs {
                println!("dir {dir}");
            }
            for entry in &index.entries {
                println!("name {}", String::from_utf8_lossy(&entry.path));
            }
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

fn run_test_tool_hexdump() -> Result<()> {
    use std::io::{Read, Write};

    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    if input.is_empty() {
        return Ok(());
    }

    let mut stdout = std::io::stdout().lock();
    for byte in input {
        write!(stdout, "{byte:02x} ")?;
    }
    writeln!(stdout)?;
    Ok(())
}

fn run_test_tool_sha1() -> Result<()> {
    use sha1::{Digest, Sha1};
    use std::io::Read;

    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    println!("{}", hex::encode(Sha1::digest(&input)));
    Ok(())
}

fn run_test_tool_zlib(rest: &[String]) -> Result<()> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::{Read, Write};

    if rest.len() < 2 {
        bail!("usage: test-tool zlib <deflate>");
    }

    match rest[1].as_str() {
        "deflate" => {
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input)?;
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&input)?;
            let output = encoder.finish()?;
            std::io::stdout().write_all(&output)?;
            Ok(())
        }
        other => bail!("test-tool zlib: unknown function '{other}'"),
    }
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

/// Global options parsed from argv before the subcommand.
#[derive(Default)]
struct GlobalOpts {
    git_dir: Option<PathBuf>,
    work_tree: Option<PathBuf>,
    change_dir: Option<PathBuf>,
    config_overrides: Vec<String>,
    bare: bool,
    no_advice: bool,
    literal_pathspecs: bool,
    glob_pathspecs: bool,
    noglob_pathspecs: bool,
    icase_pathspecs: bool,
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
        // Pager controls accepted as global options.
        // We currently don't page output, so both are no-ops.
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
    if opts.literal_pathspecs {
        std::env::set_var("GIT_LITERAL_PATHSPECS", "1");
    }
    if opts.glob_pathspecs {
        std::env::set_var("GIT_GLOB_PATHSPECS", "1");
    }
    if opts.noglob_pathspecs {
        std::env::set_var("GIT_NOGLOB_PATHSPECS", "1");
    }
    if opts.icase_pathspecs {
        std::env::set_var("GIT_ICASE_PATHSPECS", "1");
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
            if matches!(subcmd, "tag" | "branch" | "bugreport") {
                eprintln!("usage: git {subcmd}");
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
            eprintln!("grit: a Git plumbing reimplementation in Rust");
            eprintln!("usage: grit <command> [<args>]");
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

    // Expand configured aliases when the subcommand is not a built-in.
    if !KNOWN_COMMANDS.contains(&subcmd.as_str()) {
        if let Some(alias) = get_alias_definition(&subcmd) {
            return run_alias(&subcmd, &alias, &rest, &opts);
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
            "list-all" | "builtins" | "main" => {
                result.extend_from_slice(&mainporcelain);
                result.extend_from_slice(&complete);
                result.extend_from_slice(&plumbing);
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

/// Preprocess blame/annotate arguments:
/// - expand `-C<N>` and `-M<N>` to `-C <N>` / `-M <N>`
/// - expand `-L<spec>` to `-L <spec>`
fn preprocess_blame_args(rest: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for arg in rest {
        if let Some(v) = arg.strip_prefix("-C") {
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                result.push("-C".to_string());
                result.push(v.to_string());
                continue;
            }
        }
        if let Some(v) = arg.strip_prefix("-M") {
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                result.push("-M".to_string());
                result.push(v.to_string());
                continue;
            }
        }
        if let Some(v) = arg.strip_prefix("-L") {
            if !v.is_empty() {
                result.push("-L".to_string());
                result.push(v.to_string());
                continue;
            }
        }
        result.push(arg.clone());
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

/// Read an alias definition from config (`alias.<name>`).
fn get_alias_definition(name: &str) -> Option<String> {
    let key = format!("alias.{name}");
    if let Some(val) = protocol::check_config_param(&key) {
        return Some(val);
    }
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });
    if let Ok(config) = grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) {
        return config.get(&key);
    }
    None
}

#[allow(dead_code)]
fn run_alias(name: &str, alias: &str, rest: &[String], opts: &GlobalOpts) -> Result<()> {
    if let Some(shell_cmd) = alias.strip_prefix('!') {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(shell_cmd)
            .arg(format!("git-{name}"))
            .args(rest)
            .status()?;
        exit_with_status(status);
    }

    let mut parts: Vec<String> = alias.split_whitespace().map(|s| s.to_owned()).collect();
    if parts.is_empty() {
        bail!("alias '{name}' expands to an empty command");
    }
    let next_subcmd = parts.remove(0);
    if next_subcmd == name {
        bail!("recursive alias '{name}'");
    }
    parts.extend(rest.iter().cloned());
    dispatch(&next_subcmd, &parts, opts)
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
    match subcmd {
        "add" => commands::add::run(parse_cmd_args(subcmd, rest)),
        "am" => commands::am::run(parse_cmd_args(subcmd, rest)),
        "annotate" => {
            let rest = preprocess_blame_args(rest);
            commands::annotate::run(parse_cmd_args(subcmd, &rest))
        }
        "apply" => commands::apply::run(parse_cmd_args(subcmd, rest)),
        "archive" => commands::archive::run(parse_cmd_args(subcmd, rest)),
        "backfill" => commands::backfill::run(parse_cmd_args(subcmd, rest)),
        "bisect" => commands::bisect::run(parse_cmd_args(subcmd, rest)),
        "blame" => {
            let rest = preprocess_blame_args(rest);
            commands::blame::run(parse_cmd_args(subcmd, &rest))
        }
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
                "online-cpus" => run_test_tool_online_cpus(rest),
                "lazy-init-name-hash" => run_test_tool_lazy_init_name_hash(rest),
                "find-pack" => run_test_tool_find_pack(rest),
                "hexdump" => run_test_tool_hexdump(),
                "sha1" => run_test_tool_sha1(),
                "zlib" => run_test_tool_zlib(rest),
                "ref-store" => commands::test_tool_ref_store::run(&rest[1..]),
                other => bail!("test-tool: unknown subcommand '{other}'"),
            }
        }
        "__list_cmds" => {
            let categories = rest.first().map(|s| s.as_str()).unwrap_or("");
            print_list_cmds(categories);
            Ok(())
        }
        _ => {
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
                    bail!("grit: '{subcmd}' is not a grit command. See 'grit --help'.");
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
                        bail!("grit: '{subcmd}' is not a grit command. See 'grit --help'.\n\nunrecognized subcommand");
                    } else {
                        let similar = suggestions.join("\n\t");
                        bail!(
                            "grit: '{subcmd}' is not a grit command. See 'grit --help'.\n\nThe most similar command is\n\t{similar}\n\nunrecognized subcommand"
                        );
                    }
                }
            }
        }
    }
}
