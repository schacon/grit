//! Parsing Git commit-graph files and Bloom filter lookup (`commit-graph.c` / `bloom.c` compatible).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::bloom::{
    bloom_filter_contains, bloom_keyvec_for_path, BloomBuildOutcome, BloomFilterSettings,
};
use crate::objects::ObjectId;
use crate::odb::Odb;

const SIGNATURE: &[u8; 4] = b"CGPH";
const GRAPH_VERSION: u8 = 1;
const HASH_VERSION_SHA1: u8 = 1;
const HASH_LEN: usize = 20;

const CHUNK_OID_FANOUT: u32 = 0x4f49_4446; // OIDF
const CHUNK_OID_LOOKUP: u32 = 0x4f49_444c; // OIDL
const CHUNK_COMMIT_DATA: u32 = 0x4344_4154; // CDAT
const CHUNK_GENERATION_DATA: u32 = 0x4744_4132; // GDA2
const CHUNK_EXTRA_EDGES: u32 = 0x4544_4745; // EDGE
const CHUNK_BLOOM_INDEXES: u32 = 0x4249_4458; // BIDX
const CHUNK_BLOOM_DATA: u32 = 0x4244_4154; // BDAT

const BLOOM_HEADER: usize = crate::bloom::BLOOMDATA_HEADER_LEN;

fn warn_path_for_graph_file(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(idx) = s.find(".git/") {
        return s[idx..].replace('\\', "/");
    }
    s.replace('\\', "/")
}

/// One layer from `.git/objects/info/commit-graph` or `commit-graphs/<hash>.graph`.
#[derive(Debug, Clone)]
pub struct CommitGraphLayer {
    pub path: PathBuf,
    body: Vec<u8>,
    num_commits: u32,
    oid_lookup_off: usize,
    #[allow(dead_code)]
    chunk_commit_data_off: usize,
    #[allow(dead_code)]
    chunk_generation_data: Option<usize>,
    read_generation_data: bool,
    chunk_bloom_indexes: Option<usize>,
    chunk_bloom_data: Option<(usize, usize)>,
    bloom_settings: Option<BloomFilterSettings>,
    bloom_disabled: bool,
}

