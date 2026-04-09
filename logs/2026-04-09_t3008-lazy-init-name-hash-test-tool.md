## 2026-04-09 — t3008 lazy-init name hash / test-tool

- Added `grit-lib::index_name_hash_lazy` implementing Git-compatible `memihash`, sequential and multi-threaded directory/name hash build (mirrors `name-hash.c` thresholds: `LAZY_THREAD_COST`, `online_cpus`-style parallelism).
- Wired `grit test-tool online-cpus` and `test-tool lazy-init-name-hash` in `grit/src/main.rs` (dump/perf/analyze modes aligned with `git/t/helper/test-lazy-init-name-hash.c`).
- `./scripts/run-tests.sh t3008-ls-files-lazy-init-name-hash.sh` → 1/1; `scripts/run-upstream-tests.sh t3008` → 1/1.
