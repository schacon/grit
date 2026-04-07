# t0450-txt-doc-vs-help

## Goal

Make `tests/t0450-txt-doc-vs-help.sh` fully pass: `-h` synopsis must match vendored `git/Documentation/git-<cmd>.adoc` for builtins Grit lists.

## Changes

- `scripts/generate-upstream-help-synopsis.py`: extract `[verse]` / `[synopsis]` blocks from `git/Documentation/*.adoc` (same rules as the test), apply merge-tree `(deprecated)` strip.
- `grit/build.rs`: run the script at build time into `OUT_DIR/upstream_help_synopsis.rs`.
- `grit/src/main.rs`: for lone `-h` / `--help`, print Git-style `usage:` / `   or:` lines with space padding for continuations; split adoc into variants on each line starting with `git `; special path for `grep -h` before clap.
- `tests/test-lib.sh`: default `GIT_SOURCE_DIR` to `<repo>/git` when unset.
- `scripts/run-tests.sh`: export `GIT_SOURCE_DIR` for harness runs.
- `tests/t0450/adoc-help-mismatches`: removed builtins that now agree; left only commands not in Grit’s builtin list (expected skips).

## Verification

`./scripts/run-tests.sh t0450-txt-doc-vs-help.sh` → 548/548.