impl CommitGraphLayer {
    fn parse(path: PathBuf, raw: Vec<u8>) -> Option<Self> {
        if raw.len() < 28 {
            return None;
        }
        let body = raw[..raw.len() - HASH_LEN].to_vec();
        if body.len() < 8 || &body[0..4] != SIGNATURE {
            return None;
        }
        if body[4] != GRAPH_VERSION || body[5] != HASH_VERSION_SHA1 {
            return None;
        }
        let num_chunks = body[6] as usize;
        let toc_start = 8;
        let toc_end = toc_start + (num_chunks + 1) * 12;
        if body.len() < toc_end {
            return None;
        }

        let mut fanout_off = None;
        let mut oid_lookup_off = None;
        let mut commit_data_off = None;
        let mut generation_off = None;
        let mut bloom_idx_off = None;
        let mut bloom_data_range = None;
        let mut chunk_offsets: Vec<usize> = Vec::new();

        for i in 0..num_chunks {
            let e = toc_start + i * 12;
            let id = u32::from_be_bytes(body[e..e + 4].try_into().ok()?);
            let off = u64::from_be_bytes(body[e + 4..e + 12].try_into().ok()?) as usize;
            chunk_offsets.push(off);
            match id {
                CHUNK_OID_FANOUT => fanout_off = Some(off),
                CHUNK_OID_LOOKUP => oid_lookup_off = Some(off),
                CHUNK_COMMIT_DATA => commit_data_off = Some(off),
                CHUNK_GENERATION_DATA => generation_off = Some(off),
                CHUNK_BLOOM_INDEXES => bloom_idx_off = Some(off),
                CHUNK_BLOOM_DATA => {
                    let end = if i + 1 < num_chunks {
                        let e2 = toc_start + (i + 1) * 12;
                        u64::from_be_bytes(body[e2 + 4..e2 + 12].try_into().ok()?) as usize
                    } else {
                        let term = toc_start + num_chunks * 12;
                        u64::from_be_bytes(body[term + 4..term + 12].try_into().ok()?) as usize
                    };
                    bloom_data_range = Some((off, end.saturating_sub(off)));
                }
                _ => {}
            }
        }
        let file_end = u64::from_be_bytes(
            body[toc_start + num_chunks * 12 + 4..toc_start + num_chunks * 12 + 12]
                .try_into()
                .ok()?,
        ) as usize;
        chunk_offsets.push(file_end);
        chunk_offsets.sort_unstable();
        chunk_offsets.dedup();
        let bidx_len = bloom_idx_off.and_then(|b| {
            chunk_offsets
                .iter()
                .find(|&&o| o > b)
                .map(|&next| next.saturating_sub(b))
        });

        let fanout_off = fanout_off?;
        let oid_lookup_off = oid_lookup_off?;
        let commit_data_off = commit_data_off?;
        if fanout_off + 256 * 4 > body.len() || oid_lookup_off + 4 > body.len() {
            return None;
        }
        let num_commits = u32::from_be_bytes(
            body[fanout_off + 255 * 4..fanout_off + 256 * 4]
                .try_into()
                .ok()?,
        );
        if oid_lookup_off + num_commits as usize * HASH_LEN > body.len() {
            return None;
        }
        let graph_data_width = HASH_LEN + 16;
        if commit_data_off + num_commits as usize * graph_data_width > body.len() {
            return None;
        }

        let read_generation_data = generation_off.is_some();
        let mut bloom_settings = None;
        let mut chunk_bloom_data = None;
        if let (Some(_bidx), Some((bdat_off, bdat_len))) = (bloom_idx_off, bloom_data_range) {
            if bdat_len < BLOOM_HEADER {
                eprintln!(
                    "warning: ignoring too-small changed-path chunk ({} < {}) in commit-graph file",
                    bdat_len, BLOOM_HEADER
                );
            } else if bdat_off + bdat_len <= body.len() {
                let hdr = &body[bdat_off..bdat_off + BLOOM_HEADER];
                bloom_settings = Some(BloomFilterSettings {
                    hash_version: u32::from_be_bytes(hdr[0..4].try_into().ok()?),
                    num_hashes: u32::from_be_bytes(hdr[4..8].try_into().ok()?),
                    bits_per_entry: u32::from_be_bytes(hdr[8..12].try_into().ok()?),
                    max_changed_paths: 512,
                });
                chunk_bloom_data = Some((bdat_off, bdat_len));
            }
        }

        let bloom_indexes_ok = if let (Some(bidx), Some(bsize)) = (bloom_idx_off, bidx_len) {
            if bsize / 4 != num_commits as usize {
                eprintln!("warning: commit-graph changed-path index chunk is too small");
                false
            } else if bidx + bsize > body.len() {
                eprintln!("warning: commit-graph changed-path index chunk is too small");
                false
            } else {
                true
            }
        } else {
            false
        };
        let bloom_pair_ok = bloom_settings.is_some()
            && chunk_bloom_data.is_some()
            && bloom_indexes_ok
            && chunk_bloom_data.is_some_and(|(_, len)| len >= BLOOM_HEADER);

        let (chunk_bloom_indexes, bloom_settings) = if bloom_pair_ok {
            (bloom_idx_off, bloom_settings)
        } else {
            (None, None)
        };
        let chunk_bloom_data = if bloom_pair_ok {
            chunk_bloom_data
        } else {
            None
        };

        Some(Self {
            path,
            body,
            num_commits,
            oid_lookup_off,
            chunk_commit_data_off: commit_data_off,
            chunk_generation_data: generation_off,
            read_generation_data,
            chunk_bloom_indexes,
            chunk_bloom_data,
            bloom_settings,
            bloom_disabled: false,
        })
    }

    fn oid_at_lex(&self, lex_index: u32) -> Option<ObjectId> {
        if lex_index >= self.num_commits {
            return None;
        }
        let off = self.oid_lookup_off + lex_index as usize * HASH_LEN;
        ObjectId::from_bytes(self.body.get(off..off + HASH_LEN)?.try_into().ok()?).ok()
    }

