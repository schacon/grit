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
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

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
    Write,
    /// Verify an existing commit-graph file.
    Verify,
}

// ── Constants ──────────────────────────────────────────────────────────
const SIGNATURE: &[u8; 4] = b"CGPH";
const VERSION: u8 = 1;
const HASH_VERSION_SHA1: u8 = 1;
const HASH_LEN: usize = 20;

// Chunk IDs
const CHUNK_OID_FANOUT: u32 = 0x4f494446; // "OIDF"
const CHUNK_OID_LOOKUP: u32 = 0x4f49444c; // "OIDL"
const CHUNK_COMMIT_DATA: u32 = 0x43444154; // "CDAT"

const GENERATION_NUMBER_INFINITY: u32 = 0xFFFF_FFFF;
const PARENT_NONE: u32 = 0x7000_0000;

/// Run `grit commit-graph`.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        CommitGraphCommand::Write => cmd_write(args.object_dir),
        CommitGraphCommand::Verify => cmd_verify(args.object_dir),
    }
}

// ── Write ──────────────────────────────────────────────────────────────

fn cmd_write(object_dir: Option<PathBuf>) -> Result<()> {
    let repo = Repository::discover(None)?;
    let objects_dir = object_dir.unwrap_or_else(|| repo.git_dir.join("objects"));
    let odb = Odb::new(&objects_dir);

    // Collect all reachable commits by walking refs
    let commits = collect_all_commits(&repo, &odb)?;
    if commits.is_empty() {
        // Nothing to do
        return Ok(());
    }

    // Sort commits by OID
    let mut sorted_oids: Vec<ObjectId> = commits.keys().copied().collect();
    sorted_oids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));

    // Build OID → index mapping
    let oid_to_idx: HashMap<ObjectId, u32> = sorted_oids
        .iter()
        .enumerate()
        .map(|(i, oid)| (*oid, i as u32))
        .collect();

    let num_commits = sorted_oids.len() as u32;

    // Compute generation numbers (topological)
    let generations = compute_generations(&sorted_oids, &commits, &oid_to_idx);

    // Build chunks in memory
    let fanout = build_fanout(&sorted_oids);
    let oid_lookup = build_oid_lookup(&sorted_oids);
    let commit_data = build_commit_data(
        &sorted_oids,
        &commits,
        &oid_to_idx,
        &generations,
    );

    // Chunk table: 3 chunks + terminator
    let num_chunks: u8 = 3;
    let header_size: u64 = 8;
    let chunk_toc_size: u64 = (num_chunks as u64 + 1) * 12; // each entry: 4-byte id + 8-byte offset
    let fanout_size: u64 = 256 * 4;
    let oid_lookup_size: u64 = num_commits as u64 * HASH_LEN as u64;
    let commit_data_size: u64 = num_commits as u64 * 36;

    let offset_fanout = header_size + chunk_toc_size;
    let offset_oid_lookup = offset_fanout + fanout_size;
    let offset_commit_data = offset_oid_lookup + oid_lookup_size;
    let offset_end = offset_commit_data + commit_data_size;

    // Write to file
    let info_dir = objects_dir.join("info");
    fs::create_dir_all(&info_dir)?;
    let graph_path = info_dir.join("commit-graph");
    let file = fs::File::create(&graph_path)
        .with_context(|| format!("creating {:?}", graph_path))?;
    let mut w = BufWriter::new(file);

    // Header
    w.write_all(SIGNATURE)?;
    w.write_all(&[VERSION, HASH_VERSION_SHA1, num_chunks, 0])?;

    // Chunk TOC
    write_chunk_entry(&mut w, CHUNK_OID_FANOUT, offset_fanout)?;
    write_chunk_entry(&mut w, CHUNK_OID_LOOKUP, offset_oid_lookup)?;
    write_chunk_entry(&mut w, CHUNK_COMMIT_DATA, offset_commit_data)?;
    // Terminator
    w.write_all(&[0u8; 4])?;
    w.write_all(&offset_end.to_be_bytes())?;

    // Fanout
    w.write_all(&fanout)?;
    // OID Lookup
    w.write_all(&oid_lookup)?;
    // Commit Data
    w.write_all(&commit_data)?;

    w.flush()?;

    // Write trailing checksum (SHA-1 of everything written)
    drop(w);
    let content = fs::read(&graph_path)?;
    let checksum = sha1_hash(&content);
    let mut f = fs::OpenOptions::new().append(true).open(&graph_path)?;
    f.write_all(&checksum)?;

    Ok(())
}

