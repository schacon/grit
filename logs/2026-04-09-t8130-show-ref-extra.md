# t8130-show-ref-extra

- **Issue:** Tests failed because the simplified TAP harness never reset `cwd` to `$TRASH_DIRECTORY` between cases. Test 1 ended in `repo/`, so later tests ran `cd repo` from the wrong directory and hit the wrong (or no) repository.
- **Fix:** `tests/test-lib-tap.sh` — `cd "$TRASH_DIRECTORY"` before each `test_run_` in `test_expect_success` / `test_expect_failure` (matches upstream git test-lib behavior).
- **Extra:** `collect_loose_refs` / `collect_refs` now use `fs::metadata` on the path so symlinked loose refs are followed (`DirEntry::file_type` does not follow symlinks on Unix).

Verification: `./scripts/run-tests.sh t8130-show-ref-extra.sh` → 31/31.
