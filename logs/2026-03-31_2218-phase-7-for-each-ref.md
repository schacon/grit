# Phase 7: for-each-ref (7.1/7.2/7.3/7.4)

## Scope

- Implement a coherent `for-each-ref` subset in `grit/src/commands/for_each_ref.rs`.
- Port selected coverage from upstream `t6300`, `t6301`, and `t6302` into local `tests/`.

## Implemented command behavior

- Argument parsing for:
  - `--count`
  - `--format`
  - repeated `--sort`
  - repeated `--exclude`
  - `--stdin`
  - `--ignore-case`
  - `--points-at`
  - `--merged` / `--no-merged` (with optional commit-ish)
  - `--contains` / `--no-contains` (with optional commit-ish)
- Ref enumeration from loose refs and packed-refs.
- Broken loose-ref handling:
  - warn and ignore invalid or zero-OID refs.
- Format expansion atoms covered by tests:
  - `%(refname)`, `%(refname:short)`, `%(objectname)`, `%(objecttype)`, `%(subject)`, `%(*subject)`.
- Missing-object behavior:
  - object-dependent formats fail with a fatal missing-object error.
  - objectname-only formats still list refs.
- Filter semantics implemented for:
  - points-at target object
  - merged / no-merged relative to commit-ish
  - contains / no-contains relative to commit-ish.

## Ported tests

- Added `tests/t6300-for-each-ref.sh` (basic behavior subset).
- Added `tests/t6301-for-each-ref-errors.sh` (error handling subset).
- Added `tests/t6302-for-each-ref-filter.sh` (filter behavior subset).
- Added all three scripts to `tests/harness/selected-tests.txt`.

## Validation

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)
- `tests/t6300-for-each-ref.sh` -> PASS (6/6)
- `tests/t6301-for-each-ref-errors.sh` -> PASS (5/5)
- `tests/t6302-for-each-ref-filter.sh` -> PASS (7/7)

## Notes

- The upstream `t630*` files are extensive; this port intentionally targets a coherent subset that exercises the implemented behavior thoroughly without depending on unsupported porcelain commands.
