# t2016-checkout-patch

## Summary

Fixed `grit checkout -p` to match Git’s interactive checkout patch behavior so `tests/t2016-checkout-patch.sh` passes (19/19).

## Changes

- Disambiguate first positional when it is only a path (e.g. `checkout -p dir`), not a revision; keep ambiguous rev+path error when both apply.
- Default `checkout -p`: diff index vs worktree; `HEAD` / `@`: diff `HEAD^{tree}` vs worktree with correct prompts; other tree-ish: `resolve_revision` + `peel_to_tree` (empty tree, etc.).
- Write patch UI to stdout so `test_grep "Discard" output` works.
- For `HEAD`/`@`, apply selected hunks via subprocess `grit apply --check` / `--cached --check` then apply (mirrors Git `apply_for_checkout`), including “does not apply to the index” + worktree-only fallback prompt.
- Non-`HEAD` tree-ish modes still use line-blending + index update when staged content differs from the source tree.

## Validation

- `./scripts/run-tests.sh t2016-checkout-patch.sh` — all pass
- `cargo test -p grit-lib --lib`
