//! Reftable format — binary reference storage.
//!
//! Implements the [reftable file format](https://git-scm.com/docs/reftable)
//! for efficient, sorted reference storage.  A reftable file contains
//! ref blocks (sorted ref records with prefix compression), optional log
//! blocks (reflog entries), optional index blocks, and a footer.
//!
//! # Architecture
//!
//! - [`ReftableWriter`] writes a single `.ref` (or `.log`) reftable file.
//! - [`ReftableReader`] reads and searches a single reftable file.
//! - [`ReftableStack`] manages the `tables.list` stack, providing a
//!   merged view of all tables and auto-compaction on writes.
//!
//! # On-disk layout
//!
//! ```text
//! first_block { header, first_ref_block }
//! ref_block*
//! ref_index?
//! obj_block*    (not yet implemented)
//! obj_index?    (not yet implemented)
//! log_block*
//! log_index?
//! footer
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::objects::ObjectId;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic bytes at the start of every reftable file.
const REFTABLE_MAGIC: &[u8; 4] = b"REFT";

/// File header size (version 1): magic(4) + version(1) + block_size(3)
/// + min_update_index(8) + max_update_index(8) = 24 bytes.
const HEADER_SIZE: usize = 24;

/// Footer size for version 1.
const FOOTER_V1_SIZE: usize = 68;

/// Block type: ref block.
const BLOCK_TYPE_REF: u8 = b'r';
/// Block type: index block.
const BLOCK_TYPE_INDEX: u8 = b'i';
/// Block type: log block (zlib-compressed).
const BLOCK_TYPE_LOG: u8 = b'g';

/// Value types encoded in the low 3 bits of the suffix_length varint.
const VALUE_DELETION: u8 = 0;
const VALUE_ONE_OID: u8 = 1;
const VALUE_TWO_OID: u8 = 2;
const VALUE_SYMREF: u8 = 3;

/// Hash size (SHA-1).
const HASH_SIZE: usize = 20;

/// Default block size when none is configured (4 KiB).
const DEFAULT_BLOCK_SIZE: u32 = 4096;

/// How many records between restart points.
const RESTART_INTERVAL: usize = 16;

// ---------------------------------------------------------------------------
// Varint encoding (Git pack-style)
// ---------------------------------------------------------------------------

/// Encode a u64 as a varint into `out`. Returns number of bytes written.
fn put_varint(mut val: u64, out: &mut Vec<u8>) -> usize {
    // First, collect 7-bit groups.
    let mut buf = [0u8; 10];
    let mut i = 0;
    buf[i] = (val & 0x7f) as u8;
    i += 1;
    val >>= 7;
    while val > 0 {
        val -= 1;
        buf[i] = (val & 0x7f) as u8;
        i += 1;
        val >>= 7;
    }
    // Write in reverse, with continuation bits.
    let len = i;
    for j in (1..len).rev() {
        out.push(buf[j] | 0x80);
    }
    out.push(buf[0]);
    len
}

/// Decode a varint from `data` starting at `pos`. Returns (value, new_pos).
fn get_varint(data: &[u8], mut pos: usize) -> Result<(u64, usize)> {
    if pos >= data.len() {
        return Err(Error::InvalidRef("varint: unexpected end of data".into()));
    }
    let mut val = (data[pos] & 0x7f) as u64;
    while data[pos] & 0x80 != 0 {
        pos += 1;
        if pos >= data.len() {
            return Err(Error::InvalidRef("varint: unexpected end of data".into()));
        }
        val = ((val + 1) << 7) | (data[pos] & 0x7f) as u64;
    }
    Ok((val, pos + 1))
}

// ---------------------------------------------------------------------------
// Ref record types
// ---------------------------------------------------------------------------

/// A single reference record as stored in a reftable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefValue {
    /// Deletion tombstone (value_type 0x0).
    Deletion,
    /// A direct ref pointing to one OID (value_type 0x1).
    Val1(ObjectId),
    /// An annotated tag: value + peeled target (value_type 0x2).
    Val2(ObjectId, ObjectId),
    /// A symbolic reference (value_type 0x3).
    Symref(String),
}

/// A decoded ref record.
#[derive(Debug, Clone)]
pub struct RefRecord {
    /// Full reference name.
    pub name: String,
    /// Update index (absolute).
    pub update_index: u64,
    /// The value.
    pub value: RefValue,
}

/// A decoded log record.
#[derive(Debug, Clone)]
pub struct LogRecord {
    /// Reference name.
    pub refname: String,
    /// Update index.
    pub update_index: u64,
    /// Old object ID.
    pub old_id: ObjectId,
    /// New object ID.
    pub new_id: ObjectId,
    /// Committer name.
    pub name: String,
    /// Committer email (without angle brackets).
    pub email: String,
    /// Time in seconds since epoch.
    pub time_seconds: u64,
    /// Timezone offset in minutes (signed).
    pub tz_offset: i16,
    /// Log message.
    pub message: String,
}

/// Write options for reftable creation.
#[derive(Debug, Clone)]
pub struct WriteOptions {
    /// Block size in bytes. 0 means unaligned (variable-sized blocks).
    pub block_size: u32,
    /// Restart interval (number of records between restart points).
    pub restart_interval: usize,
    /// Whether to write log blocks.
    pub write_log: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            block_size: DEFAULT_BLOCK_SIZE,
            restart_interval: RESTART_INTERVAL,
            write_log: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Writes a single reftable file.
///
/// Usage:
/// ```ignore
/// let mut w = ReftableWriter::new(opts, min_idx, max_idx);
/// w.add_ref(&RefRecord { .. })?;
/// w.add_log(&LogRecord { .. })?;
/// let bytes = w.finish()?;
/// ```
pub struct ReftableWriter {
    opts: WriteOptions,
    min_update_index: u64,
    max_update_index: u64,

    // Accumulated ref records (must be added in sorted order).
    refs: Vec<RefRecord>,
    // Accumulated log records.
    logs: Vec<LogRecord>,
}

impl ReftableWriter {
    /// Create a new writer.
    pub fn new(opts: WriteOptions, min_update_index: u64, max_update_index: u64) -> Self {
        Self {
            opts,
            min_update_index,
            max_update_index,
            refs: Vec::new(),
            logs: Vec::new(),
        }
    }

    /// Add a ref record. Records **must** be added in sorted name order.
    pub fn add_ref(&mut self, rec: RefRecord) -> Result<()> {
        if let Some(last) = self.refs.last() {
            if rec.name <= last.name {
                return Err(Error::InvalidRef(format!(
                    "reftable: refs must be sorted, got '{}' after '{}'",
                    rec.name, last.name
                )));
            }
        }
        self.refs.push(rec);
        Ok(())
    }

    /// Add a log record.
    pub fn add_log(&mut self, rec: LogRecord) -> Result<()> {
        self.logs.push(rec);
        Ok(())
    }

    /// Finish writing and return the complete reftable file bytes.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let block_size = self.opts.block_size;

        // --- Header (24 bytes) ---
        out.extend_from_slice(REFTABLE_MAGIC);
        out.push(1); // version
        out.push(((block_size >> 16) & 0xff) as u8);
        out.push(((block_size >> 8) & 0xff) as u8);
        out.push((block_size & 0xff) as u8);
        out.extend_from_slice(&self.min_update_index.to_be_bytes());
        out.extend_from_slice(&self.max_update_index.to_be_bytes());

        assert_eq!(out.len(), HEADER_SIZE);

        // --- Ref blocks ---
        let ref_block_positions = self.write_ref_blocks(&mut out)?;

        // --- Ref index (if ≥ 4 ref blocks) ---
        let ref_index_position = if ref_block_positions.len() >= 4 {
            let pos = out.len() as u64;
            self.write_ref_index(&mut out, &ref_block_positions)?;
            pos
        } else {
            0
        };

        // --- Log blocks ---
        let log_position = if self.opts.write_log && !self.logs.is_empty() {
            let pos = out.len() as u64;
            self.write_log_blocks(&mut out)?;
            pos
        } else {
            0
        };

