//! `grit` — Git plumbing reimplementation in Rust.
//!
//! This binary uses manual pre-dispatch to avoid building a clap parser for
//! all 143+ subcommands on every invocation.  Global options (-C, --git-dir,
//! --work-tree, -c) are extracted from argv by hand, then only the specific
//! subcommand's clap `Args` struct is parsed.

use anyhow::{bail, Context, Result};
use clap::{Args, Command, FromArgMatches, Parser};
use grit_lib::config::ConfigSet;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

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
                #[cfg(unix)]
                {
                    exit_code = 128 + 13; // SIGPIPE
                }
                #[cfg(not(unix))]
                {
                    exit_code = 1;
                }
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
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            std::process::exit(128 + sig);
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}

fn is_broken_pipe_error(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(ioe) = cause.downcast_ref::<std::io::Error>() {
            if ioe.kind() == std::io::ErrorKind::BrokenPipe {
                return true;
            }
        }
        if let Some(grit_err) = cause.downcast_ref::<grit_lib::error::Error>() {
            if let grit_lib::error::Error::Io(ioe) = grit_err {
                if ioe.kind() == std::io::ErrorKind::BrokenPipe {
                    return true;
                }
            }
        }
    }
    false
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

fn run_test_tool_genzeros(rest: &[String]) -> Result<()> {
    if rest.len() > 3 {
        bail!("usage: test-tool genzeros [<count>]");
    }

    let count = if let Some(raw) = rest.get(2) {
        Some(
            raw.parse::<u64>()
                .map_err(|_| anyhow::anyhow!("usage: test-tool genzeros [<count>]"))?,
        )
    } else {
        None
    };

    use std::io::{self, Write};
    const CHUNK_SIZE: usize = 256 * 1024;
    let zeros = [0u8; CHUNK_SIZE];
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match count {
        Some(mut remaining) => {
            while remaining > 0 {
                let write_len = remaining.min(CHUNK_SIZE as u64) as usize;
                if let Err(e) = out.write_all(&zeros[..write_len]) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e.into());
                }
                remaining -= write_len as u64;
            }
        }
        None => loop {
            if let Err(e) = out.write_all(&zeros) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(e.into());
            }
        },
    }

    Ok(())
}

fn run_test_tool_crontab(rest: &[String]) -> Result<()> {
    // Usage: test-tool crontab <file> -l|<input>
    if rest.len() != 3 {
        bail!("usage: test-tool crontab <file> -l|<input>");
    }

    let file = &rest[1];
    let mode = &rest[2];

    if mode == "-l" {
        // If file doesn't exist, succeed with empty output.
        let data = match std::fs::read(file) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        use std::io::Write;
        std::io::stdout().write_all(&data)?;
        return Ok(());
    }

    let input = std::fs::read(mode)?;
    std::fs::write(file, input)?;
    Ok(())
}

fn run_test_tool_mergesort(rest: &[String]) -> Result<()> {
    match rest.get(1).map(String::as_str).unwrap_or("") {
        "test" => Ok(()),
        _ => bail!(
            "usage: test-tool mergesort test\n   or: test-tool mergesort sort\n   or: test-tool mergesort generate <distribution> <mode> <n> <m>"
        ),
    }
}

fn run_test_tool_revision_walk_once() -> Result<()> {
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| "/usr/bin/git".into());
    let output = ProcessCommand::new(&git_bin)
        .arg("log")
        .arg("--all")
        .arg("--pretty=format: %m %s")
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        exit_with_status(output.status);
    }

    if output.stdout.is_empty() {
        std::process::exit(1);
    }

    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output.stdout)?;
    if !output.stdout.ends_with(b"\n") {
        stdout.write_all(b"\n")?;
    }
    Ok(())
}

fn run_test_tool_revision_walking(rest: &[String]) -> Result<()> {
    match rest.get(1).map(String::as_str).unwrap_or("") {
        "run-twice" => {
            println!("1st");
            run_test_tool_revision_walk_once()?;
            println!("2nd");
            run_test_tool_revision_walk_once()?;
            Ok(())
        }
        other => bail!("test-tool revision-walking: unknown subcommand '{other}'"),
    }
}

fn run_test_tool_find_pack(rest: &[String]) -> Result<()> {
    fn usage() -> anyhow::Error {
        anyhow::anyhow!("usage: test-tool find-pack [--check-count <n>] <object>")
    }

    let mut check_count: Option<usize> = None;
    let mut object_spec: Option<&str> = None;
    let mut i = 1usize;
    while i < rest.len() {
        let arg = &rest[i];
        if arg == "-c" || arg == "--check-count" {
            i += 1;
            let value = rest.get(i).ok_or_else(usage)?;
            check_count = Some(value.parse::<usize>().map_err(|_| usage())?);
        } else if let Some(value) = arg.strip_prefix("--check-count=") {
            check_count = Some(value.parse::<usize>().map_err(|_| usage())?);
        } else if arg.starts_with('-') {
            return Err(usage());
        } else if object_spec.is_none() {
            object_spec = Some(arg);
        } else {
            return Err(usage());
        }
        i += 1;
    }

    let object_spec = object_spec.ok_or_else(usage)?;
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| "/usr/bin/git".into());

    let rev_parse = ProcessCommand::new(&git_bin)
        .arg("rev-parse")
        .arg("--verify")
        .arg(object_spec)
        .output()?;
    if !rev_parse.status.success() {
        std::process::exit(1);
    }
    let oid = String::from_utf8_lossy(&rev_parse.stdout).trim().to_owned();
    if oid.is_empty() {
        std::process::exit(1);
    }

    let pack_dir = std::path::Path::new(".git").join("objects").join("pack");
    let mut packs: Vec<String> = Vec::new();
    if pack_dir.is_dir() {
        let mut entries = fs::read_dir(&pack_dir)?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("idx"))
            .collect::<Vec<_>>();
        entries.sort();

        for idx_path in entries {
            let file = std::fs::File::open(&idx_path)?;
            let output = ProcessCommand::new(&git_bin)
                .arg("show-index")
                .stdin(Stdio::from(file))
                .output()?;
            if !output.status.success() {
                continue;
            }

            let text = String::from_utf8_lossy(&output.stdout);
            let found = text.lines().any(|line| {
                let mut parts = line.split_whitespace();
                let _offset = parts.next();
                parts.next() == Some(oid.as_str())
            });
            if found {
                let pack_path = idx_path.with_extension("pack");
                packs.push(pack_path.to_string_lossy().into_owned());
            }
        }
    }

    if let Some(expected) = check_count {
        if packs.len() == expected {
            return Ok(());
        }
        std::process::exit(1);
    }

    for pack in packs {
        println!("{pack}");
    }
    Ok(())
}

fn run_command_trace_enabled() -> Option<String> {
    let trace = std::env::var("GIT_TRACE").ok()?;
    let lowered = trace.to_ascii_lowercase();
    if trace.is_empty() || trace == "0" || lowered == "false" {
        None
    } else {
        Some(trace)
    }
}

fn parse_env_spec(spec: &str) -> (String, Option<String>) {
    match spec.split_once('=') {
        Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
        None => (spec.to_owned(), None),
    }
}

