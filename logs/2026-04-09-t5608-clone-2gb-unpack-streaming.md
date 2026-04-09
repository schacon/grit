# t5608-clone-2gb — streaming unpack-objects

## Problem

`file://` clones use upload-pack and `unpack_objects`, which previously called
`read_to_end` on the entire pack into a `Vec` and kept every resolved object in
`HashMap` values. A >2GB pack could exhaust memory (OOM) during t5608.

## Change

- `grit-lib/src/unpack_objects.rs`: stream-parse the pack with incremental SHA-1
  (only zlib-compressed bytes counted via `total_in()`, lookahead/trailer not
  hashed incorrectly).
- After writing blobs larger than 1 MiB, drop body from in-memory maps
  (`BlobOnDisk`); delta bases still load from ODB when needed.
- `--strict` verification skips blob bodies (unchanged semantics for refs).

## Validation

- `cargo test -p grit-lib --lib` (all pass).
- Harness `t5608-clone-2gb.sh` with `GIT_TEST_CLONE_2GB=false` skips (0 tests);
  full run requires `GIT_TEST_CLONE_2GB=true` and large disk/RAM.
