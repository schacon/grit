# t2101-update-index-reupdate — `update-index --again`

## Problem

Harness showed 4/7: failures on `--again` without `--remove`, reupdate from a subdirectory, and pathspec `dir1/`.

Root cause: `--again` was implemented as “refresh when mtime/size differed from the index”, not Git’s `do_reupdate` (only touch entries that differ from **HEAD**, respect cwd prefix for pathspecs, exit 128 on missing paths without `--remove`).

## Fix

- Early-return when `--again`: trailing paths are **pathspecs**, not immediate update targets.
- Resolve HEAD → commit tree; walk index stage-0 entries; skip entries whose mode+OID match HEAD tree at that path (or update all when no HEAD / unborn).
- Apply pathspecs with cwd-relative prefix (same idea as `PATHSPEC_PREFER_CWD`).
- Re-stat + rehash from work tree; `--remove` removes missing paths; submodule gitlinks updated when `.git` exists.

## Validation

- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t2101-update-index-reupdate.sh` → 7/7
- `cargo test -p grit-lib --lib`