        // --- Footer ---
        let footer_start = out.len();
        // Repeat header
        out.extend_from_slice(REFTABLE_MAGIC);
        out.push(1);
        out.push(((block_size >> 16) & 0xff) as u8);
        out.push(((block_size >> 8) & 0xff) as u8);
        out.push((block_size & 0xff) as u8);
        out.extend_from_slice(&self.min_update_index.to_be_bytes());
        out.extend_from_slice(&self.max_update_index.to_be_bytes());

        // ref_index_position
        out.extend_from_slice(&ref_index_position.to_be_bytes());
        // (obj_position << 5) | obj_id_len — no obj blocks yet
        out.extend_from_slice(&0u64.to_be_bytes());
        // obj_index_position
        out.extend_from_slice(&0u64.to_be_bytes());
        // log_position
        out.extend_from_slice(&log_position.to_be_bytes());
        // log_index_position (we skip log index for simplicity)
        out.extend_from_slice(&0u64.to_be_bytes());

        // CRC-32 of footer (everything from footer_start to here)
        let crc = crc32(&out[footer_start..]);
        out.extend_from_slice(&crc.to_be_bytes());

        Ok(out)
    }

    /// Write ref blocks, returning (block_start_position, last_refname) per block.
    fn write_ref_blocks(&self, out: &mut Vec<u8>) -> Result<Vec<(u64, String)>> {
        if self.refs.is_empty() {
            return Ok(Vec::new());
        }

        let block_size = self.opts.block_size as usize;
        let restart_interval = self.opts.restart_interval;
        let mut block_positions: Vec<(u64, String)> = Vec::new();
        let mut i = 0;

        while i < self.refs.len() {
            let block_start = out.len();
            let is_first_block = block_start == HEADER_SIZE;

            // We accumulate records into a buffer, then write the block.
            let mut records_buf = Vec::new();
            let mut restart_offsets: Vec<u32> = Vec::new();
            let mut prev_name = String::new();
            let mut count = 0;
            let mut last_name = String::new();

            while i < self.refs.len() {
                let rec = &self.refs[i];
                let is_restart = count % restart_interval == 0;

                let mut rec_buf = Vec::new();
                let prefix_len = if is_restart {
                    0
                } else {
                    common_prefix_len(prev_name.as_bytes(), rec.name.as_bytes())
                };
                let suffix = &rec.name.as_bytes()[prefix_len..];
                let suffix_len = suffix.len();

                let value_type = match &rec.value {
                    RefValue::Deletion => VALUE_DELETION,
                    RefValue::Val1(_) => VALUE_ONE_OID,
                    RefValue::Val2(_, _) => VALUE_TWO_OID,
                    RefValue::Symref(_) => VALUE_SYMREF,
                };

                put_varint(prefix_len as u64, &mut rec_buf);
                put_varint(((suffix_len as u64) << 3) | value_type as u64, &mut rec_buf);
                rec_buf.extend_from_slice(suffix);

                let update_index_delta = rec.update_index.saturating_sub(self.min_update_index);
                put_varint(update_index_delta, &mut rec_buf);

                match &rec.value {
                    RefValue::Deletion => {}
                    RefValue::Val1(oid) => {
                        rec_buf.extend_from_slice(oid.as_bytes());
                    }
                    RefValue::Val2(oid, peeled) => {
                        rec_buf.extend_from_slice(oid.as_bytes());
                        rec_buf.extend_from_slice(peeled.as_bytes());
                    }
                    RefValue::Symref(target) => {
                        put_varint(target.len() as u64, &mut rec_buf);
                        rec_buf.extend_from_slice(target.as_bytes());
                    }
                }

                // Check if adding this record would overflow the block.
                // Block overhead: 4 (block header) + restart table
                let restart_count = restart_offsets.len() + if is_restart { 1 } else { 0 };
                let trailer_size = restart_count * 3 + 2;
                let total = 4 + records_buf.len() + rec_buf.len() + trailer_size;
                let effective_block_size = if is_first_block && block_size > 0 {
                    block_size // first block includes header
                } else if block_size > 0 {
                    block_size
                } else {
                    usize::MAX // unaligned
                };
                // For first block, block_len includes the 24-byte header
                let block_len = if is_first_block {
                    HEADER_SIZE + total
                } else {
                    total
                };

                if block_size > 0 && block_len > effective_block_size && count > 0 {
                    break; // Start a new block
                }

                if is_restart {
                    let offset = if is_first_block {
                        HEADER_SIZE + 4 + records_buf.len()
                    } else {
                        4 + records_buf.len()
                    };
                    restart_offsets.push(offset as u32);
                }

                records_buf.extend_from_slice(&rec_buf);
                last_name = rec.name.clone();
                prev_name = rec.name.clone();
                count += 1;
                i += 1;
            }

            if count == 0 {
                return Err(Error::InvalidRef(
                    "reftable: ref record too large for block size".into(),
                ));
            }

            // Ensure at least one restart point
            if restart_offsets.is_empty() {
                restart_offsets.push(if is_first_block {
                    HEADER_SIZE as u32 + 4
                } else {
                    4
                });
            }

            // Compute block_len
            let trailer_size = restart_offsets.len() * 3 + 2;
            let block_len_val = if is_first_block {
                HEADER_SIZE + 4 + records_buf.len() + trailer_size
            } else {
                4 + records_buf.len() + trailer_size
            };

            // Write block header: type(1) + block_len(3)
            out.push(BLOCK_TYPE_REF);
            out.push(((block_len_val >> 16) & 0xff) as u8);
            out.push(((block_len_val >> 8) & 0xff) as u8);
            out.push((block_len_val & 0xff) as u8);

            // Write records
            out.extend_from_slice(&records_buf);

            // Write restart offsets (3 bytes each)
            for &off in &restart_offsets {
                out.push(((off >> 16) & 0xff) as u8);
                out.push(((off >> 8) & 0xff) as u8);
                out.push((off & 0xff) as u8);
            }

            // Write restart count (2 bytes)
            let rc = restart_offsets.len() as u16;
            out.push((rc >> 8) as u8);
            out.push((rc & 0xff) as u8);

            // Pad to block alignment if needed
            if block_size > 0 {
                let written = out.len() - block_start;
                let target = if is_first_block {
                    block_size
                } else {
                    block_size
                };
                if written < target {
                    out.resize(block_start + target, 0);
                }
            }

            block_positions.push((block_start as u64, last_name.clone()));
        }

        Ok(block_positions)
    }

    /// Write a single-level ref index block.
    fn write_ref_index(&self, out: &mut Vec<u8>, block_positions: &[(u64, String)]) -> Result<()> {
        let mut records_buf = Vec::new();
        let mut restart_offsets: Vec<u32> = Vec::new();
        let mut prev_name = String::new();

        for (idx, (block_pos, last_ref)) in block_positions.iter().enumerate() {
            let is_restart = idx % self.opts.restart_interval == 0;
            let prefix_len = if is_restart {
                0
            } else {
                common_prefix_len(prev_name.as_bytes(), last_ref.as_bytes())
            };
            let suffix = &last_ref.as_bytes()[prefix_len..];

            if is_restart {
                restart_offsets.push(4 + records_buf.len() as u32);
            }

            put_varint(prefix_len as u64, &mut records_buf);
            put_varint((suffix.len() as u64) << 3, &mut records_buf);
            records_buf.extend_from_slice(suffix);
            put_varint(*block_pos, &mut records_buf);

            prev_name = last_ref.clone();
        }

        if restart_offsets.is_empty() {
            restart_offsets.push(4);
        }

        let trailer_size = restart_offsets.len() * 3 + 2;
        let block_len = 4 + records_buf.len() + trailer_size;

        out.push(BLOCK_TYPE_INDEX);
        out.push(((block_len >> 16) & 0xff) as u8);
        out.push(((block_len >> 8) & 0xff) as u8);
        out.push((block_len & 0xff) as u8);

        out.extend_from_slice(&records_buf);

        for &off in &restart_offsets {
            out.push(((off >> 16) & 0xff) as u8);
            out.push(((off >> 8) & 0xff) as u8);
            out.push((off & 0xff) as u8);
        }
        let rc = restart_offsets.len() as u16;
        out.push((rc >> 8) as u8);
        out.push((rc & 0xff) as u8);

        Ok(())
    }

