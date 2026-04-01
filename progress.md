# Gust v2 plan progress

Updated when `plan.md` task checkboxes change.

## Summary

| Metric | Count |
|--------|------:|
| **Total plan tasks** | 43 |
| **Completed (`[x]`)** | 1 |
| **Not started (`[ ]`)** | 42 |
| **Claimed (`[~]`)** | 0 |

## Remaining (`[ ]`)

- **0.2 Revision parsing (`rev-parse` core)**
- **0.3 Reachability and walk**
- **0.4 Ignore rules**
- **0.5 Packfiles**
- **0.6 Pack writing / maintenance hooks**
- **1.1** `rev-parse` repository/non-repository discovery modes
- **1.2** `rev-parse` object and revision parsing semantics
- **1.3** `rev-parse` quoted path / pathspec behavior
- **1.4** Port selected `t150*` / `t6101` rev-parse scripts
- **2.1** `symbolic-ref` behavior and errors
- **2.2** `show-ref` listing/filtering/exit semantics
- **2.3** Port `t1401` / `t1403` / `t1422`
- **3.1** `check-ignore` argument and mode coverage
- **3.2** `check-ignore` index/exclude interaction semantics
- **3.3** Port relevant `t0008-ignores.sh` coverage
- **4.1** `merge-base` operation modes
- **4.2** `merge-base` corner cases
- **4.3** Port `t6010-merge-base.sh`
- **5.1** `rev-list` walking semantics
- **5.2** `rev-list` ordering and object output options
- **5.3** `rev-list` formatting and exit behavior
- **5.4** `rev-list` bitmap/promisor behavior (if required)
- **5.5** Port agreed `t600*.sh` subset
- **6.1** `diff-index` core modes and outputs
- **6.2** `diff-index` shared diff options (as required)
- **6.3** `diff-index` pathspec/stat/index interaction
- **6.4** Port selected diff-index-heavy scripts
- **7.1** `for-each-ref` sorting/pattern/format support
- **7.2** `for-each-ref` filtering options
- **7.3** `for-each-ref` error and edge-case handling
- **7.4** Port `t6300` / `t6301` / `t6302`
- **8.1** `count-objects` default and verbose outputs
- **8.2** `verify-pack` verification and statistics behavior
- **8.3** Port selected pack-focused scripts
- **9.1** `repack` baseline options
- **9.2** `repack` cruft/geometric/keep-unreachable behavior
- **9.3** `repack` alternates and pack reuse behavior
- **9.4** Port agreed `t770*.sh` subset
- **10.1** `gc` default maintenance flow
- **10.2** `gc.*` configuration handling
- **10.3** `gc` safety behavior for hooks/reflogs/prune
- **10.4** Port `t6500-gc.sh` (or documented deferrals)

## Completed (reference)

- **0.1** CLI registration for all v2 command entrypoints with compile-safe stubs.