fn write_chunk_entry(w: &mut impl Write, chunk_id: u32, offset: u64) -> Result<()> {
    w.write_all(&chunk_id.to_be_bytes())?;
    w.write_all(&offset.to_be_bytes())?;
    Ok(())
}

/// Collect all commits reachable from refs.
fn collect_all_commits(
    repo: &Repository,
    odb: &Odb,
) -> Result<HashMap<ObjectId, CommitInfo>> {
    let mut commits: HashMap<ObjectId, CommitInfo> = HashMap::new();
    let mut stack: Vec<ObjectId> = Vec::new();

    // Walk all refs to find tip commits
    let refs_dir = repo.git_dir.join("refs");
    collect_ref_tips(&repo.git_dir, &refs_dir, &mut stack)?;

    // Also read packed-refs
    let packed_refs = repo.git_dir.join("packed-refs");
    if packed_refs.exists() {
        if let Ok(content) = fs::read_to_string(&packed_refs) {
            for line in content.lines() {
                if line.starts_with('#') || line.starts_with('^') {
                    continue;
                }
                if let Some(hex) = line.split_whitespace().next() {
                    if let Ok(oid) = ObjectId::from_hex(hex) {
                        stack.push(oid);
                    }
                }
            }
        }
    }

    // Also read HEAD
    let head_path = repo.git_dir.join("HEAD");
    if head_path.exists() {
        let head = fs::read_to_string(&head_path)?;
        let head = head.trim();
        if let Some(refpath) = head.strip_prefix("ref: ") {
            let full = repo.git_dir.join(refpath);
            if full.exists() {
                if let Ok(content) = fs::read_to_string(&full) {
                    if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                        stack.push(oid);
                    }
                }
            }
        } else if let Ok(oid) = ObjectId::from_hex(head) {
            stack.push(oid);
        }
    }

    // BFS/DFS walk
    while let Some(oid) = stack.pop() {
        if commits.contains_key(&oid) {
            continue;
        }

        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue, // might be a tag or missing
        };

        if obj.kind != ObjectKind::Commit {
            // Might be an annotated tag — try to peel
            if obj.kind == ObjectKind::Tag {
                // Simple peel: look for "object " line
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
        let info = CommitInfo {
            tree: commit.tree,
            parents: commit.parents.clone(),
            commit_time: parse_commit_time(&commit.committer),
        };

        for parent in &commit.parents {
            stack.push(*parent);
        }

        commits.insert(oid, info);
    }

    Ok(commits)
}

fn collect_ref_tips(
    _git_dir: &Path,
    dir: &Path,
    stack: &mut Vec<ObjectId>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_ref_tips(_git_dir, &path, stack)?;
        } else if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(oid) = ObjectId::from_hex(content.trim()) {
                stack.push(oid);
            }
        }
    }
    Ok(())
}

struct CommitInfo {
    tree: ObjectId,
    parents: Vec<ObjectId>,
    commit_time: u32,
}

fn parse_commit_time(committer: &str) -> u32 {
    // Format: "Name <email> <timestamp> <tz>"
    let parts: Vec<&str> = committer.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse::<u32>().unwrap_or(0)
    } else {
        0
    }
}

fn compute_generations(
    sorted_oids: &[ObjectId],
    commits: &HashMap<ObjectId, CommitInfo>,
    oid_to_idx: &HashMap<ObjectId, u32>,
) -> Vec<u32> {
    let n = sorted_oids.len();
    let mut gen = vec![0u32; n];
    let mut computed = vec![false; n];

    // Iterative topological generation computation
    for i in 0..n {
        if computed[i] {
            continue;
        }
        let mut work_stack: Vec<(usize, bool)> = vec![(i, false)];
        while let Some((idx, parents_done)) = work_stack.pop() {
            if computed[idx] {
                continue;
            }
            let oid = &sorted_oids[idx];
            let info = &commits[oid];

            if parents_done {
                let mut max_parent_gen = 0u32;
                for p in &info.parents {
                    if let Some(&pidx) = oid_to_idx.get(p) {
                        max_parent_gen = max_parent_gen.max(gen[pidx as usize]);
                    }
                }
                gen[idx] = max_parent_gen + 1;
                computed[idx] = true;
            } else {
                // Check if all parents are computed
                let mut all_done = true;
                for p in &info.parents {
                    if let Some(&pidx) = oid_to_idx.get(p) {
                        if !computed[pidx as usize] {
                            all_done = false;
                        }
                    }
                }
                if all_done {
                    let mut max_parent_gen = 0u32;
                    for p in &info.parents {
                        if let Some(&pidx) = oid_to_idx.get(p) {
                            max_parent_gen = max_parent_gen.max(gen[pidx as usize]);
                        }
                    }
                    gen[idx] = max_parent_gen + 1;
                    computed[idx] = true;
                } else {
                    work_stack.push((idx, true));
                    for p in &info.parents {
                        if let Some(&pidx) = oid_to_idx.get(p) {
                            if !computed[pidx as usize] {
                                work_stack.push((pidx as usize, false));
                            }
                        }
                    }
                }
            }
        }
    }

    gen
}

