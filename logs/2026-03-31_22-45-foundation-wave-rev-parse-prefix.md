# Foundation wave: rev-parse core/prefix coverage

## Scope

- Targeted plan items: 0.2, 0.3, 0.4, 1.3 (without touching 0.5/0.6/9.x/10.x docs).
- Implemented missing `rev-parse` behavior needed by `t1513` subset:
  - `--prefix` / `--prefix=<arg>`
  - path rewriting across `--` boundaries
  - `treeish:path` resolution for `HEAD:path` style expressions
  - `HEAD:./path` and `HEAD:../path` normalization with prefix context

## Files touched in this wave

- `gust-lib/src/rev_parse.rs`
- `gust/src/commands/rev_parse.rs`
- `tests/t1513-rev-parse-prefix.sh`
- `tests/harness/selected-tests.txt`
- `logs/2026-03-31_22-45-foundation-wave-rev-parse-prefix.md`

## Validation

- `cargo fmt` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo test --workspace` ✅
- `tests/t1513-rev-parse-prefix.sh` ✅ (11/11)
- `./tests/harness/run.sh` ❌
  - New script `t1513-rev-parse-prefix.sh` passes in harness.
  - Existing unrelated failure persists in `t6003-rev-list-topo-order.sh` (`--topo-order`/`--date-order`/`--reverse` cases fail with repository discovery error).

## Notes

- Ignore parity (`t0008-ignores.sh`) continues to pass unchanged in harness run.
- `rev-list`/`merge-base` foundation still appears partially blocked by existing `t6003` failure, so 0.3 cannot be claimed complete from this run alone.
