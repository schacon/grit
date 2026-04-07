//! `grit pack-objects` — create a packed archive of objects.
//!
//! Reads object IDs (or revisions with `--revs`) from stdin and writes a
//! `.pack` file and corresponding `.idx` index file.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use grit_lib::config::ConfigSet;
use sha1::{Digest, Sha1};
use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;

/// Arguments for `grit pack-objects`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Base name for the output files (writes <base>-<hash>.pack and .idx).
    #[arg(value_name = "BASE-NAME")]
    pub base_name: Option<String>,

    /// Write the pack data to stdout instead of a file.
    #[arg(long)]
    pub stdout: bool,

    /// Read revision list instead of object list from stdin.
    #[arg(long)]
    pub revs: bool,

    /// Pack all objects in the repository.
    #[arg(long)]
    pub all: bool,

    /// Read pack filenames from stdin instead of object IDs.
    #[arg(long = "stdin-packs")]
    pub stdin_packs: bool,

    /// Use OFS_DELTA (delta-base-offset) format in pack output.
    #[arg(long = "delta-base-offset")]
    pub delta_base_offset: bool,

    /// Hash algorithm (accepted for compat).
    #[arg(long = "object-format")]
    pub object_format: Option<String>,

    /// Keep true parents (accepted for compat, no-op in grit).
    #[arg(long = "keep-true-parents")]
    pub keep_true_parents: bool,

    /// Suppress progress output (accepted for compat).
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Keep unreachable objects (accepted for compat).
    #[arg(long = "keep-unreachable")]
    pub keep_unreachable: bool,

    /// Unpack unreachable objects (accepted for compat).
    #[arg(long = "unpack-unreachable")]
    pub unpack_unreachable: Option<String>,

    /// Window size for delta compression (accepted for compat).
    #[arg(long = "window", allow_hyphen_values = true)]
    pub window: Option<i64>,

    /// Depth for delta compression (accepted for compat).
    #[arg(long = "depth", allow_hyphen_values = true)]
    pub depth: Option<i64>,

    /// Honor pack-keep files (accepted for compat).
    #[arg(long = "honor-pack-keep")]
    pub honor_pack_keep: bool,

    /// Only use local objects (accepted for compat).
    #[arg(long = "local")]
    pub local: bool,

    /// Write bitmap index (accepted for compat).
    #[arg(long = "write-bitmap-index")]
    pub write_bitmap_index: bool,

    /// Do not write bitmap index (accepted for compat).
    #[arg(long = "no-write-bitmap-index")]
    pub no_write_bitmap_index: bool,

    /// Filter specification (accepted for compat).
    #[arg(long = "filter")]
    pub filter: Option<String>,

    /// Missing objects are ok (accepted for compat).
    #[arg(long = "missing")]
    pub missing: Option<String>,

    /// Exclude pack (accepted for compat).
    #[arg(long = "exclude-promisor-objects")]
    pub exclude_promisor_objects: bool,

    /// Include redundant objects (accepted for compat).
    #[arg(long = "include-redundant")]
    pub include_redundant: bool,

    /// Incremental pack (accepted for compat).
    #[arg(long = "incremental")]
    pub incremental: bool,

    /// Do not create empty pack (accepted for compat).
    #[arg(long = "non-empty")]
    pub non_empty: bool,

    /// Pack reachable loose objects (accepted for compat).
    #[arg(long = "loosen-unreachable")]
    pub loosen_unreachable: bool,

    /// Keep unreachable objects in pack (accepted for compat).
    #[arg(long = "pack-loose-unreachable")]
    pub pack_loose_unreachable: bool,

    /// Include objects reachable from reflog (accepted for compat).
    #[arg(long = "reflog")]
    pub reflog: bool,

    /// Index version (accepted for compat).
    #[arg(long = "index-version")]
    pub index_version: Option<String>,

    /// Number of threads (accepted for compat).
    #[arg(long = "threads")]
    pub threads: Option<u32>,

    /// Maximum output size (accepted for compat).
    #[arg(long = "max-pack-size")]
    pub max_pack_size: Option<String>,

    /// Sparse reachability traversal (accepted for compat).
    #[arg(long = "sparse")]
    pub sparse: bool,

    /// Progress output (accepted for compat).
    #[arg(long = "progress")]
    pub progress: bool,

    /// Include indexed objects (accepted for compat).
    #[arg(long = "indexed-objects")]
    pub indexed_objects: bool,

    /// Cruft pack options (accepted for compat).
    #[arg(long = "cruft")]
    pub cruft: bool,

    #[arg(long = "cruft-expiration")]
    pub cruft_expiration: Option<String>,

    /// Extra args passed through (for forward compat with unknown flags).
    #[arg(value_name = "EXTRA", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true, hide = true)]
    pub extra: Vec<String>,
}

