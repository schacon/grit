# Phase 5 rev-list wave (5.1/5.2/5.3/5.5)

## Scope

- Implemented a coherent `gust rev-list` subset covering:
  - commit range parsing (`^A`, `A..B`)
  - `--all` (including detached `HEAD`)
  - `--stdin` revision ingestion (with `--not` and `--all`)
  - walk controls: `--first-parent`, `--ancestry-path`, `--simplify-by-decoration`
  - ordering: default date order, `--topo-order`, `--date-order`, `--reverse`
  - limits: `--max-count`, `-n`, `-<n>`, `--skip`, `--count`
  - formatting: default OID, `--parents`, `--format=%s/%H/%h`, `--quiet`

## Files changed for implementation

- `gust-lib/src/rev_list.rs` (new)
- `gust-lib/src/lib.rs` (module export)
- `gust/src/commands/rev_list.rs` (CLI wiring and option parsing)

## Ported test scripts

- `tests/t6000-rev-list-misc.sh`
- `tests/t6003-rev-list-topo-order.sh`
- `tests/t6005-rev-list-count.sh`
- `tests/t6006-rev-list-format.sh`
- `tests/t6014-rev-list-all.sh`
- `tests/t6017-rev-list-stdin.sh`

## Validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)
- New rev-list scripts run directly with isolated `TRASH_DIRECTORY` -> PASS

## Notes

- Test scripts were adapted to use existing Gust plumbing commands (not porcelain):
  graph creation is done with `commit-tree` + `update-ref`.
- `--simplify-by-decoration` is validated against decorated refs in the selected subset.
- Deferred rev-list features remain documented in `plan.md` item **5.4**.
