# t2002-checkout-cache-u

## Problem

Harness failed test 2: after `read-tree` + `checkout-index -f -a` (no `-u`), `diff-files` must exit non-zero because index stat cache is still "smudged" / uninitialized. Grit treated matching blob OID as clean regardless of index stat.

## Fix

In `diff_files::collect_changes`, only skip reporting when worktree content matches the index OID **and** the index entry has trusted stat data (`index_stat_is_trusted`: any of size, mtime, ctime, dev, or ino non-zero). Otherwise emit an `M` change when OID+mode match (Git `ce_match_stat_basic` behavior for zeroed stat after `read-tree`).

## Validation

- `bash tests/t2002-checkout-cache-u.sh` — all 3 pass
- `cargo test -p grit-lib --lib` — pass
