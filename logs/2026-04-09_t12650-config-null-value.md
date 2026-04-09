# t12650-config-null-value

**Date:** 2026-04-09

## Outcome

`./scripts/run-tests.sh t12650-config-null-value.sh` reports **34/34** passing on branch `cursor/t12650-config-null-value-91db`. No Rust changes were required; implementation already matches Git for null keys (implicit true), empty `key =` values, `--bool`/`--int` parsing, `config -l`, and `--get-regexp` output.

## Verification

- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t12650-config-null-value.sh`
- `cargo test -p grit-lib --lib`

## Follow-up

Harness CSV/dashboards refreshed (`data/test-files.csv`, `docs/index.html`, `docs/testfiles.html`). `t1-plan.md` entry marked complete.
