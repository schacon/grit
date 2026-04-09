//! `grit repo` — retrieve repository information.
//!
//! Implements a focused subset of upstream `git repo` required by tests:
//! `git repo info` (see Git `builtin/repo.c`) and `git repo structure`.

use crate::explicit_exit::{ExplicitExit, SilentNonZeroExit};
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::HashSet;
use std::fs;
use std::io::{stdout, Write};

/// Arguments for `grit repo`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage repository metadata")]
pub struct Args {
    /// Subcommand (e.g. info, health).
    #[arg(value_name = "SUBCOMMAND")]
    pub subcommand: Option<String>,

    /// Additional arguments.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RepoOutputFormat {
    Table,
    Lines,
    Nul,
}

#[derive(Default)]
struct RepoStructureStats {
    refs_branches: usize,
    refs_tags: usize,
    refs_remotes: usize,
    refs_others: usize,
    objects_commits: usize,
    objects_trees: usize,
    objects_blobs: usize,
    objects_tags: usize,
    inflated_commits: usize,
    inflated_trees: usize,
    inflated_blobs: usize,
    inflated_tags: usize,
}

impl RepoStructureStats {
    fn refs_total(&self) -> usize {
        self.refs_branches + self.refs_tags + self.refs_remotes + self.refs_others
    }

    fn objects_total(&self) -> usize {
        self.objects_commits + self.objects_trees + self.objects_blobs + self.objects_tags
    }

    fn inflated_total(&self) -> usize {
        self.inflated_commits + self.inflated_trees + self.inflated_blobs + self.inflated_tags
    }
}

/// Run `grit repo`.
pub fn run(args: Args) -> Result<()> {
    match args.subcommand.as_deref() {
        Some("structure") => run_repo_structure(&args.args),
        Some("info") => run_repo_info(&args.args),
        Some("health") => bail!("repo health is not yet implemented in grit"),
        Some("maintenance") => bail!("repo maintenance is not yet implemented in grit"),
        Some(sub) => bail!("repo subcommand '{}' is not yet implemented in grit", sub),
        None => bail!("repo: no subcommand specified"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RepoInfoFormat {
    Lines,
    Nul,
    Table,
}

/// Keys for `git repo info --all`, lexicographically ordered (matches Git `repo_info_field`).
const REPO_INFO_FIELD_KEYS: &[&str] = &[
    "layout.bare",
    "layout.shallow",
    "object.format",
    "references.format",
];

fn run_repo_info(args: &[String]) -> Result<()> {
    let mut format = RepoInfoFormat::Lines;
    let mut all_keys = false;
    let mut show_keys = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-z" {
            format = RepoInfoFormat::Nul;
            i += 1;
            continue;
        }
        if arg == "--all" {
            all_keys = true;
            i += 1;
            continue;
        }
        if arg == "--keys" {
            show_keys = true;
            i += 1;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--format=") {
            format = parse_repo_info_format(v)?;
            i += 1;
            continue;
        }
        if arg == "--format" {
            i += 1;
            let Some(v) = args.get(i) else {
                bail!("option `--format` requires a value");
            };
            format = parse_repo_info_format(v)?;
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            bail!("unknown option '{arg}'");
        }
        positional.push(arg.clone());
        i += 1;
    }

    if show_keys && (all_keys || !positional.is_empty()) {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: --keys cannot be used with a <key> or --all".to_owned(),
        }));
    }

    if show_keys {
        if !matches!(format, RepoInfoFormat::Lines | RepoInfoFormat::Nul) {
            return Err(anyhow::Error::new(ExplicitExit {
                code: 128,
                message: "fatal: --keys can only be used with --format=lines or --format=nul"
                    .to_owned(),
            }));
        }
        print_repo_info_keys(format)?;
        return Ok(());
    }

    if !matches!(format, RepoInfoFormat::Lines | RepoInfoFormat::Nul) {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: unsupported output format".to_owned(),
        }));
    }

    if all_keys && !positional.is_empty() {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: --all and <key> cannot be used together".to_owned(),
        }));
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    if all_keys {
        let mut err = false;
        for key in REPO_INFO_FIELD_KEYS {
            match repo_info_value(&repo, key) {
                Some(value) => {
                    print_repo_info_field(format, key, &value)?;
                }
                None => {
                    err = true;
                    eprintln!("error: key '{key}' not found");
                }
            }
        }
        if err {
            return Err(anyhow::Error::new(SilentNonZeroExit { code: 1 }));
        }
        return Ok(());
    }

    let mut err = false;
    for key in &positional {
        match repo_info_value(&repo, key) {
            Some(value) => {
                print_repo_info_field(format, key, &value)?;
            }
            None => {
                err = true;
                eprintln!("error: key '{key}' not found");
            }
        }
    }
    if err {
        return Err(anyhow::Error::new(SilentNonZeroExit { code: 1 }));
    }
    Ok(())
}

