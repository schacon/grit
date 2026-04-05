//! `grit repo` — repository metadata and structure reporting.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::pack::{read_local_pack_indexes, verify_pack_and_collect};
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Write};

/// Arguments for `grit repo`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage repository metadata")]
pub struct Args {
    /// Subcommand (e.g. info, structure).
    #[arg(value_name = "SUBCOMMAND")]
    pub subcommand: Option<String>,

    /// Additional arguments.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Table,
    Lines,
    Nul,
}

#[derive(Debug, Default, Clone)]
struct RefStats {
    branches: usize,
    tags: usize,
    remotes: usize,
    others: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct ObjectData {
    value: usize,
    oid: Option<ObjectId>,
}

#[derive(Debug, Default, Clone)]
struct LargestObjects {
    tag_size: ObjectData,
    commit_size: ObjectData,
    tree_size: ObjectData,
    blob_size: ObjectData,
    parent_count: ObjectData,
    tree_entries: ObjectData,
}

#[derive(Debug, Default, Clone)]
struct ObjectValues {
    tags: usize,
    commits: usize,
    trees: usize,
    blobs: usize,
}

#[derive(Debug, Default, Clone)]
struct ObjectStats {
    type_counts: ObjectValues,
    inflated_sizes: ObjectValues,
    disk_sizes: ObjectValues,
    largest: LargestObjects,
}

#[derive(Debug, Default, Clone)]
struct RepoStructure {
    refs: RefStats,
    objects: ObjectStats,
}

#[derive(Debug, Default, Clone)]
struct TableRow {
    name: String,
    value: Option<String>,
    unit: Option<String>,
    annotation_index: Option<usize>,
}

/// Run `grit repo`.
pub fn run(args: Args) -> Result<()> {
    match args.subcommand.as_deref() {
        Some("structure") => run_structure(&args.args),
        Some("info") => bail!("repo subcommand 'info' is not yet implemented in grit"),
        Some(sub) => bail!("repo subcommand '{sub}' is not yet implemented in grit"),
        None => bail!("repo: no subcommand specified"),
    }
}

fn run_structure(argv: &[String]) -> Result<()> {
    let mut format = OutputFormat::Table;
    let mut show_progress: Option<bool> = None;

    for arg in argv {
        if let Some(value) = arg.strip_prefix("--format=") {
            format = parse_format(value)?;
            continue;
        }
        match arg.as_str() {
            "-z" => format = OutputFormat::Nul,
            "--progress" => show_progress = Some(true),
            "--no-progress" => show_progress = Some(false),
            _ => bail!("unsupported argument to 'repo structure': {arg}"),
        }
    }

    let repo = Repository::discover(None).context("opening repository")?;
    let show_progress = show_progress.unwrap_or(false);
    let refs = refs::list_refs(&repo.git_dir, "refs/").context("listing refs")?;
    let ref_stats = count_references(&refs);

    if show_progress {
        eprintln!(
            "Counting references: {}, done.",
            ref_stats.branches + ref_stats.tags + ref_stats.remotes + ref_stats.others
        );
    }

    let stats = RepoStructure {
        refs: ref_stats,
        objects: count_objects(&repo, &refs, show_progress)?,
    };

    match format {
        OutputFormat::Table => print_table(&stats),
        OutputFormat::Lines => print_keyvalues(&stats, '=', '\n'),
        OutputFormat::Nul => print_keyvalues(&stats, '\n', '\0'),
    }
}

fn parse_format(value: &str) -> Result<OutputFormat> {
    match value {
        "table" => Ok(OutputFormat::Table),
        "lines" => Ok(OutputFormat::Lines),
        "nul" => Ok(OutputFormat::Nul),
        _ => bail!("invalid format '{value}'"),
    }
}

fn count_references(refs: &[(String, ObjectId)]) -> RefStats {
    let mut stats = RefStats::default();
    for (name, _) in refs {
        if name.starts_with("refs/heads/") {
            stats.branches += 1;
        } else if name.starts_with("refs/tags/") {
            stats.tags += 1;
        } else if name.starts_with("refs/remotes/") {
            stats.remotes += 1;
        } else {
            stats.others += 1;
        }
    }
    stats
}

fn count_objects(
    repo: &Repository,
    refs: &[(String, ObjectId)],
    show_progress: bool,
) -> Result<ObjectStats> {
    let pack_disk_sizes = collect_pack_disk_sizes(repo)?;
    let mut stats = ObjectStats::default();
    let mut seen = HashSet::new();

    for (_, oid) in refs {
        walk_object(repo, *oid, &pack_disk_sizes, &mut seen, &mut stats)?;
    }

    if show_progress {
        eprintln!(
            "Counting objects: {}, done.",
            total_object_values(&stats.type_counts)
        );
    }

    Ok(stats)
}

fn collect_pack_disk_sizes(repo: &Repository) -> Result<BTreeMap<ObjectId, u64>> {
    let mut out = BTreeMap::new();
    for idx in read_local_pack_indexes(repo.odb.objects_dir()).context("reading pack indexes")? {
        for record in verify_pack_and_collect(&idx.idx_path)
            .with_context(|| format!("verifying {:?}", idx.idx_path))?
        {
            out.insert(record.oid, record.size_in_pack);
        }
    }
    Ok(out)
}

fn walk_object(
    repo: &Repository,
    oid: ObjectId,
    pack_disk_sizes: &BTreeMap<ObjectId, u64>,
    seen: &mut HashSet<ObjectId>,
    stats: &mut ObjectStats,
) -> Result<()> {
    if !seen.insert(oid) {
        return Ok(());
    }

    let object = repo
        .odb
        .read(&oid)
        .with_context(|| format!("reading object {oid}"))?;
    let inflated_size = object.data.len();
    let disk_size = object_disk_size(repo, oid, pack_disk_sizes)?;

    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data)?;
            stats.type_counts.commits += 1;
            stats.inflated_sizes.commits += inflated_size;
            stats.disk_sizes.commits += disk_size as usize;
            update_largest(&mut stats.largest.commit_size, oid, inflated_size);
            update_largest(&mut stats.largest.parent_count, oid, commit.parents.len());

