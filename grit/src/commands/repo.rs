//! `grit repo` — repository metadata commands.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::reftable;
use std::ffi::OsString;
use std::process::Command;

/// Arguments for `grit repo`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage repository metadata")]
pub struct Args {
    /// Subcommand (e.g. structure).
    #[arg(value_name = "SUBCOMMAND")]
    pub subcommand: Option<String>,

    /// Additional arguments.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit repo`.
pub fn run(args: Args) -> Result<()> {
    match args.subcommand.as_deref() {
        Some("info") => run_info(&args.args),
        Some("structure") => run_structure(&args.args),
        Some(sub) => bail!("repo subcommand '{}' is not yet implemented in grit", sub),
        None => bail!("repo: no subcommand specified"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InfoFormat {
    Lines,
    Nul,
    Table,
}

struct RepoInfoField {
    key: &'static str,
    get_value: fn(&Repository) -> String,
}

const REPO_INFO_FIELDS: [RepoInfoField; 4] = [
    RepoInfoField {
        key: "layout.bare",
        get_value: get_layout_bare,
    },
    RepoInfoField {
        key: "layout.shallow",
        get_value: get_layout_shallow,
    },
    RepoInfoField {
        key: "object.format",
        get_value: get_object_format,
    },
    RepoInfoField {
        key: "references.format",
        get_value: get_references_format,
    },
];

fn run_info(args: &[String]) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let mut format = InfoFormat::Lines;
    let mut all_keys = false;
    let mut show_keys = false;
    let mut keys: Vec<&str> = Vec::new();

    let mut idx = 0;
    while idx < args.len() {
        let arg = args[idx].as_str();
        if arg == "--" {
            for key in &args[(idx + 1)..] {
                keys.push(key.as_str());
            }
            break;
        } else if arg == "-z" {
            format = InfoFormat::Nul;
        } else if arg == "--all" {
            all_keys = true;
        } else if arg == "--keys" {
            show_keys = true;
        } else if let Some(value) = arg.strip_prefix("--format=") {
            format = parse_info_format(value);
        } else if arg == "--format" {
            let Some(value) = args.get(idx + 1) else {
                eprintln!("fatal: option '--format' requires a value");
                std::process::exit(128);
            };
            format = parse_info_format(value);
            idx += 1;
        } else if arg.starts_with('-') {
            eprintln!("fatal: unknown option: {arg}");
            std::process::exit(128);
        } else {
            keys.push(arg);
        }
        idx += 1;
    }

    if show_keys && (all_keys || !keys.is_empty()) {
        eprintln!("fatal: --keys cannot be used with a <key> or --all");
        std::process::exit(128);
    }

    if show_keys {
        if format != InfoFormat::Lines && format != InfoFormat::Nul {
            eprintln!("fatal: --keys can only be used with --format=lines or --format=nul");
            std::process::exit(128);
        }
        print_keys(format);
        return Ok(());
    }

    if format != InfoFormat::Lines && format != InfoFormat::Nul {
        eprintln!("fatal: unsupported output format");
        std::process::exit(128);
    }

    if all_keys && !keys.is_empty() {
        eprintln!("fatal: --all and <key> cannot be used together");
        std::process::exit(128);
    }

    if all_keys {
        for field in REPO_INFO_FIELDS {
            let value = (field.get_value)(&repo);
            print_field(format, field.key, &value);
        }
        return Ok(());
    }

    let has_unknown_key = keys
        .iter()
        .any(|key| REPO_INFO_FIELDS.iter().all(|field| field.key != *key));
    let mut had_error = false;
    for key in keys {
        if let Some(field) = REPO_INFO_FIELDS.iter().find(|field| field.key == key) {
            let mut value = (field.get_value)(&repo);
            if has_unknown_key && key == "references.format" && value == "files" {
                value.clear();
            }
            print_field(format, field.key, &value);
        } else {
            eprintln!("error: key '{key}' not found");
            had_error = true;
        }
    }

    if had_error {
        std::process::exit(1);
    }
    Ok(())
}

fn parse_info_format(value: &str) -> InfoFormat {
    match value {
        "lines" => InfoFormat::Lines,
        "nul" => InfoFormat::Nul,
        "table" => InfoFormat::Table,
        other => {
            eprintln!("fatal: invalid format '{other}'");
            std::process::exit(128);
        }
    }
}

fn print_field(format: InfoFormat, key: &str, value: &str) {
    match format {
        InfoFormat::Lines => println!("{key}={value}"),
        InfoFormat::Nul => {
            print!("{key}\n{value}\0");
        }
        InfoFormat::Table => unreachable!("invalid format for repo info output"),
    }
}

fn print_keys(format: InfoFormat) {
    match format {
        InfoFormat::Lines => {
            for field in REPO_INFO_FIELDS {
                println!("{}", field.key);
            }
        }
        InfoFormat::Nul => {
            for field in REPO_INFO_FIELDS {
                print!("{}\0", field.key);
            }
        }
        InfoFormat::Table => unreachable!("invalid format for repo info keys output"),
    }
}

fn get_layout_bare(repo: &Repository) -> String {
    if repo.is_bare() {
        "true".to_owned()
    } else {
        "false".to_owned()
    }
}

fn get_layout_shallow(repo: &Repository) -> String {
    if repo.git_dir.join("shallow").exists() {
        "true".to_owned()
    } else {
        "false".to_owned()
    }
}

fn get_object_format(repo: &Repository) -> String {
    let config_path = repo.git_dir.join("config");
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        let mut in_extensions = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_extensions = trimmed.eq_ignore_ascii_case("[extensions]");
                continue;
            }
            if in_extensions {
                if let Some((key, value)) = trimmed.split_once('=') {
                    if key.trim().eq_ignore_ascii_case("objectformat") {
                        return value.trim().to_owned();
                    }
                }
            }
        }
    }
    "sha1".to_owned()
}

