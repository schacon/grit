## 2026-04-08 — t6501-freshen-objects

### Failures observed

- `git gc --no-cruft --prune=12.hours.ago` failed: `gc` did not accept `--prune=<date>` and never ran loose-object pruning after repack (objects with old mtimes were removed while still referenced only from a temp index / in-progress tree).
- After wiring prune: `write-tree` reused existing tree OIDs without touching loose/pack mtimes, so prune still removed “rescued” blobs.
- `git gc -q` left pack-objects summary on stderr (`Total N (delta …)`), breaking `test_must_be_empty stderr` in broken-link tests.

### Fixes

- **`grit-lib` `Odb`**: `freshen_object` — `utimes` loose file or containing `.pack` when object already exists; `write` / `write_raw` call it on duplicate (Git `odb_freshen_object` behavior).
- **`grit gc`**: `--prune[=<date>]`, `--no-prune`; always run `prune-packed`; after repack invoke `prune` with expire from CLI, `gc.pruneExpire`, or default `2.weeks.ago`; treat `never` as skip loose prune.
- **`grit prune`**: `--expire=never` → do not delete unreachable loose objects.
- **`pack-objects`**: honor `-q` / `--quiet` for the “Total …” stderr line.

### Validation

- `GUST_BIN=… bash tests/t6501-freshen-objects.sh` → 42/42
- `./scripts/run-tests.sh t6501-freshen-objects.sh` → 42/42
- `cargo test -p grit-lib --lib`
