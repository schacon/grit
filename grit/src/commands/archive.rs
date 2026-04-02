//! `grit archive` — create a tar/zip archive of a tree.
//!
//! Walks the tree object and writes entries in tar (default) or zip format.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use std::fs::File;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// Arguments for `grit archive`.
#[derive(Debug, ClapArgs)]
#[command(about = "Create an archive of files from a named tree")]
pub struct Args {
    /// Format of the resulting archive: tar or zip.
    #[arg(long, default_value = "tar")]
    pub format: String,

    /// Prepend <prefix> to each filename in the archive.
    #[arg(long)]
    pub prefix: Option<String>,

    /// Write the archive to <file> instead of stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<String>,

    /// The tree or commit to produce an archive for.
    pub tree_ish: String,

    /// Paths to restrict archiving (optional).
    pub paths: Vec<String>,
}

/// Run `grit archive`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let oid = resolve_tree_ish(&repo, &args.tree_ish)?;
    let obj = repo.odb.read(&oid)?;

    // Dereference commits to their tree.
    let tree_data = if obj.kind == ObjectKind::Commit {
        let commit = parse_commit(&obj.data).context("parsing commit")?;
        let tree_obj = repo.odb.read(&commit.tree).context("reading tree")?;
        tree_obj.data
    } else if obj.kind == ObjectKind::Tree {
        obj.data
    } else {
        bail!("'{}' is not a tree or commit", args.tree_ish);
    };

    let prefix = args.prefix.as_deref().unwrap_or("");

    let format = match args.format.as_str() {
        "tar" => ArchiveFormat::Tar,
        "zip" => ArchiveFormat::Zip,
        other => bail!("unsupported archive format: '{other}'"),
    };

    // Collect all entries
    let mut entries: Vec<ArchiveEntry> = Vec::new();
    collect_entries(&repo, &tree_data, prefix, &args.paths, &mut entries)?;

    // If prefix ends with '/', add a directory entry for it
    if !prefix.is_empty() && prefix.ends_with('/') {
        entries.insert(
            0,
            ArchiveEntry {
                path: prefix.to_string(),
                mode: 0o040000,
                data: Vec::new(),
            },
        );
    }

    // Write archive
    if let Some(output_path) = &args.output {
        // Infer format from extension if --format wasn't explicitly set
        let format = if args.format == "tar" && output_path.ends_with(".zip") {
            ArchiveFormat::Zip
        } else {
            format
        };
        let file = File::create(output_path)
            .with_context(|| format!("creating output file '{output_path}'"))?;
        let mut buf = io::BufWriter::new(file);
        write_archive(&mut buf, &entries, format)?;
        buf.flush()?;
    } else {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        write_archive(&mut out, &entries, format)?;
        out.flush()?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ArchiveFormat {
    Tar,
    Zip,
}

struct ArchiveEntry {
    path: String,
    mode: u32,
    data: Vec<u8>,
}

fn collect_entries(
    repo: &Repository,
    tree_data: &[u8],
    prefix: &str,
    filter_paths: &[String],
    entries: &mut Vec<ArchiveEntry>,
) -> Result<()> {
    let tree_entries = parse_tree(tree_data)?;

    for entry in &tree_entries {
        let name = String::from_utf8_lossy(&entry.name);
        let full_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}{name}")
        };

        // Apply path filter
        if !filter_paths.is_empty() {
            let matches = filter_paths.iter().any(|p| {
                full_path == *p
                    || full_path.starts_with(&format!("{p}/"))
                    || p.starts_with(&format!("{full_path}/"))
            });
            if !matches {
                continue;
            }
        }

        let is_tree = entry.mode == 0o040000;
        if is_tree {
            let sub_obj = repo.odb.read(&entry.oid)?;
            let dir_path = format!("{full_path}/");
            entries.push(ArchiveEntry {
                path: dir_path.clone(),
                mode: entry.mode,
                data: Vec::new(),
            });
            collect_entries(repo, &sub_obj.data, &dir_path, filter_paths, entries)?;
        } else {
            let blob = repo.odb.read(&entry.oid)?;
            entries.push(ArchiveEntry {
                path: full_path,
                mode: entry.mode,
                data: blob.data,
            });
        }
    }
    Ok(())
}

