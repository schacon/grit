# Grit Performance Analysis

> Benchmark date: 2026-04-01  
> Test repo: ~1000 files across 10 directories, ~50 commits (all loose objects)  
> System: Linux 6.8.0 x64, git 2.x vs grit (Rust, release build)

## Executive Summary

Grit's biggest performance gaps are in working-tree commands (`add`, `status`, `diff-files`).
The root cause is the same across all three: **grit reads and hashes every file on every invocation**,
while C git uses the index's cached stat data to skip unchanged files entirely. This single
architectural gap accounts for ~80% of the performance difference in everyday commands.

## Benchmark Results — Sorted by Slowdown

| Command | C git | grit | Ratio | Abs. gap |
|---------|------:|-----:|------:|---------:|
| `add -A` | 5.1 ms | 30.2 ms | **5.9×** | 25.1 ms |
| `diff-files` | 3.8 ms | 16.2 ms | **4.3×** | 12.4 ms |
| `status --porcelain` | 5.9 ms | 18.5 ms | **3.1×** | 12.6 ms |
| `write-tree` | 9.2 ms | 15.5 ms | **1.7×** | 6.3 ms |
| `ls-files` | 2.2 ms | 3.2 ms | **1.5×** | 1.0 ms |
| `rev-parse HEAD` | 1.8 ms | 2.8 ms | **1.5×** | 1.0 ms |
| `ls-tree -r HEAD` | 2.9 ms | 4.1 ms | 1.4× | 1.2 ms |
| `cat-file -p` | 2.1 ms | 2.6 ms | 1.2× | 0.5 ms |
| `config --list` | 2.3 ms | 3.1 ms | 1.3× | 0.8 ms |
| `show-ref` | 4.2 ms | 5.2 ms | 1.2× | 1.0 ms |

Commands where grit is **faster** than C git: `init` (1.35×), `cat-file --batch` (1.63×),
`log --oneline` (1.21×), `branch --list` (1.07×), `hash-object` (1.04×).

---

## 1. `add -A` — 5.9× slower (Priority: **Critical**)

### Symptoms

| Metric | C git | grit |
|--------|------:|-----:|
| `openat` calls | 48 | 1,018 |
| `read` calls | 22 | 2,007 |
| `statx` calls | 0 | 6,017 |
| Wall time | 5.1 ms | 30.2 ms |

### Root Cause: Every File Is Read and Hashed Unconditionally

In `grit/src/commands/add.rs`, the `add_all` → `stage_file` path does this for
**every file** in the working tree:

```rust
// add.rs: stage_file()
let data = if meta.file_type().is_symlink() {
    let target = fs::read_link(abs_path)?;
    target.to_string_lossy().into_owned().into_bytes()
} else {
    fs::read(abs_path)?         // ← reads every file
};

let oid = odb.write(ObjectKind::Blob, &data)?;  // ← hashes + zlib-compresses every file
```

For 1,000 files this means 1,000 `openat` + `read` + SHA-1 + zlib-compress operations, plus
1,000 more `statx` calls to check `path.exists()` in `odb.write()`. That's **3,000+ syscalls
for files that haven't changed**.

**How C git handles this:** C git's `add` checks index stat data (mtime, ctime, size, ino, dev)
against each file's current `lstat()`. If the stat fields match, the file is assumed unchanged
and skipped entirely — no read, no hash. Only genuinely dirty files (stat mismatch) get hashed.
The result: git does ~48 `openat` calls total (index + a handful of changed files) vs grit's ~1,018.

### Additional Issue: `walk_directory` Uses `fs::read_dir` with Sorting

```rust
fn walk_directory(dir: &Path, work_tree: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries = fs::read_dir(dir)?;
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());  // ← unnecessary allocation + sort
    for entry in sorted {
        // ...
    }
}
```

This sorts directory entries even though the index is already sorted. It also allocates `String`
paths for every file, then passes them to `stage_file` which joins them back into absolute paths.

