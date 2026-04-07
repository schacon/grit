//! `grit last-modified` — show when paths were last modified.

use anyhow::{anyhow, bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, CommitData, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_list::split_revision_token;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::resolve_head;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::io::{self, Write};
use std::path::{Component, Path};

/// Arguments for `grit last-modified`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show when paths were last modified")]
pub struct Args {
    /// Raw last-modified arguments (parsed manually for git-compat behavior).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
struct LastModifiedOptions {
    recursive: bool,
    show_trees: bool,
    max_depth: Option<isize>,
    nul_termination: bool,
    max_count: Option<usize>,
    positionals: Vec<String>,
    forced_pathspecs: Vec<String>,
}

#[derive(Debug, Clone)]
struct Entry {
    path: String,
}

#[derive(Debug, Clone)]
struct ResultRow {
    path: String,
    commit: ObjectId,
    boundary: bool,
}

/// Run `grit last-modified`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None)
        .context("not a git repository (or any parent up to mount point)")?;
    let options = parse_options(&args.args)?;

    let (revision_spec, raw_pathspecs) = classify_revision_and_pathspecs(&repo, &options)?;
    let (positive_specs, negative_specs) = split_revision_spec(&revision_spec);
    if positive_specs.len() > 1 {
        bail!("last-modified can only operate on one commit at a time");
    }

    let mut heads = Vec::new();
    for spec in &positive_specs {
        let oid =
            resolve_revision(&repo, spec).with_context(|| format!("bad revision '{spec}'"))?;
        let commit_oid = peel_to_commit(&repo, spec, oid)?;
        heads.push(commit_oid);
    }
    let head_oid = heads
        .first()
        .copied()
        .or_else(|| {
            resolve_head(&repo.git_dir)
                .ok()
                .and_then(|h| h.oid().copied())
        })
        .ok_or_else(|| anyhow!("cannot resolve revision"))?;

    let mut commit_cache: HashMap<ObjectId, CommitData> = HashMap::new();
    let mut path_cache: HashMap<(ObjectId, String), Option<(ObjectId, u32)>> = HashMap::new();

    let mut negative = HashSet::new();
    for spec in &negative_specs {
        let oid =
            resolve_revision(&repo, spec).with_context(|| format!("bad revision '{spec}'"))?;
        let commit_oid = peel_to_commit(&repo, spec, oid)?;
        collect_reachable_commits(&repo, &mut commit_cache, commit_oid, &mut negative)?;
    }

    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let root_prefix = working_tree_prefix(&repo, &cwd)?;
    let normalized_specs = normalize_pathspecs(&root_prefix, &raw_pathspecs);

    let head_commit = load_commit_cached(&repo, &mut commit_cache, head_oid)?;
    let target_entries = collect_target_entries(
        &repo,
        &mut path_cache,
        head_commit.tree,
        &normalized_specs,
        &options,
    )?;

    let mut results = Vec::new();
    for (idx, ent) in target_entries.into_iter().enumerate() {
        let (commit, boundary) = find_last_modified_for_path(
            &repo,
            &mut commit_cache,
            &mut path_cache,
            head_oid,
            &ent.path,
            &negative,
            options.max_count,
        )?;
        results.push((
            idx,
            ResultRow {
                path: ent.path,
                commit,
                boundary,
            },
        ));
    }

    results.sort_by(|(idx_a, a), (idx_b, b)| {
        let ta = load_commit_cached(&repo, &mut commit_cache, a.commit)
            .map(|c| commit_time(&c))
            .unwrap_or(0);
        let tb = load_commit_cached(&repo, &mut commit_cache, b.commit)
            .map(|c| commit_time(&c))
            .unwrap_or(0);
        tb.cmp(&ta)
            .then_with(|| b.boundary.cmp(&a.boundary))
            .then_with(|| idx_a.cmp(idx_b))
    });

    let mut out = io::stdout().lock();
    for (_idx, row) in results {
        if row.boundary {
            write!(out, "^")?;
        }
        write!(out, "{}\t", row.commit.to_hex())?;
        if options.nul_termination {
            write!(out, "{}\0", quote_path(&row.path))?;
        } else {
            writeln!(out, "{}", quote_path(&row.path))?;
        }
    }

    Ok(())
}

fn parse_options(args: &[String]) -> Result<LastModifiedOptions> {
    let mut opts = LastModifiedOptions {
        recursive: false,
        show_trees: false,
        max_depth: None,
        nul_termination: false,
        max_count: None,
        positionals: Vec::new(),
        forced_pathspecs: Vec::new(),
    };

    let mut i = 0usize;
    let mut forced_paths_mode = false;
    while i < args.len() {
        let arg = &args[i];
        if !forced_paths_mode && arg == "--" {
            forced_paths_mode = true;
            i += 1;
            continue;
        }

        if !forced_paths_mode && arg.starts_with('-') {
            match arg.as_str() {
                "-r" | "--recursive" => {
                    opts.recursive = true;
                    opts.max_depth = Some(-1);
                }
                "-t" | "--show-trees" => opts.show_trees = true,
                "-z" => opts.nul_termination = true,
                _ if arg.starts_with("--max-depth=") => {
                    let raw = arg.trim_start_matches("--max-depth=");
                    let val = raw
                        .parse::<isize>()
                        .with_context(|| format!("invalid --max-depth value: {raw}"))?;
                    opts.max_depth = Some(val);
                    if val < 0 {
                        opts.recursive = true;
                    }
                }
                "-1" => opts.max_count = Some(1),
                _ if arg.starts_with('-')
                    && arg.len() > 1
                    && arg[1..].chars().all(|c| c.is_ascii_digit()) =>
                {
                    let n = arg[1..]
                        .parse::<usize>()
                        .with_context(|| format!("invalid max-count argument: {arg}"))?;
                    opts.max_count = Some(n);
                }
                _ => bail!("unknown last-modified argument: {arg}"),
            }
            i += 1;
            continue;
        }

        if forced_paths_mode {
            opts.forced_pathspecs.push(arg.clone());
        } else {
            opts.positionals.push(arg.clone());
        }
        i += 1;
    }

    Ok(opts)
}

fn classify_revision_and_pathspecs(
    repo: &Repository,
    opts: &LastModifiedOptions,
) -> Result<(String, Vec<String>)> {
    let mut revision_tokens = Vec::new();
    let mut pathspecs = opts.forced_pathspecs.clone();
    let mut path_mode = false;

    for token in &opts.positionals {
        if !path_mode && token_is_revision(repo, token) {
            revision_tokens.push(token.clone());
        } else {
            path_mode = true;
            pathspecs.push(token.clone());
        }
    }

    if revision_tokens.len() > 1 {
        bail!("last-modified can only operate on one commit at a time");
    }

    let revision_spec = revision_tokens
        .first()
        .cloned()
        .unwrap_or_else(|| "HEAD".to_string());
    Ok((revision_spec, pathspecs))
}

fn token_is_revision(repo: &Repository, token: &str) -> bool {
    if token.contains("^{") {
        return resolve_revision(repo, token).is_ok();
    }

    if let Some(rest) = token.strip_prefix('^') {
        if rest.is_empty() {
            return false;
        }
        return resolve_revision(repo, rest)
            .ok()
            .is_some_and(|oid| is_commitish(repo, oid));
    }

    if token.contains("..") {
        let (pos, neg) = split_revision_token(token);
        if pos.is_empty() && neg.is_empty() {
            return false;
        }
        return pos.into_iter().chain(neg).all(|spec| {
            resolve_revision(repo, &spec)
                .ok()
                .is_some_and(|oid| is_commitish(repo, oid))
        });
    }

    resolve_revision(repo, token)
        .ok()
        .is_some_and(|oid| is_commitish(repo, oid))
}

fn is_commitish(repo: &Repository, mut oid: ObjectId) -> bool {
    loop {
        let Ok(obj) = repo.odb.read(&oid) else {
            return false;
        };
        match obj.kind {
            ObjectKind::Commit => return true,
            ObjectKind::Tag => {
                let Ok(tag) = parse_tag(&obj.data) else {
                    return false;
                };
                oid = tag.object;
            }
            _ => return false,
        }
    }
}

fn split_revision_spec(spec: &str) -> (Vec<String>, Vec<String>) {
    let (mut pos, neg) = split_revision_token(spec);
    if pos.is_empty() {
        pos.push("HEAD".to_string());
    }
    (pos, neg)
}

fn peel_to_commit(repo: &Repository, spec: &str, mut oid: ObjectId) -> Result<ObjectId> {
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Commit => return Ok(oid),
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data)?;
                oid = tag.object;
            }
            ObjectKind::Tree => bail!("revision argument '{}' is a tree, not a commit-ish", spec),
            _ => bail!("revision argument '{}' is not a commit-ish", spec),
        }
    }
}

