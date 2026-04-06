//! `grit rev-list` command.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::pack::{read_local_pack_indexes, verify_pack_and_collect};
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{
    collect_revision_specs_with_stdin, is_symmetric_diff, merge_bases, render_commit,
    render_commit_with_color, rev_list, split_symmetric_diff, tag_targets, ObjectFilter,
    OrderingMode, OutputMode, RevListOptions,
};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;

/// Arguments for `grit rev-list`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit rev-list`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;

    let mut options = RevListOptions::default();
    let mut disk_usage = None;
    let mut object_type_filter = None;
    let mut abbrev_len = 7usize;
    let mut revision_specs = Vec::new();
    let mut read_stdin = false;
    let mut end_of_options = false;
    let mut path_mode = false;
    let mut default_rev: Option<String> = None;
    let mut no_commit_header = false;
    let mut use_color = false;

    let mut i = 0usize;
    while i < args.args.len() {
        let arg = &args.args[i];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            path_mode = true;
            i += 1;
            continue;
        }
        if path_mode {
            options.paths.push(arg.clone());
            i += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') {
            match arg.as_str() {
                "--all" => options.all_refs = true,
                "--first-parent" => options.first_parent = true,
                "--ancestry-path" => options.ancestry_path = true,
                "--simplify-by-decoration" => options.simplify_by_decoration = true,
                "--topo-order" => options.ordering = OrderingMode::Topo,
                "--date-order" => options.ordering = OrderingMode::Date,
                "--reverse" => options.reverse = true,
                "--count" => options.count = true,
                "--parents" => options.output_mode = OutputMode::Parents,
                "--quiet" => options.quiet = true,
                "--stdin" => read_stdin = true,
                "--end-of-options" => end_of_options = true,
                "--objects" => options.objects = true,
                "--objects-edge" => options.objects = true,
                "--disk-usage" => disk_usage = Some(false),
                "--no-object-names" => options.no_object_names = true,
                "--object-names" => options.no_object_names = false,
                "--filter-provided-objects" => { /* accepted for disk-usage helper mode */ }
                "--boundary" => options.boundary = true,
                "--in-commit-order" => options.in_commit_order = true,
                "--no-kept-objects" => options.no_kept_objects = true,
                "--full-history" => options.full_history = true,
                "--sparse" => options.sparse = true,
                "--dense" => { /* default behavior, no-op */ }
                "--simplify-merges" => { /* accepted but not fully implemented */ }
                "--left-right" => options.left_right = true,
                "--left-only" => options.left_only = true,
                "--right-only" => options.right_only = true,
                "--cherry-mark" => {
                    options.cherry_mark = true;
                    options.left_right = true;
                }
                "--cherry-pick" => options.cherry_pick = true,
                "--merges" => options.min_parents = Some(2),
                "--no-merges" => options.max_parents = Some(1),
                "--cherry" => {
                    options.cherry_pick = true;
                    options.right_only = true;
                    options.left_right = true;
                }
                "-n" => {
                    let Some(value) = args.args.get(i + 1) else {
                        bail!("-n requires an argument");
                    };
                    options.max_count = Some(parse_non_negative(value, "-n")?);
                    i += 1;
                }
                _ if arg.starts_with("--max-count=") => {
                    let value = arg.trim_start_matches("--max-count=");
                    options.max_count = Some(parse_non_negative(value, "--max-count")?);
                }
                _ if arg.starts_with("--skip=") => {
                    let value = arg.trim_start_matches("--skip=");
                    options.skip = parse_non_negative(value, "--skip")?;
                }
                _ if arg.starts_with("--format=") => {
                    let value = arg.trim_start_matches("--format=").to_owned();
                    options.output_mode = OutputMode::Format(value);
                }
                _ if arg.starts_with("--pretty=") => {
                    let value = arg.trim_start_matches("--pretty=").to_owned();
                    // --pretty=format:xxx is the same as --format=format:xxx
                    // --pretty=oneline etc are named formats
                    options.output_mode = OutputMode::Format(value);
                }
                "--pretty" => {
                    // --pretty without a value defaults to medium
                    options.output_mode = OutputMode::Format("medium".to_owned());
                }
                _ if arg.starts_with("--abbrev=") => {
                    let value = arg.trim_start_matches("--abbrev=");
                    abbrev_len = parse_non_negative(value, "--abbrev")?;
                }
                _ if arg.starts_with("-n") && arg.len() > 2 => {
                    let value = &arg[2..];
                    options.max_count = Some(parse_non_negative(value, "-n")?);
                }
                _ if arg.starts_with('-')
                    && arg.len() > 1
                    && arg[1..].chars().all(|ch| ch.is_ascii_digit()) =>
                {
                    options.max_count = Some(parse_non_negative(&arg[1..], "-<n>")?);
                }
                _ if arg.starts_with("--glob=") => {
                    let pattern = arg.trim_start_matches("--glob=");
                    let matching = grit_lib::refs::list_refs_glob(&repo.git_dir, pattern)
                        .context("failed to list glob refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                "--glob" => {
                    // Detached option: next arg is the pattern.
                    i += 1;
                    if let Some(next) = args.args.get(i) {
                        let matching = grit_lib::refs::list_refs_glob(&repo.git_dir, next)
                            .context("failed to list glob refs")?;
                        for (_, oid) in matching {
                            revision_specs.push(oid.to_hex());
                        }
                    }
                }
                "--branches" => {
                    let matching = grit_lib::refs::list_refs(&repo.git_dir, "refs/heads/")
                        .context("failed to list branch refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                _ if arg.starts_with("--branches=") => {
                    let pattern = arg.trim_start_matches("--branches=");
                    let full_pattern = format!("refs/heads/{pattern}");
                    let matching = grit_lib::refs::list_refs_glob(&repo.git_dir, &full_pattern)
                        .context("failed to list branch refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                "--tags" => {
                    let matching = grit_lib::refs::list_refs(&repo.git_dir, "refs/tags/")
                        .context("failed to list tag refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                _ if arg.starts_with("--tags=") => {
                    let pattern = arg.trim_start_matches("--tags=");
                    let full_pattern = format!("refs/tags/{pattern}");
                    let matching = grit_lib::refs::list_refs_glob(&repo.git_dir, &full_pattern)
                        .context("failed to list tag refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                "--remotes" => {
                    let matching = grit_lib::refs::list_refs(&repo.git_dir, "refs/remotes/")
                        .context("failed to list remote refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                _ if arg.starts_with("--remotes=") => {
                    let pattern = arg.trim_start_matches("--remotes=");
                    let full_pattern = format!("refs/remotes/{pattern}");
                    let matching = grit_lib::refs::list_refs_glob(&repo.git_dir, &full_pattern)
                        .context("failed to list remote refs")?;
                    for (_, oid) in matching {
                        revision_specs.push(oid.to_hex());
                    }
                }
                "--alternate-refs" => {
                    // List refs from alternate object directories
                    let objects_dir = repo.git_dir.join("objects");
                    if let Ok(alts) = grit_lib::pack::read_alternates_recursive(&objects_dir) {
                        for alt_dir in alts {
                            // alt_dir is an objects dir; the git_dir is its parent
                            if let Some(alt_git_dir) = alt_dir.parent() {
                                if let Ok(refs) = grit_lib::refs::list_refs(alt_git_dir, "refs/") {
                                    for (_, oid) in refs {
                                        revision_specs.push(oid.to_hex());
                                    }
                                }
                                // Also include HEAD
                                let head_path = alt_git_dir.join("HEAD");
                                if let Ok(content) = std::fs::read_to_string(&head_path) {
                                    let content = content.trim();
                                    if let Some(ref_target) = content.strip_prefix("ref: ") {
                                        let ref_path = alt_git_dir.join(ref_target);
                                        if let Ok(oid_hex) = std::fs::read_to_string(&ref_path) {
                                            revision_specs.push(oid_hex.trim().to_string());
                                        }
                                    } else if content.len() == 40 {
                                        revision_specs.push(content.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                _ if arg.starts_with("--min-parents=") => {
                    let value = arg.trim_start_matches("--min-parents=");
                    options.min_parents = Some(parse_non_negative(value, "--min-parents")?);
                }
                _ if arg.starts_with("--max-parents=") => {
                    let value = arg.trim_start_matches("--max-parents=");
                    options.max_parents = Some(parse_non_negative(value, "--max-parents")?);
                }
                _ if arg.starts_with("--ancestry-path=") => {
                    let value = arg.trim_start_matches("--ancestry-path=");
                    let oid =
                        grit_lib::rev_parse::resolve_revision(&repo, value).with_context(|| {
                            format!("could not get commit for --ancestry-path argument {value}")
                        })?;
                    options.ancestry_path = true;
                    options.ancestry_path_bottoms.push(oid);
                }
                "--filter-print-omitted" => options.filter_print_omitted = true,
                "--no-commit-header" => no_commit_header = true,
                "--commit-header" => no_commit_header = false,
                "--color" => {
                    use_color = true;
                }
                "--no-color" => {
                    use_color = false;
                }
                _ if arg.starts_with("--color=") => {
                    let val = arg.trim_start_matches("--color=");
                    use_color = val == "always" || val == "true";
                }
                "--abbrev-commit" | "--no-abbrev-commit" => { /* silently accept */ }
                "--abbrev" => abbrev_len = 7,
                "--reflog" | "--walk-reflogs" | "-g" => {
                    options.walk_reflogs = true;
                }
                _ if arg.starts_with("--filter=") => {
                    let spec = arg.trim_start_matches("--filter=");
                    if let Some(kind) = spec.strip_prefix("object:type=") {
                        object_type_filter = Some(parse_object_type_filter(kind)?);
                    } else {
                        let filter =
                            ObjectFilter::parse(spec).map_err(|e| anyhow::anyhow!("{e}"))?;
                        options.filter = Some(filter);
                    }
                }
                _ if arg.starts_with("--disk-usage=") => {
                    let value = arg.trim_start_matches("--disk-usage=");
                    if value != "human" {
                        bail!("unsupported --disk-usage mode: {value}");
                    }
                    disk_usage = Some(true);
                }
                _ if arg.starts_with("--default") => {
                    // --default REV: use REV as default if no revisions given
                    if let Some(val) = arg.strip_prefix("--default=") {
                        default_rev = Some(val.to_string());
                    } else {
                        i += 1;
                        if let Some(val) = args.args.get(i) {
                            default_rev = Some(val.to_string());
                        }
                    }
                }
                _ => bail!("unsupported option: {arg}"),
            }
            i += 1;
            continue;
        }
        revision_specs.push(arg.clone());
        i += 1;
    }

    // Check config for color settings if not explicitly set via --color/--no-color
    if !use_color {
        if let Ok(config) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true) {
            if let Some(val) = config.get("color.diff") {
                if val == "always" || val == "true" {
                    use_color = true;
                }
            }
            if !use_color {
                if let Some(val) = config.get("color.ui") {
                    if val == "always" || val == "true" {
                        use_color = true;
                    }
                }
            }
        }
    }

    if options.simplify_by_decoration {
        // Decoration subset: keep commits pointed to by tags only.
        let decorated = tag_targets(&repo.git_dir).context("failed to list tag refs")?;
        if decorated.is_empty() {
            options.simplify_by_decoration = false;
        }
    }

    // Handle --walk-reflogs / -g mode
    if options.walk_reflogs {
        if revision_specs.is_empty() {
            bail!("error: no revisions passed to rev-list -g");
        }
        let refname = {
            let r = &revision_specs[0];
            if r == "HEAD" || r.starts_with("refs/") {
                r.clone()
            } else {
                format!("refs/heads/{r}")
            }
        };
        let entries = grit_lib::reflog::read_reflog(&repo.git_dir, &refname)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        let show_parents = options.output_mode == OutputMode::Parents;
        let max_count: usize = options.max_count.unwrap_or(usize::MAX);
        for (count, entry) in entries.iter().rev().enumerate() {
            if count >= max_count {
                break;
            }
            if show_parents {
                if let Ok(obj) = repo.odb.read(&entry.new_oid) {
                    if let Ok(commit) = grit_lib::objects::parse_commit(&obj.data) {
                        let mut line = entry.new_oid.to_hex();
                        for parent in &commit.parents {
                            line.push(' ');
                            line.push_str(&parent.to_hex());
                        }
                        writeln!(out, "{line}")?;
                        continue;
                    }
                }
            }
            writeln!(out, "{}", entry.new_oid.to_hex())?;
        }
        return Ok(());
    }

    // Apply --default when no revision specs given
    if revision_specs.is_empty() {
        if let Some(def) = default_rev {
            revision_specs.push(def);
        }
    }

    if let Some(human) = disk_usage {
        return print_disk_usage(
            &repo,
            options.all_refs,
            &revision_specs,
            object_type_filter,
            human,
        );
    }

    // Handle symmetric diff (A...B) tokens
    let mut symmetric_left: Option<String> = None;
    let mut symmetric_right: Option<String> = None;
    let mut processed_specs = Vec::new();
    for spec in &revision_specs {
        if is_symmetric_diff(spec) {
            if let Some((lhs, rhs)) = split_symmetric_diff(spec) {
                symmetric_left = Some(lhs);
                symmetric_right = Some(rhs);
            }
        } else {
            processed_specs.push(spec.clone());
        }
    }

    let (mut positive_specs, mut negative_specs, stdin_all_refs) =
        collect_revision_specs_with_stdin(&processed_specs, read_stdin)
            .context("failed to parse revision arguments")?;
    if stdin_all_refs {
        options.all_refs = true;
    }

    // If symmetric diff, resolve merge bases and set up positive/negative
    if let (Some(ref lhs), Some(ref rhs)) = (&symmetric_left, &symmetric_right) {
        let lhs_oid = grit_lib::rev_parse::resolve_revision(&repo, lhs)
            .with_context(|| format!("bad revision '{lhs}'"))?;
        let rhs_oid = grit_lib::rev_parse::resolve_revision(&repo, rhs)
            .with_context(|| format!("bad revision '{rhs}'"))?;
        let bases = merge_bases(&repo, lhs_oid, rhs_oid, options.first_parent)
            .context("failed to compute merge bases")?;
        positive_specs.push(lhs.clone());
        positive_specs.push(rhs.clone());
        for base in bases {
            negative_specs.push(base.to_hex());
        }
        // Pass symmetric OIDs to rev_list for left-right classification
        options.symmetric_left = Some(lhs_oid);
        options.symmetric_right = Some(rhs_oid);
    }

    let result =
        rev_list(&repo, &positive_specs, &negative_specs, &options).context("rev-list failed")?;

    if options.count {
        if options.left_right {
            let left_count = result
                .commits
                .iter()
                .filter(|oid| result.left_right_map.get(oid) == Some(&true))
                .count();
            let right_count = result
                .commits
                .iter()
                .filter(|oid| result.left_right_map.get(oid) == Some(&false))
                .count();
            let both_count = result.commits.len() - left_count - right_count;
            println!("{left_count}\t{right_count}\t{both_count}");
        } else {
            let mut total = result.commits.len();
            if options.objects {
                total += result.objects.len();
            }
            println!("{total}");
        }
        return Ok(());
    }
    if options.quiet {
        return Ok(());
    }

    let print_object = |oid: &grit_lib::objects::ObjectId, path: &str| {
        if options.no_object_names {
            println!("{oid}");
        } else if path.is_empty() {
            println!("{oid} ");
        } else {
            println!("{oid} {path}");
        }
    };

    {
        let mut obj_offset = 0usize;
        for (ci, oid) in result.commits.iter().enumerate() {
            let mut prefix = String::new();
            if options.left_right {
                if let Some(&is_left) = result.left_right_map.get(oid) {
                    if is_left {
                        prefix.push('<');
                    } else {
                        prefix.push('>');
                    }
                }
            }
            if options.cherry_mark {
                if result.cherry_equivalent.contains(oid) {
                    prefix = "=".to_owned();
                } else if !prefix.is_empty() {
                    prefix = "+".to_owned();
                }
            }
            match &options.output_mode {
                OutputMode::Format(fmt) => {
                    let is_oneline = fmt == "oneline";
                    let is_named_format = matches!(
                        fmt.as_str(),
                        "oneline" | "short" | "medium" | "full" | "fuller" | "email" | "raw"
                    );
                    if !no_commit_header && !is_oneline {
                        println!("commit {prefix}{oid}");
                    }
                    let rendered = render_commit_with_color(
                        &repo,
                        *oid,
                        &options.output_mode,
                        abbrev_len,
                        use_color,
                    )?;
                    if is_named_format {
                        print!("{rendered}");
                        if !rendered.ends_with('\n') {
                            println!();
                        }
                    } else {
                        println!("{rendered}");
                    }
                }
                _ => {
                    let rendered = render_commit(&repo, *oid, &options.output_mode, abbrev_len)?;
                    println!("{prefix}{rendered}");
                }
            }

            // In --in-commit-order mode, emit this commit's objects right after it
            if !result.per_commit_object_counts.is_empty() {
                let count = result
                    .per_commit_object_counts
                    .get(ci)
                    .copied()
                    .unwrap_or(0);
                for j in obj_offset..obj_offset + count {
                    if let Some((obj_oid, path)) = result.objects.get(j) {
                        print_object(obj_oid, path);
                    }
                }
                obj_offset += count;
            }
        }

        // Print remaining objects (non-in-commit-order mode, or leftovers)
        if options.objects && result.per_commit_object_counts.is_empty() {
            for (oid, path) in &result.objects {
                print_object(oid, path);
            }
        }
    }

    // Print omitted objects if --filter-print-omitted
    if options.filter_print_omitted {
        for oid in &result.omitted_objects {
            println!("~{oid}");
        }
    }

    // Print boundary commits
    if options.boundary {
        for oid in &result.boundary_commits {
            println!("-{oid}");
        }
    }

    Ok(())
}

fn parse_non_negative(text: &str, flag: &str) -> Result<usize> {
    let value = text
        .parse::<isize>()
        .with_context(|| format!("{flag} requires an integer"))?;
    if value < 0 {
        return Ok(usize::MAX);
    }
    Ok(value as usize)
}

fn parse_object_type_filter(value: &str) -> Result<ObjectKind> {
    match value {
        "commit" => Ok(ObjectKind::Commit),
        "tree" => Ok(ObjectKind::Tree),
        "blob" => Ok(ObjectKind::Blob),
        "tag" => Ok(ObjectKind::Tag),
        _ => bail!("unsupported object:type filter: {value}"),
    }
}

fn print_disk_usage(
    repo: &Repository,
    all_refs: bool,
    revision_specs: &[String],
    object_type_filter: Option<ObjectKind>,
    human: bool,
) -> Result<()> {
    let mut starts = Vec::new();
    if all_refs {
        if let Ok(oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
            starts.push(oid);
        }
        starts.extend(
            refs::list_refs(&repo.git_dir, "refs/")?
                .into_iter()
                .map(|(_, oid)| oid),
        );
    }
    for spec in revision_specs {
        starts.push(grit_lib::rev_parse::resolve_revision(repo, spec)?);
    }

    let pack_disk_sizes = collect_pack_disk_sizes(repo)?;
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for oid in starts {
        accumulate_disk_usage(
            repo,
            oid,
            object_type_filter,
            &pack_disk_sizes,
            &mut seen,
            &mut total,
        )?;
    }

    if human {
        let (value, unit) = humanize_bytes(total as usize);
        println!("{} {}", value.unwrap_or_default(), unit.unwrap_or_default());
    } else {
        println!("{total}");
    }
    Ok(())
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

fn accumulate_disk_usage(
    repo: &Repository,
    oid: ObjectId,
    object_type_filter: Option<ObjectKind>,
    pack_disk_sizes: &BTreeMap<ObjectId, u64>,
    seen: &mut HashSet<ObjectId>,
    total: &mut u64,
) -> Result<()> {
    if !seen.insert(oid) {
        return Ok(());
    }

    let object = repo.odb.read(&oid)?;
    if object_type_filter.is_none_or(|kind| kind == object.kind) {
        *total += object_disk_size(repo, oid, pack_disk_sizes)?;
    }

    match object.kind {
        ObjectKind::Commit => {
            let commit = parse_commit(&object.data)?;
            accumulate_disk_usage(
                repo,
                commit.tree,
                object_type_filter,
                pack_disk_sizes,
                seen,
                total,
            )?;
            for parent in commit.parents {
                accumulate_disk_usage(
                    repo,
                    parent,
                    object_type_filter,
                    pack_disk_sizes,
                    seen,
                    total,
                )?;
            }
        }
        ObjectKind::Tree => {
            for entry in parse_tree(&object.data)? {
                if entry.mode == 0o160000 {
                    continue;
                }
                accumulate_disk_usage(
                    repo,
                    entry.oid,
                    object_type_filter,
                    pack_disk_sizes,
                    seen,
                    total,
                )?;
            }
        }
        ObjectKind::Tag => {
            let tag = parse_tag(&object.data)?;
            accumulate_disk_usage(
                repo,
                tag.object,
                object_type_filter,
                pack_disk_sizes,
                seen,
                total,
            )?;
        }
        ObjectKind::Blob => {}
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
    Ok(fs::metadata(repo.odb.object_path(&oid))?.len())
}

fn humanize_bytes(value: usize) -> (Option<String>, Option<String>) {
    if value > (1 << 30) {
        return (
            Some(format!(
                "{}.{:02}",
                value >> 30,
                (value & ((1 << 30) - 1)) / 10_737_419
            )),
            Some("GiB".to_owned()),
        );
    }
    if value > (1 << 20) {
        let x = value + 5_243;
        return (
            Some(format!(
                "{}.{:02}",
                x >> 20,
                ((x & ((1 << 20) - 1)) * 100) >> 20
            )),
            Some("MiB".to_owned()),
        );
    }
    if value > (1 << 10) {
        let x = value + 5;
        return (
            Some(format!(
                "{}.{:02}",
                x >> 10,
                ((x & ((1 << 10) - 1)) * 100) >> 10
            )),
            Some("KiB".to_owned()),
        );
    }
    if value <= 1024 {
        return (Some(value.to_string()), Some("B".to_owned()));
    }
    (Some(value.to_string()), Some("B".to_owned()))
}
