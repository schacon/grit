//! Resolve tree paths with symlink following (`get_tree_entry_follow_symlinks`).
//!
//! Behaviour matches upstream Git (`tree-walk.c`).

use std::collections::HashSet;

use crate::error::Result;
use crate::objects::{parse_tree, ObjectId, ObjectKind};
use crate::odb::Odb;

const MAX_SYMLINK_FOLLOWS: usize = 40;

/// Result of resolving `tree_oid:path` with symlink following.
#[derive(Debug, Clone)]
pub enum FollowPathResult {
    /// Found object inside the repository.
    Found { oid: ObjectId, mode: u32 },
    /// Symlink target is absolute (`/`); caller prints `symlink <len> <target>`.
    OutOfRepo { path: Vec<u8> },
}

/// Failure modes reported as special `git cat-file --batch-check` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowPathFailure {
    Missing,
    DanglingSymlink,
    SymlinkLoop,
    NotDir,
}

fn git_mode_is_dir(mode: u32) -> bool {
    mode == 0o040000
}

fn git_mode_is_symlink(mode: u32) -> bool {
    mode == 0o120000
}

fn git_mode_is_blob(mode: u32) -> bool {
    (mode & 0o170000) == 0o100000
}

fn find_one_entry(tree_data: &[u8], name: &str) -> Result<Option<(ObjectId, u32)>> {
    let entries = parse_tree(tree_data)?;
    for e in entries {
        if e.name == name.as_bytes() {
            return Ok(Some((e.oid, e.mode)));
        }
    }
    Ok(None)
}

/// Walk `tree_oid` following symlinks like `get_tree_entry_follow_symlinks`.
pub fn get_tree_entry_follow_symlinks(
    odb: &Odb,
    tree_oid: &ObjectId,
    path: &str,
) -> Result<std::result::Result<FollowPathResult, FollowPathFailure>> {
    let mut stack: Vec<ObjectId> = vec![*tree_oid];
    let mut path_buf = path.to_string();
    let mut follows = 0usize;
    let mut followed_symlink_blobs: HashSet<ObjectId> = HashSet::new();

    loop {
        let Some(&tree_oid) = stack.last() else {
            return Ok(Err(FollowPathFailure::Missing));
        };

        while path_buf.starts_with('/') {
            path_buf.remove(0);
        }

        if path_buf.is_empty() {
            return Ok(Ok(FollowPathResult::Found {
                oid: tree_oid,
                mode: 0o040000,
            }));
        }

        let (first, rest) = match path_buf.split_once('/') {
            Some((a, b)) => (a.to_string(), Some(b.to_string())),
            None => (path_buf.clone(), None),
        };

        if first == ".." {
            if stack.len() <= 1 {
                return Ok(Ok(FollowPathResult::OutOfRepo {
                    path: path_buf.into_bytes(),
                }));
            }
            stack.pop();
            path_buf = rest.unwrap_or_default();
            continue;
        }

        if first.is_empty() {
            let Some(&oid) = stack.last() else {
                return Ok(Err(FollowPathFailure::Missing));
            };
            return Ok(Ok(FollowPathResult::Found {
                oid,
                mode: 0o040000,
            }));
        }

        let tree_obj = match odb.read(&tree_oid) {
            Ok(o) => o,
            Err(_) => return Ok(Err(FollowPathFailure::Missing)),
        };
        if tree_obj.kind != ObjectKind::Tree {
            return Ok(Err(FollowPathFailure::Missing));
        }

        let found = match find_one_entry(&tree_obj.data, &first) {
            Ok(x) => x,
            Err(_) => return Ok(Err(FollowPathFailure::Missing)),
        };

        let Some((entry_oid, mode)) = found else {
            return Ok(Err(FollowPathFailure::Missing));
        };

        if git_mode_is_dir(mode) {
            if rest.is_none() {
                return Ok(Ok(FollowPathResult::Found {
                    oid: entry_oid,
                    mode,
                }));
            }
            stack.push(entry_oid);
            path_buf = rest.unwrap();
            continue;
        }

        if git_mode_is_blob(mode) {
            if rest.is_none() {
                return Ok(Ok(FollowPathResult::Found {
                    oid: entry_oid,
                    mode,
                }));
            }
            return Ok(Err(FollowPathFailure::NotDir));
        }

        if git_mode_is_symlink(mode) {
            if follows >= MAX_SYMLINK_FOLLOWS {
                return Ok(Err(FollowPathFailure::SymlinkLoop));
            }
            if !followed_symlink_blobs.insert(entry_oid) {
                return Ok(Err(FollowPathFailure::SymlinkLoop));
            }
            follows += 1;

            let obj = match odb.read(&entry_oid) {
                Ok(o) => o,
                Err(_) => return Ok(Err(FollowPathFailure::DanglingSymlink)),
            };
            if obj.kind != ObjectKind::Blob {
                return Ok(Err(FollowPathFailure::DanglingSymlink));
            }

            if obj.data.first() == Some(&b'/') {
                return Ok(Ok(FollowPathResult::OutOfRepo {
                    path: obj.data.clone(),
                }));
            }

            let mut new_path = String::from_utf8_lossy(&obj.data).into_owned();
            if let Some(r) = rest {
                new_path.push('/');
                new_path.push_str(&r);
            }
            path_buf = new_path;
            continue;
        }

        // Submodule (gitlink) or other.
        if rest.is_none() {
            return Ok(Ok(FollowPathResult::Found {
                oid: entry_oid,
                mode,
            }));
        }
        return Ok(Err(FollowPathFailure::NotDir));
    }
}