            walk_object(repo, commit.tree, pack_disk_sizes, seen, stats)?;
            for parent in commit.parents {
                walk_object(repo, parent, pack_disk_sizes, seen, stats)?;
            }
        }
        ObjectKind::Tree => {
            let entries = parse_tree(&object.data)?;
            stats.type_counts.trees += 1;
            stats.inflated_sizes.trees += inflated_size;
            stats.disk_sizes.trees += disk_size as usize;
            update_largest(&mut stats.largest.tree_size, oid, inflated_size);
            update_largest(&mut stats.largest.tree_entries, oid, entries.len());

            for entry in entries {
                if entry.mode == 0o160000 {
                    continue;
                }
                walk_object(repo, entry.oid, pack_disk_sizes, seen, stats)?;
            }
        }
        ObjectKind::Blob => {
            stats.type_counts.blobs += 1;
            stats.inflated_sizes.blobs += inflated_size;
            stats.disk_sizes.blobs += disk_size as usize;
            update_largest(&mut stats.largest.blob_size, oid, inflated_size);
        }
        ObjectKind::Tag => {
            let tag = parse_tag(&object.data)?;
            stats.type_counts.tags += 1;
            stats.inflated_sizes.tags += inflated_size;
            stats.disk_sizes.tags += disk_size as usize;
            update_largest(&mut stats.largest.tag_size, oid, inflated_size);
            walk_object(repo, tag.object, pack_disk_sizes, seen, stats)?;
        }
    }

    Ok(())
}

fn object_disk_size(
    repo: &Repository,
    oid: ObjectId,
    pack_disk_sizes: &BTreeMap<ObjectId, u64>,
) -> Result<u64> {
    if let Some(size) = pack_disk_sizes.get(&oid) {
        return Ok(*size);
    }

    let path = repo.odb.object_path(&oid);
    Ok(fs::metadata(&path)
        .with_context(|| format!("reading metadata for {}", path.display()))?
        .len())
}

fn update_largest(slot: &mut ObjectData, oid: ObjectId, value: usize) {
    if value > slot.value || slot.oid.is_none() {
        slot.value = value;
        slot.oid = Some(oid);
    }
}

