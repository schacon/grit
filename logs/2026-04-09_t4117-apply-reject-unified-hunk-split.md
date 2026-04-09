## t4117-apply-reject

- **Issue:** `git apply --reject` failed tests 5–7 because `grit diff` produced one giant hunk while Git/xdiff emits three hunks for the same change. A single failed hunk rejected the whole patch instead of applying the middle hunk (insert `C`) and rejecting the others.
- **Fix:** In `grit-lib` `unified_diff_with_prefix_and_funcname_and_algorithm`, pass `group_diff_ops` a radius of `ceil((2*context + inter_hunk) / 2)` instead of `2*context + inter_hunk`, matching xdiff’s `max_common` merge rule (`xdl_get_hunk` in `git/xdiff/xemit.c`).
- **Verify:** `./scripts/run-tests.sh t4117-apply-reject.sh` → 8/8; `cargo test -p grit-lib --lib` → pass.
