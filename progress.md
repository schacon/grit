# Gust v1 plan progress

Updated when `plan.md` task checkboxes change.

## Summary

| Metric | Count |
|--------|------:|
| **Total plan tasks** | 54 |
| **Completed (`[x]`)** | 45 |
| **Not started (`[ ]`)** | 9 |
| **Claimed (`[~]`)** | 0 |

## Remaining (`[ ]`)

1. **7.2** — `read-tree`: `-m` two-tree and three-tree merge rules (trivial, non-trivial, conflicts).
2. **7.3** — `read-tree`: `-u` / `--reset` integration with working tree.
3. **7.4** — `read-tree`: `--prefix`, aggressive / trivial merge driver flags as tests need.
4. **7.5** — Port merge tests in dependency order (new index → overlay → 2-way → 3-way → edge cases).
5. **11.1** — Port `t1020-subdirectory.sh` (plumbing from subdirs).
6. **11.2** — Port `t0000-basic.sh` incrementally / by groups.
7. **11.3** — Manpage / behavior parity checklist per v1 command.
8. **11.4** — Logs: one timestamped file under `logs/` per claimed task (AGENT.md).
9. **11.5** — Final sweep: every `[x]` has tests green under `./tests` with `gust` as git substitute.

## Completed (reference)

Phases **0–6** complete; **7.1** complete; **8.x** complete; **9.x** complete; **10.x** complete.
