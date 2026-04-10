# rev-parse --prefix path disambiguation (t1513)

## Problem

`t1513-rev-parse-prefix` failed on cases without `--` where a token is both
non-resolvable as a revision and ambiguous (e.g. `file1` under `--prefix sub1/`).

## Fix

Match Git: when resolution returns the standard "ambiguous argument" message and
`--prefix` is set, `lstat(prefix_filename(prefix, arg))`; if the path exists,
print `apply_prefix_for_forced_path(prefix, arg)` and continue instead of the
deferred fatal path.

## Validation

- `./scripts/run-tests.sh t1513-rev-parse-prefix.sh` → 11/11
- `cargo test -p grit-lib --lib`