/// A pack entry to be written.
struct PackEntry {
    oid: ObjectId,
    kind: ObjectKind,
    data: Vec<u8>,
}

/// Run `grit pack-objects`.
pub fn run(args: Args) -> Result<()> {
    if let Some(fmt) = &args.object_format {
        if fmt != "sha1" {
            bail!("unsupported object format: {fmt}");
        }
    }

    if !args.stdout && args.base_name.is_none() {
        bail!("usage: grit pack-objects [--stdout] <base-name>");
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    // Collect object IDs.
    let oids = collect_oids(&repo, &args)?;

    if oids.is_empty() {
        if !args.stdout {
            eprintln!("Total 0 (delta 0), reused 0 (delta 0)");
        }
        return Ok(());
    }

    // Read all objects.
    let mut entries: Vec<PackEntry> = Vec::with_capacity(oids.len());
    for oid in &oids {
        let obj = read_object_from_repo(&repo, oid)?;
        entries.push(PackEntry {
            oid: *oid,
            kind: obj.kind,
            data: obj.data,
        });
    }

    // Build pack bytes.
    let pack_bytes = build_pack(&entries)?;

    if args.stdout {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        out.write_all(&pack_bytes)?;
        out.flush()?;
    } else {
        let base = args
            .base_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no base name"))?;

        // Pack hash is the trailing 20 bytes.
        let pack_hash = hex::encode(&pack_bytes[pack_bytes.len() - 20..]);
        let pack_path = format!("{base}-{pack_hash}.pack");
        let idx_path = format!("{base}-{pack_hash}.idx");

        std::fs::write(&pack_path, &pack_bytes)?;

        // Build and write idx.
        let idx_bytes = build_idx_for_pack(&pack_bytes, &entries)?;
        std::fs::write(&idx_path, &idx_bytes)?;

        println!("{pack_hash}");
        eprintln!("Total {} (delta 0), reused 0 (delta 0)", entries.len());
    }

    Ok(())
}

/// Collect object IDs from stdin or --all.
fn collect_oids(repo: &Repository, args: &Args) -> Result<Vec<ObjectId>> {
    let mut oids = BTreeSet::new();

    if args.all {
        // Walk all loose objects.
        collect_all_loose(&repo.odb, &mut oids)?;
        // Walk all packed objects.
        let pack_dir = repo.odb.objects_dir().join("pack");
        if pack_dir.exists() {
            let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            for idx in indexes {
                for entry in idx.entries {
                    oids.insert(entry.oid);
                }
            }
        }
    }

    if args.revs {
        // Read revision specs from stdin — for simplicity, treat each line as a
        // ref/rev that we resolve, then walk its reachable objects.
        // Lines starting with '^' exclude objects reachable from that ref.
        let stdin = io::stdin();
        let mut exclude = BTreeSet::new();
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(neg_ref) = trimmed.strip_prefix('^') {
                // Exclusion: walk reachable from this ref and exclude them.
                let oid = if let Ok(oid) = ObjectId::from_hex(neg_ref) {
                    oid
                } else {
                    resolve_ref(repo, neg_ref)?
                };
                walk_reachable(repo, &oid, &mut exclude)?;
            } else {
                // Inclusion: walk reachable from this ref.
                let oid = if let Ok(oid) = ObjectId::from_hex(trimmed) {
                    oid
                } else {
                    resolve_ref(repo, trimmed)?
                };
                walk_reachable(repo, &oid, &mut oids)?;
            }
        }
        // Remove excluded objects.
        for oid in &exclude {
            oids.remove(oid);
        }
    } else if args.stdin_packs {
        // Read pack filenames from stdin and include all objects in those packs.
        let stdin = io::stdin();
        let pack_dir = repo.odb.objects_dir().join("pack");
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // The input can be a bare name like "pack-<hash>" or full path.
            let idx_path = if trimmed.contains('/') || trimmed.contains('\\') {
                std::path::PathBuf::from(trimmed)
            } else {
                pack_dir.join(format!("{}.idx", trimmed))
            };
            // If given a .pack, convert to .idx
            let idx_path = if idx_path.extension().is_some_and(|e| e == "pack") {
                idx_path.with_extension("idx")
            } else {
                idx_path
            };
            if idx_path.exists() {
                let idx = grit_lib::pack::read_pack_index(&idx_path)
                    .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", idx_path.display()))?;
                for entry in idx.entries {
                    oids.insert(entry.oid);
                }
            } else {
                bail!("pack index not found: {}", idx_path.display());
            }
        }
    } else if !args.all {
        // Read bare object IDs from stdin.
        // Lines may have the form:
        //   <oid>          — include this object
        //   <oid> <name>   — include this object (name hint for delta selection)
        //   -<oid>         — exclude this object (negation)
        let stdin = io::stdin();
        let mut exclude = BTreeSet::new();
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Check for negation prefix
            if let Some(rest) = trimmed.strip_prefix('-') {
                // Negated object: take just the hex part (may have trailing name)
                let hex_part = rest.split_whitespace().next().unwrap_or(rest);
                let oid = ObjectId::from_hex(hex_part)
                    .map_err(|e| anyhow::anyhow!("invalid object id '{hex_part}': {e}"))?;
                exclude.insert(oid);
                continue;
            }
            // Positive object: may have "<oid> <name>" format
            let hex_part = trimmed.split_whitespace().next().unwrap_or(trimmed);
            let oid = ObjectId::from_hex(hex_part)
                .map_err(|e| anyhow::anyhow!("invalid object id '{hex_part}': {e}"))?;
            oids.insert(oid);
        }
        // Remove excluded objects.
        for oid in &exclude {
            oids.remove(oid);
        }
    }

    Ok(oids.into_iter().collect())
}