    fn bsearch_oid(&self, oid: &ObjectId) -> Option<u32> {
        let mut lo = 0u32;
        let mut hi = self.num_commits;
        let bytes = oid.as_bytes();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let off = self.oid_lookup_off + mid as usize * HASH_LEN;
            let slice = &self.body[off..off + HASH_LEN];
            match slice.cmp(bytes) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    fn disable_bloom(&mut self) {
        self.chunk_bloom_indexes = None;
        self.chunk_bloom_data = None;
        self.bloom_settings = None;
        self.bloom_disabled = true;
    }

    fn layer_display_id(&self) -> String {
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.strip_prefix("graph-").unwrap_or(s).to_string())
            .unwrap_or_else(|| "commit-graph".to_string())
    }

    fn bloom_filter_slice(&self, lex_index: u32) -> Option<&[u8]> {
        let _settings = self.bloom_settings.as_ref()?;
        let bidx_base = self.chunk_bloom_indexes?;
        let (bdat_off, bdat_total) = self.chunk_bloom_data?;
        let graph_warn = warn_path_for_graph_file(self.path.as_path());
        if lex_index >= self.num_commits {
            return None;
        }
        let payload_len = bdat_total.saturating_sub(BLOOM_HEADER);
        let end_rel = u32::from_be_bytes(
            self.body[bidx_base + lex_index as usize * 4..bidx_base + lex_index as usize * 4 + 4]
                .try_into()
                .ok()?,
        ) as usize;
        let start_rel = if lex_index == 0 {
            0usize
        } else {
            u32::from_be_bytes(
                self.body[bidx_base + (lex_index as usize - 1) * 4
                    ..bidx_base + (lex_index as usize - 1) * 4 + 4]
                    .try_into()
                    .ok()?,
            ) as usize
        };
        if end_rel < start_rel {
            eprintln!(
                "warning: ignoring decreasing changed-path index offsets ({start_rel} > {end_rel}) for positions {} and {} of {}",
                lex_index.saturating_sub(1),
                lex_index,
                graph_warn
            );
            return None;
        }
        let max_payload = payload_len;
        if end_rel > max_payload {
            eprintln!(
                "warning: ignoring out-of-range offset ({end_rel}) for changed-path filter at pos {} of {} (chunk size: {bdat_total})",
                lex_index,
                graph_warn,
                bdat_total = bdat_total
            );
            return None;
        }
        if start_rel > max_payload {
            eprintln!(
                "warning: ignoring out-of-range offset ({start_rel}) for changed-path filter at pos {} of {} (chunk size: {bdat_total})",
                lex_index.saturating_sub(1),
                graph_warn,
                bdat_total = bdat_total
            );
            return None;
        }
        let data_base = bdat_off + BLOOM_HEADER;
        let abs_start = data_base + start_rel;
        let abs_end = data_base + end_rel;
        if abs_end > bdat_off + bdat_total || abs_start > abs_end {
            return None;
        }
        Some(&self.body[abs_start..abs_end])
    }
}

/// Result of consulting Bloom filters before running a tree diff (matches `revision.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BloomPrecheck {
    /// No commit-graph, pathspecs disallow Bloom, or wrong parent count — no statistics.
    Inapplicable,
    /// Commit not in graph or generation unavailable — skip Bloom (`-1` without `filter_not_present`).
    NotInGraph,
    /// Bloom filter missing or unusable (`filter_not_present` in Git).
    FilterNotPresent,
    /// Bloom says path cannot be in this commit (`definitely_not`).
    DefinitelyNot,
    /// Bloom says maybe — caller must run diff and may count `false_positive`.
    Maybe,
}

/// Counters for `GIT_TRACE2_PERF` Bloom statistics (`revision.c` `trace2_bloom_filter_statistics_atexit`).
#[derive(Debug, Default, Clone)]
pub struct BloomWalkStats {
    pub filter_not_present: u32,
    pub maybe: u32,
    pub definitely_not: u32,
    pub false_positive: u32,
}

impl BloomWalkStats {
    pub fn record_precheck(&mut self, pre: BloomPrecheck) {
        match pre {
            BloomPrecheck::Inapplicable | BloomPrecheck::NotInGraph => {}
            BloomPrecheck::FilterNotPresent => self.filter_not_present += 1,
            BloomPrecheck::DefinitelyNot => self.definitely_not += 1,
            BloomPrecheck::Maybe => self.maybe += 1,
        }
    }

