# t5334-incremental-multi-pack-index

## Summary

Implemented Git-compatible incremental multi-pack-index layout in `grit-lib` (`multi-pack-index.d/`, chain file, migrating root MIDX on first `--incremental`), wired `multi-pack-index write` with `--bitmap` / `--incremental` and `GIT_TEST_MIDX_WRITE_REV`, chain-aware `verify`, `test-tool read-midx --bitmap` for split layout, `rev-list --test-bitmap` no-op, `repack -a/-A/-cruft` clears stale MIDX state, and fixed rev-parse for hex-like tag names (e.g. `2.2`).

## Validation

- `./scripts/run-tests.sh t5334-incremental-multi-pack-index.sh` → 16/16
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy --fix --allow-dirty`
