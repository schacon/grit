# t7524-commit-summary

## Goal

Make `tests/t7524-commit-summary.sh` pass: commit summary line must match `git diff --stat` (Myers line counts), while `git diff --stat` and `git diff --stat --break-rewrites` must produce different output for the rewrite scenario.

## Root cause

Grit treated `--break-rewrites` as a no-op for `--stat` / `--numstat`, so `diffstat` and `diffstatrewrite` were byte-identical and the test failed at `! test_cmp_bin diffstat diffstatrewrite`.

## Fix

- `grit-lib`: Port Git `diffcore_count_changes` (span-hash chunks) and `should_break` logic as `should_break_rewrite_for_stat`; add `count_git_lines` matching Git `count_lines` for the complete-rewrite diffstat path.
- `grit diff`: When `--break-rewrites` is set and a modified text pair should break, use full new/old line counts for per-file stat and totals (like Git `builtin_diffstat` with `complete_rewrite`); otherwise keep Myers `count_changes`.

## Verification

- `./scripts/run-tests.sh t7524-commit-summary.sh` → 2/2
- Manual repro in `/tmp`: `cmp` on diffstats differs; `grep "1 file"` lines from commit output vs plain `--stat` match

## Note

Workspace `cargo clippy -D warnings` still fails on many pre-existing issues; this change was validated with `cargo test -p grit-lib --lib` and release build.