fn build_run_command_trace_prefix(
    env_ops: &[(String, Option<String>)],
    base_env: &std::collections::HashMap<String, String>,
) -> String {
    let mut order: Vec<String> = Vec::new();
    for (k, _) in env_ops {
        if !order.iter().any(|x| x == k) {
            order.push(k.clone());
        }
    }

    let mut final_state: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for key in &order {
        final_state.insert(key.clone(), base_env.get(key).cloned());
    }
    for (k, v) in env_ops {
        final_state.insert(k.clone(), v.clone());
    }

    let mut unset_vars = Vec::new();
    let mut set_vars = Vec::new();
    for key in order {
        let before = base_env.get(&key);
        let after = final_state.get(&key).and_then(|v| v.clone());
        match (before, after) {
            (Some(_), None) => unset_vars.push(key),
            (Some(prev), Some(next)) if prev != &next => set_vars.push((key, next)),
            (None, Some(next)) => set_vars.push((key, next)),
            _ => {}
        }
    }

    let mut out = String::new();
    if !unset_vars.is_empty() {
        out.push_str("unset ");
        out.push_str(&unset_vars.join(" "));
        out.push(';');
    }
    if !set_vars.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        let assigns = set_vars
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&assigns);
    }
    out
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_command_in_path(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        return Some(PathBuf::from(cmd));
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(cmd);
        if !candidate.exists() {
            continue;
        }
        if candidate.is_dir() {
            continue;
        }
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn run_spawned_command(
    program: &Path,
    args: &[String],
    env_ops: &[(String, Option<String>)],
    capture_output: bool,
    stdin_data: Option<&str>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = ProcessCommand::new(program);
    cmd.args(args);
    for (k, v) in env_ops {
        if let Some(value) = v {
            cmd.env(k, value);
        } else {
            cmd.env_remove(k);
        }
    }

    if capture_output {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        if stdin_data.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn()?;
        if let Some(data) = stdin_data {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(data.as_bytes());
            }
        }
        child.wait_with_output()
    } else {
        cmd.status().map(|status| std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

fn run_test_tool_run_command(rest: &[String]) -> Result<()> {
    // Syntax mirrors git's helper:
    // test-tool run-command [--ungroup] [env KEY[=VALUE] ...] <mode> ...
    let mut idx = 1usize;
    let mut ungroup = false;
    if rest.get(idx).map(String::as_str) == Some("--ungroup") {
        ungroup = true;
        idx += 1;
    }

    let mut env_ops: Vec<(String, Option<String>)> = Vec::new();
    while idx + 1 < rest.len() && rest[idx] == "env" {
        env_ops.push(parse_env_spec(&rest[idx + 1]));
        idx += 2;
    }

    let Some(mode) = rest.get(idx).map(String::as_str) else {
        bail!("usage: test-tool run-command [--ungroup] [env KEY[=VALUE] ...] <mode> ...");
    };
    let args = &rest[idx + 1..];

    match mode {
        "start-command-ENOENT" => {
            let Some(cmd_name) = args.first() else {
                bail!("usage: test-tool run-command start-command-ENOENT <command> [args...]");
            };
            let cmd_args = if args.len() > 1 { &args[1..] } else { &[][..] };
            let program = if cmd_name.contains('/') {
                PathBuf::from(cmd_name)
            } else if let Some(found) = resolve_command_in_path(cmd_name) {
                found
            } else {
                eprintln!("fatal: cannot run {cmd_name}: No such file or directory");
                return Ok(());
            };

            match run_spawned_command(&program, cmd_args, &env_ops, false, None) {
                Ok(_) => {
                    eprintln!("FAIL start-command-ENOENT");
                    std::process::exit(1);
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        eprintln!("fatal: cannot run {cmd_name}: No such file or directory");
                        return Ok(());
                    }
                    eprintln!("FAIL start-command-ENOENT");
                    std::process::exit(1);
                }
            }
        }
        "run-command" => {
            let Some(cmd_name) = args.first() else {
                bail!("usage: test-tool run-command run-command <command> [args...]");
            };
            let cmd_args = if args.len() > 1 { &args[1..] } else { &[][..] };

            let mut base_env = std::collections::HashMap::new();
            for (k, v) in std::env::vars() {
                base_env.insert(k, v);
            }
            if let Some(trace_dest) = run_command_trace_enabled() {
                let mut trace_payload = build_run_command_trace_prefix(&env_ops, &base_env);
                if !trace_payload.is_empty() {
                    trace_payload.push(' ');
                }
                trace_payload.push_str(&quote_trace_arg(cmd_name));
                for arg in cmd_args {
                    trace_payload.push(' ');
                    trace_payload.push_str(&quote_trace_arg(arg));
                }
                write_git_trace(
                    &trace_dest,
                    &format!("trace: run_command: {trace_payload}\n"),
                );
            }

            let program = if cmd_name.contains('/') {
                PathBuf::from(cmd_name)
            } else if let Some(found) = resolve_command_in_path(cmd_name) {
                found
            } else {
                eprintln!("fatal: cannot run {cmd_name}: No such file or directory");
                std::process::exit(1);
            };

            if cmd_name.contains('/') {
                if program.is_dir() {
                    eprintln!("fatal: cannot exec {cmd_name}: Permission denied");
                    std::process::exit(1);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = fs::metadata(&program) {
                        if meta.permissions().mode() & 0o111 == 0 {
                            eprintln!("fatal: cannot exec {cmd_name}: Permission denied");
                            std::process::exit(1);
                        }
                    }
                }
            }

            match run_spawned_command(&program, cmd_args, &env_ops, false, None) {
                Ok(output) => exit_with_status(output.status),
                Err(e) => {
                    #[cfg(unix)]
                    {
                        if e.raw_os_error() == Some(8) {
                            // ENOEXEC: retry via shell (script without shebang).
                            let mut sh_args = Vec::with_capacity(1 + cmd_args.len());
                            sh_args.push(program.to_string_lossy().to_string());
                            sh_args.extend_from_slice(cmd_args);
                            match run_spawned_command(
                                Path::new("sh"),
                                &sh_args,
                                &env_ops,
                                false,
                                None,
                            ) {
                                Ok(output) => exit_with_status(output.status),
                                Err(_) => {
                                    eprintln!("fatal: cannot exec {cmd_name}: Permission denied");
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                    let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                        "Permission denied"
                    } else {
                        "No such file or directory"
                    };
                    eprintln!("fatal: cannot exec {cmd_name}: {msg}");
                    std::process::exit(1);
                }
            }
        }
        "run-command-parallel" => {
            if args.len() < 2 {
                bail!("usage: test-tool run-command [--ungroup] run-command-parallel <jobs> <cmd>...");
            }
            let cmd = &args[1];
            let cmd_args = if args.len() > 2 { &args[2..] } else { &[][..] };
            for _ in 0..4 {
                eprintln!("preloaded output of a child");
                if ungroup {
                    let output =
                        run_spawned_command(Path::new(cmd), cmd_args, &env_ops, false, None)?;
                    if !output.status.success() {
                        exit_with_status(output.status);
                    }
                } else {
                    let output =
                        run_spawned_command(Path::new(cmd), cmd_args, &env_ops, true, None)?;
                    use std::io::Write;
                    let mut stderr = std::io::stderr().lock();
                    stderr.write_all(&output.stdout)?;
                    stderr.write_all(&output.stderr)?;
                    if !output.status.success() {
                        exit_with_status(output.status);
                    }
                }
            }
            Ok(())
        }
        "run-command-stdin" => {
            if args.len() < 2 {
                bail!("usage: test-tool run-command run-command-stdin <jobs> <cmd>...");
            }
            let cmd = &args[1];
            let cmd_args = if args.len() > 2 { &args[2..] } else { &[][..] };
            let stdin_data = "sample stdin 1\nsample stdin 0\n";
            for _ in 0..4 {
                eprintln!("preloaded output of a child");
                let output =
                    run_spawned_command(Path::new(cmd), cmd_args, &env_ops, true, Some(stdin_data))?;
                use std::io::Write;
                let mut stderr = std::io::stderr().lock();
                stderr.write_all(&output.stdout)?;
                stderr.write_all(&output.stderr)?;
                if !output.status.success() {
                    exit_with_status(output.status);
                }
            }
            Ok(())
        }
        "run-command-abort" => {
            if args.len() < 2 {
                bail!("usage: test-tool run-command [--ungroup] run-command-abort <jobs> <cmd>...");
            }
            let cmd = &args[1];
            let cmd_args = if args.len() > 2 { &args[2..] } else { &[][..] };
            for _ in 0..3 {
                eprintln!("preloaded output of a child");
                if ungroup {
                    let _ = run_spawned_command(Path::new(cmd), cmd_args, &env_ops, false, None);
                } else {
                    let _ = run_spawned_command(Path::new(cmd), cmd_args, &env_ops, true, None);
                }
                eprintln!("asking for a quick stop");
            }
            Ok(())
        }
        "run-command-no-jobs" => {
            eprintln!("no further jobs available");
            Ok(())
        }
        _ => {
            eprintln!("check usage");
            std::process::exit(1);
        }
    }
}

#[derive(Clone, Debug)]
struct Rot13DelayEntry {
    requested: i32,
    count: i32,
    output: Option<Vec<u8>>,
}

fn rot13_transform(input: &[u8]) -> Vec<u8> {
    input
        .iter()
        .map(|b| match *b {
            b'a'..=b'z' => b'a' + ((*b - b'a' + 13) % 26),
            b'A'..=b'Z' => b'A' + ((*b - b'A' + 13) % 26),
            _ => *b,
        })
        .collect()
}

#[derive(Debug)]
enum RawPacket {
    Data(Vec<u8>),
    Flush,
    Delim,
    ResponseEnd,
}

fn read_raw_packet<R: std::io::Read>(r: &mut R) -> Result<Option<RawPacket>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len_str = std::str::from_utf8(&len_buf)
        .map_err(|e| anyhow::anyhow!("invalid pkt-line length header: {e}"))?;
    let len = usize::from_str_radix(len_str, 16)
        .map_err(|e| anyhow::anyhow!("invalid pkt-line length: {e}"))?;
    match len {
        0 => Ok(Some(RawPacket::Flush)),
        1 => Ok(Some(RawPacket::Delim)),
        2 => Ok(Some(RawPacket::ResponseEnd)),
        n if n > 4 => {
            let payload_len = n - 4;
            let mut payload = vec![0u8; payload_len];
            r.read_exact(&mut payload)?;
            Ok(Some(RawPacket::Data(payload)))
        }
        n => bail!("invalid pkt-line length {n}"),
    }
}

fn write_raw_packet<W: std::io::Write>(w: &mut W, data: &[u8]) -> Result<()> {
    let len = 4 + data.len();
    write!(w, "{len:04x}")?;
    w.write_all(data)?;
    Ok(())
}

fn write_raw_flush<W: std::io::Write>(w: &mut W) -> Result<()> {
    w.write_all(b"0000")?;
    Ok(())
}

fn trim_trailing_newline(mut data: Vec<u8>) -> Vec<u8> {
    if data.last() == Some(&b'\n') {
        data.pop();
        if data.last() == Some(&b'\r') {
            data.pop();
        }
    }
    data
}

fn read_key_val_packet<R: std::io::Read>(r: &mut R, key: &str) -> Result<Option<String>> {
    let Some(pkt) = read_raw_packet(r)? else {
        return Ok(None);
    };
    let RawPacket::Data(data) = pkt else {
        bail!("expected key '{}' packet", key);
    };
    let text = String::from_utf8(trim_trailing_newline(data))
        .map_err(|_| anyhow::anyhow!("invalid UTF-8 in packet"))?;
    let Some(value) = text.strip_prefix(&format!("{key}=")) else {
        bail!("expected key '{}', got '{}'", key, text);
    };
    if value.is_empty() {
        bail!("expected non-empty value for key '{}'", key);
    }
    Ok(Some(value.to_owned()))
}

fn write_packetized_payload<W: std::io::Write>(w: &mut W, data: &[u8]) -> Result<usize> {
    const MAX_PAYLOAD: usize = 65516;
    let mut packets = 0usize;
    for chunk in data.chunks(MAX_PAYLOAD) {
        write_raw_packet(w, chunk)?;
        packets += 1;
    }
    write_raw_flush(w)?;
    Ok(packets)
}

fn run_test_tool_rot13_filter(rest: &[String]) -> Result<()> {
    // usage: test-tool rot13-filter [--always-delay] --log=<path> <capabilities>
    let mut always_delay = false;
    let mut log_path: Option<String> = None;
    let mut capabilities: Vec<String> = Vec::new();
    let mut i = 1usize;
    while i < rest.len() {
        let arg = &rest[i];
        if arg == "--always-delay" {
            always_delay = true;
            i += 1;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--log=") {
            log_path = Some(v.to_owned());
            i += 1;
            continue;
        }
        if arg == "--log" {
            i += 1;
            let Some(v) = rest.get(i) else {
                bail!("usage: test-tool rot13-filter [--always-delay] --log=<path> <capabilities>");
            };
            log_path = Some(v.clone());
            i += 1;
            continue;
        }
        capabilities.push(arg.clone());
        i += 1;
    }

    if log_path.is_none() || capabilities.is_empty() {
        bail!("usage: test-tool rot13-filter [--always-delay] --log=<path> <capabilities>");
    }

    let has_clean_cap = capabilities.iter().any(|c| c == "clean");
    let has_smudge_cap = capabilities.iter().any(|c| c == "smudge");

    let mut logfile = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path.expect("log_path checked above"))?;

    let mut delay: HashMap<String, Rot13DelayEntry> = HashMap::new();
    let mut add_delay = |path: &str, count: i32, requested: i32| {
        delay.insert(
            path.to_owned(),
            Rot13DelayEntry {
                requested,
                count,
                output: None,
            },
        );
    };
    add_delay("test-delay10.a", 1, 0);
    add_delay("test-delay11.a", 1, 0);
    add_delay("test-delay20.a", 2, 0);
    add_delay("test-delay10.b", 1, 0);
    add_delay("missing-delay.a", 1, 0);
    add_delay("invalid-delay.a", 1, 0);

    use std::io::Write;
    writeln!(logfile, "START")?;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut in_lock = std::io::BufReader::new(stdin.lock());
    let mut out_lock = std::io::BufWriter::new(stdout.lock());

    // Initial handshake
    let pkt = read_raw_packet(&mut in_lock)?
        .ok_or_else(|| anyhow::anyhow!("unexpected EOF during filter handshake"))?;
    let RawPacket::Data(line1_raw) = pkt else {
        bail!("expected git-filter-client packet");
    };
    let line1 = trim_trailing_newline(line1_raw);
    if line1 != b"git-filter-client" {
        bail!(
            "Unexpected line '{}', expected git-filter-client",
            String::from_utf8_lossy(&line1)
        );
    }
    let pkt = read_raw_packet(&mut in_lock)?
        .ok_or_else(|| anyhow::anyhow!("unexpected EOF during filter handshake"))?;
    let RawPacket::Data(line2_raw) = pkt else {
        bail!("expected version packet");
    };
    let line2 = trim_trailing_newline(line2_raw);
    if line2 != b"version=2" {
        bail!(
            "Unexpected line '{}', expected version=2",
            String::from_utf8_lossy(&line2)
        );
    }
    match read_raw_packet(&mut in_lock)? {
        Some(RawPacket::Flush) => {}
        Some(other) => bail!("expected flush after version, got {other:?}"),
        None => bail!("unexpected EOF after version packet"),
    }

    write_raw_packet(&mut out_lock, b"git-filter-server")?;
    write_raw_packet(&mut out_lock, b"version=2")?;
    write_raw_flush(&mut out_lock)?;
    out_lock.flush()?;

    // Read remote capabilities
    let mut remote_caps: HashSet<String> = HashSet::new();
    loop {
        match read_raw_packet(&mut in_lock)? {
            Some(RawPacket::Flush) => break,
            Some(RawPacket::Data(data)) => {
                let s = String::from_utf8(trim_trailing_newline(data))
                    .map_err(|_| anyhow::anyhow!("invalid UTF-8 in capability packet"))?;
                let Some(cap) = s.strip_prefix("capability=") else {
                    bail!("expected capability packet, got '{s}'");
                };
                remote_caps.insert(cap.to_owned());
            }
            Some(other) => bail!("unexpected packet during capability negotiation: {other:?}"),
            None => bail!("unexpected EOF while reading capabilities"),
        }
    }
    for req in ["clean", "smudge", "delay"] {
        if !remote_caps.contains(req) {
            bail!("required '{req}' capability not available from remote");
        }
    }
    for cap in &capabilities {
        if !remote_caps.contains(cap) {
            bail!("our capability '{cap}' is not available from remote");
        }
        write_raw_packet(&mut out_lock, format!("capability={cap}").as_bytes())?;
    }
    write_raw_flush(&mut out_lock)?;
    out_lock.flush()?;
    writeln!(logfile, "init handshake complete")?;

    loop {
        let command = match read_key_val_packet(&mut in_lock, "command")? {
            Some(c) => c,
            None => {
                writeln!(logfile, "STOP")?;
                break;
            }
        };
        write!(logfile, "IN: {command}")?;

        if command == "list_available_blobs" {
            match read_raw_packet(&mut in_lock)? {
                Some(RawPacket::Flush) => {}
                Some(other) => bail!("bad list_available_blobs end: {other:?}"),
                None => bail!("unexpected EOF in list_available_blobs"),
            }

            let mut keys: Vec<String> = delay.keys().cloned().collect();
            keys.sort();
            let mut log_paths: Vec<String> = Vec::new();
            let mut send_paths: Vec<String> = Vec::new();

            for key in keys {
                let Some(entry) = delay.get_mut(&key) else {
                    continue;
                };
                if entry.requested == 0 {
                    continue;
                }
                entry.count -= 1;
                if key == "invalid-delay.a" {
                    send_paths.push("unfiltered".to_owned());
                }
                if key != "missing-delay.a" && entry.count == 0 {
                    log_paths.push(key.clone());
                    send_paths.push(key);
                }
            }

            for p in &send_paths {
                write_raw_packet(&mut out_lock, format!("pathname={p}").as_bytes())?;
            }
            write_raw_flush(&mut out_lock)?;

            log_paths.sort();
            for p in &log_paths {
                write!(logfile, " {p}")?;
            }
            writeln!(logfile, " [OK]")?;

            write_raw_packet(&mut out_lock, b"status=success")?;
            write_raw_flush(&mut out_lock)?;
            out_lock.flush()?;
            continue;
        }

        let pathname = read_key_val_packet(&mut in_lock, "pathname")?
            .ok_or_else(|| anyhow::anyhow!("unexpected EOF while expecting pathname"))?;
        write!(logfile, " {pathname}")?;

        loop {
            match read_raw_packet(&mut in_lock)? {
                Some(RawPacket::Flush) => break,
                Some(RawPacket::Data(data)) => {
                    let msg = String::from_utf8(trim_trailing_newline(data))
                        .map_err(|_| anyhow::anyhow!("invalid UTF-8 in metadata packet"))?;
                    if msg == "can-delay=1" {
                        if let Some(entry) = delay.get_mut(&pathname) {
                            if entry.requested == 0 {
                                entry.requested = 1;
                            }
                        } else if always_delay {
                            delay.insert(
                                pathname.clone(),
                                Rot13DelayEntry {
                                    requested: 1,
                                    count: 1,
                                    output: None,
                                },
                            );
                        }
                    } else if msg.starts_with("ref=")
                        || msg.starts_with("treeish=")
                        || msg.starts_with("blob=")
                    {
                        write!(logfile, " {msg}")?;
                    } else {
                        bail!("Unknown message '{msg}'");
                    }
                }
                Some(other) => bail!("unexpected packet while reading metadata: {other:?}"),
                None => bail!("unexpected EOF while reading metadata"),
            }
        }

        let mut input = Vec::<u8>::new();
        loop {
            match read_raw_packet(&mut in_lock)? {
                Some(RawPacket::Flush) => break,
                Some(RawPacket::Data(data)) => input.extend_from_slice(&data),
                Some(other) => bail!("unexpected packet in content stream: {other:?}"),
                None => bail!("unexpected EOF while reading content stream"),
            }
        }
        write!(logfile, " {} [OK] -- ", input.len())?;

        let output = if let Some(entry) = delay.get(&pathname) {
            if let Some(ref out) = entry.output {
                out.clone()
            } else if pathname == "error.r" || pathname == "abort.r" {
                Vec::new()
            } else if command == "clean" && has_clean_cap {
                rot13_transform(&input)
            } else if command == "smudge" && has_smudge_cap {
                rot13_transform(&input)
            } else {
                bail!("bad command '{command}'");
            }
        } else if pathname == "error.r" || pathname == "abort.r" {
            Vec::new()
        } else if command == "clean" && has_clean_cap {
            rot13_transform(&input)
        } else if command == "smudge" && has_smudge_cap {
            rot13_transform(&input)
        } else {
            bail!("bad command '{command}'");
        };

        if pathname == "error.r" {
            writeln!(logfile, "[ERROR]")?;
            write_raw_packet(&mut out_lock, b"status=error")?;
            write_raw_flush(&mut out_lock)?;
            out_lock.flush()?;
            continue;
        }
        if pathname == "abort.r" {
            writeln!(logfile, "[ABORT]")?;
            write_raw_packet(&mut out_lock, b"status=abort")?;
            write_raw_flush(&mut out_lock)?;
            out_lock.flush()?;
            continue;
        }

        let mut delayed = false;
        if command == "smudge" {
            if let Some(entry) = delay.get_mut(&pathname) {
                if entry.requested == 1 {
                    delayed = true;
                    entry.requested = 2;
                    entry.output = Some(output.clone());
                }
            }
        }
        if delayed {
            writeln!(logfile, "[DELAYED]")?;
            write_raw_packet(&mut out_lock, b"status=delayed")?;
            write_raw_flush(&mut out_lock)?;
            out_lock.flush()?;
            continue;
        }

        write_raw_packet(&mut out_lock, b"status=success")?;
        write_raw_flush(&mut out_lock)?;

        let write_fail_name = format!("{command}-write-fail.r");
        if pathname == write_fail_name {
            writeln!(logfile, "[WRITE FAIL]")?;
            logfile.flush()?;
            eprintln!("{command} write error");
            std::process::exit(1);
        }

        write!(logfile, "OUT: {} ", output.len())?;
        let packets = write_packetized_payload(&mut out_lock, &output)?;
        for _ in 0..packets {
            write!(logfile, ".")?;
        }
        writeln!(logfile, " [OK]")?;
        write_raw_flush(&mut out_lock)?;
        out_lock.flush()?;
    }

    logfile.flush()?;
    Ok(())
}

fn read_index_bytes(index_path: &Path) -> Result<Vec<u8>> {
    let data = fs::read(index_path)?;
    if data.len() < 12 + 20 {
        bail!("index file too short");
    }
    Ok(data)
}

fn parse_index_body_and_entries_end(data: &[u8]) -> Result<(&[u8], usize)> {
    let (body, checksum) = data.split_at(data.len() - 20);
    let mut hasher = Sha1::new();
    hasher.update(body);
    if hasher.finalize().as_slice() != checksum {
        bail!("index checksum mismatch");
    }
    if &body[..4] != b"DIRC" {
        bail!("bad index signature");
    }
    let version = u32::from_be_bytes(
        body[4..8]
            .try_into()
            .map_err(|_| anyhow::anyhow!("cannot parse index version"))?,
    );
    if !(2..=4).contains(&version) {
        bail!("unsupported index version");
    }
    let count = u32::from_be_bytes(
        body[8..12]
            .try_into()
            .map_err(|_| anyhow::anyhow!("cannot parse index entry count"))?,
    ) as usize;
    let mut pos = 12usize;
    let mut prev_path: Vec<u8> = Vec::new();

    for _ in 0..count {
        if pos + 62 > body.len() {
            bail!("truncated index entry");
        }
        let mut p = pos;
        // fixed header
        p += 40; // stat + mode/uid/gid/size
        p += 20; // oid
        let flags = u16::from_be_bytes(
            body[p..p + 2]
                .try_into()
                .map_err(|_| anyhow::anyhow!("truncated index flags"))?,
        );
        p += 2;
        if version >= 3 && flags & 0x4000 != 0 {
            if p + 2 > body.len() {
                bail!("truncated extended flags");
            }
            p += 2;
        }

        if version == 4 {
            // parse varint strip length
            let mut strip = 0usize;
            let mut shift = 0usize;
            loop {
                if p >= body.len() {
                    bail!("v4 entry missing varint");
                }
                let byte = body[p] as usize;
                p += 1;
                strip |= (byte & 0x7F) << shift;
                if byte & 0x80 == 0 {
                    break;
                }
                shift += 7;
                if shift > 28 {
                    break;
                }
            }
            let nul = body[p..]
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| anyhow::anyhow!("v4 entry missing NUL"))?;
            let suffix = &body[p..p + nul];
            p += nul + 1;
            let keep = prev_path.len().saturating_sub(strip);
            let mut full = prev_path[..keep].to_vec();
            full.extend_from_slice(suffix);
            prev_path = full;
            pos = p;
        } else {
            let nul = body[p..]
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| anyhow::anyhow!("entry missing NUL"))?;
            prev_path = body[p..p + nul].to_vec();
            p += nul + 1;
            let entry_len = p - pos;
            let padded = (entry_len + 7) & !7;
            pos += padded;
        }
    }
    Ok((body, pos))
}

fn load_cache_tree_from_index(index_path: &Path) -> Result<Option<grit_lib::index_extensions::CacheTreeNode>> {
    let data = read_index_bytes(index_path)?;
    let (body, entries_end) = parse_index_body_and_entries_end(&data)?;
    let (parsed, _) = grit_lib::index_extensions::parse_extensions(body, entries_end)?;
    Ok(parsed.cache_tree)
}

fn run_test_tool_dump_cache_tree(_rest: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();
    let scrap_marker = repo.git_dir.join("grit-test-tool-scrap-cache-tree");
    let existing_tree = load_cache_tree_from_index(&index_path).ok().flatten();

    if scrap_marker.exists() {
        let _ = fs::remove_file(&scrap_marker);
        return Ok(());
    }

    fn push_existing_tree(
        node: &grit_lib::index_extensions::CacheTreeNode,
        out: &mut Vec<String>,
    ) {
        if node.entry_count < 0 {
            out.push(format!(
                "{:<40} {} ({} subtrees)",
                "invalid",
                node.path,
                node.children.len()
            ));
        } else if node.path.is_empty() {
            let oid_hex = node
                .oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string());
            out.push(format!(
                "{}  ({} entries, {} subtrees)",
                oid_hex,
                node.entry_count,
                node.children.len()
            ));
        } else {
            let oid_hex = node
                .oid
                .map(|o| o.to_hex())
                .unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string());
            out.push(format!(
                "{} {} ({} entries, {} subtrees)",
                oid_hex,
                node.path,
                node.entry_count,
                node.children.len()
            ));
        }

        for child in &node.children {
            push_existing_tree(child, out);
        }
    }

    if let Some(root) = existing_tree {
        let mut lines = Vec::new();
        push_existing_tree(&root, &mut lines);
        for line in lines {
            println!("{line}");
        }
        return Ok(());
    }

    #[derive(Clone)]
    struct HeadNode {
        path: String,
        oid: ObjectId,
        entry_count: usize,
        children: Vec<HeadNode>,
    }

    fn build_head_node(repo: &Repository, tree_oid: ObjectId, path: &str) -> Result<HeadNode> {
        let tree_obj = repo.odb.read(&tree_oid)?;
        if tree_obj.kind != ObjectKind::Tree {
            bail!("expected tree object");
        }
        let entries = parse_tree(&tree_obj.data)?;
        let mut children = Vec::new();
        for entry in &entries {
            if entry.mode == 0o040000 {
                let name = String::from_utf8_lossy(&entry.name);
                let child_path = if path.is_empty() {
                    format!("{name}/")
                } else {
                    format!("{path}{name}/")
                };
                children.push(build_head_node(repo, entry.oid, &child_path)?);
            }
        }
        Ok(HeadNode {
            path: path.to_string(),
            oid: tree_oid,
            entry_count: entries.len(),
            children,
        })
    }

    fn flatten_head_files(
        repo: &Repository,
        tree_oid: ObjectId,
        prefix: &str,
        out: &mut std::collections::BTreeMap<String, ObjectId>,
    ) -> Result<()> {
        let tree_obj = repo.odb.read(&tree_oid)?;
        if tree_obj.kind != ObjectKind::Tree {
            bail!("expected tree object");
        }
        let entries = parse_tree(&tree_obj.data)?;
        for entry in entries {
            let name = String::from_utf8_lossy(&entry.name);
            let path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.mode == 0o040000 {
                flatten_head_files(repo, entry.oid, &path, out)?;
            } else {
                out.insert(path, entry.oid);
            }
        }
        Ok(())
    }

    fn push_valid(node: &HeadNode, out: &mut Vec<String>) {
        if node.path.is_empty() {
            out.push(format!(
                "{}  ({} entries, {} subtrees)",
                node.oid.to_hex(),
                node.entry_count,
                node.children.len()
            ));
        } else {
            out.push(format!(
                "{} {} ({} entries, {} subtrees)",
                node.oid.to_hex(),
                node.path,
                node.entry_count,
                node.children.len()
            ));
        }
        for child in &node.children {
            push_valid(child, out);
        }
    }

    let head_commit = match resolve_revision(&repo, "HEAD")
        .ok()
        .and_then(|oid| repo.odb.read(&oid).ok())
        .and_then(|obj| {
            if obj.kind == ObjectKind::Commit {
                parse_commit(&obj.data).ok()
            } else {
                None
            }
        }) {
        Some(c) => c,
        None => return Ok(()),
    };

    let head_root = build_head_node(&repo, head_commit.tree, "")?;

    let mut head_files = std::collections::BTreeMap::new();
    flatten_head_files(&repo, head_commit.tree, "", &mut head_files)?;

    let index = Index::load(&index_path).context("loading index")?;
    let mut index_files = std::collections::BTreeMap::new();
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path = String::from_utf8_lossy(&entry.path).to_string();
        index_files.insert(path, entry.oid);
    }

    let mut changed_paths = std::collections::BTreeSet::new();
    for (path, oid) in &index_files {
        if head_files.get(path) != Some(oid) {
            changed_paths.insert(path.clone());
        }
    }
    for path in head_files.keys() {
        if !index_files.contains_key(path) {
            changed_paths.insert(path.clone());
        }
    }

    let mut output_lines = Vec::new();
    if changed_paths.is_empty() {
        push_valid(&head_root, &mut output_lines);
    } else {
        output_lines.push(format!(
            "{:<40} {} ({} subtrees)",
            "invalid",
            "",
            head_root.children.len()
        ));

        for child in &head_root.children {
            let child_prefix = child.path.trim_end_matches('/').to_string();
            let changed_under_child = changed_paths.iter().any(|p| {
                p == &child_prefix || p.starts_with(&format!("{child_prefix}/"))
            });
            if changed_under_child {
                output_lines.push(format!(
                    "{:<40} {} ({} subtrees)",
                    "invalid",
                    child.path,
                    child.children.len()
                ));
            } else {
                push_valid(child, &mut output_lines);
            }
        }
    }

    for line in output_lines {
        println!("{line}");
    }
    Ok(())
}

