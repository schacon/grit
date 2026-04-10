# t3452-history-split

- Implemented `grit history split` in `grit/src/commands/history.rs`: interactive hunk selection (similar + `group_diff_ops`), mode-vs-content prompts, pathspec filtering, dry-run ref updates, merge/parent checks matching upstream messages.
- `git refs list --include-root-refs`: HEAD + `*_HEAD` / `FETCH_HEAD` / `ORIG_HEAD` in `for_each_ref.rs`.
- `history`/`reword`: optional commit arg for clearer `command expects a committish` / `single revision` errors; unknown rev maps to `commit cannot be found`.
- Commit: stop mirroring branch reflog onto HEAD (append instead) so `checkout: moving from` survives commits; `launch_commit_editor` runs from work tree; skip broken absolute `GIT_EDITOR` paths; `resolve_commit_editor` only treats ineffective `:` when vars are set.
- Checkout: write HEAD before checkout reflog line so `switch -` works after branch switches.
- Log: `reorder_graph_all_branches_no_explicit_rev` for `log --graph --branches` with no rev args (t3452 graph expectations).
- Harness: `./scripts/run-tests.sh t3452-history-split.sh` → 25/25.