fn write_archive(
    out: &mut impl Write,
    entries: &[ArchiveEntry],
    format: ArchiveFormat,
) -> Result<()> {
    match format {
        ArchiveFormat::Tar => write_tar(out, entries),
        ArchiveFormat::Zip => write_zip(out, entries),
    }
}

// ─── TAR implementation ───

fn write_tar(out: &mut impl Write, entries: &[ArchiveEntry]) -> Result<()> {
    let mtime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for entry in entries {
        let is_dir = entry.path.ends_with('/');
        let typeflag = if is_dir { b'5' } else { b'0' };
        let mode = if is_dir { 0o755 } else { entry.mode & 0o777 };
        // Default to 0o644 for regular files if mode looks like a git tree mode
        let mode = if !is_dir && mode == 0 { 0o644 } else { mode };

        let size = if is_dir { 0 } else { entry.data.len() };

        write_tar_header(out, &entry.path, size, mode, mtime, typeflag)?;

        if !is_dir {
            out.write_all(&entry.data)?;
            // Pad to 512-byte boundary
            let remainder = size % 512;
            if remainder != 0 {
                let padding = 512 - remainder;
                out.write_all(&vec![0u8; padding])?;
            }
        }
    }

    // End-of-archive: two 512-byte blocks of zeros
    out.write_all(&[0u8; 1024])?;
    Ok(())
}

fn write_tar_header(
    out: &mut impl Write,
    path: &str,
    size: usize,
    mode: u32,
    mtime: u64,
    typeflag: u8,
) -> Result<()> {
    let mut header = [0u8; 512];

    // name (0..100)
    let name_bytes = path.as_bytes();
    let name_len = name_bytes.len().min(100);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);

    // mode (100..108)
    let mode_str = format!("{mode:07o}");
    header[100..100 + mode_str.len()].copy_from_slice(mode_str.as_bytes());

    // uid (108..116) — 0
    header[108..115].copy_from_slice(b"0000000");

    // gid (116..124) — 0
    header[116..123].copy_from_slice(b"0000000");

    // size (124..136)
    let size_str = format!("{size:011o}");
    header[124..124 + size_str.len()].copy_from_slice(size_str.as_bytes());

    // mtime (136..148)
    let mtime_str = format!("{mtime:011o}");
    header[136..136 + mtime_str.len()].copy_from_slice(mtime_str.as_bytes());

    // typeflag (156)
    header[156] = typeflag;

    // magic (257..263) — "ustar\0"
    header[257..263].copy_from_slice(b"ustar\0");

    // version (263..265) — "00"
    header[263..265].copy_from_slice(b"00");

    // Compute checksum: fill checksum field with spaces first
    header[148..156].copy_from_slice(b"        ");
    let cksum: u32 = header.iter().map(|&b| b as u32).sum();
    let cksum_str = format!("{cksum:06o}\0 ");
    header[148..148 + cksum_str.len()].copy_from_slice(cksum_str.as_bytes());

    out.write_all(&header)?;
    Ok(())
}

// ─── ZIP implementation ───