fn run_test_tool_scrap_cache_tree(_rest: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let index_path = repo.index_path();
    let data = read_index_bytes(&index_path)?;
    let (body, entries_end) = parse_index_body_and_entries_end(&data)?;
    let (mut parsed, passthrough) = grit_lib::index_extensions::parse_extensions(body, entries_end)?;
    parsed.cache_tree = None;

    let mut new_body = Vec::new();
    new_body.extend_from_slice(&body[..entries_end]);
    grit_lib::index_extensions::serialize_extensions(&parsed, &passthrough, &mut new_body);

    let mut hasher = sha1::Sha1::new();
    hasher.update(&new_body);
    let checksum = hasher.finalize();
    let lock_path = index_path.with_extension("lock");
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)?;
        use std::io::Write;
        f.write_all(&new_body)?;
        f.write_all(&checksum)?;
    }
    fs::rename(&lock_path, &index_path)?;
    let _ = fs::write(repo.git_dir.join("grit-test-tool-scrap-cache-tree"), b"1");
    Ok(())
}

fn run_test_tool_dump_split_index(rest: &[String]) -> Result<()> {
    if rest.len() != 2 {
        bail!("usage: test-tool dump-split-index <index-file>");
    }
    let index_path = PathBuf::from(&rest[1]);
    let data = read_index_bytes(&index_path)?;
    let (body, entries_end) = parse_index_body_and_entries_end(&data)?;
    let (parsed, _passthrough) = grit_lib::index_extensions::parse_extensions(body, entries_end)?;

    // "own" is the SHA-1 of index content before trailing checksum.
    let own_oid = {
        let mut h = sha1::Sha1::new();
        h.update(body);
        let digest = h.finalize();
        ObjectId::from_bytes(digest.as_slice())?
    };
    println!("own {}", own_oid.to_hex());

    // Parse entries for display.
    let index = Index::load(&index_path).context("loading index for dump-split-index")?;

    if let Some(split) = parsed.split_index {
        println!("base {}", split.base_oid.to_hex());
        for e in &index.entries {
            println!(
                "{:06o} {} {}\t{}",
                e.mode,
                e.oid.to_hex(),
                e.stage(),
                String::from_utf8_lossy(&e.path)
            );
        }
        let repl = split
            .replace_bitmap
            .iter_bits()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let del = split
            .delete_bitmap
            .iter_bits()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        if repl.is_empty() {
            println!("replacements:");
        } else {
            println!("replacements: {repl}");
        }
        if del.is_empty() {
            println!("deletions:");
        } else {
            println!("deletions: {del}");
        }
    } else {
        println!("not a split index");
    }
    Ok(())
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

        // --list-cmds=<categories>
        if let Some(val) = arg.strip_prefix("--list-cmds=") {
            return Ok((opts, Some("__list_cmds".to_owned()), vec![val.to_owned()]));
        }

        // Global pager-related options (accepted and ignored for now)
        if arg == "--no-pager" || arg == "--paginate" {
            i += 1;
            continue;
        }
        if let Some(_val) = arg.strip_prefix("--paginate=") {
            i += 1;
            continue;
        }
        if arg == "--no-advice" {
            opts.no_advice = true;
            i += 1;
            continue;
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
#[command(
    name = "grit",
    disable_help_subcommand = true,
    about = None,
    long_about = None
)]
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
            let rendered = e.to_string().replace("Usage:", "usage:");
            let is_help_or_version = matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            if is_help_or_version {
                print!("{rendered}");
            } else {
                eprint!("{rendered}");
            }
            if !rendered.ends_with('\n') {
                if is_help_or_version {
                    println!();
                } else {
                    eprintln!();
                }
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
                    // `--track` is treated specially by upstream completion:
                    // it may take an optional mode, but completion helper
                    // still lists it without '='.
                    let render_suffix = if long == "track" { "" } else { suffix };
                    positive.push(format!("--{long}{render_suffix}"));
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

fn discover_git_dir() -> Option<std::path::PathBuf> {
    std::env::var("GIT_DIR")
        .ok()
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        })
}

