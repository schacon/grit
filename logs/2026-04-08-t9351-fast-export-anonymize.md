# t9351-fast-export-anonymize

- Added `grit-lib::fast_export` with anonymized `--all` export (topo + reverse), marks, tree diffs, tags, ref-source propagation, path-component anonymize-map (filename stem → seed).
- `fast-import`: `feature done`, `tag` command, required trailing `done` when negotiated.
- `log`: symmetric range (`A...B`) with `--left-right` / `--boundary` and `%m`; peel tags in `WalkCommitsIter` for `log --all`.
- `ls-tree`: default `tree_ish` to `HEAD` when omitted.

Harness: `./scripts/run-tests.sh t9351-fast-export-anonymize.sh` → 17/17.
