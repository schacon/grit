# t4129-apply-samemode

## Summary

Made `tests/t4129-apply-samemode.sh` pass (23/23).

## Root causes

1. **`git diff --stat -p` produced no patch** — `format_besides_unified_patch` treated `--stat` as exclusive; Git emits stat then patch when `-p` wins. Fixed in `grit/src/commands/diff.rs`.

2. **`diff --git` paths kept `b/` prefix** — default strip is 1; paths must become `d/f2` not `b/d/f2`. Fixed when assigning `old_path`/`new_path` from `split_diff_git_paths`.

3. **Mode handling** — Implemented Git-style `canon_mode`, strict `parse_mode_line` errors, optional mode on `index` lines, `prepare_patch_modes_for_apply` mirroring `check_preimage` for `--cached`, `--index`, and worktree, umask-aware checkout permissions, and correct `reverse_patches` mode swap (`new_mode || is_delete`).

## Validation

- `./scripts/run-tests.sh t4129-apply-samemode.sh`
- `cargo test -p grit-lib --lib`
- `cargo clippy -p grit-rs --fix --allow-dirty`