fn total_object_values(values: &ObjectValues) -> usize {
    values.tags + values.commits + values.trees + values.blobs
}

fn print_keyvalues(stats: &RepoStructure, key_delim: char, value_delim: char) -> Result<()> {
    let mut out = io::stdout().lock();
    for (key, value) in structure_keyvalues(stats) {
        write!(out, "{key}{key_delim}{value}{value_delim}")?;
    }
    Ok(())
}

fn structure_keyvalues(stats: &RepoStructure) -> Vec<(String, String)> {
    let mut out = Vec::new();
    out.push((
        "references.branches.count".to_owned(),
        stats.refs.branches.to_string(),
    ));
    out.push((
        "references.tags.count".to_owned(),
        stats.refs.tags.to_string(),
    ));
    out.push((
        "references.remotes.count".to_owned(),
        stats.refs.remotes.to_string(),
    ));
    out.push((
        "references.others.count".to_owned(),
        stats.refs.others.to_string(),
    ));

    out.push((
        "objects.commits.count".to_owned(),
        stats.objects.type_counts.commits.to_string(),
    ));
    out.push((
        "objects.trees.count".to_owned(),
        stats.objects.type_counts.trees.to_string(),
    ));
    out.push((
        "objects.blobs.count".to_owned(),
        stats.objects.type_counts.blobs.to_string(),
    ));
    out.push((
        "objects.tags.count".to_owned(),
        stats.objects.type_counts.tags.to_string(),
    ));

    out.push((
        "objects.commits.inflated_size".to_owned(),
        stats.objects.inflated_sizes.commits.to_string(),
    ));
    out.push((
        "objects.trees.inflated_size".to_owned(),
        stats.objects.inflated_sizes.trees.to_string(),
    ));
    out.push((
        "objects.blobs.inflated_size".to_owned(),
        stats.objects.inflated_sizes.blobs.to_string(),
    ));
    out.push((
        "objects.tags.inflated_size".to_owned(),
        stats.objects.inflated_sizes.tags.to_string(),
    ));

    out.push((
        "objects.commits.disk_size".to_owned(),
        stats.objects.disk_sizes.commits.to_string(),
    ));
    out.push((
        "objects.trees.disk_size".to_owned(),
        stats.objects.disk_sizes.trees.to_string(),
    ));
    out.push((
        "objects.blobs.disk_size".to_owned(),
        stats.objects.disk_sizes.blobs.to_string(),
    ));
    out.push((
        "objects.tags.disk_size".to_owned(),
        stats.objects.disk_sizes.tags.to_string(),
    ));

    push_object_pair(
        &mut out,
        "objects.commits.max_size",
        stats.objects.largest.commit_size,
    );
    push_object_pair(
        &mut out,
        "objects.trees.max_size",
        stats.objects.largest.tree_size,
    );
    push_object_pair(
        &mut out,
        "objects.blobs.max_size",
        stats.objects.largest.blob_size,
    );
    push_object_pair(
        &mut out,
        "objects.tags.max_size",
        stats.objects.largest.tag_size,
    );
    push_object_pair(
        &mut out,
        "objects.commits.max_parents",
        stats.objects.largest.parent_count,
    );
    push_object_pair(
        &mut out,
        "objects.trees.max_entries",
        stats.objects.largest.tree_entries,
    );
    out
}

fn push_object_pair(out: &mut Vec<(String, String)>, key: &str, value: ObjectData) {
    out.push((key.to_owned(), value.value.to_string()));
    out.push((
        format!("{key}_oid"),
        value.oid.map(|oid| oid.to_string()).unwrap_or_default(),
    ));
}