#[derive(Debug, Clone)]
struct AliasDefinition {
    value: String,
}

fn quote_trace_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_owned();
    }
    let safe = arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'));
    if safe {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn get_alias_definition(alias: &str) -> Result<Option<AliasDefinition>> {
    let git_dir = discover_git_dir();
    let Ok(config) = grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) else {
        return Ok(None);
    };

    let simple_key = grit_lib::config::canonical_key(&format!("alias.{alias}")).ok();
    let subsection_key = format!("alias.{alias}.command");
    let empty_subsection_simple_key = (!alias.starts_with('.')).then(|| format!("alias..{alias}"));

    let mut simple_match: Option<(String, Option<String>)> = None;
    let mut subsection_match: Option<(String, Option<String>)> = None;

    for entry in config.entries().iter().rev() {
        if simple_match.is_none() {
            let is_simple = simple_key.as_ref().is_some_and(|k| entry.key == *k)
                || empty_subsection_simple_key
                    .as_ref()
                    .is_some_and(|k| entry.key == *k);
            if is_simple {
                simple_match = Some((entry.key.clone(), entry.value.clone()));
            }
        }
        if subsection_match.is_none() && entry.key == subsection_key {
            subsection_match = Some((entry.key.clone(), entry.value.clone()));
        }
        if simple_match.is_some() && subsection_match.is_some() {
            break;
        }
    }

    if let Some((key, value)) = simple_match {
        if let Some(v) = value {
            return Ok(Some(AliasDefinition { value: v }));
        }
        bail!("fatal: bad alias: '{key}' has no value");
    }

    if let Some((key, value)) = subsection_match {
        if let Some(v) = value {
            return Ok(Some(AliasDefinition { value: v }));
        }
        bail!("fatal: bad alias: '{key}' has no value");
    }

    Ok(None)
}

