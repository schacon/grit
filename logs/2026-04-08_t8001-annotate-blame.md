# t8001-annotate / blame work (2026-04-08)

## Done

- Added missing `tests/annotate-tests.sh` (upstream copy) so `t8001-annotate.sh` can source it.
- Blame/annotate: peel tags to commits; git-style `file rev` argument order; `-h rev` handling in annotate; merge blame via sequential parents + deferred queue fix; `.git/info/grafts` in blame walk; `--contents`, `--progress`, annotate tab output; extended `-L` parsing (regex, `^/`, `:funcname` via userdiff).
- `checkout --orphan` without start: keep index/worktree (matches git) for graft/orphan loops.
- `commit -a`: skip re-staging when worktree blob OID matches index (fixes false "nothing to commit" on newline-only or identical content).

## Harness

`./scripts/run-tests.sh t8001-annotate.sh` → **97/117** passing (was 0/117 before `annotate-tests.sh` + fixes).

## Remaining failures (~20)

Mostly `-L` edge cases with incomplete final lines, empty-file ranges, and a few `:regex` / relative-range subtleties; possible `commit -a` + `test_tick` interaction in full harness for graft test.