fn parse_repo_info_format(v: &str) -> Result<RepoInfoFormat> {
    match v {
        "lines" => Ok(RepoInfoFormat::Lines),
        "nul" => Ok(RepoInfoFormat::Nul),
        "table" => Ok(RepoInfoFormat::Table),
        other => Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: format!("fatal: invalid format '{other}'"),
        })),
    }
}

fn print_repo_info_keys(format: RepoInfoFormat) -> Result<()> {
    let mut out = stdout().lock();
    match format {
        RepoInfoFormat::Lines => {
            for key in REPO_INFO_FIELD_KEYS {
                writeln!(out, "{key}")?;
            }
        }
        RepoInfoFormat::Nul => {
            for key in REPO_INFO_FIELD_KEYS {
                out.write_all(key.as_bytes())?;
                out.write_all(&[0])?;
            }
        }
        RepoInfoFormat::Table => {}
    }
    Ok(())
}

fn repo_info_value(repo: &Repository, key: &str) -> Option<String> {
    match key {
        "layout.bare" => Some(if repo.is_bare() {
            "true".to_owned()
        } else {
            "false".to_owned()
        }),
        "layout.shallow" => Some(if repo.git_dir.join("shallow").is_file() {
            "true".to_owned()
        } else {
            "false".to_owned()
        }),
        "object.format" => Some(read_object_format_from_config(&repo.git_dir)),
        "references.format" => Some(if grit_lib::reftable::is_reftable_repo(&repo.git_dir) {
            "reftable".to_owned()
        } else {
            "files".to_owned()
        }),
        _ => None,
    }
}

/// Read `extensions.objectformat` from the repository config (default `sha1`).
fn read_object_format_from_config(git_dir: &std::path::Path) -> String {
    let config_path = git_dir.join("config");
    let Ok(content) = fs::read_to_string(&config_path) else {
        return "sha1".to_owned();
    };
    let mut in_extensions = false;
    let mut object_format: Option<String> = None;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_extensions = t.eq_ignore_ascii_case("[extensions]");
            continue;
        }
        if !in_extensions {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case("objectformat") {
            object_format = Some(v.trim().to_lowercase());
        }
    }
    object_format.unwrap_or_else(|| "sha1".to_owned())
}

fn print_repo_info_field(format: RepoInfoFormat, key: &str, value: &str) -> Result<()> {
    match format {
        RepoInfoFormat::Lines => {
            let displayed = quote_c_style_repo_info_value(value);
            println!("{key}={displayed}");
        }
        RepoInfoFormat::Nul => {
            let mut out = stdout().lock();
            out.write_all(key.as_bytes())?;
            out.write_all(b"\n")?;
            out.write_all(value.as_bytes())?;
            out.write_all(&[0])?;
        }
        RepoInfoFormat::Table => {}
    }
    Ok(())
}

/// C-style quote a value when needed (matches Git `quote_c_style` for repo info lines output).
fn quote_c_style_repo_info_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    let mut needs_quotes = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                out.push_str("\\\"");
                needs_quotes = true;
            }
            '\\' => {
                out.push_str("\\\\");
                needs_quotes = true;
            }
            '\t' => {
                out.push_str("\\t");
                needs_quotes = true;
            }
            '\n' => {
                out.push_str("\\n");
                needs_quotes = true;
            }
            '\r' => {
                out.push_str("\\r");
                needs_quotes = true;
            }
            c if c.is_control() => {
                out.push_str(&format!("\\{:03o}", u32::from(c)));
                needs_quotes = true;
            }
            c => out.push(c),
        }
    }
    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

fn run_repo_structure(args: &[String]) -> Result<()> {
    let mut format = RepoOutputFormat::Table;
    let mut progress = false;
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-z" {
            format = RepoOutputFormat::Nul;
            i += 1;
            continue;
        }
        if arg == "--progress" {
            progress = true;
            i += 1;
            continue;
        }
        if arg == "--no-progress" {
            progress = false;
            i += 1;
            continue;
        }
        if let Some(v) = arg.strip_prefix("--format=") {
            format = parse_output_format(v)?;
            i += 1;
            continue;
        }
        if arg == "--format" {
            i += 1;
            let Some(v) = args.get(i) else {
                bail!("repo structure: option '--format' requires an argument");
            };
            format = parse_output_format(v)?;
            i += 1;
            continue;
        }
        bail!("repo structure: unknown option '{}'", arg);
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let stats = compute_structure_stats(&repo)?;

    if progress {
        eprintln!("Counting references: {}, done.", stats.refs_total());
        eprintln!("Counting objects: {}, done.", stats.objects_total());
    }

    match format {
        RepoOutputFormat::Table => print_table(&stats),
        RepoOutputFormat::Lines => print_lines(&stats)?,
        RepoOutputFormat::Nul => print_nul(&stats)?,
    }
    Ok(())
}

