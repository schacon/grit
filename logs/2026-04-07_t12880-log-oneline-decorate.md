# t12880-log-notes-display — oneline decorations

## Problem

Harness expected `grit log --oneline` to print short ref decorations like Git on a TTY, e.g.  
`<abbrev> (HEAD -> master, master) <subject>`. Grit omitted decorations and dropped the duplicate branch name when HEAD pointed at `master`.

## Fix

- Added `resolve_decoration_display()` so `--oneline` / `format:oneline` enables short decorations by default (still overridable with `--no-decorate` / `--decorate=full` and argv order).
- Graph `--graph --oneline` now loads the same decoration map and appends the parenthesized suffix in `render_graph_commit_text`.
- Removed the post-pass in `collect_decorations` that stripped the branch label when it matched HEAD’s branch, so `master` appears alongside `HEAD -> master`.

## Validation

- `./scripts/run-tests.sh t12880-log-notes-display.sh` → 34/34
- `cargo test -p grit-lib --lib`
