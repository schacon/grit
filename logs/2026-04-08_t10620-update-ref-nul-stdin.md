# t10620-update-ref-nul-stdin

## Symptom

Harness reported 1/30 passing: only the setup test ran from the trash root; later tests used `cd repo` while the shell was still inside `repo/` from the previous block, so `cd repo` failed (no nested `repo/repo`).

## Fix

Prefix each `test_expect_success` body with `cd "$TRASH_DIRECTORY" &&` so every block starts at the trash root. This matches the intent of upstream-style tests that assume a fresh trash cwd per case; our TAP harness intentionally keeps cwd between blocks (see `progress.md` note on t6409-merge-subtree).

## Verification

- `./scripts/run-tests.sh t10620-update-ref-nul-stdin.sh` — 30/30
- `t6409-merge-subtree.sh` — 12/12 (no harness-wide `cd` reset; avoids breaking cwd-carrying tests)
