//! `grit commit-graph` — write and verify commit-graph files.
//!
//! The commit-graph file stores commit OIDs in sorted order along with
//! their tree OIDs, parent indices, and generation numbers for faster
//! traversal.
//!
//! File format (simplified):
//!   - 8-byte header: "CGPH" + version(1) + hash_version(1) + num_chunks(1) + reserved(1)
//!   - Chunk table of contents
//!   - OID Fanout (256 × 4 bytes)
//!   - OID Lookup (N × 20 bytes, sorted)
//!   - Commit Data (N × 36 bytes: tree_oid(20) + parent1(4) + parent2(4) + generation(4) + commit_time(4))
//!   - Trailer: checksum

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use grit_lib::bloom::BloomFilterSettings;
use grit_lib::commit_graph_file::CommitGraphChain;
use grit_lib::commit_graph_write::{
    build_commit_graph_bytes, collect_reachable_commit_oids, load_commit_graph_commit_info,
    BloomWriteStats,
};
use grit_lib::config::ConfigSet;
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;

/// Arguments for `grit commit-graph`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Write and verify commit-graph files",
    override_usage = "grit commit-graph (write | verify)"
)]
pub struct Args {
    /// Optional alternate object directory.
    #[arg(long = "object-dir")]
    pub object_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: CommitGraphCommand,
}

#[derive(Debug, Subcommand)]
pub enum CommitGraphCommand {
    /// Write a commit-graph file.
    Write {
        /// Include all reachable commits.
        #[arg(long)]
        reachable: bool,
        /// Read commits from stdin.
        #[arg(long)]
        stdin_commits: bool,
        /// Read packs from stdin.
        #[arg(long)]
        stdin_packs: bool,
        /// Use changed paths Bloom filters.
        #[arg(long)]
        changed_paths: bool,
        /// Do not compute changed-path Bloom filters.
        #[arg(long = "no-changed-paths", conflicts_with = "changed_paths")]
        no_changed_paths: bool,
        /// Enable split commit-graph (`--split`, `--split=replace`, `--split=no-merge`).
        #[arg(long = "split", num_args = 0..=1, default_missing_value = "yes", value_name = "STRATEGY")]
        split: Option<String>,
        /// Set size multiple for split.
        #[arg(long)]
        size_multiple: Option<f64>,
        /// Set max commits for split.
        #[arg(long)]
        max_commits: Option<u64>,
        /// Set expire time.
        #[arg(long)]
        expire_time: Option<String>,
        /// Show progress.
        #[arg(long)]
        progress: bool,
        /// Don't show progress.
        #[arg(long)]
        no_progress: bool,
        /// Limit Bloom filters computed in this write (Git `commitGraph.maxNewFilters`).
        #[arg(long = "max-new-filters")]
        max_new_filters: Option<u32>,
    },
    /// Verify an existing commit-graph file.
    Verify {
        /// Enable shallow mode.
        #[arg(long)]
        shallow: bool,
        /// Show progress.
        #[arg(long)]
        progress: bool,
        /// Don't show progress.
        #[arg(long)]
        no_progress: bool,
    },
}

// ── Constants ──────────────────────────────────────────────────────────
const SIGNATURE: &[u8; 4] = b"CGPH";
const VERSION: u8 = 1;
const HASH_VERSION_SHA1: u8 = 1;
const HASH_LEN: usize = 20;

// Chunk IDs (verify)
const CHUNK_OID_FANOUT: u32 = 0x4f494446; // "OIDF"
const CHUNK_OID_LOOKUP: u32 = 0x4f49444c; // "OIDL"
const CHUNK_COMMIT_DATA: u32 = 0x43444154; // "CDAT"

