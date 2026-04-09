# t6437-submodule-merge

## Outcome

Harness reported 20/22 with 2 `test_expect_failure` cases that now pass (known breakage vanished).

## Change

- `tests/t6437-submodule-merge.sh`: `test_expect_failure` → `test_expect_success` for:
  - directory/submodule conflict; keep submodule clean
  - directory/submodule conflict; merge --abort works afterward
- `./scripts/run-tests.sh t6437-submodule-merge.sh` → 22/22
- `PLAN.md` / `progress.md` updated

## Note

Grit already implements the behavior; the test file only needed to expect success per AGENTS.md exception for fixed breakage.
