//! Helpers for reading and writing index extensions.
//!
//! This module currently implements:
//! - `TREE` (cache-tree) extension parsing/serialization.
//! - `link` (split-index) extension parsing for debug output.

use crate::error::{Error, Result};
use crate::objects::ObjectId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheTreeNode {
    pub path: String,
    pub entry_count: i32,
    pub subtree_count: i32,
    pub oid: Option<ObjectId>,
    pub children: Vec<CacheTreeNode>,
}

impl CacheTreeNode {
    fn new(path: String, entry_count: i32, subtree_count: i32, oid: Option<ObjectId>) -> Self {
        Self {
            path,
            entry_count,
            subtree_count,
            oid,
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitIndexData {
    pub base_oid: ObjectId,
    pub delete_bitmap: EwahBitmap,
    pub replace_bitmap: EwahBitmap,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EwahBitmap {
    pub bits: Vec<usize>,
}

impl EwahBitmap {
    pub fn iter_bits(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.iter().copied()
    }
}

/// Parsed index extensions that we currently care about.
#[derive(Debug, Clone, Default)]
pub struct IndexExtensions {
    pub cache_tree: Option<CacheTreeNode>,
    pub split_index: Option<SplitIndexData>,
}

#[derive(Debug, Clone)]
pub struct RawExtension {
    pub signature: [u8; 4],
    pub data: Vec<u8>,
}

pub fn parse_extensions(body: &[u8], start: usize) -> Result<(IndexExtensions, Vec<RawExtension>)> {
    let mut pos = start;
    let body_len = body.len();
    let mut parsed = IndexExtensions::default();
    let mut passthrough = Vec::new();

    while pos + 8 <= body_len {
        let sig: [u8; 4] = body[pos..pos + 4]
            .try_into()
            .map_err(|_| Error::IndexError("truncated extension signature".to_owned()))?;
        let len = u32::from_be_bytes(
            body[pos + 4..pos + 8]
                .try_into()
                .map_err(|_| Error::IndexError("truncated extension length".to_owned()))?,
        ) as usize;
        pos += 8;
        if pos + len > body_len {
            return Err(Error::IndexError("extension length out of bounds".to_owned()));
        }
        let ext_data = body[pos..pos + len].to_vec();
        pos += len;

        match &sig {
            b"TREE" => {
                let mut off = 0usize;
                let root = parse_cache_tree_node(&ext_data, &mut off, "")?;
                parsed.cache_tree = Some(root);
            }
            b"link" => {
                let data = parse_split_index_extension(&ext_data)?;
                parsed.split_index = Some(data);
            }
            _ => {
                passthrough.push(RawExtension {
                    signature: sig,
                    data: ext_data,
                });
            }
        }
    }

    Ok((parsed, passthrough))
}

pub fn serialize_extensions(
    parsed: &IndexExtensions,
    passthrough: &[RawExtension],
    out: &mut Vec<u8>,
) {
    for ext in passthrough {
        out.extend_from_slice(&ext.signature);
        out.extend_from_slice(&(ext.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&ext.data);
    }

    if let Some(root) = &parsed.cache_tree {
        let mut data = Vec::new();
        serialize_cache_tree_node(root, "", &mut data);
        out.extend_from_slice(b"TREE");
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&data);
    }
}

fn parse_cache_tree_node(data: &[u8], pos: &mut usize, parent_path: &str) -> Result<CacheTreeNode> {
    // NUL-terminated path component (relative to parent)
    let start = *pos;
    let nul = data[start..]
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| Error::IndexError("TREE extension missing NUL".to_owned()))?;
    let name_bytes = &data[start..start + nul];
    *pos = start + nul + 1;
    let name = String::from_utf8_lossy(name_bytes).to_string();
    let full_path = if parent_path.is_empty() {
        if name.is_empty() {
            String::new()
        } else {
            format!("{name}/")
        }
    } else if name.is_empty() {
        parent_path.to_owned()
    } else {
        format!("{parent_path}{name}/")
    };

    // ASCII decimal entry_count
    let space = data[*pos..]
        .iter()
        .position(|b| *b == b' ')
        .ok_or_else(|| Error::IndexError("TREE extension missing space".to_owned()))?;
    let entry_count_str = String::from_utf8_lossy(&data[*pos..*pos + space]).to_string();
    *pos += space + 1;
    let entry_count: i32 = entry_count_str
        .parse()
        .map_err(|_| Error::IndexError("TREE invalid entry_count".to_owned()))?;

    // ASCII decimal subtree_count
    let nl = data[*pos..]
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| Error::IndexError("TREE extension missing newline".to_owned()))?;
    let subtree_count_str = String::from_utf8_lossy(&data[*pos..*pos + nl]).to_string();
    *pos += nl + 1;
    let subtree_count: i32 = subtree_count_str
        .parse()
        .map_err(|_| Error::IndexError("TREE invalid subtree_count".to_owned()))?;

    let oid = if entry_count < 0 {
        None
    } else {
        if *pos + 20 > data.len() {
            return Err(Error::IndexError("TREE extension truncated oid".to_owned()));
        }
        let oid = ObjectId::from_bytes(&data[*pos..*pos + 20])?;
        *pos += 20;
        Some(oid)
    };

    let mut node = CacheTreeNode::new(full_path, entry_count, subtree_count, oid);
    for _ in 0..subtree_count {
        let child = parse_cache_tree_node(data, pos, &node.path)?;
        node.children.push(child);
    }
    Ok(node)
}

fn serialize_cache_tree_node(node: &CacheTreeNode, parent_path: &str, out: &mut Vec<u8>) {
    let name = if parent_path.is_empty() {
        node.path.trim_end_matches('/')
    } else {
        node.path
            .trim_end_matches('/')
            .strip_prefix(parent_path.trim_end_matches('/'))
            .unwrap_or(node.path.trim_end_matches('/'))
            .trim_start_matches('/')
    };

    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out.extend_from_slice(node.entry_count.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(node.children.len().to_string().as_bytes());
    out.push(b'\n');
    if node.entry_count >= 0 {
        if let Some(oid) = node.oid {
            out.extend_from_slice(oid.as_bytes());
        } else {
            out.extend_from_slice(&[0u8; 20]);
        }
    }
    for child in &node.children {
        serialize_cache_tree_node(child, &node.path, out);
    }
}

fn parse_split_index_extension(data: &[u8]) -> Result<SplitIndexData> {
    if data.len() < 20 {
        return Err(Error::IndexError("link extension too short".to_owned()));
    }
    let base_oid = ObjectId::from_bytes(&data[..20])?;
    let mut pos = 20usize;
    let delete_bitmap = parse_ewah(data, &mut pos)?;
    let replace_bitmap = parse_ewah(data, &mut pos)?;
    Ok(SplitIndexData {
        base_oid,
        delete_bitmap,
        replace_bitmap,
    })
}

fn parse_ewah(data: &[u8], pos: &mut usize) -> Result<EwahBitmap> {
    if *pos + 12 > data.len() {
        return Err(Error::IndexError("ewah header truncated".to_owned()));
    }
    // u32 bit-size
    let bit_size = u32::from_be_bytes(
        data[*pos..*pos + 4]
            .try_into()
            .map_err(|_| Error::IndexError("ewah bit-size truncated".to_owned()))?,
    ) as usize;
    *pos += 4;
    // u32 word-count
    let word_count = u32::from_be_bytes(
        data[*pos..*pos + 4]
            .try_into()
            .map_err(|_| Error::IndexError("ewah word-count truncated".to_owned()))?,
    ) as usize;
    *pos += 4;
    if *pos + word_count * 8 + 4 > data.len() {
        return Err(Error::IndexError("ewah payload truncated".to_owned()));
    }
    let words_start = *pos;
    *pos += word_count * 8;
    // trailing rlw position u32
    let _rlw_pos = u32::from_be_bytes(
        data[*pos..*pos + 4]
            .try_into()
            .map_err(|_| Error::IndexError("ewah rlw pos truncated".to_owned()))?,
    ) as usize;
    *pos += 4;

    let mut bits = Vec::new();
    let mut out_word_index = 0usize;
    let mut wi = 0usize;
    while wi < word_count {
        let word_off = words_start + wi * 8;
        let marker = u64::from_be_bytes(
            data[word_off..word_off + 8]
                .try_into()
                .map_err(|_| Error::IndexError("ewah marker word truncated".to_owned()))?,
        );
        wi += 1;

        let running_bit = (marker & 1) != 0;
        let running_len = ((marker >> 1) & ((1u64 << 32) - 1)) as usize;
        let literal_words = ((marker >> 33) & ((1u64 << 31) - 1)) as usize;

        // running words
        if running_bit {
            for r in 0..running_len {
                let base = (out_word_index + r) * 64;
                for b in 0..64usize {
                    let bit = base + b;
                    if bit < bit_size {
                        bits.push(bit);
                    }
                }
            }
        }
        out_word_index += running_len;

        // literal words
        for _ in 0..literal_words {
            if wi >= word_count {
                return Err(Error::IndexError("ewah literal words overflow".to_owned()));
            }
            let lit_off = words_start + wi * 8;
            let lit = u64::from_be_bytes(
                data[lit_off..lit_off + 8]
                    .try_into()
                    .map_err(|_| Error::IndexError("ewah literal word truncated".to_owned()))?,
            );
            let base = out_word_index * 64;
            for b in 0..64usize {
                if (lit >> b) & 1 == 1 {
                    let bit = base + b;
                    if bit < bit_size {
                        bits.push(bit);
                    }
                }
            }
            out_word_index += 1;
            wi += 1;
        }
    }

    Ok(EwahBitmap { bits })
}