fn load_commit_cached(
    repo: &Repository,
    cache: &mut HashMap<ObjectId, CommitData>,
    oid: ObjectId,
) -> Result<CommitData> {
    if let Some(cached) = cache.get(&oid) {
        return Ok(cached.clone());
    }
    let obj = repo.odb.read(&oid)?;
    if obj.kind != ObjectKind::Commit {
        bail!("object {} is not a commit", oid);
    }
    let commit = parse_commit(&obj.data).context("failed to parse commit")?;
    cache.insert(oid, commit.clone());
    Ok(commit)
}

fn commit_time(commit: &CommitData) -> i64 {
    commit
        .committer
        .rsplit(' ')
        .nth(1)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

fn collect_reachable_commits(
    repo: &Repository,
    commit_cache: &mut HashMap<ObjectId, CommitData>,
    start: ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    let mut stack = vec![start];
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let commit = load_commit_cached(repo, commit_cache, oid)?;
        stack.extend(commit.parents);
    }
    Ok(())
}

fn collect_target_entries(
    repo: &Repository,
    path_cache: &mut HashMap<(ObjectId, String), Option<(ObjectId, u32)>>,
    root_tree_oid: ObjectId,
    pathspecs: &[String],
    opts: &LastModifiedOptions,
) -> Result<Vec<Entry>> {
    let effective_depth = if let Some(depth) = opts.max_depth {
        if depth < 0 {
            None
        } else {
            Some(depth as usize)
        }
    } else if opts.recursive {
        None
    } else {
        Some(0)
    };

    if opts.show_trees && opts.recursive && effective_depth.is_none() {
        let mut entries = Vec::new();
        collect_recursive_with_trees(repo, root_tree_oid, "", pathspecs, &mut entries)?;
        return Ok(entries);
    }

    let mut files = Vec::new();
    collect_recursive_files(repo, root_tree_oid, "", pathspecs, &mut files)?;

    if let Some(depth) = effective_depth {
        collapse_entries(repo, path_cache, root_tree_oid, files, depth, pathspecs)
    } else {
        Ok(files)
    }
}

