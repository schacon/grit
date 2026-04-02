//! `grit diagnose` — generate detailed diagnostic information.
//!
//! Collects repository statistics, pack info, object counts, and
//! configuration into a zip-style directory for debugging.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::repo::Repository;
use std::fs;
use std::io;
use std::path::Path;

/// Arguments for `grit diagnose`.
#[derive(Debug, ClapArgs)]
#[command(about = "Generate diagnostic information")]
pub struct Args {
    /// Output directory (default: auto-generated timestamped name).
    #[arg(short = 'o', long = "output-directory")]
    pub output_directory: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    let mut report = String::new();

    report.push_str("=== grit diagnose ===\n\n");

    // Version
    report.push_str("[Version]\n");
    report.push_str("git version 2.47.0.grit\n\n");

    // OS info
    report.push_str("[System]\n");
    report.push_str(&format!("os: {} {}\n", std::env::consts::OS, std::env::consts::ARCH));
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(pretty) = line.strip_prefix("PRETTY_NAME=") {
                report.push_str(&format!("distro: {}\n", pretty.trim_matches('"')));
                break;
            }
        }
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    report.push_str(&format!("shell: {shell}\n"));
    report.push('\n');

    // Repository info
    report.push_str("[Repository]\n");
    match Repository::discover(None) {
        Ok(repo) => {
            report.push_str(&format!("git_dir: {}\n", repo.git_dir.display()));
            report.push_str(&format!(
                "work_tree: {}\n",
                repo.work_tree
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(bare)".to_string())
            ));
            report.push_str(&format!("is_bare: {}\n", repo.is_bare()));
            report.push('\n');

            // HEAD
            report.push_str("[HEAD]\n");
            if let Ok(head) = fs::read_to_string(repo.head_path()) {
                report.push_str(&format!("{}\n", head.trim()));
            } else {
                report.push_str("(unable to read HEAD)\n");
            }
            report.push('\n');

            // Index info
            report.push_str("[Index]\n");
            match Index::load(&repo.index_path()) {
                Ok(index) => {
                    let entries = &index.entries;
                    report.push_str(&format!("entries: {}\n", entries.len()));
                    let unmerged = entries.iter().filter(|e| e.stage() > 0).count();
                    report.push_str(&format!("unmerged: {unmerged}\n"));
                }
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    report.push_str("(no index file)\n");
                }
                Err(e) => {
                    report.push_str(&format!("(failed to load: {e})\n"));
                }
            }
            report.push('\n');

            // Pack files
            report.push_str("[Packs]\n");
            let pack_dir = repo.git_dir.join("objects/pack");
            if pack_dir.is_dir() {
                let mut pack_count = 0u32;
                let mut total_pack_size = 0u64;
                let mut idx_count = 0u32;
                if let Ok(entries) = fs::read_dir(&pack_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if name.ends_with(".pack") {
                            pack_count += 1;
                            if let Ok(meta) = fs::metadata(&path) {
                                total_pack_size += meta.len();
                            }
                        } else if name.ends_with(".idx") {
                            idx_count += 1;
                        }
                    }
                }
                report.push_str(&format!("pack files: {pack_count}\n"));
                report.push_str(&format!("idx files: {idx_count}\n"));
                report.push_str(&format!(
                    "total pack size: {}\n",
                    format_size(total_pack_size)
                ));
            } else {
                report.push_str("(no pack directory)\n");
            }
            report.push('\n');

            // Loose objects
            report.push_str("[Loose Objects]\n");
            let objects_dir = repo.git_dir.join("objects");
            let mut loose_count = 0u32;
            let mut loose_size = 0u64;
            for i in 0..=0xffu32 {
                let fanout = objects_dir.join(format!("{:02x}", i));
                if let Ok(entries) = fs::read_dir(&fanout) {
                    for entry in entries.flatten() {
                        loose_count += 1;
                        if let Ok(meta) = fs::metadata(entry.path()) {
                            loose_size += meta.len();
                        }
                    }
                }
            }
            report.push_str(&format!("count: {loose_count}\n"));
            report.push_str(&format!("size: {}\n", format_size(loose_size)));
            report.push('\n');

            // Refs
            report.push_str("[Refs]\n");
            let refs_dir = repo.refs_dir();
            let mut ref_count = 0u32;
            count_files_recursive(&refs_dir, &mut ref_count);
            report.push_str(&format!("total ref files: {ref_count}\n"));

            // Packed refs
            let packed_refs = repo.git_dir.join("packed-refs");
            if packed_refs.is_file() {
                if let Ok(content) = fs::read_to_string(&packed_refs) {
                    let packed_count = content
                        .lines()
                        .filter(|l| !l.starts_with('#') && !l.starts_with('^') && !l.is_empty())
                        .count();
                    report.push_str(&format!("packed refs: {packed_count}\n"));
                }
            }
            report.push('\n');

            // Config
            report.push_str("[Config]\n");
            match ConfigSet::load(Some(&repo.git_dir), true) {
                Ok(config) => {
                    for entry in config.entries() {
                        let key = &entry.key;
                        let raw_value = entry.value.as_deref().unwrap_or("true");
                        let value = if key.contains("password")
                            || key.contains("token")
                            || key.contains("secret")
                            || key.contains("credential")
                        {
                            "***REDACTED***"
                        } else {
                            raw_value
                        };
                        report.push_str(&format!("  {key} = {value}\n"));
                    }
                }
                Err(e) => {
                    report.push_str(&format!("  (failed to load: {e})\n"));
                }
            }
        }
        Err(e) => {
            report.push_str(&format!("(not a git repository: {e})\n"));
        }
    }

    // Determine output filename
    let filename = if let Some(ref path) = args.output_directory {
        path.clone()
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("git-diagnostics-{now}.txt")
    };

    fs::write(&filename, &report)
        .with_context(|| format!("failed to write diagnostics to {filename}"))?;

    println!("Created diagnostics report at '{filename}'");
    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn count_files_recursive(dir: &Path, count: &mut u32) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count_files_recursive(&path, count);
            } else {
                *count += 1;
            }
        }
    }
}
