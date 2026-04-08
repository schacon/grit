# t12300 rev-list merge / default ordering

## Problem

`grit rev-list` used a global sort by committer date over the reachable set. That can list an ancestor before a descendant when the ancestor has a later timestamp (e.g. merge commits with default `git merge` author/committer dates). That broke t12300 expectations: HEAD not first, `^` exclusion checks, `--skip` / `--max-count`, and root `%P`.

## Fix

Replaced default ordering with a Git-style walk: max-heap by committer time among "ready" commits; a parent is ready only after all selected children pointing to it have been emitted (`date_order_walk` in `grit-lib/src/rev_list.rs`).

## Validation

- `./scripts/run-tests.sh t12300-rev-list-merge-left-right.sh` → 33/33
- `cargo test -p grit-lib --lib`
