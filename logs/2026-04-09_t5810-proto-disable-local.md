# t5810-proto-disable-local

## Goal

Make harness `t5810-proto-disable-local.sh` fully pass (local protocol allow/deny + dash-prefixed repo paths).

## Failure

Test 53: `git fetch -- -repo.git` must fail **before** `upload-pack` runs so `GIT_TRACE` contains no `upload-pack` line. Grit was resolving `-repo.git` to a valid path and negotiating.

## Fix

- Added `grit/src/transport_path.rs`: `check_local_url_path_not_option_like` — if the path part (after optional `file://`, before `?`) starts with `-`, bail with Git’s message `fatal: strange pathname '…' blocked`.
- Call sites: `fetch_remote` after URL resolution (`fetch.rs`); local `clone` path before `open_source_repo` (`clone.rs`).
- Registered module in `main.rs`.

## Verification

- `./scripts/run-tests.sh t5810-proto-disable-local.sh` → 54/54
- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`
