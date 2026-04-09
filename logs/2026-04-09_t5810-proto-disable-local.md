# t5810-proto-disable-local

**Date:** 2026-04-09

## Outcome

Harness `./scripts/run-tests.sh t5810-proto-disable-local.sh` reports **54/54** passing. No Rust changes were required on this branch; behavior is already implemented in `grit/src/protocol.rs` (GIT_ALLOW_PROTOCOL, protocol.*.allow, protocol.allow, GIT_PROTOCOL_FROM_USER) and clone/fetch path handling.

## Actions

- Ran release build and harness for `t5810-proto-disable-local.sh` — full pass.
- Updated `PLAN.md` to mark the file complete.
- Updated `progress.md` counts and "Recently completed" entry.
- `data/test-files.csv` / dashboards already showed 54/54 from prior runs; harness re-run refreshed them.
