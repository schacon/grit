## t4055-diff-context — `run_diff_two_paths` parity

- **Context:** Harness `t4055-diff-context.sh` was already 10/10; `run_diff_two_paths` (in-repo path vs other path) still read `diff.context` via loose `parse()` and silently fell back to 3 on invalid values, unlike the main `git diff` path.
- **Change:** Use `ConfigSet::load` + `resolve_diff_context_lines` so invalid/negative `diff.context` fails with the same fatal messages as repo diffs.
- **Verify:** `cargo test -p grit-lib --lib`, `./scripts/run-tests.sh t4055-diff-context.sh` → 10/10.