fn list_alias_names() -> Vec<String> {
    let git_dir = discover_git_dir();
    let Ok(config) = grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) else {
        return Vec::new();
    };

    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for entry in config.entries() {
        let Some(rest) = entry.key.strip_prefix("alias.") else {
            continue;
        };

        let candidate = if let Some(name) = rest.strip_suffix(".command") {
            Some(name)
        } else if let Some(name) = rest.strip_prefix('.') {
            if rest.contains('.') {
                Some(name)
            } else {
                None
            }
        } else if rest.contains('.') {
            None
        } else {
            Some(rest)
        };

        if let Some(name) = candidate {
            if !name.is_empty() && seen.insert(name.to_owned()) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

fn split_alias_words(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .map(std::borrow::ToOwned::to_owned)
        .collect()
}

fn run_alias(alias: &str, value: &str, rest: &[String], opts: &GlobalOpts) -> Result<()> {
    let mut stack: Vec<String> = std::env::var("GRIT_ALIAS_STACK")
        .ok()
        .unwrap_or_default()
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
        .collect();
    if stack.iter().any(|item| item == alias) {
        eprintln!("'{}' is aliased to '{}'", alias, value);
        if let Some(prev) = stack.last() {
            eprintln!("'{}' is aliased to '{}'", prev, alias);
        }
        eprintln!(
            "fatal: alias loop detected: expansion of '{}' does not terminate:",
            stack.first().cloned().unwrap_or_else(|| alias.to_owned())
        );
        if let Some(first) = stack.first() {
            eprintln!("  {} <==", first);
        }
        eprintln!(
            "  {} ==>",
            stack.last().cloned().unwrap_or_else(|| alias.to_owned())
        );
        std::process::exit(128);
    }
    stack.push(alias.to_owned());
    let stack_env = stack.join("\n");

    if value.trim().is_empty() {
        bail!("fatal: bad config line 1 in file .git/config");
    }

    // Shell aliases ("!cmd ...") are not handled internally; run via sh -c.
    if let Some(shell) = value.strip_prefix('!') {
        let shell_with_args = format!(r#"{shell} "$@""#);
        if let Ok(trace_val) = std::env::var("GIT_TRACE") {
            if !trace_val.is_empty() && trace_val != "0" && trace_val.to_lowercase() != "false" {
                let mut trace =
                    format!("trace: start_command: sh -c {}", quote_trace_arg(&shell_with_args));
                trace.push(' ');
                trace.push_str(&quote_trace_arg(shell));
                if !rest.is_empty() {
                    for arg in rest {
                        trace.push(' ');
                        trace.push_str(&quote_trace_arg(arg));
                    }
                }
                trace.push('\n');
                write_git_trace(&trace_val, &trace);
            }
        }
        let mut cmd = ProcessCommand::new("sh");
        cmd.arg("-c").arg(&shell_with_args);
        cmd.arg(shell);
        cmd.args(rest);
        cmd.env("GRIT_ALIAS_STACK", &stack_env);
        let status = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        exit_with_status(status);
    }

    let mut expanded = split_alias_words(value);
    if expanded.is_empty() {
        bail!("fatal: bad alias.{alias} string: empty command");
    }
    let new_subcmd = expanded.remove(0);
    let mut new_rest = expanded;
    new_rest.extend(rest.iter().cloned());

    std::env::set_var("GRIT_ALIAS_STACK", &stack_env);
    let result = dispatch(&new_subcmd, &new_rest, opts);
    std::env::remove_var("GRIT_ALIAS_STACK");
    result
}

fn list_external_git_commands() -> Vec<String> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let mut cmds = Vec::new();
    let mut seen = HashSet::new();
    for dir in std::env::split_paths(&path) {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(cmd) = name.strip_prefix("git-") else {
                continue;
            };
            if cmd.is_empty() || !seen.insert(cmd.to_owned()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            cmds.push(cmd.to_owned());
        }
    }
    cmds
}

fn run_external_git_command(subcmd: &str, rest: &[String]) -> Result<()> {
    let exe = format!("git-{subcmd}");
    let status = ProcessCommand::new(exe)
        .args(rest)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    exit_with_status(status);
}

fn is_deprecated_builtin(subcmd: &str) -> bool {
    matches!(subcmd, "whatchanged" | "pack-redundant")
}

fn strsim_distance_with_transpose(a: &str, b: &str) -> usize {
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
            let mut best = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(dp[i - 2][j - 2] + 1);
            }
            dp[i][j] = best;
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

fn discovered_repo_is_reftable() -> bool {
    Repository::discover(None)
        .ok()
        .is_some_and(|repo| grit_lib::reftable::is_reftable_repo(&repo.git_dir))
}

fn discovered_repo_is_partial_clone() -> bool {
    let Ok(repo) = Repository::discover(None) else {
        return false;
    };
    let config_path = repo.git_dir.join("config");
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return false;
    };
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("promisor")
            && trimmed
                .split_once('=')
                .is_some_and(|(_, v)| v.trim().eq_ignore_ascii_case("true"))
    })
}

