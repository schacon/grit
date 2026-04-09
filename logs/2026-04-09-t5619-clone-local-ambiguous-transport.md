# t5619-clone-local-ambiguous-transport

**Date:** 2026-04-09

## Outcome

Harness `./scripts/run-tests.sh t5619-clone-local-ambiguous-transport.sh` reports **2/2** passing on current `grit` (branch `cursor/t5619-ambiguous-transport-139c`). No Rust changes were required; the test verifies that a malicious `.gitmodules` URL that resolves to an ambiguous local path does not allow reading arbitrary files via submodule init, and that failure matches the expected `protocol .* is not supported` stderr pattern.

## Actions

- Ran release build and harness for `t5619-clone-local-ambiguous-transport.sh`.
- Refreshed `data/test-files.csv` and dashboards via `run-tests.sh`.
- Marked task complete in `PLAN.md`, updated counts in `progress.md`.