    /// Write log blocks (zlib-compressed).
    fn write_log_blocks(&mut self, out: &mut Vec<u8>) -> Result<()> {
        use flate2::write::DeflateEncoder;
        use flate2::Compression;

        // Sort logs by (refname, reverse update_index)
        self.logs.sort_by(|a, b| {
            a.refname
                .cmp(&b.refname)
                .then_with(|| b.update_index.cmp(&a.update_index))
        });

        // Build the uncompressed log block content
        let mut inner = Vec::new();
        let mut restart_offsets: Vec<u32> = Vec::new();
        let mut prev_key = Vec::<u8>::new();

        for (idx, log) in self.logs.iter().enumerate() {
            let is_restart = idx % self.opts.restart_interval == 0;

            // Log key: refname \0 reverse_int64(update_index)
            let mut key = Vec::new();
            key.extend_from_slice(log.refname.as_bytes());
            key.push(0);
            key.extend_from_slice(&(0xffffffffffffffffu64 - log.update_index).to_be_bytes());

            let prefix_len = if is_restart {
                0
            } else {
                common_prefix_len(&prev_key, &key)
            };
            let suffix = &key[prefix_len..];

            if is_restart {
                // Offset within the decompressed block (4 byte header + inner.len())
                restart_offsets.push(4 + inner.len() as u32);
            }

            // log_type = 1 (standard reflog data)
            let log_type: u8 = 1;
            put_varint(prefix_len as u64, &mut inner);
            put_varint(((suffix.len() as u64) << 3) | log_type as u64, &mut inner);
            inner.extend_from_slice(suffix);

            // log_data
            inner.extend_from_slice(log.old_id.as_bytes());
            inner.extend_from_slice(log.new_id.as_bytes());
            put_varint(log.name.len() as u64, &mut inner);
            inner.extend_from_slice(log.name.as_bytes());
            put_varint(log.email.len() as u64, &mut inner);
            inner.extend_from_slice(log.email.as_bytes());
            put_varint(log.time_seconds, &mut inner);
            inner.extend_from_slice(&log.tz_offset.to_be_bytes());
            put_varint(log.message.len() as u64, &mut inner);
            inner.extend_from_slice(log.message.as_bytes());

            prev_key = key;
        }

        if restart_offsets.is_empty() {
            restart_offsets.push(4);
        }

        // Append restart table
        for &off in &restart_offsets {
            inner.push(((off >> 16) & 0xff) as u8);
            inner.push(((off >> 8) & 0xff) as u8);
            inner.push((off & 0xff) as u8);
        }
        let rc = restart_offsets.len() as u16;
        inner.push((rc >> 8) as u8);
        inner.push((rc & 0xff) as u8);

        // block_len is the *inflated* size including the 4-byte block header
        let block_len = 4 + inner.len();

        // Deflate the inner content
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&inner)
            .map_err(|e| Error::Zlib(e.to_string()))?;
        let compressed = encoder.finish().map_err(|e| Error::Zlib(e.to_string()))?;

        // Write block header + compressed data
        out.push(BLOCK_TYPE_LOG);
        out.push(((block_len >> 16) & 0xff) as u8);
        out.push(((block_len >> 8) & 0xff) as u8);
        out.push((block_len & 0xff) as u8);
        out.extend_from_slice(&compressed);

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Reads a single reftable file from a byte buffer.
pub struct ReftableReader {
    data: Vec<u8>,
    version: u8,
    block_size: u32,
    min_update_index: u64,
    max_update_index: u64,
    ref_index_position: u64,
    log_position: u64,
}

/// Parsed footer fields.
#[derive(Debug)]
#[allow(dead_code)]
struct Footer {
    version: u8,
    block_size: u32,
    min_update_index: u64,
    max_update_index: u64,
    ref_index_position: u64,
    obj_position_and_id_len: u64,
    obj_index_position: u64,
    log_position: u64,
    log_index_position: u64,
}

impl ReftableReader {
    /// Open a reftable from bytes.
    pub fn new(data: Vec<u8>) -> Result<Self> {
        if data.len() < HEADER_SIZE + FOOTER_V1_SIZE {
            // Could be an empty table (header + footer only = 24 + 68 = 92)
            if data.len() < HEADER_SIZE {
                return Err(Error::InvalidRef("reftable: file too small".into()));
            }
        }

        // Parse header
        if &data[0..4] != REFTABLE_MAGIC {
            return Err(Error::InvalidRef("reftable: bad magic".into()));
        }
        let version = data[4];
        if version != 1 && version != 2 {
            return Err(Error::InvalidRef(format!(
                "reftable: unsupported version {version}"
            )));
        }
        let _block_size = ((data[5] as u32) << 16) | ((data[6] as u32) << 8) | (data[7] as u32);
        let _min_update_index = u64::from_be_bytes(data[8..16].try_into().unwrap());
        let _max_update_index = u64::from_be_bytes(data[16..24].try_into().unwrap());

        // Parse footer
        let footer_size = if version == 2 { 72 } else { FOOTER_V1_SIZE };
        if data.len() < footer_size {
            return Err(Error::InvalidRef(
                "reftable: file too small for footer".into(),
            ));
        }
        let footer_start = data.len() - footer_size;
        let footer = parse_footer(&data[footer_start..], version)?;

        Ok(Self {
            data,
            version,
            block_size: footer.block_size,
            min_update_index: footer.min_update_index,
            max_update_index: footer.max_update_index,
            ref_index_position: footer.ref_index_position,
            log_position: footer.log_position,
        })
    }

    /// Read all ref records from the table.
    pub fn read_refs(&self) -> Result<Vec<RefRecord>> {
        let mut refs = Vec::new();
        let footer_size = if self.version == 2 {
            72
        } else {
            FOOTER_V1_SIZE
        };
        let file_end = self.data.len() - footer_size;

        // Determine where ref blocks end
        let ref_end = if self.ref_index_position > 0 {
            self.ref_index_position as usize
        } else if self.log_position > 0 {
            self.log_position as usize
        } else {
            file_end
        };

        let mut pos = 0usize;
        // Skip the header — first ref block starts at offset 24 but shares
        // the same physical block as the header.
        if pos < HEADER_SIZE {
            pos = HEADER_SIZE;
        }

        while pos < ref_end {
            if pos >= self.data.len() {
                break;
            }
            let block_type = self.data[pos];
            if block_type == 0 {
                // Padding — skip to next block boundary
                if self.block_size > 0 {
                    let bs = self.block_size as usize;
                    pos = ((pos / bs) + 1) * bs;
                    continue;
                } else {
                    break;
                }
            }
            if block_type != BLOCK_TYPE_REF {
                break;
            }

            let block_len = read_u24(&self.data, pos + 1);
            // Determine the data range for this block
            let block_data_start = pos + 4; // after type(1) + len(3)

            // The first block's block_len includes the 24-byte header
            let is_first = pos == HEADER_SIZE;
            let records_end = if is_first {
                // block_len is from file start
                block_len
            } else {
                pos + block_len
            };

            if records_end > ref_end {
                break;
            }

            // Read restart count (last 2 bytes before padding)
            let rc = read_u16(&self.data, records_end - 2);
            // Restart table is rc * 3 bytes before the restart_count
            let restart_table_start = records_end - 2 - (rc * 3);

            // Read records from block_data_start to restart_table_start
            let mut rpos = block_data_start;
            let mut prev_name = Vec::<u8>::new();

            while rpos < restart_table_start {
                let (rec, new_pos) =
                    decode_ref_record(&self.data, rpos, &prev_name, self.min_update_index)?;
                prev_name = rec.name.as_bytes().to_vec();
                refs.push(rec);
                rpos = new_pos;
            }

            // Advance to next block
            if self.block_size > 0 {
                let bs = self.block_size as usize;
                if is_first {
                    pos = bs;
                } else {
                    pos += bs;
                }
            } else {
                pos = records_end;
            }
        }

        Ok(refs)
    }