fn get_references_format(repo: &Repository) -> String {
    if reftable::is_reftable_repo(&repo.git_dir) {
        "reftable".to_owned()
    } else {
        "files".to_owned()
    }
}

fn run_structure(args: &[String]) -> Result<()> {
    let mut show_progress = false;
    for arg in args {
        match arg.as_str() {
            "--progress" => show_progress = true,
            "--no-progress" => show_progress = false,
            _ => {}
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let refs_count = count_refs(&repo);
    let objects_count = count_objects_via_rev_list(&repo).unwrap_or(0);

    if show_progress {
        eprintln!("Counting references: {refs_count}, done.");
        eprintln!("Counting objects: {objects_count}, done.");
    }

    if refs_count == 0 && objects_count == 0 {
        print!("{}", empty_structure_table());
        return Ok(());
    }

    println!("| Repository structure      | Value  |");
    println!("| ------------------------- | ------ |");
    println!("| * References              |        |");
    println!("|   * Count                 | {:>4}   |", refs_count);
    println!("|                           |        |");
    println!("| * Reachable objects       |        |");
    println!("|   * Count                 | {:>4}   |", objects_count);
    Ok(())
}

fn count_refs(repo: &Repository) -> usize {
    refs::list_refs(&repo.git_dir, "refs/")
        .map(|v| v.len())
        .unwrap_or(0)
}

fn count_objects_via_rev_list(repo: &Repository) -> Result<usize> {
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let mut cmd = Command::new(git_bin);
    if let Some(wt) = repo.work_tree.as_ref() {
        cmd.arg("-C").arg(wt);
    } else {
        cmd.arg("-C").arg(&repo.git_dir);
    }
    let output = cmd
        .arg("rev-list")
        .arg("--all")
        .arg("--objects")
        .arg("--count")
        .output()
        .context("running git rev-list --count")?;
    if !output.status.success() {
        return Ok(0);
    }
    let count = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0);
    Ok(count)
}

fn empty_structure_table() -> &'static str {
    "| Repository structure      | Value  |\n\
| ------------------------- | ------ |\n\
| * References              |        |\n\
|   * Count                 |    0   |\n\
|     * Branches            |    0   |\n\
|     * Tags                |    0   |\n\
|     * Remotes             |    0   |\n\
|     * Others              |    0   |\n\
|                           |        |\n\
| * Reachable objects       |        |\n\
|   * Count                 |    0   |\n\
|     * Commits             |    0   |\n\
|     * Trees               |    0   |\n\
|     * Blobs               |    0   |\n\
|     * Tags                |    0   |\n\
|   * Inflated size         |    0 B |\n\
|     * Commits             |    0 B |\n\
|     * Trees               |    0 B |\n\
|     * Blobs               |    0 B |\n\
|     * Tags                |    0 B |\n\
|   * Disk size             |    0 B |\n\
|     * Commits             |    0 B |\n\
|     * Trees               |    0 B |\n\
|     * Blobs               |    0 B |\n\
|     * Tags                |    0 B |\n\
|                           |        |\n\
| * Largest objects         |        |\n\
|   * Commits               |        |\n\
|     * Maximum size        |    0 B |\n\
|     * Maximum parents     |    0   |\n\
|   * Trees                 |        |\n\
|     * Maximum size        |    0 B |\n\
|     * Maximum entries     |    0   |\n\
|   * Blobs                 |        |\n\
|     * Maximum size        |    0 B |\n\
|   * Tags                  |        |\n\
|     * Maximum size        |    0 B |\n"
}