fn build_fanout(sorted_oids: &[ObjectId]) -> Vec<u8> {
    let mut fanout = vec![0u8; 256 * 4];
    let mut counts = [0u32; 256];
    for oid in sorted_oids {
        let bucket = oid.as_bytes()[0] as usize;
        counts[bucket] += 1;
    }
    // Fanout is cumulative
    let mut cumulative = 0u32;
    for i in 0..256 {
        cumulative += counts[i];
        let offset = i * 4;
        fanout[offset..offset + 4].copy_from_slice(&cumulative.to_be_bytes());
    }
    fanout
}

fn build_oid_lookup(sorted_oids: &[ObjectId]) -> Vec<u8> {
    let mut data = Vec::with_capacity(sorted_oids.len() * HASH_LEN);
    for oid in sorted_oids {
        data.extend_from_slice(oid.as_bytes());
    }
    data
}

fn build_commit_data(
    sorted_oids: &[ObjectId],
    commits: &HashMap<ObjectId, CommitInfo>,
    oid_to_idx: &HashMap<ObjectId, u32>,
    generations: &[u32],
) -> Vec<u8> {
    let mut data = Vec::with_capacity(sorted_oids.len() * 36);
    for (i, oid) in sorted_oids.iter().enumerate() {
        let info = &commits[oid];

        // Tree OID (20 bytes)
        data.extend_from_slice(info.tree.as_bytes());

        // Parent 1 (4 bytes) — index or PARENT_NONE
        let parent1 = if !info.parents.is_empty() {
            oid_to_idx
                .get(&info.parents[0])
                .copied()
                .unwrap_or(PARENT_NONE)
        } else {
            PARENT_NONE
        };
        data.extend_from_slice(&parent1.to_be_bytes());

        // Parent 2 (4 bytes) — for octopus merges this would need extra chunks
        let parent2 = if info.parents.len() > 1 {
            oid_to_idx
                .get(&info.parents[1])
                .copied()
                .unwrap_or(PARENT_NONE)
        } else {
            PARENT_NONE
        };
        data.extend_from_slice(&parent2.to_be_bytes());

        // Generation number (top 30 bits) + commit time offset (bottom 2 bits)
        // Simplified: store generation in top 30 bits, low 2 bits of commit time
        let gen = generations[i];
        let time_low2 = info.commit_time & 0x3;
        let gen_and_time = (gen << 2) | time_low2;
        data.extend_from_slice(&gen_and_time.to_be_bytes());

        // Commit timestamp (4 bytes)
        data.extend_from_slice(&info.commit_time.to_be_bytes());
    }
    data
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
            let output = child.wait_with_output().unwrap();
            let hex = String::from_utf8_lossy(&output.stdout);
            let hex = hex.trim().split_whitespace().next().unwrap_or("");
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

    let data = fs::read(&graph_path)
        .with_context(|| format!("reading {:?}", graph_path))?;

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
        let chunk_id = u32::from_be_bytes(data[entry_off..entry_off + 4].try_into().unwrap());
        let offset = u64::from_be_bytes(data[entry_off + 4..entry_off + 12].try_into().unwrap());
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
    let total_commits = u32::from_be_bytes(
        data[fanout_off + 255 * 4..fanout_off + 256 * 4]
            .try_into()
            .unwrap(),
    );

    // Verify fanout is monotonically increasing
    let mut prev = 0u32;
    for i in 0..256 {
        let off = fanout_off + i * 4;
        let val = u32::from_be_bytes(data[off..off + 4].try_into().unwrap());
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
