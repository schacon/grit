## t4042-diff-textconv-caching

### Summary
- Implemented Git-compatible `diff.<driver>.cachetextconv` using `refs/notes/textconv/<driver>` with commit subject = current textconv command string (cache invalidation on command change), matching `git/notes-cache.c`.
- Fixed textconv execution to match Git: write blob to a tempfile and run `sh -c 'pgm "$@"' pgm <tmp>` when the command contains shell metacharacters (including spaces); stdin mode when config ends with ` <`.
- `grit diff` patch path: when a path has an active textconv driver, do not short-circuit on NUL “binary”; use `blob_text_for_diff_with_oid` so cache hits skip the helper.
- `diff --no-index`: load `core.attributesFile` (relative paths resolved from cwd), apply textconv when `diff=<driver>` matches; prepend `diff --git` + `index` line with blob hashes.
- `run_textconv_raw` now takes explicit command working directory; `cat-file --textconv` uses the work tree (or git-dir parent for bare).
- `scripts/run-tests.sh`: use `timeout_prefix` variable so `timeout` is actually applied (was always empty).
- `tests/test-lib.sh`: `nongit` matches upstream `git/t/test-lib-functions.sh` (cd `non-repo` under trash) so t4042.8 paths resolve.

### Validation
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t4042-diff-textconv-caching.sh` → 8/8