fn write_zip(out: &mut impl Write, entries: &[ArchiveEntry]) -> Result<()> {
    // We'll track offsets for the central directory
    let mut central_entries: Vec<ZipCentralEntry> = Vec::new();
    let mut offset: u32 = 0;

    for entry in entries {
        let is_dir = entry.path.ends_with('/');
        let path_bytes = entry.path.as_bytes();

        // Compress data
        let (compressed, method, crc) = if is_dir {
            (Vec::new(), 0u16, 0u32)
        } else {
            let crc = crc32(&entry.data);
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&entry.data)?;
            let compressed = encoder.finish()?;
            // Use deflate (method 8) if it saves space, otherwise store (0)
            if compressed.len() < entry.data.len() {
                (compressed, 8u16, crc)
            } else {
                (entry.data.clone(), 0u16, crc)
            }
        };

        let uncompressed_size = if is_dir { 0u32 } else { entry.data.len() as u32 };
        let compressed_size = compressed.len() as u32;

        // External attributes
        let external_attr = if is_dir {
            0o40755u32 << 16
        } else {
            let mode = entry.mode & 0o777;
            let mode = if mode == 0 { 0o644 } else { mode };
            (mode as u32) << 16
        };

        // Local file header
        let local_header_size = 30 + path_bytes.len();
        out.write_all(&0x04034b50u32.to_le_bytes())?; // signature
        out.write_all(&20u16.to_le_bytes())?; // version needed
        out.write_all(&0u16.to_le_bytes())?; // flags
        out.write_all(&method.to_le_bytes())?; // compression method
        out.write_all(&0u16.to_le_bytes())?; // mod time
        out.write_all(&0u16.to_le_bytes())?; // mod date
        out.write_all(&crc.to_le_bytes())?; // crc32
        out.write_all(&compressed_size.to_le_bytes())?;
        out.write_all(&uncompressed_size.to_le_bytes())?;
        out.write_all(&(path_bytes.len() as u16).to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?; // extra field len

        out.write_all(path_bytes)?;
        out.write_all(&compressed)?;

        central_entries.push(ZipCentralEntry {
            path: entry.path.clone(),
            method,
            crc,
            compressed_size,
            uncompressed_size,
            external_attr,
            local_header_offset: offset,
        });

        offset += local_header_size as u32 + compressed_size;
    }

    // Central directory
    let cd_offset = offset;
    let mut cd_size: u32 = 0;

    for ce in &central_entries {
        let path_bytes = ce.path.as_bytes();
        let entry_size = 46 + path_bytes.len();

        out.write_all(&0x02014b50u32.to_le_bytes())?; // signature
        out.write_all(&20u16.to_le_bytes())?; // version made by
        out.write_all(&20u16.to_le_bytes())?; // version needed
        out.write_all(&0u16.to_le_bytes())?; // flags
        out.write_all(&ce.method.to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?; // mod time
        out.write_all(&0u16.to_le_bytes())?; // mod date
        out.write_all(&ce.crc.to_le_bytes())?;
        out.write_all(&ce.compressed_size.to_le_bytes())?;
        out.write_all(&ce.uncompressed_size.to_le_bytes())?;
        out.write_all(&(path_bytes.len() as u16).to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?; // extra field len
        out.write_all(&0u16.to_le_bytes())?; // comment len
        out.write_all(&0u16.to_le_bytes())?; // disk number
        out.write_all(&0u16.to_le_bytes())?; // internal attrs
        out.write_all(&ce.external_attr.to_le_bytes())?;
        out.write_all(&ce.local_header_offset.to_le_bytes())?;
        out.write_all(path_bytes)?;

        cd_size += entry_size as u32;
    }

    // End of central directory record
    out.write_all(&0x06054b50u32.to_le_bytes())?; // signature
    out.write_all(&0u16.to_le_bytes())?; // disk number
    out.write_all(&0u16.to_le_bytes())?; // disk with cd
    out.write_all(&(central_entries.len() as u16).to_le_bytes())?;
    out.write_all(&(central_entries.len() as u16).to_le_bytes())?;
    out.write_all(&cd_size.to_le_bytes())?;
    out.write_all(&cd_offset.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?; // comment len

    Ok(())
}

struct ZipCentralEntry {
    path: String,
    method: u16,
    crc: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    external_attr: u32,
    local_header_offset: u32,
}

/// Simple CRC-32 (ISO 3309 / ITU-T V.42).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn resolve_tree_ish(repo: &Repository, s: &str) -> Result<ObjectId> {
    if let Ok(oid) = s.parse::<ObjectId>() {
        return Ok(oid);
    }
    if let Ok(oid) = resolve_ref(&repo.git_dir, s) {
        return Ok(oid);
    }
    let as_branch = format!("refs/heads/{s}");
    if let Ok(oid) = resolve_ref(&repo.git_dir, &as_branch) {
        return Ok(oid);
    }
    bail!("not a valid tree-ish: '{s}'")
}
