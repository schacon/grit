## Task: t4122-apply-symlink-inside (regression)

### Symptom
Harness reported 5/7 passing: tests 6–7 failed because `git apply` on a concatenated patch (symlink add + header-only new file) succeeded instead of failing with `beyond a symbolic link`.

### Root cause
`git diff` can emit a `diff --git` section with only `index` / mode lines and **no** `---` / `+++` headers when the file is new and empty. In that case `parse_patch` left `old_path` / `new_path` as `a/arch/...` and `b/arch/...`. Symlink safety (`verify_patch_paths_not_beyond_symlink` / `symlink_prefix`) compared those strings to the overlay key `arch/x86_64/dir`, so `arch/x86_64/dir` was not recognized as a prefix of `b/arch/x86_64/dir/file`.

### Fix
After parsing hunks for a `diff --git` block, if `---` was never seen (`!saw_old_header`), strip `-p` from `old_path` / `new_path` with `path_after_strip`, matching the behavior of `find_name_tab_terminated` when headers exist.

### Validation
- `bash tests/t4122-apply-symlink-inside.sh` with `GUST_BIN`: **7/7**
- `./scripts/run-tests.sh t4122-apply-symlink-inside.sh`: **7/7**
- `cargo test -p grit-lib --lib`: pass
