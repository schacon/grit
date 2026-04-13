# t4051-diff-function-context

## Summary

Implemented `git diff -W` / `--function-context` to expand unified-diff hunks to whole logical functions (aligned with Git `XDL_EMIT_FUNCCONTEXT` in `xemit.c`).

## Changes

- **`grit`**: `-W` / `--function-context` on `diff` Args; trailing-arg parser no longer treats `-W` as a revision; `--no-index` directory diff passes the flag through.
- **`grit-lib`**: `unified_diff_with_prefix_and_funcname_and_algorithm(..., function_context)` delegates to a new path that:
  - Computes per-hunk ranges from grouped ops + hunk headers
  - Expands pre/post context to funcname boundaries (userdiff matcher or Git default)
  - Handles EOF appends (skip preimage pull when appropriate; `old_index == n_old` detection)
  - Maps old slice bounds to new file via full-file `DiffOp` walk (`map_old_line_to_new`)
  - Re-diff slices with large inner context; shifts `@@` line numbers; re-enriches funcname from full-file lines
  - Preserves trailing newline on slice join to avoid spurious `\\ No newline at end of file`

## Validation

- `./scripts/run-tests.sh t4051-diff-function-context.sh` → 42/42
- `cargo test -p grit-lib --lib` → pass