### Recommendations

1. **Implement stat-based skip logic** (biggest win, ~5× improvement expected):
   ```rust
   fn stage_file(...) -> Result<()> {
       let meta = fs::symlink_metadata(abs_path)?;
       // Check if index already has this file with matching stat data
       if let Some(existing) = index.get(rel_path.as_bytes(), 0) {
           if stat_matches_index(existing, &meta) {
               return Ok(()); // Skip — file unchanged
           }
       }
       // Only read + hash if stat data differs
       let data = fs::read(abs_path)?;
       let oid = odb.write(ObjectKind::Blob, &data)?;
       // ...
   }
   ```

2. **Hash-only mode for add**: Compute the OID in-memory (`hash_object_data`) and compare
   against the existing index entry's OID. Only call `odb.write()` (which does zlib compression
   + disk write) if the OID actually changed. Currently `odb.write()` is called for every file
   and does an `exists()` stat check on the object file path.

3. **Use `OsString` paths instead of `String`** to avoid UTF-8 conversion overhead in
   `walk_directory`. Use `&[u8]` slices where possible instead of allocating.

4. **Consider parallel directory walking** with `rayon` for large repos.

---

## 2. `status --porcelain` — 3.1× slower (Priority: **Critical**)

### Symptoms

| Metric | C git | grit |
|--------|------:|-----:|
| `statx` calls | 50 | 3,024 |
| `read` calls | 33 | 2,112 |
| `openat` calls | 67 | 1,030 |
| Wall time | 5.9 ms | 18.5 ms |

### Root Cause: Broken `stat_matches` — Always Returns `false`

The stat-caching logic in `grit-lib/src/diff.rs` exists but is **intentionally disabled**:

```rust
fn stat_matches(ie: &IndexEntry, meta: &fs::Metadata) -> bool {
    if meta.len() as u32 != ie.size {
        return false;
    }
    if meta.mtime() as u32 != ie.mtime_sec {
        return false;
    }
    false // Conservative: if size + mtime match, still hash to be safe for now
          // TODO: Full stat comparison (ctime, ino, dev, uid, gid)
}
```

That trailing `false` means **every file falls through to the slow path**: read the entire file,
SHA-1 hash it, and compare OIDs. For a clean 1,000-file repo, that's 1,000 unnecessary file reads
and hashes.

**How C git handles this:** C git's `ce_uptodate()` / `ie_match_stat()` checks ~8 stat fields
(ctime, mtime, ino, dev, uid, gid, size, mode). If they all match, the file is assumed clean —
no read, no hash. The typical cost of `git status` on a clean repo is: parse index (1 read) +
lstat each file (fast kernel calls) + scan for untracked files = a few hundred lightweight
syscalls total.

### Additional Issue: Double Work in Status

`status` calls both `diff_index_to_tree()` (staged changes) and `diff_index_to_worktree()`
(unstaged changes), then also `find_untracked()`. The `find_untracked()` function does a
**separate full directory walk** using `walk_for_untracked()`, which is another complete traversal
with `read_dir` + sort per directory:

```rust
fn walk_for_untracked(dir: &Path, work_tree: &Path, tracked: &BTreeSet<String>, out: &mut Vec<String>) {
    let entries = fs::read_dir(dir)?;
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());  // Another sort!
    // ...
}
```

C git merges the untracked-file scan into its single directory traversal pass.

### Recommendations

1. **Fix `stat_matches` to actually work** (immediate ~3× speedup):
   ```rust
   fn stat_matches(ie: &IndexEntry, meta: &fs::Metadata) -> bool {
       if meta.len() as u32 != ie.size { return false; }
       if meta.mtime() as u32 != ie.mtime_sec { return false; }
       if meta.mtime_nsec() as u32 != ie.mtime_nsec { return false; }
       if meta.ctime() as u32 != ie.ctime_sec { return false; }
       if meta.ctime_nsec() as u32 != ie.ctime_nsec { return false; }
       if meta.ino() as u32 != ie.ino { return false; }
       if meta.dev() as u32 != ie.dev { return false; }
       true  // ← return true when stat data matches!
   }
   ```

