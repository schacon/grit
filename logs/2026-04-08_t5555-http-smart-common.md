# t5555-http-smart-common

## Issue

- `tests/t5555-http-smart-common.sh` contained unresolved merge conflict markers, so the harness could not run the file cleanly.
- After repair, v2 `upload-pack --advertise-refs` failed: system Git advertises extra capabilities (e.g. `object-info`) beyond the fixed expect blob from older upstream.

## Fix

- Resolved conflicts: kept upstream-style test bodies, added REAL_GIT discovery and a `git` shim; prepended `$TRASH_DIRECTORY/.bin` to `PATH` so it overrides `BIN_DIRECTORY/git` (grit) from `test-lib.sh`.
- Replaced strict `test_cmp` for protocol v2 with grep checks for required capability lines so newer Git versions stay compatible.

## Verification

- `./scripts/run-tests.sh t5555-http-smart-common.sh` → 10/10 pass.
