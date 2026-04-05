# 2026-04-05 — t1051-large-conversion

## Scope
- Target file: `tests/t1051-large-conversion.sh`
- Initial status: `8/12` passing (4 failing)
- Goal: make `t1051-large-conversion` fully pass without modifying tests.

## Failure observed
Running `bash tests/t1051-large-conversion.sh` failed cases 7-10 with:

```
error: '055c8729cdcc372500a08db659c045e16c4409fb' is not a commit-ish
```

All failing cases were output-conversion checks that call:

```
git checkout small large
```

with no `--` separator.

## Root cause
`checkout` path-mode dispatch incorrectly treated the first token (`small`) as a
tree-ish source whenever multiple positional args were present. This worked only
when that token was commit-ish. In this test, `small` is a pathspec, so checkout
entered source-tree restore mode and tried to resolve blob OIDs as commit-ish,
producing the error above.

Git behavior here: without `--`, if the first token is **not** commit-ish, treat
all tokens as pathspecs (`checkout -- <paths...>` semantics).

## Fix implemented
File changed: `grit/src/commands/checkout.rs`

1. In Case 2 (`checkout [<tree-ish>] -- <paths>` / path-restore path):
   - Added logic to compute:
     - `is_commitish = resolve_to_commit(&repo, t).is_ok()`
     - `is_path = pathspec_exists_in_worktree_or_index(&repo, t)`
   - Preserved ambiguity diagnostic when both are true.
   - Added fallback behavior:
     - if `!is_commitish`, reinterpret as path mode by prepending `target` to
       pathspecs and using `source_spec = None`.

2. Added helper:
   - `pathspec_exists_in_worktree_or_index(repo, spec) -> bool`
   - Checks worktree path existence and index exact/prefix matches for stage-0.

## Validation
- `cargo fmt && cargo build --release -p grit-rs` ✅
- `bash tests/t1051-large-conversion.sh` ✅ `12/12` pass (`2` skipped by prereq)
- `./scripts/run-tests.sh t1051-large-conversion.sh` ✅ `12/12` pass
- `cargo test -p grit-lib --lib` ✅ `96/96` pass

## Tracking updates
- `PLAN.md`: marked `t1051-large-conversion` as complete (`12/12`).
- `data/file-results.tsv`: updated by run-tests cache refresh.
- `progress.md`: updated counts to Completed `72`, Remaining `695`, Total `767`.