2. **Merge untracked-file detection into the worktree diff pass**: Instead of doing two
   separate walks (one for `diff_index_to_worktree`, one for `find_untracked`), do a single
   pass that walks the directory tree in parallel with the sorted index entries. This is how
   C git's `read_directory()` + `run_diff_files()` work.

3. **Refresh index stat data on status**: When `status` reads a file to hash it (because stat
   data changed racily), write the updated stat data back to the index. C git does this as the
   "index refresh" step, saving future `status` calls from re-hashing.

---

## 3. `diff-files` — 4.3× slower (Priority: **High**)

### Symptoms

| Metric | C git | grit |
|--------|------:|-----:|
| `statx` calls | 12 | 2,006 |
| `read` calls | 19 | 2,007 |
| `openat` calls | 23 | 1,005 |
| Wall time | 3.8 ms | 16.2 ms |

### Root Cause: Same as Status — Reads and Hashes Every File

In `diff_files.rs`, the `read_worktree_info()` function reads and hashes **every indexed file**
to detect changes:

```rust
fn read_worktree_info(repo: &Repository, abs_path: &Path) -> Result<Option<(u32, ObjectId)>> {
    let meta = fs::symlink_metadata(abs_path)?;
    // ...
    let data = fs::read(abs_path)?;              // ← reads every file
    let oid = Odb::hash_object_data(ObjectKind::Blob, &data);  // ← hashes every file
    Ok(Some((mode, oid)))
}
```

And in `collect_changes()`, there's no stat-based shortcut:

```rust
// collect_changes()
for (path, (idx_mode, idx_oid)) in &stage0 {
    let abs = work_tree.join(path);
    match read_worktree_info(repo, &abs)? {
        Some((wt_mode, wt_oid)) => {
            if wt_oid != *idx_oid || wt_mode != idx_canonical {
                // changed
            }
        }
        // ...
    }
}
```

There is no call to `stat_matches()` or any equivalent. Every file gets the full
read-and-hash treatment.

**How C git handles `diff-files`:** C git's `run_diff_files()` calls `ie_match_stat()` on each
index entry. If the stat data matches, the file is skipped immediately. Only files with stat
mismatches are reported (and even then, diff-files doesn't hash — it just reports the index OID
with zeroed-out worktree OID).

### Important Optimization C git Uses

C git's `diff-files` in raw output mode **doesn't even hash the worktree file**. It reports the
index-side OID and zeros for the worktree side. Grit is hashing every file just to compare OIDs,
but then the raw output format discards the worktree OID anyway.

### Recommendations

1. **Add stat-based filtering** (same fix as status — reuse `stat_matches`):
   ```rust
   fn collect_changes(...) -> Result<Vec<Change>> {
       for (path, (idx_mode, idx_oid)) in &stage0 {
           let abs = work_tree.join(path);
           let meta = match fs::symlink_metadata(&abs) {
               Ok(m) => m,
               Err(e) if e.kind() == ErrorKind::NotFound => { /* deleted */ continue; }
               Err(e) => return Err(e.into()),
           };
           // Skip if stat matches index
           if let Some(ie) = index.get(path.as_bytes(), 0) {
               if stat_matches(ie, &meta) && mode_matches(ie.mode, &meta) {
                   continue;
               }
           }
           // Only then read + hash (or just lstat for raw format)
       }
   }
   ```

2. **For raw output format, skip hashing entirely**: When outputting in raw format, just
   report the stat-mismatched files with the index OID and zero OID. This matches C git behavior
   and avoids thousands of unnecessary reads.

---

## 4. `write-tree` — 1.7× slower (Priority: **Medium**)

### Symptoms

