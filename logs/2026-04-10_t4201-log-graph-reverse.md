# t4201-log-graph: `--graph` + `--reverse`

## Issue

Harness `tests/t4201-log-graph.sh` failed on `log --graph --reverse --oneline` because grit rejected the combination (matching upstream Git’s error).

## Change

- Removed the `--reverse` + `--graph` conflict checks in `log` (main path and `-L` line-log path).
- In `run_graph_log`, set `RevListOptions.reverse` from `args.reverse` so `rev_list` applies the same skip/max/reverse ordering as non-graph log.

## Verification

- `./scripts/run-tests.sh t4201-log-graph.sh` → 23/23 pass; `data/test-files.csv` + dashboards updated.