fn print_table(stats: &RepoStructure) -> Result<()> {
    let (rows, annotations) = build_table_rows(stats);
    let name_title = "Repository structure";
    let value_title = "Value";
    let index_width = 4usize;

    let name_width = rows
        .iter()
        .map(|row| row.name.len())
        .max()
        .unwrap_or(0)
        .max(name_title.len());
    let mut value_width = rows
        .iter()
        .map(|row| row.value.as_deref().unwrap_or("").len())
        .max()
        .unwrap_or(0);
    let unit_width = rows
        .iter()
        .map(|row| row.unit.as_deref().unwrap_or("").len())
        .max()
        .unwrap_or(0);
    if value_title.len() > value_width + unit_width + 1 {
        value_width = value_title.len().saturating_sub(unit_width);
    }

    let right_width =
        (value_width + unit_width + 1).max(value_title.len() + usize::from(unit_width > 0));

    let mut out = io::stdout().lock();
    writeln!(
        out,
        "| {:<name_width$}{} | {:<right_width$} |",
        name_title,
        " ".repeat(index_width),
        value_title
    )?;
    writeln!(
        out,
        "| {} | {} |",
        "-".repeat(name_width + index_width),
        "-".repeat(right_width)
    )?;

    for row in &rows {
        let mut name_cell = format!("{:<name_width$}", row.name);
        if let Some(index) = row.annotation_index {
            name_cell.push_str(&format!(" [{index}]"));
        } else {
            name_cell.push_str(&" ".repeat(index_width));
        }

        let value = row.value.as_deref().unwrap_or("");
        let unit = row.unit.as_deref().unwrap_or("");
        let value_cell = format!(
            "{:<right_width$}",
            format!("{value:>value_width$} {unit:<unit_width$}")
        );

        writeln!(out, "| {name_cell} | {value_cell} |")?;
    }

    if !annotations.is_empty() {
        writeln!(out)?;
        for annotation in annotations {
            writeln!(out, "{annotation}")?;
        }
    }

    Ok(())
}

fn build_table_rows(stats: &RepoStructure) -> (Vec<TableRow>, Vec<String>) {
    let mut rows = Vec::new();
    let mut annotations = Vec::new();

    push_count_row(&mut rows, "* References", None, None, None);
    push_count_row(
        &mut rows,
        "  * Count",
        Some(stats.refs.branches + stats.refs.tags + stats.refs.remotes + stats.refs.others),
        None,
        None,
    );
    push_count_row(
        &mut rows,
        "    * Branches",
        Some(stats.refs.branches),
        None,
        None,
    );
    push_count_row(&mut rows, "    * Tags", Some(stats.refs.tags), None, None);
    push_count_row(
        &mut rows,
        "    * Remotes",
        Some(stats.refs.remotes),
        None,
        None,
    );
    push_count_row(
        &mut rows,
        "    * Others",
        Some(stats.refs.others),
        None,
        None,
    );

    push_count_row(&mut rows, "", None, None, None);
    push_count_row(&mut rows, "* Reachable objects", None, None, None);
    push_count_row(
        &mut rows,
        "  * Count",
        Some(total_object_values(&stats.objects.type_counts)),
        None,
        None,
    );
    push_count_row(
        &mut rows,
        "    * Commits",
        Some(stats.objects.type_counts.commits),
        None,
        None,
    );
    push_count_row(
        &mut rows,
        "    * Trees",
        Some(stats.objects.type_counts.trees),
        None,
        None,
    );
    push_count_row(
        &mut rows,
        "    * Blobs",
        Some(stats.objects.type_counts.blobs),
        None,
        None,
    );
    push_count_row(
        &mut rows,
        "    * Tags",
        Some(stats.objects.type_counts.tags),
        None,
        None,
    );

    push_size_row(
        &mut rows,
        "  * Inflated size",
        Some(total_object_values(&stats.objects.inflated_sizes)),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Commits",
        Some(stats.objects.inflated_sizes.commits),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Trees",
        Some(stats.objects.inflated_sizes.trees),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Blobs",
        Some(stats.objects.inflated_sizes.blobs),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Tags",
        Some(stats.objects.inflated_sizes.tags),
        None,
        &mut annotations,
    );

    push_size_row(
        &mut rows,
        "  * Disk size",
        Some(total_object_values(&stats.objects.disk_sizes)),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Commits",
        Some(stats.objects.disk_sizes.commits),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Trees",
        Some(stats.objects.disk_sizes.trees),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Blobs",
        Some(stats.objects.disk_sizes.blobs),
        None,
        &mut annotations,
    );
    push_size_row(
        &mut rows,
        "    * Tags",
        Some(stats.objects.disk_sizes.tags),
        None,
        &mut annotations,
    );

    push_count_row(&mut rows, "", None, None, None);
    push_count_row(&mut rows, "* Largest objects", None, None, None);
    push_count_row(&mut rows, "  * Commits", None, None, None);
    push_size_row(
        &mut rows,
        "    * Maximum size",
        Some(stats.objects.largest.commit_size.value),
        stats.objects.largest.commit_size.oid,
        &mut annotations,
    );
    push_count_row(
        &mut rows,
        "    * Maximum parents",
        Some(stats.objects.largest.parent_count.value),
        stats.objects.largest.parent_count.oid,
        Some(&mut annotations),
    );
    push_count_row(&mut rows, "  * Trees", None, None, None);
    push_size_row(
        &mut rows,
        "    * Maximum size",
        Some(stats.objects.largest.tree_size.value),
        stats.objects.largest.tree_size.oid,
        &mut annotations,
    );
    push_count_row(
        &mut rows,
        "    * Maximum entries",
        Some(stats.objects.largest.tree_entries.value),
        stats.objects.largest.tree_entries.oid,
        Some(&mut annotations),
    );
    push_count_row(&mut rows, "  * Blobs", None, None, None);
    push_size_row(
        &mut rows,
        "    * Maximum size",
        Some(stats.objects.largest.blob_size.value),
        stats.objects.largest.blob_size.oid,
        &mut annotations,
    );
    push_count_row(&mut rows, "  * Tags", None, None, None);
    push_size_row(
        &mut rows,
        "    * Maximum size",
        Some(stats.objects.largest.tag_size.value),
        stats.objects.largest.tag_size.oid,
        &mut annotations,
    );

    (rows, annotations)
}