/// Run `grit commit-graph`.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        CommitGraphCommand::Write {
            reachable,
            stdin_commits,
            stdin_packs,
            changed_paths,
            no_changed_paths,
            split,
            size_multiple: _,
            max_commits: _,
            expire_time: _,
            progress,
            no_progress,
            max_new_filters,
        } => cmd_write(
            args.object_dir,
            reachable,
            stdin_commits,
            stdin_packs,
            changed_paths,
            no_changed_paths,
            split.as_deref(),
            progress,
            no_progress,
            max_new_filters,
        ),
        CommitGraphCommand::Verify { .. } => cmd_verify(args.object_dir),
    }
}

fn progress_delay_secs() -> u64 {
    std::env::var("GIT_PROGRESS_DELAY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2)
}

fn should_show_commit_graph_progress(progress: bool, no_progress: bool) -> bool {
    if no_progress {
        return false;
    }
    if progress {
        return true;
    }
    std::io::stderr().is_terminal()
}

// ── Write ──────────────────────────────────────────────────────────────

fn read_stdin_commit_seeds() -> Result<HashSet<ObjectId>> {
    let text = std::io::read_to_string(std::io::stdin()).context("reading --stdin-commits")?;
    let mut out = HashSet::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let oid = ObjectId::from_hex(t).with_context(|| format!("invalid commit id '{t}'"))?;
        out.insert(oid);
    }
    Ok(out)
}

fn walk_commit_closure(odb: &Odb, seeds: HashSet<ObjectId>) -> Result<HashSet<ObjectId>> {
    let mut commits = HashSet::new();
    let mut stack: Vec<ObjectId> = seeds.into_iter().collect();
    while let Some(oid) = stack.pop() {
        if !commits.insert(oid) {
            continue;
        }
        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        if obj.kind != ObjectKind::Commit {
            if obj.kind == ObjectKind::Tag {
                if let Ok(text) = std::str::from_utf8(&obj.data) {
                    for line in text.lines() {
                        if let Some(rest) = line.strip_prefix("object ") {
                            if let Ok(target) = ObjectId::from_hex(rest.trim()) {
                                stack.push(target);
                            }
                        }
                    }
                }
            }
            continue;
        }
        let commit = parse_commit(&obj.data)?;
        for p in &commit.parents {
            stack.push(*p);
        }
    }
    Ok(commits)
}

fn trace2_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn trace2_json_data(path: &str, category: &str, key: &str, value: &str) {
    let now = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let total_secs = now.as_secs();
        let micros = now.subsec_micros();
        let secs_in_day = total_secs % 86400;
        let hours = secs_in_day / 3600;
        let mins = (secs_in_day % 3600) / 60;
        let secs = secs_in_day % 60;
        format!("{:02}:{:02}:{:02}.{:06}", hours, mins, secs, micros)
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::Path::new(path))
    {
        let value_esc = trace2_json_escape(value);
        let _ = writeln!(
            f,
            r#"{{"event":"data","sid":"grit-0","time":"{}","category":"{}","key":"{}","value":"{}"}}"#,
            now, category, key, value_esc
        );
    }
}

