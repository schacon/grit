//! `grit bundle` — move objects and refs by archive.
//!
//! Implements create, verify, list-heads, and unbundle subcommands.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;

/// Arguments for `grit bundle`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub action: BundleAction,
}

#[derive(Debug, Subcommand)]
pub enum BundleAction {
    /// Create a bundle file.
    Create(CreateArgs),
    /// Verify a bundle file.
    Verify(VerifyArgs),
    /// List references in a bundle.
    #[command(name = "list-heads")]
    ListHeads(ListHeadsArgs),
    /// Unbundle objects from a bundle file.
    Unbundle(UnbundleArgs),
}

#[derive(Debug, ClapArgs)]
pub struct CreateArgs {
    /// Output bundle file path.
    #[arg(value_name = "FILE")]
    pub file: String,

    /// Revision arguments (refs, commit ranges, --all).
    #[arg(value_name = "REV", num_args = 0.., trailing_var_arg = true)]
    pub rev_list_args: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct VerifyArgs {
    /// Bundle file to verify.
    #[arg(value_name = "FILE")]
    pub file: String,
}

#[derive(Debug, ClapArgs)]
pub struct ListHeadsArgs {
    /// Bundle file.
    #[arg(value_name = "FILE")]
    pub file: String,
}

#[derive(Debug, ClapArgs)]
pub struct UnbundleArgs {
    /// Bundle file to unbundle.
    #[arg(value_name = "FILE")]
    pub file: String,
}

/// Run `grit bundle`.
pub fn run(args: Args) -> Result<()> {
    match args.action {
        BundleAction::Create(a) => run_create(a),
        BundleAction::Verify(a) => run_verify(a),
        BundleAction::ListHeads(a) => run_list_heads(a),
        BundleAction::Unbundle(a) => run_unbundle(a),
    }
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

fn run_create(args: CreateArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Collect refs to include.
    let refs = collect_refs_for_bundle(&repo, &args.rev_list_args)?;
    if refs.is_empty() {
        bail!("refusing to create empty bundle");
    }

    // Collect all reachable objects from those refs.
    let mut oids = std::collections::BTreeSet::new();
    for oid in refs.values() {
        walk_reachable(&repo, oid, &mut oids)?;
    }

    // Read all objects.
    let mut objects: Vec<(ObjectId, ObjectKind, Vec<u8>)> = Vec::new();
    for oid in &oids {
        let obj = read_object(&repo, oid)?;
        objects.push((*oid, obj.kind, obj.data));
    }

    // Build pack data.
    let pack_data = build_pack_data(&objects)?;

    // Write bundle file.
    let mut out = fs::File::create(&args.file)
        .with_context(|| format!("cannot create {}", args.file))?;

    // Bundle v2 header.
    out.write_all(b"# v2 git bundle\n")?;

    // Write refs.
    for (refname, oid) in &refs {
        writeln!(out, "{} {}", oid.to_hex(), refname)?;
    }
    out.write_all(b"\n")?; // blank line separates header from pack data

    // Write pack data.
    out.write_all(&pack_data)?;

    eprintln!(
        "Total {} (delta 0), reused 0 (delta 0)",
        objects.len()
    );

    Ok(())
}

fn collect_refs_for_bundle(
    repo: &Repository,
    rev_args: &[String],
) -> Result<BTreeMap<String, ObjectId>> {
    let mut refs = BTreeMap::new();

    let include_all = rev_args.iter().any(|a| a == "--all");

    if include_all {
        // Include all refs.
        collect_all_refs(repo, &mut refs)?;
    } else if rev_args.is_empty() {
        // Default: include HEAD if it exists.
        if let Ok(oid) = resolve_ref(repo, "HEAD") {
            refs.insert("HEAD".to_string(), oid);
        }
    } else {
        for arg in rev_args {
            if arg.starts_with('-') {
                continue; // skip flags
            }
            let oid = resolve_ref(repo, arg)
                .with_context(|| format!("cannot resolve '{arg}'"))?;
            refs.insert(arg.clone(), oid);
        }
    }

    Ok(refs)
}

fn collect_all_refs(repo: &Repository, refs: &mut BTreeMap<String, ObjectId>) -> Result<()> {
    // HEAD
    if let Ok(oid) = resolve_ref(repo, "HEAD") {
        refs.insert("HEAD".to_string(), oid);
    }

    // Walk refs/ directory.
    let refs_dir = repo.git_dir.join("refs");
    if refs_dir.exists() {
        walk_refs_dir(&refs_dir, "refs", repo, refs)?;
    }

    Ok(())
}

fn walk_refs_dir(
    dir: &std::path::Path,
    prefix: &str,
    repo: &Repository,
    refs: &mut BTreeMap<String, ObjectId>,
) -> Result<()> {
    let rd = fs::read_dir(dir)?;
    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let full_ref = format!("{prefix}/{name_str}");

        if path.is_dir() {
            walk_refs_dir(&path, &full_ref, repo, refs)?;
        } else if path.is_file() {
            if let Ok(oid) = resolve_ref(repo, &full_ref) {
                refs.insert(full_ref, oid);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

fn run_verify(args: VerifyArgs) -> Result<()> {
    let data = fs::read(&args.file).with_context(|| format!("cannot read {}", args.file))?;
    let (refs, pack_start) = parse_bundle_header(&data)?;

    // Validate pack data.
    let pack_data = &data[pack_start..];
    if pack_data.len() < 12 + 20 {
        bail!("bundle pack data too small");
    }
    if &pack_data[0..4] != b"PACK" {
        bail!("bundle does not contain valid pack data");
    }

    eprintln!(
        "The bundle contains {} ref(s)",
        refs.len()
    );
    for (refname, oid) in &refs {
        eprintln!("{} {refname}", oid.to_hex());
    }

    println!("{} is okay", args.file);
    Ok(())
}

// ---------------------------------------------------------------------------
// list-heads
// ---------------------------------------------------------------------------

fn run_list_heads(args: ListHeadsArgs) -> Result<()> {
    let data = fs::read(&args.file).with_context(|| format!("cannot read {}", args.file))?;
    let (refs, _) = parse_bundle_header(&data)?;

    for (refname, oid) in &refs {
        println!("{} {refname}", oid.to_hex());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// unbundle
// ---------------------------------------------------------------------------

fn run_unbundle(args: UnbundleArgs) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let data = fs::read(&args.file).with_context(|| format!("cannot read {}", args.file))?;
    let (refs, pack_start) = parse_bundle_header(&data)?;

    let pack_data = &data[pack_start..];
    if pack_data.len() < 12 + 20 {
        bail!("bundle pack data too small");
    }

    // Use unpack-objects to extract into the ODB.
    let opts = grit_lib::unpack_objects::UnpackOptions {
        dry_run: false,
        quiet: false,
    };
    let count = grit_lib::unpack_objects::unpack_objects(&mut &pack_data[..], &repo.odb, &opts)
        .map_err(|e| anyhow::anyhow!("unbundle failed: {e}"))?;

    // Update refs.
    for (refname, oid) in &refs {
        let ref_path = repo.git_dir.join(refname);
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&ref_path, format!("{}\n", oid.to_hex()))?;
    }

    eprintln!("Unbundled {count} objects");
    Ok(())
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/// Parse the bundle v2 header, returning refs and the byte offset where pack data starts.
fn parse_bundle_header(data: &[u8]) -> Result<(BTreeMap<String, ObjectId>, usize)> {
    // Find the header line.
    let header_line = b"# v2 git bundle\n";
    if !data.starts_with(header_line) {
        bail!("not a v2 git bundle");
    }

    let mut pos = header_line.len();
    let mut refs = BTreeMap::new();

    loop {
        // Find end of line.
        let eol = data[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| pos + i)
            .ok_or_else(|| anyhow::anyhow!("truncated bundle header"))?;

        let line = &data[pos..eol];
        if line.is_empty() {
            // Blank line → pack data follows.
            pos = eol + 1;
            break;
        }

        let line_str = std::str::from_utf8(line)
            .context("invalid UTF-8 in bundle header")?;

        // Prerequisite lines start with '-'.
        if line_str.starts_with('-') {
            pos = eol + 1;
            continue;
        }

        // ref line: "<hex-oid> <refname>"
        if let Some((hex, refname)) = line_str.split_once(' ') {
            let oid = ObjectId::from_hex(hex)
                .map_err(|e| anyhow::anyhow!("bad oid in bundle header: {e}"))?;
            refs.insert(refname.to_string(), oid);
        }

        pos = eol + 1;
    }

    Ok((refs, pos))
}

fn resolve_ref(repo: &Repository, refname: &str) -> Result<ObjectId> {
    let candidates = [
        repo.git_dir.join(refname),
        repo.git_dir.join("refs/heads").join(refname),
        repo.git_dir.join("refs/tags").join(refname),
    ];
    for path in &candidates {
        if path.is_file() {
            let content = fs::read_to_string(path)?;
            let trimmed = content.trim();
            if trimmed.starts_with("ref: ") {
                return resolve_ref(repo, &trimmed[5..]);
            }
            return ObjectId::from_hex(trimmed)
                .map_err(|e| anyhow::anyhow!("cannot resolve ref '{refname}': {e}"));
        }
    }
    if refname == "HEAD" {
        let head = fs::read_to_string(repo.git_dir.join("HEAD"))?;
        let trimmed = head.trim();
        if trimmed.starts_with("ref: ") {
            return resolve_ref(repo, &trimmed[5..]);
        }
        return ObjectId::from_hex(trimmed)
            .map_err(|e| anyhow::anyhow!("cannot resolve HEAD: {e}"));
    }
    bail!("cannot resolve ref '{refname}'")
}

fn walk_reachable(
    repo: &Repository,
    oid: &ObjectId,
    oids: &mut std::collections::BTreeSet<ObjectId>,
) -> Result<()> {
    if !oids.insert(*oid) {
        return Ok(());
    }
    let obj = match read_object(repo, oid) {
        Ok(o) => o,
        Err(_) => return Ok(()),
    };
    match obj.kind {
        ObjectKind::Commit => {
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                for line in text.lines() {
                    if let Some(hex) = line.strip_prefix("tree ") {
                        if let Ok(tree_oid) = ObjectId::from_hex(hex.trim()) {
                            walk_reachable(repo, &tree_oid, oids)?;
                        }
                    } else if let Some(hex) = line.strip_prefix("parent ") {
                        if let Ok(parent_oid) = ObjectId::from_hex(hex.trim()) {
                            walk_reachable(repo, &parent_oid, oids)?;
                        }
                    } else if line.is_empty() {
                        break;
                    }
                }
            }
        }
        ObjectKind::Tree => {
            let data = &obj.data;
            let mut pos = 0;
            while pos < data.len() {
                let nul = data[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|i| pos + i);
                if let Some(nul) = nul {
                    if nul + 21 <= data.len() {
                        if let Ok(entry_oid) = ObjectId::from_bytes(&data[nul + 1..nul + 21]) {
                            walk_reachable(repo, &entry_oid, oids)?;
                        }
                        pos = nul + 21;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        ObjectKind::Tag => {
            if let Ok(text) = std::str::from_utf8(&obj.data) {
                if let Some(first_line) = text.lines().next() {
                    if let Some(hex) = first_line.strip_prefix("object ") {
                        if let Ok(target_oid) = ObjectId::from_hex(hex.trim()) {
                            walk_reachable(repo, &target_oid, oids)?;
                        }
                    }
                }
            }
        }
        ObjectKind::Blob => {}
    }
    Ok(())
}

fn read_object(
    repo: &Repository,
    oid: &ObjectId,
) -> Result<grit_lib::objects::Object> {
    if let Ok(obj) = repo.odb.read(oid) {
        return Ok(obj);
    }
    // Try pack files.
    let indexes = grit_lib::pack::read_local_pack_indexes(repo.odb.objects_dir())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for idx in &indexes {
        if let Some(entry) = idx.entries.iter().find(|e| e.oid == *oid) {
            let pack_bytes = fs::read(&idx.pack_path)?;
            return read_from_pack(&pack_bytes, entry.offset, &indexes);
        }
    }
    bail!("object not found: {}", oid.to_hex())
}

fn read_from_pack(
    pack_bytes: &[u8],
    offset: u64,
    indexes: &[grit_lib::pack::PackIndex],
) -> Result<grit_lib::objects::Object> {
    let mut pos = offset as usize;
    let c = pack_bytes.get(pos).copied().ok_or_else(|| anyhow::anyhow!("truncated"))?;
    pos += 1;
    let type_code = (c >> 4) & 0x7;
    let mut size = (c & 0x0f) as usize;
    let mut shift = 4u32;
    let mut cur = c;
    while cur & 0x80 != 0 {
        cur = pack_bytes.get(pos).copied().ok_or_else(|| anyhow::anyhow!("truncated"))?;
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
            let mut dec = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut data = Vec::with_capacity(size);
            dec.read_to_end(&mut data)?;
            Ok(grit_lib::objects::Object::new(kind, data))
        }
        6 => {
            let mut c2 = pack_bytes.get(pos).copied().ok_or_else(|| anyhow::anyhow!("truncated"))?;
            pos += 1;
            let mut neg_off = (c2 & 0x7f) as u64;
            while c2 & 0x80 != 0 {
                c2 = pack_bytes.get(pos).copied().ok_or_else(|| anyhow::anyhow!("truncated"))?;
                pos += 1;
                neg_off = ((neg_off + 1) << 7) | (c2 & 0x7f) as u64;
            }
            let base_offset = offset.checked_sub(neg_off)
                .ok_or_else(|| anyhow::anyhow!("ofs-delta underflow"))?;

            use flate2::read::ZlibDecoder;
            let mut dec = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta = Vec::with_capacity(size);
            dec.read_to_end(&mut delta)?;

            let base = read_from_pack(pack_bytes, base_offset, indexes)?;
            let result = grit_lib::unpack_objects::apply_delta(&base.data, &delta)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base.kind, result))
        }
        7 => {
            if pos + 20 > pack_bytes.len() {
                bail!("truncated ref-delta");
            }
            let base_oid = ObjectId::from_bytes(&pack_bytes[pos..pos + 20])
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            pos += 20;

            use flate2::read::ZlibDecoder;
            let mut dec = ZlibDecoder::new(&pack_bytes[pos..]);
            let mut delta = Vec::with_capacity(size);
            dec.read_to_end(&mut delta)?;

            let mut base_obj = None;
            for idx in indexes {
                if let Some(e) = idx.entries.iter().find(|e| e.oid == base_oid) {
                    let pb = fs::read(&idx.pack_path)?;
                    base_obj = Some(read_from_pack(&pb, e.offset, indexes)?);
                    break;
                }
            }
            let base = base_obj.ok_or_else(|| anyhow::anyhow!("ref-delta base not found"))?;
            let result = grit_lib::unpack_objects::apply_delta(&base.data, &delta)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grit_lib::objects::Object::new(base.kind, result))
        }
        other => bail!("unknown pack type {other}"),
    }
}

fn build_pack_data(objects: &[(ObjectId, ObjectKind, Vec<u8>)]) -> Result<Vec<u8>> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    let mut buf = Vec::new();
    buf.extend_from_slice(b"PACK");
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&(objects.len() as u32).to_be_bytes());

    for (_, kind, data) in objects {
        let type_code: u8 = match kind {
            ObjectKind::Commit => 1,
            ObjectKind::Tree => 2,
            ObjectKind::Blob => 3,
            ObjectKind::Tag => 4,
        };
        let mut size = data.len();
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

        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data)?;
        let compressed = enc.finish()?;
        buf.extend_from_slice(&compressed);
    }

    let mut hasher = Sha1::new();
    hasher.update(&buf);
    let digest = hasher.finalize();
    buf.extend_from_slice(digest.as_slice());

    Ok(buf)
}
