# t6422-merge-rename-corner-cases

- Grit already passed all 26 scenarios; harness exited non-zero because six cases were still marked `test_expect_failure` (Git "known breakage vanished").
- Flipped those six to `test_expect_success` in `tests/t6422-merge-rename-corner-cases.sh`.
- `./scripts/run-tests.sh t6422-merge-rename-corner-cases.sh` reports 26/26.