    /// Look up a single ref by name.
    pub fn lookup_ref(&self, name: &str) -> Result<Option<RefRecord>> {
        // Simple: scan all refs. For large files the index would speed this up.
        let refs = self.read_refs()?;
        Ok(refs.into_iter().find(|r| r.name == name))
    }

    /// Read all log records from the table.
    pub fn read_logs(&self) -> Result<Vec<LogRecord>> {
        if self.log_position == 0 {
            return Ok(Vec::new());
        }

        let footer_size = if self.version == 2 {
            72
        } else {
            FOOTER_V1_SIZE
        };
        let file_end = self.data.len() - footer_size;
        let mut pos = self.log_position as usize;
        let mut logs = Vec::new();

        while pos < file_end {
            if pos >= self.data.len() {
                break;
            }
            let block_type = self.data[pos];
            if block_type != BLOCK_TYPE_LOG {
                break;
            }
            let block_len = read_u24(&self.data, pos + 1);
            let compressed_start = pos + 4;

            // The inflated size is block_len - 4 (block_len includes the 4-byte header)
            let inflated_size = block_len - 4;

            // Decompress
            use flate2::read::DeflateDecoder;
            let remaining = &self.data[compressed_start..file_end];
            let mut decoder = DeflateDecoder::new(remaining);
            let mut inflated = vec![0u8; inflated_size];
            decoder
                .read_exact(&mut inflated)
                .map_err(|e| Error::Zlib(e.to_string()))?;

            // How many compressed bytes were consumed?
            let consumed = decoder.total_in() as usize;

            // Parse log records from inflated data
            // Read restart_count from end
            if inflated.len() < 2 {
                break;
            }
            let rc = read_u16(&inflated, inflated.len() - 2);
            let restart_table_start = inflated.len() - 2 - (rc * 3);

            let mut rpos = 0usize;
            let mut prev_key = Vec::<u8>::new();

            while rpos < restart_table_start {
                let (log, new_pos) = decode_log_record(&inflated, rpos, &prev_key)?;
                // Reconstruct key for prefix compression
                let mut key = Vec::new();
                key.extend_from_slice(log.refname.as_bytes());
                key.push(0);
                key.extend_from_slice(&(0xffffffffffffffffu64 - log.update_index).to_be_bytes());
                prev_key = key;
                logs.push(log);
                rpos = new_pos;
            }

            pos = compressed_start + consumed;
        }

        Ok(logs)
    }

    /// Get the block size from the header.
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Get the min update index.
    pub fn min_update_index(&self) -> u64 {
        self.min_update_index
    }

    /// Get the max update index.
    pub fn max_update_index(&self) -> u64 {
        self.max_update_index
    }
}

// ---------------------------------------------------------------------------
// Record decoding helpers
// ---------------------------------------------------------------------------

fn decode_ref_record(
    data: &[u8],
    pos: usize,
    prev_name: &[u8],
    min_update_index: u64,
) -> Result<(RefRecord, usize)> {
    let (prefix_len, p) = get_varint(data, pos)?;
    let (suffix_and_type, mut p) = get_varint(data, p)?;
    let suffix_len = (suffix_and_type >> 3) as usize;
    let value_type = (suffix_and_type & 0x7) as u8;

    // Reconstruct name
    let mut name = Vec::with_capacity(prefix_len as usize + suffix_len);
    if prefix_len > 0 {
        if (prefix_len as usize) > prev_name.len() {
            return Err(Error::InvalidRef(
                "reftable: prefix_len exceeds prev name".into(),
            ));
        }
        name.extend_from_slice(&prev_name[..prefix_len as usize]);
    }
    if p + suffix_len > data.len() {
        return Err(Error::InvalidRef("reftable: suffix overflows block".into()));
    }
    name.extend_from_slice(&data[p..p + suffix_len]);
    p += suffix_len;

    let name_str = String::from_utf8(name)
        .map_err(|_| Error::InvalidRef("reftable: invalid UTF-8 in ref name".into()))?;

    let (update_index_delta, mut p) = get_varint(data, p)?;
    let update_index = min_update_index + update_index_delta;

    let value = match value_type {
        VALUE_DELETION => RefValue::Deletion,
        VALUE_ONE_OID => {
            if p + HASH_SIZE > data.len() {
                return Err(Error::InvalidRef("reftable: truncated OID".into()));
            }
            let oid = ObjectId::from_bytes(&data[p..p + HASH_SIZE])?;
            p += HASH_SIZE;
            RefValue::Val1(oid)
        }
        VALUE_TWO_OID => {
            if p + 2 * HASH_SIZE > data.len() {
                return Err(Error::InvalidRef("reftable: truncated OID pair".into()));
            }
            let oid = ObjectId::from_bytes(&data[p..p + HASH_SIZE])?;
            p += HASH_SIZE;
            let peeled = ObjectId::from_bytes(&data[p..p + HASH_SIZE])?;
            p += HASH_SIZE;
            RefValue::Val2(oid, peeled)
        }
        VALUE_SYMREF => {
            let (target_len, p2) = get_varint(data, p)?;
            p = p2;
            let target_len = target_len as usize;
            if p + target_len > data.len() {
                return Err(Error::InvalidRef(
                    "reftable: truncated symref target".into(),
                ));
            }
            let target = String::from_utf8(data[p..p + target_len].to_vec())
                .map_err(|_| Error::InvalidRef("reftable: invalid UTF-8 in symref".into()))?;
            p += target_len;
            RefValue::Symref(target)
        }
        _ => {
            return Err(Error::InvalidRef(format!(
                "reftable: unknown value_type {value_type}"
            )));
        }
    };

    Ok((
        RefRecord {
            name: name_str,
            update_index,
            value,
        },
        p,
    ))
}

fn decode_log_record(data: &[u8], pos: usize, prev_key: &[u8]) -> Result<(LogRecord, usize)> {
    let (prefix_len, p) = get_varint(data, pos)?;
    let (suffix_and_type, mut p) = get_varint(data, p)?;
    let suffix_len = (suffix_and_type >> 3) as usize;
    let log_type = (suffix_and_type & 0x7) as u8;

    // Reconstruct key
    let mut key = Vec::with_capacity(prefix_len as usize + suffix_len);
    if prefix_len > 0 {
        if (prefix_len as usize) > prev_key.len() {
            return Err(Error::InvalidRef(
                "reftable: log prefix_len exceeds prev key".into(),
            ));
        }
        key.extend_from_slice(&prev_key[..prefix_len as usize]);
    }
    if p + suffix_len > data.len() {
        return Err(Error::InvalidRef("reftable: log suffix overflows".into()));
    }
    key.extend_from_slice(&data[p..p + suffix_len]);
    p += suffix_len;

    // Parse key: refname \0 reverse_int64(update_index)
    let null_pos = key
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::InvalidRef("reftable: log key missing null separator".into()))?;
    let refname = String::from_utf8(key[..null_pos].to_vec())
        .map_err(|_| Error::InvalidRef("reftable: invalid UTF-8 in log refname".into()))?;
    if null_pos + 9 > key.len() {
        return Err(Error::InvalidRef("reftable: log key too short".into()));
    }
    let reversed_idx = u64::from_be_bytes(key[null_pos + 1..null_pos + 9].try_into().unwrap());
    let update_index = 0xffffffffffffffffu64 - reversed_idx;

    if log_type == 0 {
        // Deletion
        let zero_oid = ObjectId::from_bytes(&[0u8; 20])?;
        return Ok((
            LogRecord {
                refname,
                update_index,
                old_id: zero_oid,
                new_id: zero_oid,
                name: String::new(),
                email: String::new(),
                time_seconds: 0,
                tz_offset: 0,
                message: String::new(),
            },
            p,
        ));
    }

