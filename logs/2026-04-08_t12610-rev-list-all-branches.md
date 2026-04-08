# t12610-rev-list-all-branches

## Symptom

Harness reported 1/32 passing. Verbose run showed `cd: repo: No such file or directory` on test 2+.

## Root cause

First test runs `grit init repo && cd repo && …` in the main shell. This harness evaluates `test_expect_success` bodies in the persistent shell without resetting cwd between tests (unlike upstream Git’s test-lib, which runs each body in a subshell). The shell therefore stayed in `repo/`, so `(cd repo && …)` looked for `repo/repo` and failed. `rev-list --all` behavior was already correct once cwd was fixed.

## Fix

Append `cd ..` at the end of the setup block in `tests/t12610-rev-list-all-branches.sh` so subsequent tests start from the trash directory root.

## Verification

`./scripts/run-tests.sh t12610-rev-list-all-branches.sh` → 32/32 pass.