| Metric | C git | grit |
|--------|------:|-----:|
| `newfstatat`/`statx` | 1,024 | 1,017 |
| `openat` | 39 | 5 |
| Wall time | 9.2 ms | 15.5 ms |

### Root Cause: `odb.exists()` Check for Every Index Entry

In `grit/src/commands/write_tree.rs`, the `write_tree_from_index()` function verifies that every
blob OID exists in the object store:

```rust
if !missing_ok {
    for entry in &entries {
        if !odb.exists(&entry.oid) {  // ← stat() on every object file
            bail!("invalid object {} for '{}': object does not exist", ...);
        }
    }
}
```

`odb.exists()` calls `path.exists()` which does a `statx()` syscall for each of the ~1,000
entries. This is the same count as C git's `newfstatat` calls — but the interesting part is
that grit has very few `openat` calls (5 vs 39), meaning grit's tree-building code is efficient
once it gets past the existence check. The extra time comes from the overhead of 1,000 stat
calls plus the tree construction being done with `BTreeMap` (sorted map with allocation per entry).

**How C git handles this:** C git also checks object existence, but it does so using the
in-memory pack index (binary search, no syscall) for packed objects, and only falls back to
loose-object stat checks for objects not in packs. With a packed repo, this is nearly free.
Even for loose objects, C git uses a cached directory listing of the `objects/xx/` fan-out
directories.

### Recommendations

1. **Cache the `objects/xx/` directories**: Instead of stat-ing each object file individually,
   read the directory listing of each `objects/xx/` prefix once and build an in-memory set.
   This turns 1,000 stat calls into ~256 readdir calls (or fewer, since not all prefixes exist).

2. **Skip existence check when index is trusted**: If the index was written by grit itself (or
   just validated on load), assume the OIDs are valid. Add a `--trust-index` fast path. C git
   doesn't skip this check, but grit could as a competitive advantage.

3. **Pack file support for reads**: When objects are packed, implement pack-index lookup
   (binary search into `.idx` fanout table) instead of filesystem stat. This is a larger effort
   but essential for real-world repos where most objects are packed.

---

## 5. `rev-parse HEAD` — 1.5× slower (Priority: **Low**)

### Symptoms

| Metric | C git | grit |
|--------|------:|-----:|
| Total syscalls | ~180 | ~80 |
| Wall time | 1.8 ms | 2.8 ms |

Grit actually makes fewer syscalls than C git for `rev-parse HEAD`, yet is slower. The gap
(~1ms) is likely due to:

1. **Rust binary startup overhead**: The Rust runtime (stack guard page setup, thread-local
   storage, allocator init) adds ~0.5-1ms compared to C git's lean startup.
2. **`Repository::discover()`** does `canonicalize()` calls (which resolve symlinks via
   `readlink` syscalls — visible as 5 failed readlink calls in strace).
3. **Clap argument parsing** adds some overhead for simple commands.

### Recommendations

1. **Avoid `canonicalize()` in hot paths**: Use the raw path from `getcwd()` or the `-C` argument
   directly. Only canonicalize when needed for symlink resolution.
2. This is largely startup overhead — not worth major effort until the bigger items are addressed.

---

## 6. `ls-files` — 1.5× slower (Priority: **Low**)

### Analysis

`ls-files` simply loads the index and prints entry paths. The gap is ~1ms, which is mostly:

1. Rust binary startup overhead (~0.5ms)
2. Index parsing (grit parses the full index into heap-allocated `Vec<IndexEntry>` with
   `path: Vec<u8>` per entry, while C git uses memory-mapped I/O with zero-copy path access)
3. Output buffering (grit does per-entry `write!` calls rather than batching into a single buffer)

### Recommendations

1. **Memory-map the index file** and parse entries lazily or in-place (avoid per-entry `Vec<u8>`
   allocation for paths). This would also speed up every other command that loads the index.
