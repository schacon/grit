# t6012-rev-list-simplify

## Summary

- Fixed `git commit -a` after merge conflicts: `auto_stage_tracked` uses `stage_file` and skips the OID fast-path when unmerged stages exist; merge commits no longer rejected when resolved tree matches first parent (`parents.len() > 1`).
- Extended `grit-lib` `rev_list`: author timestamps for ordering, `--exclude-first-parent-only`, `--show-pulls`, `--simplify-merges` path simplification (approximation), path graph reorder helpers, `path_graph_reorder` for `--sparse` dense pass.
- Routed non-graph `log` through `rev_list` when path-limited / history flags apply; added `--full-diff`, `--exclude-first-parent-only`, `--show-pulls`, `--ancestry-path=<rev>` via argv scan; `--full-diff` forces separate merge diffs.
- Graph log: pass new rev-list options; simplify merge parents when `--simplify-merges` + pathspecs.

## Test status

`./scripts/run-tests.sh t6012-rev-list-simplify.sh`: **24/42** passing at last run (remaining: path-limited default/dense/simplify-merges ordering and second-repo graph).

## Note

Workspace `cargo clippy -D warnings` fails with many pre-existing grit-lib issues; validated with `cargo check` and `cargo test -p grit-lib --lib`.
