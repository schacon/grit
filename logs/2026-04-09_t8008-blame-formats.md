# t8008-blame-formats

## Symptom

Harness reported 4/5: `--porcelain detects first non-blank line as subject` failed because `git write-tree` under `GIT_INDEX_FILE=.git/tmp-index` produced the empty tree while `git add` had written the alternate index.

## Fix (Grit)

- `grit write-tree` now loads the index from `GIT_INDEX_FILE` when set (same resolution as `git add`), so the synthetic commit in test 5 gets the correct tree and blame can run.

## Fix (harness)

- `scripts/run-tests.sh` clears inherited `test_tick`, `GIT_AUTHOR_DATE`, and `GIT_COMMITTER_DATE` so a stale shell environment does not shift `test_tick()` by an extra 60s per run (breaking blame timestamps in tests 2–4).

## Verification

- `./scripts/run-tests.sh t8008-blame-formats.sh` → 5/5
- `cargo test -p grit-lib --lib` → 160 passed