2. **Buffer output**: Use `BufWriter` around stdout (this is already `stdout.lock()` but without
   explicit buffering in the ls-files implementation).

---

## Cross-Cutting Recommendations (Applies to All Commands)

### A. Index Parsing Allocations

Every command loads the index with `Index::load()` → `Index::parse()`, which allocates a
`Vec<u8>` for each entry's path:

```rust
let path = data[pos..pos + nul].to_vec();  // ← allocation per entry
```

For 1,000 entries, that's 1,000 small heap allocations. Consider:
- Memory-mapping the index and storing `&[u8]` slices (requires lifetime management)
- Using a single `Vec<u8>` arena with offset+length pairs
- At minimum, use `Vec::with_capacity` for the path based on the length in flags

### B. `add_or_replace` Does Linear Search + Full Sort

```rust
pub fn add_or_replace(&mut self, entry: IndexEntry) {
    if let Some(pos) = self.entries.iter().position(|e| e.path == path && e.stage() == stage) {
        self.entries[pos] = entry;
    } else {
        self.entries.push(entry);
    }
    self.sort();  // ← full O(n log n) sort after every single insertion!
}
```

For `add -A` with 1,000 files, this calls `.sort()` 1,000 times — each time O(n log n).
Total: O(n² log n). Fix: batch insertions and sort once, or use binary search for insertion.

### C. String Conversions

Many paths go through `String::from_utf8_lossy(&ie.path).to_string()` which allocates a new
`String` every time. Since Git paths are always valid UTF-8 in practice, use `from_utf8` and
handle the error case, or work with `&[u8]` directly.

### D. `BTreeSet`/`BTreeMap` for Path Lookups

Several places (e.g., `find_untracked`, `format_short` in status) build `BTreeSet<String>` or
`HashMap<String, char>` from index entries. These should use `&[u8]` or `&str` borrows from
the already-loaded index, avoiding repeated allocation.

---

## Priority Ordering

| Priority | Fix | Expected Speedup | Effort |
|----------|-----|-------------------|--------|
| 🔴 P0 | Fix `stat_matches` to return `true` | **3-5× for status/diff-files** | 5 min |
| 🔴 P0 | Add stat-based skip to `add -A` | **4-5× for add** | 30 min |
| 🟠 P1 | Skip file hashing in diff-files raw mode | **2-3× for diff-files** | 15 min |
| 🟠 P1 | Fix `add_or_replace` to not sort every call | **1.5× for add on large repos** | 15 min |
| 🟡 P2 | Merge untracked scan into single walk | **1.3× for status** | 2 hr |
| 🟡 P2 | Cache ODB directory listings for write-tree | **1.3× for write-tree** | 1 hr |
| 🟢 P3 | Memory-map index file | **1.2× for all commands** | 4 hr |
| 🟢 P3 | Avoid `canonicalize()` in discover | **~0.3ms for all commands** | 30 min |
| 🟢 P3 | Pack file read support in ODB | **Required for real-world repos** | 1-2 days |

### The One-Line Fix

The single highest-impact change is fixing the `stat_matches` function in `grit-lib/src/diff.rs`
to return `true` instead of `false` when stat data matches. This one change would make `status`
and `diff-files` approximately 3× faster immediately, bringing them close to C git performance
for clean working trees.

```diff
 fn stat_matches(ie: &IndexEntry, meta: &fs::Metadata) -> bool {
     if meta.len() as u32 != ie.size {
         return false;
     }
     if meta.mtime() as u32 != ie.mtime_sec {
         return false;
     }
-    false // Conservative: if size + mtime match, still hash to be safe for now
-          // TODO: Full stat comparison (ctime, ino, dev, uid, gid)
+    if meta.ctime() as u32 != ie.ctime_sec {
+        return false;
+    }
+    if meta.ino() as u32 != ie.ino {
+        return false;
+    }
+    true
 }
```

Then wire the same check into `add -A` and `diff-files` for the other ~2× each.
