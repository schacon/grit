# t12000-rev-list-topo-order

**Date:** 2026-04-09

## Goal

Ensure `tests/t12000-rev-list-topo-order.sh` passes (34 tests): `rev-list` with `--topo-order`, `--reverse`, `--count`, `--max-count`, `--skip`, ranges, `--all`, `--first-parent`, `--date-order`, branch tips.

## Result

`./scripts/run-tests.sh t12000-rev-list-topo-order.sh` → **34/34** on branch `cursor/t12000-rev-list-topo-order-7ab3` at `737de55e`. No Rust changes required; behavior already matches Git for this subset.

## Follow-up

Upstream-scale `t6003-rev-list-topo-order` in `PLAN.md` remains partially passing (separate, larger test file).
