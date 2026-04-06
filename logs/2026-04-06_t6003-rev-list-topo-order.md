# t6003-rev-list-topo-order

Date: 2026-04-06

## Baseline

- Harness baseline:
  - `./scripts/run-tests.sh t6003-rev-list-topo-order.sh` → **21/36**
- Direct baseline:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6003-rev-list-topo-order.sh`
  - **22/36** with failures concentrated in:
    - topo-order parent traversal order (`a1`/`c1` positioning),
    - missing option support for `--author-date-order`,
    - missing option support for `--max-age=<ts>`.

## Root causes

1. `--topo-order` implementation used a date-priority heap and did not preserve
   Git-like stack/parent-order semantics.
2. `--author-date-order` was not parsed.
3. `--max-age`/`--min-age` filters were not parsed or applied.

## Changes made

### `grit/src/commands/rev_list.rs`

- Added parsing support for:
  - `--author-date-order` → `OrderingMode::AuthorDate`
  - `--max-age=<unix-ts>` and `--min-age=<unix-ts>`
- Added `parse_i64(...)` helper with integer diagnostics matching existing style.

### `grit-lib/src/rev_list.rs`

- Extended `OrderingMode`:
  - added `AuthorDate`.
- Extended `RevListOptions`:
  - added `max_age: Option<i64>`
  - added `min_age: Option<i64>`
- Added age filtering stage before ordering:
  - keeps commits with `committer_time >= max_age` when set
  - keeps commits with `committer_time <= min_age` when set
- Reworked ordering behavior:
  - `OrderingMode::Topo` now uses stack-like topo traversal:
    - initial ready set sorted ascending by `(committer_time, oid)` and popped from end
    - newly unlocked parents are pushed and popped in LIFO fashion
    - this preserves Git-compatible parent-order-sensitive topo output used by t6003.
  - `OrderingMode::Date` and `OrderingMode::AuthorDate` use a time-priority topo variant (`topo_sort_by_time`) with:
    - committer time for `Date`
    - author time for `AuthorDate`
- Added author-time caching in `CommitGraph` (`author_time` map + accessor).

## Validation

- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6003-rev-list-topo-order.sh`
  - result: **36/36**
- Harness:
  - `./scripts/run-tests.sh t6003-rev-list-topo-order.sh`
  - result: **36/36**

### Targeted regressions

- `./scripts/run-tests.sh t6005-rev-list-count.sh` → **6/6**
- `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → **7/7**
- `./scripts/run-tests.sh t6115-rev-list-du.sh` → **17/17**

## Notes

- One intermediate harness run produced stale `19/36` while direct was green;
  immediate re-run stabilized at `36/36` and TSV now reflects full pass.