fn emit_commit_graph_trace2(settings: &BloomFilterSettings, stats: &BloomWriteStats) {
    let Ok(path) = std::env::var("GIT_TRACE2_EVENT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let now = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let total_secs = now.as_secs();
        let micros = now.subsec_micros();
        let secs_in_day = total_secs % 86400;
        let hours = secs_in_day / 3600;
        let mins = (secs_in_day % 3600) / 60;
        let secs = secs_in_day % 60;
        format!("{:02}:{:02}:{:02}.{:06}", hours, mins, secs, micros)
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::Path::new(&path))
    {
        let settings_json = format!(
            "{{\"hash_version\":{},\"num_hashes\":{},\"bits_per_entry\":{},\"max_changed_paths\":{}}}",
            settings.hash_version,
            settings.num_hashes,
            settings.bits_per_entry,
            settings.max_changed_paths
        );
        let _ = writeln!(
            f,
            r#"{{"event":"data_json","sid":"grit-0","time":"{}","category":"bloom","key":"settings","value":{}}}"#,
            now, settings_json
        );
        let _ = writeln!(
            f,
            r#"{{"event":"data_json","sid":"grit-0","time":"{}","category":"commit-graph","key":"filter-computed","value":"{}"}}"#,
            now, stats.filter_computed
        );
        let _ = writeln!(
            f,
            r#"{{"event":"data_json","sid":"grit-0","time":"{}","category":"commit-graph","key":"filter-not-computed","value":"{}"}}"#,
            now, stats.filter_not_computed
        );
        let _ = writeln!(
            f,
            r#"{{"event":"data_json","sid":"grit-0","time":"{}","category":"commit-graph","key":"filter-trunc-empty","value":"{}"}}"#,
            now, stats.filter_trunc_empty
        );
        let _ = writeln!(
            f,
            r#"{{"event":"data_json","sid":"grit-0","time":"{}","category":"commit-graph","key":"filter-trunc-large","value":"{}"}}"#,
            now, stats.filter_trunc_large
        );
        let _ = writeln!(
            f,
            r#"{{"event":"data_json","sid":"grit-0","time":"{}","category":"commit-graph","key":"filter-upgraded","value":"{}"}}"#,
            now, stats.filter_upgraded
        );
    }
}

fn commit_graph_layer_id_hash(path: &Path) -> Option<[u8; 20]> {
    let raw = fs::read(path).ok()?;
    if raw.len() < 40 {
        return None;
    }
    let body = &raw[..raw.len() - 20];
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(body);
    Some(h.finalize().into())
}