    pub fn record_false_positive(&mut self) {
        self.false_positive += 1;
    }
}

/// Shared stats handle for [`crate::rev_list::RevListOptions::bloom_stats`].
pub type BloomWalkStatsHandle = Arc<Mutex<BloomWalkStats>>;

/// Loaded commit-graph chain (newest layer first, matching `commit-graph-chain` file order).
#[derive(Debug, Clone)]
pub struct CommitGraphChain {
    layers: Vec<CommitGraphLayer>,
}

impl CommitGraphChain {
    /// Bloom settings from the newest layer, if that layer carries Bloom data.
    #[must_use]
    pub fn top_layer_bloom_settings(&self) -> Option<BloomFilterSettings> {
        self.layers.first()?.bloom_settings
    }

    /// Total commits across all layers (Git `num_commits_in_base` offset for new layers).
    #[must_use]
    pub fn total_commits(&self) -> u32 {
        self.layers.iter().map(|l| l.num_commits).sum()
    }

    /// Layer file paths from oldest base to newest (reverse of chain file order).
    #[must_use]
    pub fn layer_paths_oldest_first(&self) -> Vec<PathBuf> {
        self.layers.iter().rev().map(|l| l.path.clone()).collect()
    }

    /// Load from `objects/info/commit-graph` or `objects/info/commit-graphs/commit-graph-chain`.
    pub fn load(objects_dir: &Path) -> Option<Self> {
        let info = objects_dir.join("info");
        let chain_path = info.join("commit-graphs").join("commit-graph-chain");
        if chain_path.is_file() {
            let content = std::fs::read_to_string(&chain_path).ok()?;
            let mut layers = Vec::new();
            for line in content.lines() {
                let h = line.trim();
                if h.len() != 40 {
                    continue;
                }
                let graph_path = info.join("commit-graphs").join(format!("graph-{h}.graph"));
                let raw = std::fs::read(&graph_path).ok()?;
                let layer = CommitGraphLayer::parse(graph_path, raw)?;
                layers.push(layer);
            }
            if layers.is_empty() {
                return None;
            }
            let mut chain = Self { layers };
            chain.validate_bloom_compatibility();
            return Some(chain);
        }
        let single = info.join("commit-graph");
        if single.is_file() {
            let raw = std::fs::read(&single).ok()?;
            let layer = CommitGraphLayer::parse(single.clone(), raw)?;
            let mut chain = Self {
                layers: vec![layer],
            };
            chain.validate_bloom_compatibility();
            return Some(chain);
        }
        None
    }

    fn validate_bloom_compatibility(&mut self) {
        let mut ref_settings: Option<BloomFilterSettings> = None;
        for layer in &mut self.layers {
            let Some(bs) = layer.bloom_settings else {
                continue;
            };
            match ref_settings {
                None => ref_settings = Some(bs),
                Some(r) => {
                    if r.hash_version != bs.hash_version
                        || r.num_hashes != bs.num_hashes
                        || r.bits_per_entry != bs.bits_per_entry
                    {
                        let id = layer.layer_display_id();
                        eprintln!(
                            "warning: disabling Bloom filters for commit-graph layer '{id}' due to incompatible settings"
                        );
                        layer.disable_bloom();
                    }
                }
            }
        }
    }

    /// Lexicographic position in the full chain, or `None` if not in any layer.
    pub fn find_commit(&self, oid: &ObjectId) -> Option<(usize, u32)> {
        for (i, layer) in self.layers.iter().enumerate() {
            if let Some(lex) = layer.bsearch_oid(oid) {
                return Some((i, lex));
            }
        }
        None
    }

    /// Global commit-graph position (Git `graph_pos`): base layers first, then newer layers.
    pub fn global_position(&self, oid: &ObjectId) -> Option<u32> {
        let (layer_idx, lex) = self.find_commit(oid)?;
        let below: u32 = self.layers[layer_idx + 1..]
            .iter()
            .map(|l| l.num_commits)
            .sum();
        Some(below + lex)
    }

