# Gust v2 plan progress

Updated when `plan.md` task checkboxes change.

## Summary

| Metric | Count |
|--------|------:|
| **Total plan tasks** | 43 |
| **Completed (`[x]`)** | 20 |
| **Not started (`[ ]`)** | 23 |
| **Claimed (`[~]`)** | 0 |

## Remaining (`[ ]`)

- **0.2 Revision parsing (`rev-parse` core)**
- **0.3 Reachability and walk**
- **0.4 Ignore rules**
- **0.5 Packfiles**
- **0.6 Pack writing / maintenance hooks**
- **1.3** `rev-parse` quoted path / pathspec behavior
- **5.1** `rev-list` walking semantics
- **5.2** `rev-list` ordering and object output options
- **5.3** `rev-list` formatting and exit behavior
- **5.4** `rev-list` bitmap/promisor behavior (if required)
- **5.5** Port agreed `t600*.sh` subset
- **6.1** `diff-index` core modes and outputs
- **6.2** `diff-index` shared diff options (as required)
- **6.3** `diff-index` pathspec/stat/index interaction
- **6.4** Port selected diff-index-heavy scripts
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
- **1.1** `rev-parse` repository/non-repository discovery modes (`--is-inside-work-tree`, `--show-toplevel`, `--git-dir`, `--show-prefix`) implemented and validated with `t1500` subset.
- **1.2** `rev-parse` revision/object parsing in scope (`--verify`, `--default`, `--short`, `--end-of-options`, full/abbrev OIDs, `^{}`/`^{commit}` peeling) implemented and validated with `t1503` subset.
- **1.4** Ported rev-parse scripts: `tests/t1500-rev-parse.sh` and `tests/t1503-rev-parse-verify.sh`.
- **2.1** `symbolic-ref` read/create/delete support with target validation and quiet non-symbolic exit behavior.
- **2.2** `show-ref` pattern listing, `--branches`/`--tags`, `--verify`, `--exists`, `--dereference`, and `--hash`.
- **2.3** Ported and passing `tests/t1401-symbolic-ref.sh`, `tests/t1403-show-ref.sh`, and `tests/t1422-show-ref-exists.sh`.
- **3.1** `check-ignore` path-argument and stdin modes (`--stdin`, `-z`, `-v`, `-n`, `--no-index`) implemented for the selected `t0008` subset.
- **3.2** `check-ignore` precedence and semantics for `.gitignore`, `.git/info/exclude`, `core.excludesfile`, tracked-vs-untracked handling, and directory rules.
- **3.3** Ported and passing `tests/t0008-ignores.sh` subset (12/12).
- **4.1** `merge-base` operation modes implemented: default, `--all`, `--octopus`, `--independent`, and `--is-ancestor`.
- **4.2** `merge-base` corner cases implemented: disjoint histories, root ancestry graphs, and repeated commit arguments.
- **4.3** Ported and passing `tests/t6010-merge-base.sh` subset for merge-base behavior.
- **8.1** `count-objects` default and `-v` output implemented, including pack totals, prune-packable counting, garbage accounting, and recursive alternate listing.
- **8.2** `verify-pack` implemented for `.idx`/`.pack` normalization, pack/index validation, `-v` object enumeration, histogram output, and exit-code signaling on bad input.
- **8.3** Ported and passing `tests/t5301-sliding-window.sh`, `tests/t5304-prune.sh`, and `tests/t5613-info-alternate.sh` subsets.
- **7.1** `for-each-ref` listing support implemented for `--sort`, patterns, `--exclude`, `--count`, `--stdin`, and covered format atoms.
- **7.2** `for-each-ref` filtering implemented for `--points-at`, `--merged`/`--no-merged`, and `--contains`/`--no-contains`.
- **7.3** `for-each-ref` error handling implemented for broken refs, zero-OID refs, and missing object behavior across default vs objectname-only formats.
- **7.4** Ported and passing `tests/t6300-for-each-ref.sh`, `tests/t6301-for-each-ref-errors.sh`, and `tests/t6302-for-each-ref-filter.sh`.