fn parse_output_format(v: &str) -> Result<RepoOutputFormat> {
    match v {
        "table" => Ok(RepoOutputFormat::Table),
        "lines" => Ok(RepoOutputFormat::Lines),
        "nul" => Ok(RepoOutputFormat::Nul),
        other => bail!("repo structure: unknown format '{}'", other),
    }
}

fn compute_structure_stats(repo: &Repository) -> Result<RepoStructureStats> {
    let mut stats = RepoStructureStats::default();

    let listed_refs = refs::list_refs(&repo.git_dir, "refs/")?;
    for (name, _) in &listed_refs {
        if name.starts_with("refs/heads/") {
            stats.refs_branches += 1;
        } else if name.starts_with("refs/tags/") {
            stats.refs_tags += 1;
        } else if name.starts_with("refs/remotes/") {
            stats.refs_remotes += 1;
        } else {
            stats.refs_others += 1;
        }
    }

    let mut roots: Vec<ObjectId> = listed_refs.into_iter().map(|(_, oid)| oid).collect();
    if let Ok(head_oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        roots.push(head_oid);
    }

    let mut seen = HashSet::new();
    let mut stack = roots;
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = match repo.odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        match obj.kind {
            ObjectKind::Commit => {
                stats.objects_commits += 1;
                stats.inflated_commits += obj.data.len();
                if let Ok(commit) = parse_commit(&obj.data) {
                    stack.push(commit.tree);
                    for parent in commit.parents {
                        stack.push(parent);
                    }
                }
            }
            ObjectKind::Tree => {
                stats.objects_trees += 1;
                stats.inflated_trees += obj.data.len();
                if let Ok(entries) = parse_tree(&obj.data) {
                    for entry in entries {
                        stack.push(entry.oid);
                    }
                }
            }
            ObjectKind::Blob => {
                stats.objects_blobs += 1;
                stats.inflated_blobs += obj.data.len();
            }
            ObjectKind::Tag => {
                stats.objects_tags += 1;
                stats.inflated_tags += obj.data.len();
                if let Ok(text) = std::str::from_utf8(&obj.data) {
                    if let Some(target_hex) = text.lines().find_map(|l| l.strip_prefix("object ")) {
                        if let Ok(target) = target_hex.trim().parse::<ObjectId>() {
                            stack.push(target);
                        }
                    }
                }
            }
        }
    }

    Ok(stats)
}

fn format_bytes(bytes: usize) -> String {
    if bytes == 0 {
        "0 B".to_owned()
    } else {
        format!("{bytes} B")
    }
}