    /// All commit OIDs in the chain (oldest base first, then newer layers).
    pub fn all_oids_in_order(&self) -> Vec<ObjectId> {
        let mut out = Vec::new();
        for layer in self.layers.iter().rev() {
            for i in 0..layer.num_commits {
                if let Some(oid) = layer.oid_at_lex(i) {
                    out.push(oid);
                }
            }
        }
        out
    }

    /// Consult Bloom filters for a single-parent commit before diffing trees.
    pub fn bloom_precheck_for_paths(
        &self,
        odb: &Odb,
        oid: ObjectId,
        pathspecs: &[String],
        bloom_cwd: Option<&str>,
        requested_hash_version: i32,
        read_changed_paths: bool,
    ) -> std::result::Result<BloomPrecheck, crate::error::Error> {
        if !read_changed_paths {
            return Ok(BloomPrecheck::Inapplicable);
        }
        let Some((layer_idx, lex)) = self.find_commit(&oid) else {
            return Ok(BloomPrecheck::NotInGraph);
        };
        let layer = &self.layers[layer_idx];
        let Some(settings) = layer.bloom_settings.as_ref() else {
            return Ok(BloomPrecheck::FilterNotPresent);
        };
        let effective_version = if requested_hash_version < 0 {
            settings.hash_version as i32
        } else {
            requested_hash_version
        };
        if effective_version != settings.hash_version as i32 {
            return Ok(BloomPrecheck::FilterNotPresent);
        }

        let commit = crate::objects::parse_commit(&odb.read(&oid)?.data)?;
        if commit.parents.len() > 1 {
            return Ok(BloomPrecheck::Inapplicable);
        }

        let filter = match layer.bloom_filter_slice(lex) {
            Some(s) => s,
            None => return Ok(BloomPrecheck::FilterNotPresent),
        };
        if filter.is_empty() {
            return Ok(BloomPrecheck::FilterNotPresent);
        }

        // Git `bloom_filter_contains_vec`: within one pathspec, every prefix key must match;
        // multiple pathspecs are ORed (`revision.c` loop over `bloom_keyvecs_nr`).
        let mut any_pathspec_maybe = false;
        let mut checked_any_keys = false;
        for spec in pathspecs {
            if spec.is_empty() || crate::pathspec::pathspec_is_exclude(spec) {
                continue;
            }
            let Some(norm) = crate::pathspec::bloom_lookup_prefix_with_cwd(spec, bloom_cwd) else {
                continue;
            };
            let keys = bloom_keyvec_for_path(norm.as_str(), settings);
            if keys.is_empty() {
                continue;
            }
            checked_any_keys = true;
            let mut all_keys_maybe = true;
            for key in &keys {
                match bloom_filter_contains(key, filter, settings) {
                    Ok(true) => {}
                    Ok(false) => {
                        all_keys_maybe = false;
                        break;
                    }
                    Err(()) => {
                        all_keys_maybe = true;
                        break;
                    }
                }
            }
            if all_keys_maybe {
                any_pathspec_maybe = true;
                break;
            }
        }
        if checked_any_keys && !any_pathspec_maybe {
            return Ok(BloomPrecheck::DefinitelyNot);
        }
        Ok(BloomPrecheck::Maybe)
    }
}

/// Compute changed paths between parent and commit trees (recursive diff, no rename detection).
pub fn diff_changed_paths_for_bloom(
    odb: &Odb,
    parent_tree: Option<ObjectId>,
    commit_tree: ObjectId,
) -> crate::error::Result<(Vec<String>, usize)> {
    use crate::diff::diff_trees;
    let entries = diff_trees(odb, parent_tree.as_ref(), Some(&commit_tree), "")?;
    let raw_len = entries.len();
    let mut paths = Vec::new();
    for e in entries {
        let p = e.path().to_string();
        if !p.is_empty() {
            paths.push(p);
        }
    }
    Ok((paths, raw_len))
}

/// Re-export for `commit-graph` write.
pub use crate::bloom::collect_changed_paths_for_bloom;