/// Walk all loose objects in the ODB.
fn collect_all_loose(odb: &Odb, oids: &mut BTreeSet<ObjectId>) -> Result<()> {
    let objects_dir = odb.objects_dir();
    for prefix in 0..=255u8 {
        let hex_prefix = format!("{prefix:02x}");
        let dir = objects_dir.join(&hex_prefix);
        if !dir.exists() {
            continue;
        }
        let rd = std::fs::read_dir(&dir)?;
        for entry in rd {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.len() == 38 {
                let full_hex = format!("{hex_prefix}{name_str}");
                if let Ok(oid) = ObjectId::from_hex(&full_hex) {
                    oids.insert(oid);
                }
            }
        }
    }
    Ok(())
}

/// Resolve a ref name to an ObjectId.
fn resolve_ref(repo: &Repository, refname: &str) -> Result<ObjectId> {
    // Check refs/heads/, refs/tags/, and direct.
    let candidates = [
        repo.git_dir.join(refname),
        repo.git_dir.join("refs/heads").join(refname),
        repo.git_dir.join("refs/tags").join(refname),
    ];
    for path in &candidates {
        if path.is_file() {
            let content = std::fs::read_to_string(path)?;
            let trimmed = content.trim();
            if let Some(target) = trimmed.strip_prefix("ref: ") {
                return resolve_ref(repo, target);
            }
            return ObjectId::from_hex(trimmed)
                .map_err(|e| anyhow::anyhow!("cannot resolve ref '{refname}': {e}"));
        }
    }
    // Try HEAD.
    if refname == "HEAD" {
        let head = std::fs::read_to_string(repo.git_dir.join("HEAD"))?;
        let trimmed = head.trim();
        if trimmed.starts_with("ref: ") {
            return resolve_ref(repo, &trimmed[5..]);
        }
        return ObjectId::from_hex(trimmed)
            .map_err(|e| anyhow::anyhow!("cannot resolve HEAD: {e}"));
    }
    bail!("cannot resolve ref '{refname}'")
}