    // log_type == 1: standard log data
    if p + 2 * HASH_SIZE > data.len() {
        return Err(Error::InvalidRef("reftable: truncated log OIDs".into()));
    }
    let old_id = ObjectId::from_bytes(&data[p..p + HASH_SIZE])?;
    p += HASH_SIZE;
    let new_id = ObjectId::from_bytes(&data[p..p + HASH_SIZE])?;
    p += HASH_SIZE;

    let (name_len, p2) = get_varint(data, p)?;
    p = p2;
    let name_len = name_len as usize;
    if p + name_len > data.len() {
        return Err(Error::InvalidRef("reftable: truncated log name".into()));
    }
    let name = String::from_utf8(data[p..p + name_len].to_vec())
        .map_err(|_| Error::InvalidRef("reftable: invalid UTF-8 in log name".into()))?;
    p += name_len;

    let (email_len, p2) = get_varint(data, p)?;
    p = p2;
    let email_len = email_len as usize;
    if p + email_len > data.len() {
        return Err(Error::InvalidRef("reftable: truncated log email".into()));
    }
    let email = String::from_utf8(data[p..p + email_len].to_vec())
        .map_err(|_| Error::InvalidRef("reftable: invalid UTF-8 in log email".into()))?;
    p += email_len;

    let (time_seconds, p2) = get_varint(data, p)?;
    p = p2;

    if p + 2 > data.len() {
        return Err(Error::InvalidRef("reftable: truncated tz_offset".into()));
    }
    let tz_offset = i16::from_be_bytes([data[p], data[p + 1]]);
    p += 2;

    let (msg_len, p2) = get_varint(data, p)?;
    p = p2;
    let msg_len = msg_len as usize;
    if p + msg_len > data.len() {
        return Err(Error::InvalidRef("reftable: truncated log message".into()));
    }
    let message = String::from_utf8(data[p..p + msg_len].to_vec())
        .map_err(|_| Error::InvalidRef("reftable: invalid UTF-8 in log message".into()))?;
    p += msg_len;

    Ok((
        LogRecord {
            refname,
            update_index,
            old_id,
            new_id,
            name,
            email,
            time_seconds,
            tz_offset,
            message,
        },
        p,
    ))
}

// ---------------------------------------------------------------------------
// Stack management
// ---------------------------------------------------------------------------

/// Manages the `$GIT_DIR/reftable/` directory and `tables.list` stack.
///
/// The stack provides a merged view of all tables, with later tables
/// taking precedence over earlier ones.
pub struct ReftableStack {
    /// Path to the `reftable/` directory.
    reftable_dir: PathBuf,
    /// Ordered list of table file names (oldest first).
    table_names: Vec<String>,
}

impl ReftableStack {
    /// Open an existing reftable stack.
    pub fn open(git_dir: &Path) -> Result<Self> {
        let reftable_dir = git_dir.join("reftable");
        let tables_list = reftable_dir.join("tables.list");
        let content = fs::read_to_string(&tables_list).map_err(Error::Io)?;
        let table_names: Vec<String> = content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect();
        Ok(Self {
            reftable_dir,
            table_names,
        })
    }

    /// Read a merged view of all ref records.
    ///
    /// Later tables override earlier ones. Deletion records cause the
    /// ref to be omitted from the result.
    pub fn read_refs(&self) -> Result<Vec<RefRecord>> {
        let mut merged: BTreeMap<String, RefRecord> = BTreeMap::new();

        for name in &self.table_names {
            let path = self.reftable_dir.join(name);
            let data = fs::read(&path).map_err(Error::Io)?;
            let reader = ReftableReader::new(data)?;
            for rec in reader.read_refs()? {
                match &rec.value {
                    RefValue::Deletion => {
                        merged.remove(&rec.name);
                    }
                    _ => {
                        merged.insert(rec.name.clone(), rec);
                    }
                }
            }
        }

        Ok(merged.into_values().collect())
    }

    /// Look up a single ref across all tables (most recent wins).
    pub fn lookup_ref(&self, name: &str) -> Result<Option<RefRecord>> {
        // Search tables in reverse (newest first)
        for table_name in self.table_names.iter().rev() {
            let path = self.reftable_dir.join(table_name);
            let data = fs::read(&path).map_err(Error::Io)?;
            let reader = ReftableReader::new(data)?;
            if let Some(rec) = reader.lookup_ref(name)? {
                return match rec.value {
                    RefValue::Deletion => Ok(None),
                    _ => Ok(Some(rec)),
                };
            }
        }
        Ok(None)
    }

    /// Read merged log records for a specific ref.
    pub fn read_logs_for_ref(&self, refname: &str) -> Result<Vec<LogRecord>> {
        let mut logs = Vec::new();
        for table_name in &self.table_names {
            let path = self.reftable_dir.join(table_name);
            let data = fs::read(&path).map_err(Error::Io)?;
            let reader = ReftableReader::new(data)?;
            for log in reader.read_logs()? {
                if log.refname == refname {
                    logs.push(log);
                }
            }
        }
        // Sort by update_index descending (most recent first)
        logs.sort_by(|a, b| b.update_index.cmp(&a.update_index));
        Ok(logs)
    }

    /// Read all log records across all tables.
    pub fn read_all_logs(&self) -> Result<Vec<LogRecord>> {
        let mut logs = Vec::new();
        for table_name in &self.table_names {
            let path = self.reftable_dir.join(table_name);
            let data = fs::read(&path).map_err(Error::Io)?;
            let reader = ReftableReader::new(data)?;
            logs.extend(reader.read_logs()?);
        }
        logs.sort_by(|a, b| {
            a.refname
                .cmp(&b.refname)
                .then_with(|| b.update_index.cmp(&a.update_index))
        });
        Ok(logs)
    }

    /// Get the current max update index across all tables.
    pub fn max_update_index(&self) -> Result<u64> {
        let mut max_idx = 0u64;
        for name in &self.table_names {
            let path = self.reftable_dir.join(name);
            let data = fs::read(&path).map_err(Error::Io)?;
            let reader = ReftableReader::new(data)?;
            max_idx = max_idx.max(reader.max_update_index());
        }
        Ok(max_idx)
    }

    /// Add a new reftable to the stack.
    ///
    /// Writes the table bytes to a new file, then atomically updates
    /// `tables.list`.
    pub fn add_table(&mut self, data: &[u8], update_index: u64) -> Result<String> {
        let random: u64 = {
            // Simple random from /dev/urandom or time-based fallback
            let mut buf = [0u8; 8];
            if let Ok(mut f) = fs::File::open("/dev/urandom") {
                let _ = f.read(&mut buf);
            }
            u64::from_le_bytes(buf)
        };
        let filename = format!(
            "{:08x}-{:08x}-{:08x}.ref",
            update_index, update_index, random as u32
        );
        let path = self.reftable_dir.join(&filename);
        fs::write(&path, data).map_err(Error::Io)?;

        self.table_names.push(filename.clone());
        self.write_tables_list()?;

        // Auto-compact if we have too many tables
        if self.table_names.len() > 5 {
            self.compact()?;
        }

        Ok(filename)
    }

    /// Write a ref update (add/update/delete) as a new reftable.
    ///
    /// This is the main entry point for updating refs in a reftable repo.
    pub fn write_ref(
        &mut self,
        refname: &str,
        value: RefValue,
        log: Option<LogRecord>,
        opts: &WriteOptions,
    ) -> Result<()> {
        let update_index = self.max_update_index()? + 1;
        let mut writer = ReftableWriter::new(opts.clone(), update_index, update_index);

        // For a single-ref update we need to write all existing refs + the update
        // into a proper sorted order, OR we can write a single-record table.
        // The stack handles merging, so a single-record table is fine.
        writer.add_ref(RefRecord {
            name: refname.to_owned(),
            update_index,
            value,
        })?;

        if let Some(log_rec) = log {
            let mut log_rec = log_rec;
            log_rec.update_index = update_index;
            writer.add_log(log_rec)?;
        }

        let data = writer.finish()?;
        self.add_table(&data, update_index)?;
        Ok(())
    }