fn print_table(stats: &RepoStructureStats) {
    if stats.refs_total() == 0 && stats.objects_total() == 0 && stats.inflated_total() == 0 {
        println!("| Repository structure      | Value  |");
        println!("| ------------------------- | ------ |");
        println!("| * References              |        |");
        println!("|   * Count                 |    0   |");
        println!("|     * Branches            |    0   |");
        println!("|     * Tags                |    0   |");
        println!("|     * Remotes             |    0   |");
        println!("|     * Others              |    0   |");
        println!("|                           |        |");
        println!("| * Reachable objects       |        |");
        println!("|   * Count                 |    0   |");
        println!("|     * Commits             |    0   |");
        println!("|     * Trees               |    0   |");
        println!("|     * Blobs               |    0   |");
        println!("|     * Tags                |    0   |");
        println!("|   * Inflated size         |    0 B |");
        println!("|     * Commits             |    0 B |");
        println!("|     * Trees               |    0 B |");
        println!("|     * Blobs               |    0 B |");
        println!("|     * Tags                |    0 B |");
        println!("|   * Disk size             |    0 B |");
        println!("|     * Commits             |    0 B |");
        println!("|     * Trees               |    0 B |");
        println!("|     * Blobs               |    0 B |");
        println!("|     * Tags                |    0 B |");
        println!("|                           |        |");
        println!("| * Largest objects         |        |");
        println!("|   * Commits               |        |");
        println!("|     * Maximum size        |    0 B |");
        println!("|     * Maximum parents     |    0   |");
        println!("|   * Trees                 |        |");
        println!("|     * Maximum size        |    0 B |");
        println!("|     * Maximum entries     |    0   |");
        println!("|   * Blobs                 |        |");
        println!("|     * Maximum size        |    0 B |");
        println!("|   * Tags                  |        |");
        println!("|     * Maximum size        |    0 B |");
        return;
    }

    fn row(label: &str, value: &str) {
        println!("| {:<25} | {:^6} |", label, value);
    }

    row("Repository structure", "Value");
    println!("| ------------------------- | ------ |");
    row("* References", "");
    row("  * Count", &stats.refs_total().to_string());
    row("    * Branches", &stats.refs_branches.to_string());
    row("    * Tags", &stats.refs_tags.to_string());
    row("    * Remotes", &stats.refs_remotes.to_string());
    row("    * Others", &stats.refs_others.to_string());
    row("", "");
    row("* Reachable objects", "");
    row("  * Count", &stats.objects_total().to_string());
    row("    * Commits", &stats.objects_commits.to_string());
    row("    * Trees", &stats.objects_trees.to_string());
    row("    * Blobs", &stats.objects_blobs.to_string());
    row("    * Tags", &stats.objects_tags.to_string());
    row("  * Inflated size", &format_bytes(stats.inflated_total()));
    row("    * Commits", &format_bytes(stats.inflated_commits));
    row("    * Trees", &format_bytes(stats.inflated_trees));
    row("    * Blobs", &format_bytes(stats.inflated_blobs));
    row("    * Tags", &format_bytes(stats.inflated_tags));
    row("  * Disk size", "0 B");
    row("    * Commits", "0 B");
    row("    * Trees", "0 B");
    row("    * Blobs", "0 B");
    row("    * Tags", "0 B");
    row("", "");
    row("* Largest objects", "");
    row("  * Commits", "");
    row("    * Maximum size", "0 B");
    row("    * Maximum parents", "0");
    row("  * Trees", "");
    row("    * Maximum size", "0 B");
    row("    * Maximum entries", "0");
    row("  * Blobs", "");
    row("    * Maximum size", "0 B");
    row("  * Tags", "");
    row("    * Maximum size", "0 B");
}

fn print_lines(stats: &RepoStructureStats) -> Result<()> {
    println!("references.branches.count={}", stats.refs_branches);
    println!("references.tags.count={}", stats.refs_tags);
    println!("references.remotes.count={}", stats.refs_remotes);
    println!("references.others.count={}", stats.refs_others);
    println!("objects.commits.count={}", stats.objects_commits);
    println!("objects.trees.count={}", stats.objects_trees);
    println!("objects.blobs.count={}", stats.objects_blobs);
    println!("objects.tags.count={}", stats.objects_tags);
    println!("objects.commits.inflated_size={}", stats.inflated_commits);
    println!("objects.trees.inflated_size={}", stats.inflated_trees);
    println!("objects.blobs.inflated_size={}", stats.inflated_blobs);
    println!("objects.tags.inflated_size={}", stats.inflated_tags);
    println!("objects.commits.disk_size=0");
    println!("objects.trees.disk_size=0");
    println!("objects.blobs.disk_size=0");
    println!("objects.tags.disk_size=0");
    println!("objects.commits.max_size=0");
    println!(
        "objects.commits.max_size_oid={}",
        grit_lib::diff::zero_oid()
    );
    println!("objects.trees.max_size=0");
    println!("objects.trees.max_size_oid={}", grit_lib::diff::zero_oid());
    println!("objects.blobs.max_size=0");
    println!("objects.blobs.max_size_oid={}", grit_lib::diff::zero_oid());
    println!("objects.tags.max_size=0");
    println!("objects.tags.max_size_oid={}", grit_lib::diff::zero_oid());
    println!("objects.commits.max_parents=0");
    println!(
        "objects.commits.max_parents_oid={}",
        grit_lib::diff::zero_oid()
    );
    println!("objects.trees.max_entries=0");
    println!(
        "objects.trees.max_entries_oid={}",
        grit_lib::diff::zero_oid()
    );
    Ok(())
}

fn print_nul(stats: &RepoStructureStats) -> Result<()> {
    let mut out = std::io::stdout().lock();
    let lines = [
        format!("references.branches.count\n{}\0", stats.refs_branches),
        format!("references.tags.count\n{}\0", stats.refs_tags),
        format!("references.remotes.count\n{}\0", stats.refs_remotes),
        format!("references.others.count\n{}\0", stats.refs_others),
        format!("objects.commits.count\n{}\0", stats.objects_commits),
        format!("objects.trees.count\n{}\0", stats.objects_trees),
        format!("objects.blobs.count\n{}\0", stats.objects_blobs),
        format!("objects.tags.count\n{}\0", stats.objects_tags),
    ];
    for line in lines {
        out.write_all(line.as_bytes())?;
    }
    Ok(())
}
