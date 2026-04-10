# t3432-rebase-fast-forward

## Summary

Made all `test_expect_success` cases in `tests/t3432-rebase-fast-forward.sh` pass. Six `test_expect_failure` rows remain (upstream Git documents fork-point edge cases); harness reports `219/225` with exit 0.

## Code changes

- **grit-lib `merge_base`**: Added `merge_base_fork_point`, `fork_point_reflog_ref` (shared with `merge-base --fork-point`).
- **`grit merge-base`**: Delegates fork-point logic to the library.
- **`grit rebase`**:
  - `replay_upstream_oid` from `merge-base --fork-point` when fork-point mode applies; honors `rebase.forkPoint` / `rebase.forkpoint` and Git’s default rules (explicit upstream/`--onto`/`--keep-base` vs implied `@{upstream}`).
  - `upstream_explicit` so default `@{upstream}` still uses fork-point.
  - `--keep-base` accepts repeats (clap count).
  - State: `upstream` file, `force-rewrite`, internal child pick via `GRIT_INTERNAL_REBASE_PICK_LINE` + `GRIT_INTERNAL_REBASE_FORCE_REWRITE`.
  - Replay: subprocess per pick for isolation; cleared `committer_raw` when rewriting committer; `format_committer_ident_now` + pick index for distinct commits when needed.
  - `--no-ff` noop fast-path: skip rewriting when single pick, `upstream == onto`, orig tip equals picked commit (matches Git for same-HEAD cases while still rewriting when upstream advanced, e.g. `--keep-base` after F on main).

## Tests

- `./scripts/run-tests.sh t3432-rebase-fast-forward.sh` → `219/225`, exit 0.
- `cargo test -p grit-lib --lib` — pass.
