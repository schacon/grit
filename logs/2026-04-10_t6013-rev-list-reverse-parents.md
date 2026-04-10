# t6013 rev-list --reverse --parents

## Issue

`git rev-list --boundary --reverse --parents` must match reversing the full
non-reverse output (commits + boundary lines). Grit collected boundary parents
after reversing commits, so boundary order stayed in forward-discovery order and
the boundary test in `t6013-rev-list-reverse-parents.sh` failed.

## Fix

In `grit-lib` `rev_list()`:

- Compute `boundary_commits` from the pre-`--reverse` commit order (same as
  before, but moved before `ordered.reverse()`).
- When `options.reverse`, reverse `boundary_commits` as well so the CLI prints
  boundary lines first with multiple boundaries in reversed order.

## Validation

Run locally:

- `cargo fmt && cargo clippy -p grit-lib -- -D warnings`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t6013-rev-list-reverse-parents.sh`
