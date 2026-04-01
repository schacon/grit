# Phase 8.1/8.2/8.3 - count-objects and verify-pack

## Scope

- Implement a coherent v2 subset for:
  - `gust count-objects`
  - `gust verify-pack`
- Read and align with upstream:
  - `git/builtin/count-objects.c`
  - `git/builtin/verify-pack.c`
  - `git/Documentation/git-count-objects.adoc`
  - `git/Documentation/git-verify-pack.adoc`
- Port matching tests from:
  - `git/t/t5301-sliding-window.sh`
  - `git/t/t5304-prune.sh`
  - `git/t/t5613-info-alternate.sh`

## Implementation notes

- Added `gust-lib/src/pack.rs` with:
  - v2 `.idx` parser (`read_pack_index`)
  - local pack aggregate metrics (`collect_local_pack_info`)
  - pack verification and object record extraction (`verify_pack_and_collect`)
  - recursive alternate discovery (`read_alternates_recursive`)
- Exported new module in `gust-lib/src/lib.rs`.
- Replaced CLI stubs:
  - `gust/src/commands/count_objects.rs`
  - `gust/src/commands/verify_pack.rs`
- `count-objects` now supports:
  - default summary (`<n> objects, <kib> kilobytes`)
  - `-v` breakdown lines (`count`, `size`, `in-pack`, `packs`, `size-pack`, `prune-packable`, `garbage`, `size-garbage`, `alternate`)
- `verify-pack` now supports:
  - argument normalization for `.pack` / `.idx` / basename
  - `-v` object listing and chain histogram
  - `-s` stat-only histogram
  - non-zero exit on verification error
  - `--object-format=sha1` compatibility

## Test ports

- Added:
  - `tests/t5301-sliding-window.sh`
  - `tests/t5304-prune.sh`
  - `tests/t5613-info-alternate.sh`
- Added scripts to `tests/harness/selected-tests.txt`.

## Validation

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 passed, 0 failed)
- New scripts:
  - `tests/t5301-sliding-window.sh` -> PASS (3/3)
  - `tests/t5304-prune.sh` -> PASS (2/2)
  - `tests/t5613-info-alternate.sh` -> PASS (3/3)
- `./tests/harness/run.sh` -> FAIL due to unrelated existing `t6300-for-each-ref.sh` failures (3 failing cases).

## Planning docs updates

- `plan.md`: marked 8.1, 8.2, 8.3 as complete.
- `progress.md`: updated task counts and completed list.
- `test-results.md`: added Phase 8 validation block.