    /// Compact all tables into a single table.
    pub fn compact(&mut self) -> Result<()> {
        if self.table_names.len() <= 1 {
            return Ok(());
        }

        // Read all refs and logs
        let refs = self.read_refs()?;
        let logs = self.read_all_logs()?;

        // Determine update index range
        let mut min_idx = u64::MAX;
        let mut max_idx = 0u64;
        for name in &self.table_names {
            let path = self.reftable_dir.join(name);
            let data = fs::read(&path).map_err(Error::Io)?;
            let reader = ReftableReader::new(data)?;
            min_idx = min_idx.min(reader.min_update_index());
            max_idx = max_idx.max(reader.max_update_index());
        }
        if min_idx == u64::MAX {
            min_idx = 0;
        }

        let mut writer = ReftableWriter::new(WriteOptions::default(), min_idx, max_idx);
        for rec in refs {
            writer.add_ref(rec)?;
        }
        for log in logs {
            writer.add_log(log)?;
        }

        let data = writer.finish()?;

        // Write new compacted table
        let old_names = self.table_names.clone();
        self.table_names.clear();
        let _name = self.add_table(&data, max_idx)?;

        // Remove old table files
        for old in &old_names {
            let path = self.reftable_dir.join(old);
            let _ = fs::remove_file(&path);
        }

        Ok(())
    }

    /// Write `tables.list` atomically.
    fn write_tables_list(&self) -> Result<()> {
        let tables_list = self.reftable_dir.join("tables.list");
        let lock = self.reftable_dir.join("tables.list.lock");
        let content = self.table_names.join("\n")
            + if self.table_names.is_empty() {
                ""
            } else {
                "\n"
            };
        fs::write(&lock, &content).map_err(Error::Io)?;
        fs::rename(&lock, &tables_list).map_err(Error::Io)?;
        Ok(())
    }

    /// Return the list of table filenames in this stack.
    pub fn table_names(&self) -> &[String] {
        &self.table_names
    }
}

// ---------------------------------------------------------------------------
// Integration helpers — used by refs.rs and commands
// ---------------------------------------------------------------------------