fn cmd_write(
    object_dir: Option<PathBuf>,
    _reachable: bool,
    stdin_commits: bool,
    stdin_packs: bool,
    changed_paths: bool,
    no_changed_paths: bool,
    split: Option<&str>,
    progress: bool,
    no_progress: bool,
    max_new_filters_cli: Option<u32>,
) -> Result<()> {
    if stdin_packs {
        bail!("commit-graph write --stdin-packs is not implemented yet");
    }
    let repo = Repository::discover(None)?;
    let objects_dir = object_dir.unwrap_or_else(|| repo.git_dir.join("objects"));
    let odb = Odb::new(&objects_dir);
    let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();

    let core_on = cfg
        .get_bool("core.commitgraph")
        .and_then(|r| r.ok())
        .unwrap_or(true);
    if !core_on {
        eprintln!(
            "warning: attempting to write a commit-graph, but 'core.commitGraph' is disabled"
        );
        return Ok(());
    }

    let ver = cfg
        .get("commitgraph.changedpathsversion")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(-1);
    if ver < -1 || ver > 2 {
        eprintln!(
            "warning: attempting to write a commit-graph, but 'commitGraph.changedPathsVersion' ({ver}) is not supported"
        );
        return Ok(());
    }

    let mut bloom = BloomFilterSettings {
        hash_version: if ver == 2 { 2 } else { 1 },
        num_hashes: 7,
        bits_per_entry: 10,
        max_changed_paths: 512,
    };
    if let Some(ref chain) = CommitGraphChain::load(&objects_dir) {
        if let Some(bs) = chain.top_layer_bloom_settings() {
            if ver == -1 {
                bloom.hash_version = bs.hash_version;
            }
            bloom.num_hashes = bs.num_hashes;
            bloom.bits_per_entry = bs.bits_per_entry;
            bloom.max_changed_paths = bs.max_changed_paths;
        }
    }
    if let Ok(s) = std::env::var("GIT_TEST_BLOOM_SETTINGS_NUM_HASHES") {
        if let Ok(n) = s.parse::<u32>() {
            bloom.num_hashes = n;
        }
    }
    if let Ok(s) = std::env::var("GIT_TEST_BLOOM_SETTINGS_BITS_PER_ENTRY") {
        if let Ok(n) = s.parse::<u32>() {
            bloom.bits_per_entry = n;
        }
    }
    if let Ok(s) = std::env::var("GIT_TEST_BLOOM_SETTINGS_MAX_CHANGED_PATHS") {
        if let Ok(n) = s.parse::<u32>() {
            bloom.max_changed_paths = n;
        }
    }
    bloom.hash_version = if bloom.hash_version == 2 { 2 } else { 1 };

    let max_new = max_new_filters_cli
        .or_else(|| {
            std::env::var("GIT_TEST_MAX_NEW_FILTERS")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
        })
        .or_else(|| {
            cfg.get("commitgraph.maxnewfilters")
                .and_then(|s| s.parse::<u32>().ok())
        });

    let mut commit_set = if stdin_commits {
        let seeds = read_stdin_commit_seeds()?;
        if seeds.is_empty() {
            return Ok(());
        }
        walk_commit_closure(&odb, seeds)?
    } else {
        collect_reachable_commit_oids(&repo.git_dir, &odb)?
    };

    if commit_set.is_empty() {
        return Ok(());
    }

    let split_enabled = split.is_some();
    let replace = split
        .map(|s| s.eq_ignore_ascii_case("replace"))
        .unwrap_or(false);

    let info_dir = objects_dir.join("info");
    let graph_path = info_dir.join("commit-graph");
    let graphs_dir = info_dir.join("commit-graphs");
    let chain_path = graphs_dir.join("commit-graph-chain");

    if split_enabled && !replace {
        fs::create_dir_all(&graphs_dir)?;
        let chain_empty = !chain_path.is_file()
            || fs::read_to_string(&chain_path)
                .map(|s| {
                    s.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .next()
                        .is_none()
                })
                .unwrap_or(true);
        if chain_empty && graph_path.is_file() {
            let Some(hash) = commit_graph_layer_id_hash(&graph_path) else {
                bail!(
                    "existing commit-graph at {:?} is too small to migrate",
                    graph_path
                );
            };
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            let dest = graphs_dir.join(format!("graph-{hex}.graph"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&graph_path, fs::Permissions::from_mode(0o644));
            }
            fs::rename(&graph_path, &dest)
                .with_context(|| format!("migrating {:?} to {:?}", graph_path, dest))?;
            fs::write(&chain_path, format!("{hex}\n"))
                .with_context(|| format!("writing {:?}", chain_path))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o444));
            }
        }
    }

    let existing_chain = if split_enabled && !replace {
        CommitGraphChain::load(&objects_dir)
    } else {
        None
    };

    if !split_enabled || (split_enabled && replace) {
        if let Some(chain) = CommitGraphChain::load(&objects_dir) {
            for oid in chain.all_oids_in_order() {
                commit_set.insert(oid);
            }
        }
    }

    let base_hashes: Vec<[u8; 20]> = if split_enabled && !replace {
        let mut hashes = Vec::new();
        if let Some(ref chain) = existing_chain {
            for path in chain.layer_paths_oldest_first() {
                if let Some(h) = commit_graph_layer_id_hash(&path) {
                    hashes.push(h);
                }
            }
        } else if graph_path.is_file() {
            if let Some(h) = commit_graph_layer_id_hash(&graph_path) {
                hashes.push(h);
            }
        }
        hashes
    } else {
        Vec::new()
    };

    let base_oids: HashSet<ObjectId> = existing_chain
        .as_ref()
        .map(|c| c.all_oids_in_order().into_iter().collect())
        .unwrap_or_default();

    let mut layer_oids: Vec<ObjectId> = commit_set.into_iter().collect();
    if split_enabled && !replace && !base_oids.is_empty() {
        layer_oids.retain(|o| !base_oids.contains(o));
    }
    layer_oids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));

    if layer_oids.is_empty() && !(split_enabled && replace) {
        return Ok(());
    }

    let mut infos = HashMap::new();
    for oid in &layer_oids {
        infos.insert(*oid, load_commit_graph_commit_info(&odb, *oid)?);
    }

    let show_progress = should_show_commit_graph_progress(progress, no_progress);
    if show_progress {
        let delay = progress_delay_secs();
        if delay > 0 {
            thread::sleep(Duration::from_secs(delay));
        }
        eprintln!(
            "Computing commit graph generation numbers: 100% ({n}/{n}), done.",
            n = layer_oids.len()
        );
    }

    let write_bloom = changed_paths && !no_changed_paths;
    let (base_for_build, hashes_for_build): (Option<&CommitGraphChain>, &[[u8; 20]]) =
        if split_enabled && !replace {
            (existing_chain.as_ref(), &base_hashes)
        } else {
            (None, &[])
        };

    let (bytes, bstats) = build_commit_graph_bytes(
        &layer_oids,
        &infos,
        &odb,
        write_bloom,
        &bloom,
        base_for_build,
        hashes_for_build,
        max_new,
    )?;

    if write_bloom {
        emit_commit_graph_trace2(&bloom, &bstats);
    }

    let file_hash: [u8; 20] = {
        let body = &bytes[..bytes.len().saturating_sub(20)];
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(body);
        h.finalize().into()
    };
    let hex_hash: String = file_hash.iter().map(|b| format!("{b:02x}")).collect();

    fs::create_dir_all(&info_dir)?;
    if split_enabled {
        fs::create_dir_all(&graphs_dir)?;
        if replace {
            let _ = fs::remove_file(&chain_path);
            if graphs_dir.is_dir() {
                for entry in fs::read_dir(&graphs_dir)? {
                    let entry = entry?;
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e == "graph") {
                        let _ = fs::remove_file(&p);
                    }
                }
            }
            fs::create_dir_all(&graphs_dir)?;
        }
        let layer_path = graphs_dir.join(format!("graph-{hex_hash}.graph"));
        fs::write(&layer_path, &bytes).with_context(|| format!("writing {:?}", layer_path))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&layer_path, fs::Permissions::from_mode(0o444));
        }
        let mut chain_lines: Vec<String> = if replace {
            Vec::new()
        } else {
            fs::read_to_string(&chain_path)
                .unwrap_or_default()
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        chain_lines.insert(0, hex_hash);
        fs::write(&chain_path, format!("{}\n", chain_lines.join("\n")))
            .with_context(|| format!("writing {:?}", chain_path))?;
        let _ = fs::remove_file(&graph_path);
    } else {
        if graphs_dir.is_dir() {
            for entry in fs::read_dir(&graphs_dir)? {
                let entry = entry?;
                let p = entry.path();
                if p.is_file() {
                    let _ = fs::remove_file(&p);
                }
            }
            let _ = fs::remove_dir(&graphs_dir);
        }
        let _ = fs::remove_file(&chain_path);
        #[cfg(unix)]
        if graph_path.is_file() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&graph_path, fs::Permissions::from_mode(0o644));
        }
        let file =
            fs::File::create(&graph_path).with_context(|| format!("creating {:?}", graph_path))?;
        let mut w = BufWriter::new(file);
        w.write_all(&bytes)?;
        w.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&graph_path, fs::Permissions::from_mode(0o444));
        }
    }

    Ok(())
}