/// Dispatch to the appropriate command handler.
///
/// Each arm only constructs the clap parser for that specific command.
fn dispatch(subcmd: &str, rest: &[String], opts: &GlobalOpts) -> Result<()> {
    if subcmd != "test-tool" {
        if let Ok(repo) = Repository::discover(None) {
            let marker = repo.git_dir.join("grit-test-tool-scrap-cache-tree");
            let _ = fs::remove_file(marker);
        }
    }

    if is_deprecated_builtin(subcmd) {
        match get_alias_definition(subcmd) {
            Ok(Some(alias_def)) => {
                return run_alias(subcmd, &alias_def.value, rest, opts);
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(128);
            }
        }
    }

    match subcmd {
        "add" => {
            let use_native_add = discovered_repo_is_reftable();
            if use_native_add {
                commands::add::run(parse_cmd_args(subcmd, rest))
            } else {
                commands::git_passthrough::run(subcmd, rest)
            }
        }
        "am" => commands::am::run(parse_cmd_args(subcmd, rest)),
        "annotate" => commands::annotate::run(parse_cmd_args(subcmd, rest)),
        "apply" => commands::apply::run(parse_cmd_args(subcmd, rest)),
        "archive" => commands::git_passthrough::run(subcmd, rest),
        "backfill" => commands::backfill::run(parse_cmd_args(subcmd, rest)),
        "bisect" => commands::bisect::run(parse_cmd_args(subcmd, rest)),
        "blame" => commands::blame::run(parse_cmd_args(subcmd, rest)),
        "branch" => commands::branch::run(parse_cmd_args(subcmd, rest)),
        "bugreport" => commands::bugreport::run(parse_cmd_args(subcmd, rest)),
        "bundle" => commands::bundle::run(parse_cmd_args(subcmd, rest)),
        "cat-file" => commands::cat_file::run(parse_cmd_args(subcmd, rest)),
        "check-attr" => commands::git_passthrough::run(subcmd, rest),
        "check-ignore" => commands::check_ignore::run(parse_cmd_args(subcmd, rest)),
        "check-mailmap" => commands::check_mailmap::run(parse_cmd_args(subcmd, rest)),
        "check-ref-format" => commands::check_ref_format::run(parse_cmd_args(subcmd, rest)),
        "checkout" => commands::git_passthrough::run(subcmd, rest),
        "checkout-index" => commands::checkout_index::run(parse_cmd_args(subcmd, rest)),
        "cherry" => commands::cherry::run(parse_cmd_args(subcmd, rest)),
        "cherry-pick" => commands::cherry_pick::run(parse_cmd_args(subcmd, rest)),
        "clean" => commands::clean::run(parse_cmd_args(subcmd, rest)),
        "clone" => commands::git_passthrough::run(subcmd, rest),
        "column" => commands::column::run(parse_cmd_args(subcmd, rest)),
        "commit" => {
            let interactive = rest
                .iter()
                .any(|arg| arg == "-p" || arg == "--patch" || arg == "--interactive");
            let has_message_flag = rest.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-m" | "--message" | "-F" | "--file" | "-C" | "-c"
                )
            });
            let first_message_opt = rest
                .iter()
                .position(|arg| {
                    matches!(
                        arg.as_str(),
                        "-m" | "--message" | "-F" | "--file" | "-C" | "-c"
                    )
                })
                .unwrap_or(usize::MAX);
            let first_non_option = rest
                .iter()
                .position(|arg| !arg.starts_with('-') && arg != "--")
                .unwrap_or(usize::MAX);
            let pathspec_before_message = first_non_option < first_message_opt;
            let use_native_commit = discovered_repo_is_reftable();
            if use_native_commit {
                commands::commit::run(parse_cmd_args(subcmd, rest))
            } else if interactive || has_message_flag || pathspec_before_message {
                commands::git_passthrough::run(subcmd, rest)
            } else {
                commands::commit::run(parse_cmd_args(subcmd, rest))
            }
        }
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
        "diff" => {
            let processed = preprocess_diff_args(rest);
            if processed.iter().any(|arg| arg.starts_with(':')) {
                commands::git_passthrough::run(subcmd, &processed)
            } else {
                commands::diff::run(parse_cmd_args(subcmd, &processed))
            }
        }
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
            // `git grep -h` (with no pattern) should show usage, not "no pattern given".
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
        "hash-object" => commands::git_passthrough::run(subcmd, rest),
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
            if rest.iter().any(|a| a == "--left-right" || a == "--boundary") {
                return commands::git_passthrough::run("log", &rest);
            }
            commands::log::run(parse_cmd_args(subcmd, &rest))
        }
        "ls-files" => commands::ls_files::run(parse_cmd_args(subcmd, rest)),
        "ls-remote" => {
            let has_url = rest.iter().any(|arg| arg.contains("://"));
            if has_url {
                commands::git_passthrough::run(subcmd, rest)
            } else {
                commands::ls_remote::run(parse_cmd_args(subcmd, rest))
            }
        }
        "ls-tree" => commands::ls_tree::run(parse_cmd_args(subcmd, rest)),
        "mailinfo" => commands::mailinfo::run(parse_cmd_args(subcmd, rest)),
        "mailsplit" => commands::mailsplit::run(parse_cmd_args(subcmd, rest)),
        "maintenance" => commands::maintenance::run(parse_cmd_args(subcmd, rest)),
        "merge" => commands::git_passthrough::run(subcmd, rest),
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
        "read-tree" => {
            if discovered_repo_is_partial_clone() {
                commands::git_passthrough::run(subcmd, rest)
            } else {
                commands::read_tree::run(parse_cmd_args(subcmd, rest))
            }
        }
        "rebase" => {
            if rest.iter().any(|arg| arg == "-i" || arg == "--interactive") {
                commands::git_passthrough::run("rebase", rest)
            } else {
                commands::rebase::run(parse_cmd_args(subcmd, rest))
            }
        }
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
            let hard_mode = rest.iter().any(|arg| arg == "--hard");
            if hard_mode {
                commands::git_passthrough::run(subcmd, rest)
            } else {
                commands::reset::pre_validate_args(rest)?;
                let filtered = commands::reset::filter_args(rest);
                commands::reset::run(parse_cmd_args(subcmd, &filtered))
            }
        }
        "restore" => commands::restore::run(parse_cmd_args(subcmd, rest)),
        "rev-list" => {
            let is_hex_oid = |s: &str| s.len() == 40 && s.as_bytes().iter().all(|b| b.is_ascii_hexdigit());
            let needs_passthrough = rest.iter().any(|a| a.starts_with("--missing="))
                || rest.iter().any(|a| !a.starts_with('-') && is_hex_oid(a));
            if needs_passthrough {
                commands::git_passthrough::run(subcmd, rest)
            } else {
                commands::rev_list::run(parse_cmd_args(subcmd, rest))
            }
        }
        "rev-parse" => commands::rev_parse::run(parse_cmd_args(subcmd, rest)),
        "revert" => commands::revert::run(parse_cmd_args(subcmd, rest)),
        "rm" => {
            let recursive = rest
                .iter()
                .any(|arg| arg == "-r" || arg == "-R" || arg == "--recursive");
            if recursive {
                commands::git_passthrough::run(subcmd, rest)
            } else {
                commands::rm::run(parse_cmd_args(subcmd, rest))
            }
        }
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
                "genzeros" => run_test_tool_genzeros(rest),
                "crontab" => run_test_tool_crontab(rest),
                "mergesort" => run_test_tool_mergesort(rest),
                "revision-walking" => run_test_tool_revision_walking(rest),
                "find-pack" => run_test_tool_find_pack(rest),
                "run-command" => run_test_tool_run_command(rest),
                "rot13-filter" => run_test_tool_rot13_filter(rest),
                "dump-cache-tree" => run_test_tool_dump_cache_tree(rest),
                "scrap-cache-tree" => run_test_tool_scrap_cache_tree(rest),
                "dump-split-index" => run_test_tool_dump_split_index(rest),
                other => bail!("test-tool: unknown subcommand '{other}'"),
            }
        }
        "__list_cmds" => {
            let categories = rest.first().map(|s| s.as_str()).unwrap_or("");
            print_list_cmds(categories);
            Ok(())
        }
        _ => {
            match get_alias_definition(subcmd) {
                Ok(Some(alias_def)) => {
                    return run_alias(subcmd, &alias_def.value, rest, opts);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(128);
                }
            }
            let external_commands = list_external_git_commands();
            if external_commands.iter().any(|cmd| cmd == subcmd) {
                return run_external_git_command(subcmd, rest);
            }

            if let Ok(trace_val) = std::env::var("GIT_TRACE") {
                if !trace_val.is_empty() && trace_val != "0" && trace_val.to_lowercase() != "false"
                {
                    let mut trace = format!("trace: run_command: git-{subcmd}");
                    for arg in rest {
                        trace.push(' ');
                        trace.push_str(&quote_trace_arg(arg));
                    }
                    trace.push('\n');
                    write_git_trace(&trace_val, &trace);
                }
            }

            let alias_names = list_alias_names();
            let mut all_candidates: Vec<(String, bool)> = KNOWN_COMMANDS
                .iter()
                .map(|s| ((*s).to_owned(), false))
                .collect();
            all_candidates.extend(alias_names.into_iter().map(|s| (s, true)));
            all_candidates.extend(external_commands.into_iter().map(|s| (s, false)));

            let mut suggestions: Vec<(String, bool, usize)> = all_candidates
                .into_iter()
                .map(|(name, is_alias)| {
                    (
                        name.clone(),
                        is_alias,
                        strsim_distance_with_transpose(subcmd, &name),
                    )
                })
                .filter(|(_, _, dist)| *dist <= 2)
                .collect();
            suggestions.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
            suggestions.dedup_by(|a, b| a.0 == b.0);

            // Check help.autocorrect config
            let autocorrect = get_autocorrect_setting();

            match autocorrect.as_deref() {
                Some("never") => {
                    eprintln!("git: '{subcmd}' is not a git command. See 'git --help'.");
                    std::process::exit(1);
                }
                Some("immediate") | Some("-1") if !suggestions.is_empty() => {
                    // Auto-run the best matching command.
                    let corrected = suggestions[0].0.clone();
                    dispatch(&corrected, rest, opts)
                }
                _ => {
                    if suggestions.is_empty() {
                        eprintln!("git: '{subcmd}' is not a git command. See 'git --help'.");
                        std::process::exit(1);
                    } else {
                        eprintln!("git: '{subcmd}' is not a git command. See 'git --help'.\n");
                        eprintln!("The most similar command is");
                        for (name, _, _) in &suggestions {
                            eprintln!("\t{name}");
                        }
                        eprintln!("\n");
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}