/// Detect whether a git directory uses the reftable backend.
pub fn is_reftable_repo(git_dir: &Path) -> bool {
    fn config_uses_reftable(config_path: &Path) -> bool {
        let Ok(content) = fs::read_to_string(config_path) else {
            return false;
        };

        let mut in_extensions = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_extensions = trimmed.eq_ignore_ascii_case("[extensions]");
                continue;
            }
            if in_extensions {
                if let Some((key, value)) = trimmed.split_once('=') {
                    if key.trim().eq_ignore_ascii_case("refstorage")
                        && value.trim().eq_ignore_ascii_case("reftable")
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    let local_config = git_dir.join("config");
    if config_uses_reftable(&local_config) {
        return true;
    }

    // Linked worktrees typically store the shared repository configuration
    // in the common directory pointed to by `commondir`.
    if let Ok(raw) = fs::read_to_string(git_dir.join("commondir")) {
        let rel = raw.trim();
        if !rel.is_empty() {
            let common = if Path::new(rel).is_absolute() {
                PathBuf::from(rel)
            } else {
                git_dir.join(rel)
            };
            let common_config = common.canonicalize().unwrap_or(common).join("config");
            if config_uses_reftable(&common_config) {
                return true;
            }
        }
    }

    false
}

/// Resolve a ref in a reftable repo, following symbolic refs.
pub fn reftable_resolve_ref(git_dir: &Path, refname: &str) -> Result<ObjectId> {
    reftable_resolve_ref_depth(git_dir, refname, 0)
}

fn reftable_resolve_ref_depth(git_dir: &Path, refname: &str, depth: usize) -> Result<ObjectId> {
    if depth > 10 {
        return Err(Error::InvalidRef(format!(
            "reftable: symlink too deep: {refname}"
        )));
    }

    // HEAD is special — stored as a file even in reftable repos
    if refname == "HEAD" {
        let head_path = git_dir.join("HEAD");
        if head_path.exists() {
            let content = fs::read_to_string(&head_path).map_err(Error::Io)?;
            let content = content.trim();
            if let Some(target) = content.strip_prefix("ref: ") {
                return reftable_resolve_ref_depth(git_dir, target.trim(), depth + 1);
            }
            // Detached HEAD
            if content.len() == 40 && content.chars().all(|c| c.is_ascii_hexdigit()) {
                return content.parse();
            }
        }
    }

    let stack = ReftableStack::open(git_dir)?;
    match stack.lookup_ref(refname)? {
        Some(rec) => match rec.value {
            RefValue::Val1(oid) => Ok(oid),
            RefValue::Val2(oid, _) => Ok(oid),
            RefValue::Symref(target) => reftable_resolve_ref_depth(git_dir, &target, depth + 1),
            RefValue::Deletion => Err(Error::InvalidRef(format!("ref not found: {refname}"))),
        },
        None => Err(Error::InvalidRef(format!("ref not found: {refname}"))),
    }
}

/// Write a ref to a reftable repo.
pub fn reftable_write_ref(
    git_dir: &Path,
    refname: &str,
    oid: &ObjectId,
    log_identity: Option<&str>,
    log_message: Option<&str>,
) -> Result<()> {
    let mut stack = ReftableStack::open(git_dir)?;
    let old_oid = stack
        .lookup_ref(refname)?
        .and_then(|r| match r.value {
            RefValue::Val1(oid) => Some(oid),
            RefValue::Val2(oid, _) => Some(oid),
            _ => None,
        })
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());

    let log = if let Some(identity) = log_identity {
        let (name, email, time_secs, tz) = parse_identity_string(identity);
        Some(LogRecord {
            refname: refname.to_owned(),
            update_index: 0, // will be set by write_ref
            old_id: old_oid,
            new_id: *oid,
            name,
            email,
            time_seconds: time_secs,
            tz_offset: tz,
            message: log_message.unwrap_or("").to_owned(),
        })
    } else {
        None
    };

    // Check config for logAllRefUpdates
    let write_log = log.is_some() || should_log_ref_updates(git_dir);
    let log = if write_log { log } else { None };

    let opts = read_write_options(git_dir);
    stack.write_ref(refname, RefValue::Val1(*oid), log, &opts)
}

/// Write a symbolic ref to a reftable repo.
pub fn reftable_write_symref(
    git_dir: &Path,
    refname: &str,
    target: &str,
    log_identity: Option<&str>,
    log_message: Option<&str>,
) -> Result<()> {
    let mut stack = ReftableStack::open(git_dir)?;
    let opts = read_write_options(git_dir);

    let log = if let Some(identity) = log_identity {
        let (name, email, time_secs, tz) = parse_identity_string(identity);
        let zero_oid = ObjectId::from_bytes(&[0u8; 20])?;
        Some(LogRecord {
            refname: refname.to_owned(),
            update_index: 0,
            old_id: zero_oid,
            new_id: zero_oid,
            name,
            email,
            time_seconds: time_secs,
            tz_offset: tz,
            message: log_message.unwrap_or("").to_owned(),
        })
    } else {
        None
    };

    stack.write_ref(refname, RefValue::Symref(target.to_owned()), log, &opts)
}

/// Delete a ref from a reftable repo.
pub fn reftable_delete_ref(git_dir: &Path, refname: &str) -> Result<()> {
    let mut stack = ReftableStack::open(git_dir)?;
    let opts = read_write_options(git_dir);
    stack.write_ref(refname, RefValue::Deletion, None, &opts)
}

/// Read the symbolic target of a ref in a reftable repo.
pub fn reftable_read_symbolic_ref(git_dir: &Path, refname: &str) -> Result<Option<String>> {
    let stack = ReftableStack::open(git_dir)?;
    match stack.lookup_ref(refname)? {
        Some(rec) => match rec.value {
            RefValue::Symref(target) => Ok(Some(target)),
            _ => Ok(None),
        },
        None => Ok(None),
    }
}

/// List all refs in a reftable repo under a given prefix.
pub fn reftable_list_refs(git_dir: &Path, prefix: &str) -> Result<Vec<(String, ObjectId)>> {
    let stack = ReftableStack::open(git_dir)?;
    let refs = stack.read_refs()?;
    let mut result = Vec::new();
    for rec in refs {
        if rec.name.starts_with(prefix) {
            match rec.value {
                RefValue::Val1(oid) => result.push((rec.name, oid)),
                RefValue::Val2(oid, _) => result.push((rec.name, oid)),
                RefValue::Symref(target) => {
                    // Try to resolve the symref
                    if let Ok(oid) = reftable_resolve_ref(git_dir, &target) {
                        result.push((rec.name, oid));
                    }
                }
                RefValue::Deletion => {}
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Read reflog entries for a ref from the reftable stack.
pub fn reftable_read_reflog(
    git_dir: &Path,
    refname: &str,
) -> Result<Vec<crate::reflog::ReflogEntry>> {
    let stack = ReftableStack::open(git_dir)?;
    let logs = stack.read_logs_for_ref(refname)?;
    let mut entries = Vec::new();
    for log in logs {
        // Reconstruct the identity string
        let tz_sign = if log.tz_offset >= 0 { '+' } else { '-' };
        let tz_abs = log.tz_offset.unsigned_abs();
        let tz_hours = tz_abs / 60;
        let tz_mins = tz_abs % 60;
        let identity = format!(
            "{} <{}> {} {}{:02}{:02}",
            log.name, log.email, log.time_seconds, tz_sign, tz_hours, tz_mins
        );
        entries.push(crate::reflog::ReflogEntry {
            old_oid: log.old_id,
            new_oid: log.new_id,
            identity,
            message: log.message,
        });
    }
    Ok(entries)
}

/// Append a reflog entry for a reftable repo.
pub fn reftable_append_reflog(
    git_dir: &Path,
    refname: &str,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    identity: &str,
    message: &str,
    force_create: bool,
) -> Result<()> {
    use crate::refs::should_autocreate_reflog;
    if !force_create
        && !should_autocreate_reflog(git_dir, refname)
        && message.is_empty()
        && !reftable_reflog_exists(git_dir, refname)
    {
        return Ok(());
    }
    let (name, email, time_secs, tz) = parse_identity_string(identity);
    let mut stack = ReftableStack::open(git_dir)?;
    let update_index = stack.max_update_index()? + 1;
    let opts = read_write_options(git_dir);

    let mut writer = ReftableWriter::new(opts, update_index, update_index);
    writer.add_log(LogRecord {
        refname: refname.to_owned(),
        update_index,
        old_id: *old_oid,
        new_id: *new_oid,
        name,
        email,
        time_seconds: time_secs,
        tz_offset: tz,
        message: message.to_owned(),
    })?;

    let data = writer.finish()?;
    stack.add_table(&data, update_index)?;
    Ok(())
}

/// Check whether a reftable repo has reflogs for the given ref.
pub fn reftable_reflog_exists(git_dir: &Path, refname: &str) -> bool {
    if let Ok(stack) = ReftableStack::open(git_dir) {
        if let Ok(logs) = stack.read_logs_for_ref(refname) {
            return !logs.is_empty();
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Write options helpers
// ---------------------------------------------------------------------------

/// Read reftable write options from the repository config.
pub fn read_write_options(git_dir: &Path) -> WriteOptions {
    let config_path = git_dir.join("config");
    let mut opts = WriteOptions::default();

    if let Ok(content) = fs::read_to_string(&config_path) {
        let mut in_reftable = false;
        let mut in_core = false;
        let mut log_all_ref_updates: Option<bool> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                let section_lower = trimmed.to_lowercase();
                in_reftable = section_lower.starts_with("[reftable]");
                in_core = section_lower.starts_with("[core]");
                continue;
            }
            if in_reftable {
                if let Some((key, value)) = trimmed.split_once('=') {
                    let key = key.trim().to_lowercase();
                    let value = value.trim();
                    match key.as_str() {
                        "blocksize" => {
                            if let Ok(v) = value.parse::<u32>() {
                                opts.block_size = v;
                            }
                        }
                        "restartinterval" => {
                            if let Ok(v) = value.parse::<usize>() {
                                opts.restart_interval = v;
                            }
                        }
                        _ => {}
                    }
                }
            }
            if in_core {
                if let Some((key, value)) = trimmed.split_once('=') {
                    let key = key.trim().to_lowercase();
                    let value = value.trim().to_lowercase();
                    if key == "logallrefupdates" {
                        log_all_ref_updates = Some(value == "true" || value == "always");
                    }
                }
            }
        }

        if let Some(false) = log_all_ref_updates {
            opts.write_log = false;
        }
    }

    opts
}

/// Check if logAllRefUpdates is enabled.
fn should_log_ref_updates(git_dir: &Path) -> bool {
    let config_path = git_dir.join("config");
    if let Ok(content) = fs::read_to_string(&config_path) {
        let mut in_core = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_core = trimmed.to_lowercase().starts_with("[core]");
                continue;
            }
            if in_core {
                if let Some((key, value)) = trimmed.split_once('=') {
                    if key.trim().eq_ignore_ascii_case("logallrefupdates") {
                        let v = value.trim().to_lowercase();
                        return v == "true" || v == "always";
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Compute the CRC-32 of a byte slice (ISO 3309 / ITU-T V.42).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffffffff;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xedb88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Compute common prefix length between two byte slices.
fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Read a big-endian u24 from 3 bytes at `pos`.
fn read_u24(data: &[u8], pos: usize) -> usize {
    ((data[pos] as usize) << 16) | ((data[pos + 1] as usize) << 8) | (data[pos + 2] as usize)
}

/// Read a big-endian u16 from 2 bytes at `pos`.
fn read_u16(data: &[u8], pos: usize) -> usize {
    ((data[pos] as usize) << 8) | (data[pos + 1] as usize)
}

/// Parse the footer of a reftable file.
fn parse_footer(data: &[u8], version: u8) -> Result<Footer> {
    let footer_size = if version == 2 { 72 } else { FOOTER_V1_SIZE };
    if data.len() < footer_size {
        return Err(Error::InvalidRef("reftable: footer too small".into()));
    }

    // Verify magic
    if &data[0..4] != REFTABLE_MAGIC {
        return Err(Error::InvalidRef("reftable: bad footer magic".into()));
    }
    let fver = data[4];
    if fver != version {
        return Err(Error::InvalidRef(format!(
            "reftable: footer version mismatch: header={version}, footer={fver}"
        )));
    }

    let block_size = ((data[5] as u32) << 16) | ((data[6] as u32) << 8) | (data[7] as u32);
    let min_update_index = u64::from_be_bytes(data[8..16].try_into().unwrap());
    let max_update_index = u64::from_be_bytes(data[16..24].try_into().unwrap());

    let off = 24;
    let ref_index_position = u64::from_be_bytes(data[off..off + 8].try_into().unwrap());
    let obj_position_and_id_len = u64::from_be_bytes(data[off + 8..off + 16].try_into().unwrap());
    let obj_index_position = u64::from_be_bytes(data[off + 16..off + 24].try_into().unwrap());
    let log_position = u64::from_be_bytes(data[off + 24..off + 32].try_into().unwrap());
    let log_index_position = u64::from_be_bytes(data[off + 32..off + 40].try_into().unwrap());

    // CRC-32 check
    let crc_stored = u32::from_be_bytes(data[footer_size - 4..footer_size].try_into().unwrap());
    let crc_computed = crc32(&data[..footer_size - 4]);
    if crc_stored != crc_computed {
        return Err(Error::InvalidRef(format!(
            "reftable: footer CRC mismatch: stored={crc_stored:08x}, computed={crc_computed:08x}"
        )));
    }

    Ok(Footer {
        version: fver,
        block_size,
        min_update_index,
        max_update_index,
        ref_index_position,
        obj_position_and_id_len,
        obj_index_position,
        log_position,
        log_index_position,
    })
}

/// Parse an identity string like `"Name <email> 1234567890 +0100"`.
fn parse_identity_string(identity: &str) -> (String, String, u64, i16) {
    // Format: "Name <email> timestamp tz"
    let parts: Vec<&str> = identity.rsplitn(3, ' ').collect();
    if parts.len() < 3 {
        return (identity.to_owned(), String::new(), 0, 0);
    }
    let tz_str = parts[0]; // e.g. "+0100"
    let time_str = parts[1]; // e.g. "1234567890"
    let name_email = parts[2]; // e.g. "Name <email>"

    let time_secs = time_str.parse::<u64>().unwrap_or(0);

    // Parse timezone: +HHMM or -HHMM
    let tz_minutes = if tz_str.len() >= 5 {
        let sign = if tz_str.starts_with('-') { -1i16 } else { 1 };
        let hours = tz_str[1..3].parse::<i16>().unwrap_or(0);
        let mins = tz_str[3..5].parse::<i16>().unwrap_or(0);
        sign * (hours * 60 + mins)
    } else {
        0
    };

    // Split name and email
    let (name, email) = if let Some(lt_pos) = name_email.find('<') {
        let name = name_email[..lt_pos].trim().to_owned();
        let email = if let Some(gt_pos) = name_email.find('>') {
            name_email[lt_pos + 1..gt_pos].to_owned()
        } else {
            name_email[lt_pos + 1..].to_owned()
        };
        (name, email)
    } else {
        (name_email.to_owned(), String::new())
    };

    (name, email, time_secs, tz_minutes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_roundtrip() {
        for val in [0u64, 1, 127, 128, 255, 256, 16383, 16384, u64::MAX] {
            let mut buf = Vec::new();
            put_varint(val, &mut buf);
            let (decoded, end) = get_varint(&buf, 0).unwrap();
            assert_eq!(decoded, val, "varint roundtrip failed for {val}");
            assert_eq!(end, buf.len());
        }
    }

    #[test]
    fn test_crc32() {
        // Known test vector: "123456789" => 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_empty_table() {
        let writer = ReftableWriter::new(WriteOptions::default(), 1, 1);
        let data = writer.finish().unwrap();
        let reader = ReftableReader::new(data).unwrap();
        let refs = reader.read_refs().unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn test_write_read_single_ref() {
        let oid = ObjectId::from_bytes(&[0xab; 20]).unwrap();
        let mut writer = ReftableWriter::new(WriteOptions::default(), 1, 1);
        writer
            .add_ref(RefRecord {
                name: "refs/heads/main".to_owned(),
                update_index: 1,
                value: RefValue::Val1(oid.clone()),
            })
            .unwrap();
        let data = writer.finish().unwrap();

        let reader = ReftableReader::new(data).unwrap();
        let refs = reader.read_refs().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "refs/heads/main");
        assert_eq!(refs[0].value, RefValue::Val1(oid));
        assert_eq!(refs[0].update_index, 1);
    }

    #[test]
    fn test_write_read_multiple_refs() {
        let oid1 = ObjectId::from_bytes(&[0x11; 20]).unwrap();
        let oid2 = ObjectId::from_bytes(&[0x22; 20]).unwrap();
        let oid3 = ObjectId::from_bytes(&[0x33; 20]).unwrap();

        let mut writer = ReftableWriter::new(WriteOptions::default(), 1, 1);
        writer
            .add_ref(RefRecord {
                name: "refs/heads/a".to_owned(),
                update_index: 1,
                value: RefValue::Val1(oid1.clone()),
            })
            .unwrap();
        writer
            .add_ref(RefRecord {
                name: "refs/heads/b".to_owned(),
                update_index: 1,
                value: RefValue::Val1(oid2.clone()),
            })
            .unwrap();
        writer
            .add_ref(RefRecord {
                name: "refs/tags/v1.0".to_owned(),
                update_index: 1,
                value: RefValue::Val2(oid3.clone(), oid1.clone()),
            })
            .unwrap();
        let data = writer.finish().unwrap();

        let reader = ReftableReader::new(data).unwrap();
        let refs = reader.read_refs().unwrap();
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].name, "refs/heads/a");
        assert_eq!(refs[1].name, "refs/heads/b");
        assert_eq!(refs[2].name, "refs/tags/v1.0");
        assert_eq!(refs[2].value, RefValue::Val2(oid3, oid1));
    }

    #[test]
    fn test_symref_roundtrip() {
        let mut writer = ReftableWriter::new(WriteOptions::default(), 1, 1);
        writer
            .add_ref(RefRecord {
                name: "refs/heads/sym".to_owned(),
                update_index: 1,
                value: RefValue::Symref("refs/heads/main".to_owned()),
            })
            .unwrap();
        let data = writer.finish().unwrap();

        let reader = ReftableReader::new(data).unwrap();
        let refs = reader.read_refs().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0].value,
            RefValue::Symref("refs/heads/main".to_owned())
        );
    }

    #[test]
    fn test_log_roundtrip() {
        let old_oid = ObjectId::from_bytes(&[0; 20]).unwrap();
        let new_oid = ObjectId::from_bytes(&[0xaa; 20]).unwrap();

        let mut opts = WriteOptions::default();
        opts.write_log = true;
        let mut writer = ReftableWriter::new(opts, 1, 1);
        writer
            .add_log(LogRecord {
                refname: "refs/heads/main".to_owned(),
                update_index: 1,
                old_id: old_oid.clone(),
                new_id: new_oid.clone(),
                name: "Test User".to_owned(),
                email: "test@example.com".to_owned(),
                time_seconds: 1700000000,
                tz_offset: -480,
                message: "initial commit".to_owned(),
            })
            .unwrap();
        let data = writer.finish().unwrap();

        let reader = ReftableReader::new(data).unwrap();
        let logs = reader.read_logs().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].refname, "refs/heads/main");
        assert_eq!(logs[0].old_id, old_oid);
        assert_eq!(logs[0].new_id, new_oid);
        assert_eq!(logs[0].name, "Test User");
        assert_eq!(logs[0].email, "test@example.com");
        assert_eq!(logs[0].time_seconds, 1700000000);
        assert_eq!(logs[0].tz_offset, -480);
        assert_eq!(logs[0].message, "initial commit");
    }

    #[test]
    fn test_unaligned_table() {
        let oid = ObjectId::from_bytes(&[0xcc; 20]).unwrap();
        let opts = WriteOptions {
            block_size: 0, // unaligned
            restart_interval: 16,
            write_log: false,
        };
        let mut writer = ReftableWriter::new(opts, 1, 1);
        writer
            .add_ref(RefRecord {
                name: "refs/heads/main".to_owned(),
                update_index: 1,
                value: RefValue::Val1(oid.clone()),
            })
            .unwrap();
        let data = writer.finish().unwrap();

        let reader = ReftableReader::new(data).unwrap();
        assert_eq!(reader.block_size(), 0);
        let refs = reader.read_refs().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].value, RefValue::Val1(oid));
    }

    #[test]
    fn test_parse_identity() {
        let (name, email, ts, tz) =
            parse_identity_string("Test User <test@example.com> 1700000000 -0800");
        assert_eq!(name, "Test User");
        assert_eq!(email, "test@example.com");
        assert_eq!(ts, 1700000000);
        assert_eq!(tz, -480);
    }

    #[test]
    fn test_deletion_record() {
        let mut writer = ReftableWriter::new(WriteOptions::default(), 1, 1);
        writer
            .add_ref(RefRecord {
                name: "refs/heads/gone".to_owned(),
                update_index: 1,
                value: RefValue::Deletion,
            })
            .unwrap();
        let data = writer.finish().unwrap();

        let reader = ReftableReader::new(data).unwrap();
        let refs = reader.read_refs().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].value, RefValue::Deletion);
    }
}