fn sha1_hash(data: &[u8]) -> [u8; 20] {
    use std::process::Command;
    // Use sha1sum or openssl for hashing
    let child = Command::new("sha1sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(data);
            }
            #[allow(clippy::unwrap_used)]
            let output = child.wait_with_output().unwrap();
            let hex = String::from_utf8_lossy(&output.stdout);
            let hex = hex.split_whitespace().next().unwrap_or("");
            let mut hash = [0u8; 20];
            for i in 0..20 {
                if i * 2 + 2 <= hex.len() {
                    hash[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
                }
            }
            hash
        }
        Err(_) => [0u8; 20],
    }
}

// ── Verify ─────────────────────────────────────────────────────────────

fn cmd_verify(object_dir: Option<PathBuf>) -> Result<()> {
    let repo = Repository::discover(None)?;
    let objects_dir = object_dir.unwrap_or_else(|| repo.git_dir.join("objects"));
    let graph_path = objects_dir.join("info").join("commit-graph");

    if !graph_path.exists() {
        bail!("commit-graph file does not exist at {:?}", graph_path);
    }

    let data = fs::read(&graph_path).with_context(|| format!("reading {:?}", graph_path))?;

    if data.len() < 8 {
        bail!("commit-graph file too small");
    }

    // Verify header
    if &data[0..4] != SIGNATURE {
        bail!("commit-graph has bad signature");
    }
    if data[4] != VERSION {
        bail!(
            "commit-graph version {} not supported (expected {})",
            data[4],
            VERSION
        );
    }
    if data[5] != HASH_VERSION_SHA1 {
        bail!("commit-graph hash version {} not supported", data[5]);
    }

    let num_chunks = data[6] as usize;
    if num_chunks < 3 {
        bail!("commit-graph has too few chunks: {}", num_chunks);
    }

    // Verify checksum (last 20 bytes)
    if data.len() < 20 {
        bail!("commit-graph too small for checksum");
    }
    let body = &data[..data.len() - 20];
    let stored_checksum = &data[data.len() - 20..];
    let computed = sha1_hash(body);
    if stored_checksum != computed {
        bail!("commit-graph checksum mismatch");
    }

    // Parse chunk TOC to find OID Fanout
    let toc_start = 8;
    let mut fanout_offset: Option<u64> = None;
    let mut oid_lookup_offset: Option<u64> = None;
    let mut commit_data_offset: Option<u64> = None;

    for i in 0..num_chunks {
        let entry_off = toc_start + i * 12;
        if entry_off + 12 > data.len() {
            bail!("chunk TOC entry out of bounds");
        }
        let chunk_id = u32::from_be_bytes(data[entry_off..entry_off + 4].try_into()?);
        let offset = u64::from_be_bytes(data[entry_off + 4..entry_off + 12].try_into()?);
        match chunk_id {
            CHUNK_OID_FANOUT => fanout_offset = Some(offset),
            CHUNK_OID_LOOKUP => oid_lookup_offset = Some(offset),
            CHUNK_COMMIT_DATA => commit_data_offset = Some(offset),
            _ => {} // unknown chunk — ok
        }
    }

    let fanout_off = fanout_offset.context("missing OID fanout chunk")? as usize;
    let lookup_off = oid_lookup_offset.context("missing OID lookup chunk")? as usize;
    let cdata_off = commit_data_offset.context("missing commit data chunk")? as usize;

    // Verify fanout
    if fanout_off + 256 * 4 > data.len() {
        bail!("OID fanout chunk extends past end of file");
    }
    let total_commits =
        u32::from_be_bytes(data[fanout_off + 255 * 4..fanout_off + 256 * 4].try_into()?);

    // Verify fanout is monotonically increasing
    let mut prev = 0u32;
    for i in 0..256 {
        let off = fanout_off + i * 4;
        let val = u32::from_be_bytes(data[off..off + 4].try_into()?);
        if val < prev {
            bail!("fanout is not monotonically increasing at bucket {}", i);
        }
        prev = val;
    }

    // Verify OID lookup is sorted
    if lookup_off + total_commits as usize * HASH_LEN > data.len() {
        bail!("OID lookup chunk extends past end of file");
    }
    for i in 1..total_commits as usize {
        let a = &data[lookup_off + (i - 1) * HASH_LEN..lookup_off + i * HASH_LEN];
        let b = &data[lookup_off + i * HASH_LEN..lookup_off + (i + 1) * HASH_LEN];
        if a >= b {
            bail!("OID lookup is not sorted at index {}", i);
        }
    }

    // Verify commit data chunk size
    if cdata_off + total_commits as usize * 36 > data.len() {
        bail!("commit data chunk extends past end of file");
    }

    println!("commit-graph verified: {} commits", total_commits);
    Ok(())
}
