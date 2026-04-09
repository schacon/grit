# t3309-notes-merge-auto-resolve

## Goal

Make `tests/t3309-notes-merge-auto-resolve.sh` pass (31/31).

## Changes

- `grit/src/commands/notes.rs`:
  - Implemented `union` and `cat_sort_uniq` merge strategies (`combine_notes_concatenate` / `combine_notes_cat_sort_uniq` parity with Git’s `notes.c`).
  - Successful non-trivial notes merges now write merge commits with **two parents** (`local`, `remote`), matching `git/notes-merge.c`.
  - Split strategy parsing: CLI `--strategy` → `unknown -s/--strategy: …`; config `notes.mergeStrategy` → two-line message expected by the test.
- `PLAN.md`, `progress.md`, harness CSV/dashboards updated after `./scripts/run-tests.sh t3309-notes-merge-auto-resolve.sh`.

## Verification

- `cd /workspace/tests && sh t3309-notes-merge-auto-resolve.sh` — all pass
- `./scripts/run-tests.sh t3309-notes-merge-auto-resolve.sh` — 31/31
- `cargo test -p grit-lib --lib` — pass
- `cargo check -p grit-rs` — pass

## Note

Full-workspace `cargo clippy -D warnings` still reports many pre-existing issues; not used as gate for this change.