fn push_count_row(
    rows: &mut Vec<TableRow>,
    name: &str,
    value: Option<usize>,
    oid: Option<ObjectId>,
    annotations: Option<&mut Vec<String>>,
) {
    let (value_text, unit) = match value {
        Some(v) => humanize_count(v),
        None => (None, None),
    };
    let annotation_index = if let (Some(oid), Some(annotations)) = (oid, annotations) {
        let index = annotations.len() + 1;
        annotations.push(format!("[{index}] {oid}"));
        Some(index)
    } else {
        None
    };
    rows.push(TableRow {
        name: name.to_owned(),
        value: value_text,
        unit,
        annotation_index,
    });
}

fn push_size_row(
    rows: &mut Vec<TableRow>,
    name: &str,
    value: Option<usize>,
    oid: Option<ObjectId>,
    annotations: &mut Vec<String>,
) {
    let (value_text, unit) = match value {
        Some(v) => humanize_bytes(v),
        None => (None, None),
    };
    let annotation_index = oid.map(|oid| {
        let index = annotations.len() + 1;
        annotations.push(format!("[{index}] {oid}"));
        index
    });
    rows.push(TableRow {
        name: name.to_owned(),
        value: value_text,
        unit,
        annotation_index,
    });
}

fn humanize_count(value: usize) -> (Option<String>, Option<String>) {
    if value < 1000 {
        return (Some(value.to_string()), None);
    }
    let (scaled, unit) = humanize_scaled(value as f64, 1000.0, &["k", "M", "G", "T"]);
    (Some(scaled), Some(unit.to_owned()))
}

fn humanize_bytes(value: usize) -> (Option<String>, Option<String>) {
    if value < 1024 {
        return (Some(value.to_string()), Some("B".to_owned()));
    }
    let (scaled, unit) =
        humanize_scaled(value as f64 / 1024.0, 1024.0, &["KiB", "MiB", "GiB", "TiB"]);
    (Some(scaled), Some(unit.to_owned()))
}

fn humanize_scaled<'a>(mut value: f64, scale: f64, units: &'a [&'a str]) -> (String, &'a str) {
    let mut unit = units
        .first()
        .copied()
        .unwrap_or_else(|| panic!("units must not be empty"));
    for candidate in units {
        unit = candidate;
        if value < 999.95 || *candidate == *units.last().unwrap_or(candidate) {
            break;
        }
        value /= scale;
    }
    (format!("{value:.2}"), unit)
}