/// Walk reachable objects from a commit/tree/tag/blob OID.
fn walk_reachable(repo: &Repository, oid: &ObjectId, oids: &mut BTreeSet<ObjectId>) -> Result<()> {
    if !oids.insert(*oid) {
        return Ok(()); // already visited
    }
    let obj = read_object_from_repo(repo, oid)?;
    match obj.kind {
        ObjectKind::Commit => {
            // Parse tree and parent lines.
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                for line in text.lines() {
                    if let Some(tree_hex) = line.strip_prefix("tree ") {
                        if let Ok(tree_oid) = ObjectId::from_hex(tree_hex.trim()) {
                            walk_reachable(repo, &tree_oid, oids)?;
                        }
                    } else if let Some(parent_hex) = line.strip_prefix("parent ") {
                        if let Ok(parent_oid) = ObjectId::from_hex(parent_hex.trim()) {
                            walk_reachable(repo, &parent_oid, oids)?;
                        }
                    } else if line.is_empty() {
                        break; // end of headers
                    }
                }
            }
        }
        ObjectKind::Tree => {
            // Parse tree entries: mode SP name NUL 20-byte-oid
            let data = &obj.data;
            let mut pos = 0;
            while pos < data.len() {
                // Find the NUL.
                let nul = data[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|i| pos + i)
                    .ok_or_else(|| anyhow::anyhow!("corrupt tree object"))?;
                if nul + 21 > data.len() {
                    break;
                }
                let entry_oid = ObjectId::from_bytes(&data[nul + 1..nul + 21])
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                walk_reachable(repo, &entry_oid, oids)?;
                pos = nul + 21;
            }
        }
        ObjectKind::Tag => {
            // Parse the object line.
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                if let Some(first_line) = text.lines().next() {
                    if let Some(obj_hex) = first_line.strip_prefix("object ") {
                        if let Ok(target_oid) = ObjectId::from_hex(obj_hex.trim()) {
                            walk_reachable(repo, &target_oid, oids)?;
                        }
                    }
                }
            }
        }
        ObjectKind::Blob => {} // leaf
    }
    Ok(())
}

/// Read an object from loose store or pack files.
fn read_object_from_repo(repo: &Repository, oid: &ObjectId) -> Result<grit_lib::objects::Object> {
    // Try loose first.
    if let Ok(obj) = repo.odb.read(oid) {
        return Ok(obj);
    }
    // Try pack files.
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in &indexes {
        if let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) {
            let pack_bytes = std::fs::read(&idx.pack_path)?;
            let obj = read_object_from_pack(&pack_bytes, entry.offset, &indexes)?;
            return Ok(obj);
        }
    }
    maybe_lazy_fetch_missing_object(repo)?;
    if let Ok(obj) = repo.odb.read(oid) {
        return Ok(obj);
    }
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in &indexes {
        if let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) {
            let pack_bytes = std::fs::read(&idx.pack_path)?;
            let obj = read_object_from_pack(&pack_bytes, entry.offset, &indexes)?;
            return Ok(obj);
        }
    }
    bail!("object not found: {}", oid.to_hex())
}

