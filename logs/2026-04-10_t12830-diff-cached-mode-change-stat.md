# t12830-diff-cached-mode-change

## Issue

Test 33 (`mode-only change with diff --cached --stat shows 0 changes`) used `! grep "insertion" actual`. Git (and grit) emit the summary line `1 file changed, 0 insertions(+), 0 deletions(-)`, which contains the substring `insertion`, so the negated grep always failed.

## Fix

Assert the full zero-count summary line instead of forbidding the word "insertion".

## Verification

- `./scripts/run-tests.sh t12830-diff-cached-mode-change.sh` → 38/38 pass.