fn collect_recursive_files(
    repo: &Repository,
    tree_oid: ObjectId,
    prefix: &str,
    pathspecs: &[String],
    out: &mut Vec<Entry>,
) -> Result<()> {
    let obj = repo.odb.read(&tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    let entries = parse_tree(&obj.data)?;

    for ent in &entries {
        if ent.mode != 0o040000 {
            continue;
        }
        let name = String::from_utf8_lossy(&ent.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if pathspecs.is_empty() || subtree_might_match(pathspecs, &path) {
            collect_recursive_files(repo, ent.oid, &path, pathspecs, out)?;
        }
    }

    for ent in entries {
        if ent.mode == 0o040000 {
            continue;
        }
        let name = String::from_utf8_lossy(&ent.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if path_matches_any(pathspecs, &path) {
            out.push(Entry { path });
        }
    }

    Ok(())
}

fn collect_recursive_with_trees(
    repo: &Repository,
    tree_oid: ObjectId,
    prefix: &str,
    pathspecs: &[String],
    out: &mut Vec<Entry>,
) -> Result<()> {
    let obj = repo.odb.read(&tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    let entries = parse_tree(&obj.data)?;

    for ent in &entries {
        if ent.mode != 0o040000 {
            continue;
        }
        let name = String::from_utf8_lossy(&ent.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if pathspecs.is_empty() || subtree_might_match(pathspecs, &path) {
            collect_recursive_with_trees(repo, ent.oid, &path, pathspecs, out)?;
        }
    }

    if !prefix.is_empty() && path_matches_any(pathspecs, prefix) {
        out.push(Entry {
            path: prefix.to_string(),
        });
    }

    for ent in entries {
        if ent.mode == 0o040000 {
            continue;
        }
        let name = String::from_utf8_lossy(&ent.name);
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if path_matches_any(pathspecs, &path) {
            out.push(Entry { path });
        }
    }

    Ok(())
}

fn collapse_entries(
    repo: &Repository,
    path_cache: &mut HashMap<(ObjectId, String), Option<(ObjectId, u32)>>,
    root_tree_oid: ObjectId,
    entries: Vec<Entry>,
    max_depth: usize,
    pathspecs: &[String],
) -> Result<Vec<Entry>> {
    let allowed_components = allowed_components(max_depth, pathspecs);
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for entry in entries {
        let components: Vec<&str> = entry.path.split('/').collect();
        let collapsed_path = if components.len() <= allowed_components {
            entry.path.clone()
        } else {
            components[..allowed_components].join("/")
        };

        if !seen.insert(collapsed_path.clone()) {
            continue;
        }

        if collapsed_path == entry.path {
            result.push(entry);
            continue;
        }

        if resolve_path_in_tree_cached(repo, path_cache, root_tree_oid, &collapsed_path)?.is_some()
        {
            result.push(Entry {
                path: collapsed_path,
            });
        }
    }

    Ok(result)
}

fn allowed_components(max_depth: usize, pathspecs: &[String]) -> usize {
    let prefix_depth = if pathspecs.is_empty() {
        0
    } else {
        pathspecs
            .iter()
            .map(|spec| {
                let trimmed = spec.trim_end_matches('/');
                if trimmed.is_empty() {
                    0
                } else {
                    trimmed.split('/').count()
                }
            })
            .min()
            .unwrap_or(0)
    };

    if prefix_depth > 0 {
        prefix_depth + max_depth
    } else {
        max_depth + 1
    }
}

fn find_last_modified_for_path(
    repo: &Repository,
    commit_cache: &mut HashMap<ObjectId, CommitData>,
    path_cache: &mut HashMap<(ObjectId, String), Option<(ObjectId, u32)>>,
    start_oid: ObjectId,
    path: &str,
    negative: &HashSet<ObjectId>,
    max_count: Option<usize>,
) -> Result<(ObjectId, bool)> {
    let mut heap: BinaryHeap<(Reverse<i64>, ObjectId)> = BinaryHeap::new();
    let mut seen = HashSet::new();
    let start_commit = load_commit_cached(repo, commit_cache, start_oid)?;
    heap.push((Reverse(-commit_time(&start_commit)), start_oid));

    let mut popped = 0usize;
    while let Some((_k, oid)) = heap.pop() {
        if !seen.insert(oid) {
            continue;
        }
        popped += 1;

        if max_count.is_some_and(|m| popped > m) || negative.contains(&oid) {
            return Ok((oid, true));
        }

        let commit = load_commit_cached(repo, commit_cache, oid)?;
        let current_entry = resolve_path_in_tree_cached(repo, path_cache, commit.tree, path)?;
        if commit.parents.is_empty() {
            return Ok((oid, false));
        }

        let mut passed = false;
        for parent_oid in commit.parents {
            let parent = load_commit_cached(repo, commit_cache, parent_oid)?;
            let parent_entry = resolve_path_in_tree_cached(repo, path_cache, parent.tree, path)?;
            if parent_entry == current_entry {
                heap.push((Reverse(-commit_time(&parent)), parent_oid));
                passed = true;
            }
        }

        if !passed {
            return Ok((oid, false));
        }
    }

    Ok((start_oid, true))
}

fn resolve_path_in_tree_cached(
    repo: &Repository,
    cache: &mut HashMap<(ObjectId, String), Option<(ObjectId, u32)>>,
    tree_oid: ObjectId,
    path: &str,
) -> Result<Option<(ObjectId, u32)>> {
    let key = (tree_oid, path.to_string());
    if let Some(cached) = cache.get(&key) {
        return Ok(*cached);
    }

    let resolved = resolve_path_in_tree_entry(repo, tree_oid, path)?;
    cache.insert(key, resolved);
    Ok(resolved)
}

fn resolve_path_in_tree_entry(
    repo: &Repository,
    tree_oid: ObjectId,
    path: &str,
) -> Result<Option<(ObjectId, u32)>> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if parts.is_empty() {
        return Ok(None);
    }

    let mut current = tree_oid;
    for (index, part) in parts.iter().enumerate() {
        let tree_obj = repo.odb.read(&current)?;
        if tree_obj.kind != ObjectKind::Tree {
            return Ok(None);
        }
        let entries = parse_tree(&tree_obj.data)?;
        let Some(entry) = entries.iter().find(|e| e.name == part.as_bytes()) else {
            return Ok(None);
        };
        if index == parts.len() - 1 {
            return Ok(Some((entry.oid, entry.mode)));
        }
        if entry.mode != 0o040000 {
            return Ok(None);
        }
        current = entry.oid;
    }

    Ok(None)
}

fn path_matches_any(pathspecs: &[String], path: &str) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    pathspecs.iter().any(|spec| pathspec_matches(spec, path))
}

fn subtree_might_match(pathspecs: &[String], path: &str) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    pathspecs.iter().any(|spec| {
        if spec.contains('*') {
            return true;
        }
        path.starts_with(spec)
            || spec.starts_with(path)
            || spec.starts_with(&format!("{path}/"))
            || path.starts_with(&format!("{spec}/"))
    })
}

fn pathspec_matches(spec: &str, path: &str) -> bool {
    if spec.contains('*') {
        return simple_glob_match(spec, path);
    }
    path == spec || path.starts_with(&format!("{spec}/"))
}

fn simple_glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti, mut star_idx, mut match_idx) = (0usize, 0usize, None, 0usize);

    while ti < t.len() {
        if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star_idx = Some(pi);
            pi += 1;
            match_idx = ti;
        } else if let Some(si) = star_idx {
            pi = si + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }

    pi == p.len()
}