fn maybe_lazy_fetch_missing_object(repo: &Repository) -> Result<()> {
    let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let is_promisor = config
        .get("remote.origin.promisor")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
        || config.get("extensions.partialClone").is_some();
    if !is_promisor {
        bail!("missing object in non-promisor repository");
    }
    if std::env::var("GIT_NO_LAZY_FETCH")
        .ok()
        .as_deref()
        .is_some_and(|v| v != "0")
    {
        bail!("lazy fetching disabled");
    }

    let upload_pack = config
        .get("remote.origin.uploadpack")
        .unwrap_or_else(|| "git-upload-pack".to_owned());
    let remote_url = config
        .get("remote.origin.url")
        .ok_or_else(|| anyhow::anyhow!("missing remote.origin.url"))?;
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{upload_pack} '{remote_url}'"))
        .current_dir(repo.git_dir.parent().unwrap_or(&repo.git_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to execute upload-pack")?;
    if !status.success() {
        bail!("lazy fetch failed");
    }
    Ok(())
}

/// Read and decompress a single object from pack bytes at the given offset.
fn read_object_from_pack(
    pack_bytes: &[u8],
    offset: u64,
    indexes: &[grit_lib::pack::PackIndex],
) -> Result<grit_lib::objects::Object> {
    let mut pos = offset as usize;
    let c = pack_bytes
        .get(pos)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("truncated pack"))?;
    pos += 1;
    let type_code = (c >> 4) & 0x7;
    let mut size = (c & 0x0f) as usize;
    let mut shift = 4u32;
    let mut cur = c;
    while cur & 0x80 != 0 {
        cur = pack_bytes
            .get(pos)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("truncated pack"))?;
        pos += 1;
        size |= ((cur & 0x7f) as usize) << shift;
        shift += 7;
    }

    match type_code {
        1..=4 => {
            let kind = match type_code {
                1 => ObjectKind::Commit,
                2 => ObjectKind::Tree,
                3 => ObjectKind::Blob,
                4 => ObjectKind::Tag,
                _ => unreachable!(),
            };
            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut data = Vec::with_capacity(size);
            decoder.read_to_end(&mut data)?;
            Ok(grit_lib::objects::Object::new(kind, data))
        }
        6 => {
            // OFS_DELTA
            let mut c2 = pack_bytes
                .get(pos)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("truncated"))?;
            pos += 1;
            let mut neg_off = (c2 & 0x7f) as u64;
            while c2 & 0x80 != 0 {
                c2 = pack_bytes
                    .get(pos)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("truncated"))?;
                pos += 1;
                neg_off = ((neg_off + 1) << 7) | (c2 & 0x7f) as u64;
            }
            let base_offset = offset
                .checked_sub(neg_off)
                .ok_or_else(|| anyhow::anyhow!("ofs-delta underflow"))?;

            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta_data = Vec::with_capacity(size);
            decoder.read_to_end(&mut delta_data)?;

            let base_obj = read_object_from_pack(pack_bytes, base_offset, indexes)?;
            let result = grit_lib::unpack_objects::apply_delta(&base_obj.data, &delta_data)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base_obj.kind, result))
        }
        7 => {
            // REF_DELTA
            if pos + 20 > pack_bytes.len() {
                bail!("truncated ref-delta");
            }
            let base_oid = ObjectId::from_bytes(&pack_bytes[pos..pos + 20])
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            pos += 20;

            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut decoder = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta_data = Vec::with_capacity(size);
            decoder.read_to_end(&mut delta_data)?;

            // Find the base in any pack.
            let mut base_obj = None;
            for idx in indexes {
                if let Some(entry) = idx.entries.iter().find(|e| e.oid == base_oid) {
                    let pb = std::fs::read(&idx.pack_path)?;
                    base_obj = Some(read_object_from_pack(&pb, entry.offset, indexes)?);
                    break;
                }
            }
            let base = base_obj.ok_or_else(|| anyhow::anyhow!("ref-delta base not found"))?;
            let result = grit_lib::unpack_objects::apply_delta(&base.data, &delta_data)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base.kind, result))
        }
        other => bail!("unknown pack type {other}"),
    }
}

/// Build a PACK v2 byte stream from entries (whole objects, no delta).
fn build_pack(entries: &[PackEntry]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"PACK");
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());

    for entry in entries {
        let type_code: u8 = match entry.kind {
            ObjectKind::Commit => 1,
            ObjectKind::Tree => 2,
            ObjectKind::Blob => 3,
            ObjectKind::Tag => 4,
        };

        // Encode type+size header.
        let mut size = entry.data.len();
        let first = ((type_code & 0x7) << 4) | (size & 0x0f) as u8;
        size >>= 4;
        if size > 0 {
            buf.push(first | 0x80);
            while size > 0 {
                let b = (size & 0x7f) as u8;
                size >>= 7;
                buf.push(if size > 0 { b | 0x80 } else { b });
            }
        } else {
            buf.push(first);
        }

        // zlib-compress data.
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&entry.data)?;
        let compressed = enc.finish()?;
        buf.extend_from_slice(&compressed);
    }

    // Trailing SHA-1 checksum.
    let mut hasher = Sha1::new();
    hasher.update(&buf);
    let digest = hasher.finalize();
    buf.extend_from_slice(digest.as_slice());

    Ok(buf)
}

