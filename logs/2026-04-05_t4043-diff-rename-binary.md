# t4043-diff-rename-binary

- Timestamp: 2026-04-05 21:27:25 CEST
- Scope: Fix the remaining failing upstream test in `git/t/t4043-diff-rename-binary.sh`.

## Findings

- The failing case was `git show -C -C --raw --binary --numstat`.
- `grit show` emitted raw rename lines and the patch together, while upstream expects `--numstat` output plus the patch, without raw lines.
- The root cause was `show_commit()` treating `--raw` as additive and leaving patch output enabled too broadly.

## Change

- Updated `grit/src/commands/show.rs` so summary formats suppress the default patch unless explicitly re-enabled.
- Made `--numstat` take precedence over `--raw` for this combination.
- Kept patch output when explicitly requested or implied by `--binary`, `--patch-with-raw`, or `--patch-with-stat`.

## Verification

- `CARGO_TARGET_DIR=/tmp/grit-build-t4043 bash scripts/run-upstream-tests.sh t4043-diff-rename-binary 2>&1 | tail -40`
- Result: 3/3 passing.
- `CARGO_TARGET_DIR=/tmp/grit-build-t4043 cargo fmt --all 2>/dev/null; true`