fn working_tree_prefix(repo: &Repository, cwd: &Path) -> Result<String> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(String::new());
    };
    let rel = cwd.strip_prefix(work_tree).unwrap_or(Path::new(""));
    let mut comps = Vec::new();
    for comp in rel.components() {
        if let Component::Normal(n) = comp {
            comps.push(n.to_string_lossy().to_string());
        }
    }
    if comps.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{}/", comps.join("/")))
    }
}

fn normalize_pathspecs(prefix: &str, specs: &[String]) -> Vec<String> {
    specs
        .iter()
        .map(|s| normalize_one_pathspec(prefix, s))
        .collect()
}

fn normalize_one_pathspec(prefix: &str, spec: &str) -> String {
    if spec.starts_with('/') {
        return spec.trim_start_matches('/').to_string();
    }
    let joined = if prefix.is_empty() {
        spec.to_string()
    } else {
        format!("{prefix}{spec}")
    };
    normalize_slash_path(&joined)
}

fn normalize_slash_path(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_string()),
        }
    }
    parts.join("/")
}

fn quote_path(path: &str) -> String {
    if !path.bytes().any(|b| b <= 0x20 || b == b'\\' || b == b'"') {
        return path.to_string();
    }
    let mut out = String::new();
    out.push('"');
    for b in path.as_bytes() {
        match *b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            c if c < 0x20 || c == 0x7f => out.push_str(&format!("\\{:03o}", c)),
            c => out.push(c as char),
        }
    }
    out.push('"');
    out
}