/// Build idx v2 for a pack we just wrote.
fn build_idx_for_pack(pack_bytes: &[u8], entries: &[PackEntry]) -> Result<Vec<u8>> {
    // We need offsets. Reparse the pack to get them.
    let nr = entries.len();
    let mut offsets = Vec::with_capacity(nr);
    let mut pos = 12usize; // skip header

    for _entry in entries {
        offsets.push(pos as u64);
        // Skip past the entry in the pack.
        let c = pack_bytes[pos];
        pos += 1;
        let mut size = (c & 0x0f) as usize;
        let mut shift = 4u32;
        let mut cur = c;
        while cur & 0x80 != 0 {
            cur = pack_bytes[pos];
            pos += 1;
            size |= ((cur & 0x7f) as usize) << shift;
            shift += 7;
        }
        // Skip compressed data.
        use flate2::read::ZlibDecoder;
        use std::io::Read;
        let mut dec = ZlibDecoder::new(&pack_bytes[pos..]);
        let mut tmp = Vec::with_capacity(size);
        dec.read_to_end(&mut tmp)?;
        pos += dec.total_in() as usize;
    }

    // Build sorted index.
    let mut sorted: Vec<(usize, &PackEntry)> = entries.iter().enumerate().collect();
    sorted.sort_by_key(|(_, e)| *e.oid.as_bytes());

    let mut buf = Vec::new();
    // Header.
    buf.extend_from_slice(&[0xFF, b't', b'O', b'c']);
    buf.extend_from_slice(&2u32.to_be_bytes());

    // Fanout.
    let mut fanout = [0u32; 256];
    for (_, entry) in &sorted {
        fanout[entry.oid.as_bytes()[0] as usize] += 1;
    }
    for i in 1..256 {
        fanout[i] += fanout[i - 1];
    }
    for slot in &fanout {
        buf.extend_from_slice(&slot.to_be_bytes());
    }

    // OID table.
    for (_, entry) in &sorted {
        buf.extend_from_slice(entry.oid.as_bytes());
    }

    // CRC32 table: compute CRC32 for each entry's raw bytes in the pack.
    for (orig_idx, _) in &sorted {
        let off = offsets[*orig_idx] as usize;
        // Find the end of this entry.
        let next_off = if *orig_idx + 1 < nr {
            offsets[*orig_idx + 1] as usize
        } else {
            pack_bytes.len() - 20 // before trailing checksum
        };
        let crc = crc32_slice(&pack_bytes[off..next_off]);
        buf.extend_from_slice(&crc.to_be_bytes());
    }

    // Offset table.
    let mut large_offsets: Vec<u64> = Vec::new();
    for (orig_idx, _) in &sorted {
        let off = offsets[*orig_idx];
        if off >= 0x8000_0000 {
            let idx = large_offsets.len() as u32;
            buf.extend_from_slice(&(idx | 0x8000_0000).to_be_bytes());
            large_offsets.push(off);
        } else {
            buf.extend_from_slice(&(off as u32).to_be_bytes());
        }
    }

    // Large offset table.
    for off in &large_offsets {
        buf.extend_from_slice(&off.to_be_bytes());
    }

    // Pack checksum.
    let pack_checksum = &pack_bytes[pack_bytes.len() - 20..];
    buf.extend_from_slice(pack_checksum);

    // Index checksum.
    let mut h = Sha1::new();
    h.update(&buf);
    let idx_checksum = h.finalize();
    buf.extend_from_slice(idx_checksum.as_slice());

    Ok(buf)
}

/// CRC32 IEEE.
fn crc32_slice(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let idx = ((crc ^ b as u32) & 0xFF) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    !crc
}

static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};
