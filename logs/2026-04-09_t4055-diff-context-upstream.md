## t4055-diff-context (upstream parity)

- **Issue:** `scripts/run-upstream-tests.sh t4055-diff-context` failed 3/10 while `./scripts/run-tests.sh` passed: `log -p` ignored `diff.context`; invalid `diff.context` values did not produce Git’s fatal messages.
- **Fix:** Added Git-compatible strict int parsing and `resolve_diff_context_lines` in `grit-lib`; `log` now resolves `patch_context` once at startup (matching Git loading `git_diff_ui_config`) and threads it into patch output; `git diff` validates `diff.context` at command start for repo diffs.
- **Verify:** `./scripts/run-tests.sh t4055-diff-context.sh` and `bash scripts/run-upstream-tests.sh t4055-diff-context` → 10/10.
