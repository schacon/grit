# t5621-clone-revision

## Goal

Make `tests/t5621-clone-revision.sh` pass (12/12) against grit.

## Changes

1. **`tests/test-lib.sh`** — Added `--annotate` to `test_commit` (upstream parity): after commit, run `git tag -a -m "$message"` with optional `test_tick`, matching `git/t/test-lib-functions.sh`.

2. **`grit/src/commands/clone.rs`** — `git clone --revision`:
   - Resolve revision in source with rules aligned to Git transport: `HEAD`, full ref names, DWIM `refs/heads|tags|remotes/<name>`, full 40-char hex only as raw OID; reject `^`/`~` and ambiguous short hex with `fatal: Remote revision … not found in upstream origin`.
   - Peel annotated tags to a commit; tree/blob → `error: object <id> is a tree|blob, not a commit` (single `error:` prefix from main).
   - After resolution: remove everything under `refs/`, write detached `HEAD`, remove `remote.<name>.fetch` and `branch.*` tracking sections for that remote.
   - Run `write_shallow_boundary` **after** applying `--revision` so `--depth` uses the final HEAD.

## Verification

- `cargo fmt`, `cargo clippy --fix --allow-dirty` (pre-existing warnings in `main.rs` unchanged)
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5621-clone-revision.sh` → 12/12
