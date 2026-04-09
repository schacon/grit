# t3422-rebase-incompatible-options

## Goal

Make `tests/t3422-rebase-incompatible-options.sh` pass (52/52): reject mixing apply-backend options (`--apply`, `-C<n>`, `--whitespace=fix|strip`) with merge-backend options and with `rebase.rebaseMerges` / `rebase.updateRefs` unless overridden.

## Changes

- `grit/src/commands/rebase.rs`: Added `--empty`, `-s/--strategy`, `-X/--strategy-option`, `--no-rebase-merges`, `--no-update-refs`. Implemented Git-style `validate_apply_merge_backend_combo` after loading config and computing `want_autosquash`. `choose_rebase_backend` uses `apply_backend_forced()` so `-C4` / whitespace fix selects apply. Fixed `reapply_cherry_picks` default to match Git when neither `--reapply` nor `--no-reapply` is given (default off unless `--keep-base`). `preprocess_rebase_argv`: split glued `-C<n>` into `-C` + `n` for clap.
- `grit/src/commands/pull.rs`: Extended manual `rebase::Args { ... }` with new fields.

## Verification

- `cargo build --release -p grit-rs`
- `GUST_BIN=... bash tests/t3422-rebase-incompatible-options.sh` — 52/52 pass
- `cargo test -p grit-lib --lib` — pass
- `./scripts/run-tests.sh t3422-rebase-incompatible-options.sh` — refresh CSV/dashboards