/// Build Bloom filter bytes for one commit; returns cumulative size contribution for BIDX.
pub fn bloom_filter_for_commit_write(
    odb: &Odb,
    parents: &[ObjectId],
    tree_oid: ObjectId,
    settings: &BloomFilterSettings,
) -> crate::error::Result<(Vec<u8>, BloomBuildOutcome)> {
    let (changed_paths_vec, raw_count) = if parents.len() == 1 {
        let p = load_commit_tree(odb, parents[0])?;
        diff_changed_paths_for_bloom(odb, Some(p), tree_oid)?
    } else if parents.is_empty() {
        diff_changed_paths_for_bloom(odb, None, tree_oid)?
    } else {
        return Ok((vec![0xff], BloomBuildOutcome::TruncatedLarge));
    };
    let set = collect_changed_paths_for_bloom(&changed_paths_vec);
    Ok(crate::bloom::build_bloom_filter_data(
        &set, raw_count, settings,
    ))
}

fn load_commit_tree(odb: &Odb, commit_oid: ObjectId) -> crate::error::Result<ObjectId> {
    let obj = odb.read(&commit_oid)?;
    let c = crate::objects::parse_commit(&obj.data)?;
    Ok(c.tree)
}

/// Parse all chunks for `test-tool read-graph` / debugging.
pub fn parse_graph_file(path: &Path) -> Option<ParsedGraphDump> {
    let raw = std::fs::read(path).ok()?;
    if raw.len() < 28 {
        return None;
    }
    let body = &raw[..raw.len() - HASH_LEN];
    if body.len() < 8 || &body[0..4] != SIGNATURE {
        return None;
    }
    let header_word = u32::from_be_bytes(body[0..4].try_into().ok()?);
    let num_chunks = body[6] as usize;
    let mut chunk_names = Vec::new();
    let toc_start = 8;
    for i in 0..num_chunks {
        let e = toc_start + i * 12;
        let id = u32::from_be_bytes(body[e..e + 4].try_into().ok()?);
        let name = match id {
            CHUNK_OID_FANOUT => "oid_fanout",
            CHUNK_OID_LOOKUP => "oid_lookup",
            CHUNK_COMMIT_DATA => "commit_metadata",
            CHUNK_GENERATION_DATA => "generation_data",
            CHUNK_EXTRA_EDGES => "extra_edges",
            CHUNK_BLOOM_INDEXES => "bloom_indexes",
            CHUNK_BLOOM_DATA => "bloom_data",
            _ => "unknown",
        };
        chunk_names.push(name.to_string());
    }
    let layer = CommitGraphLayer::parse(path.to_path_buf(), raw.clone())?;
    let bloom_opt = layer.bloom_settings.map(|s| {
        format!(
            " bloom({},{},{})",
            s.hash_version, s.bits_per_entry, s.num_hashes
        )
    });
    let mut options = String::new();
    if let Some(b) = bloom_opt {
        options.push_str(&b);
    }
    if layer.read_generation_data {
        options.push_str(" read_generation_data");
    }
    Some(ParsedGraphDump {
        header_word,
        version: body[4],
        hash_ver: body[5],
        num_chunks: body[6],
        reserved: body[7],
        num_commits: layer.num_commits,
        chunks: chunk_names.join(" "),
        options,
    })
}

pub struct ParsedGraphDump {
    pub header_word: u32,
    pub version: u8,
    pub hash_ver: u8,
    pub num_chunks: u8,
    pub reserved: u8,
    pub num_commits: u32,
    pub chunks: String,
    pub options: String,
}

/// Dump hex lines of Bloom filters (one per commit, empty line for empty filter).
pub fn dump_bloom_filters(path: &Path) -> Option<Vec<String>> {
    let raw = std::fs::read(path).ok()?;
    let layer = CommitGraphLayer::parse(path.to_path_buf(), raw)?;
    let mut out = Vec::new();
    for i in 0..layer.num_commits {
        let slice = layer.bloom_filter_slice(i).unwrap_or(&[]);
        if slice.is_empty() {
            out.push(String::new());
        } else {
            let hex: String = slice.iter().map(|b| format!("{b:02x}")).collect();
            out.push(hex);
        }
    }
    Some(out)
}
