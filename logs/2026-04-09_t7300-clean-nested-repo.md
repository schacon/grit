# t7300-clean nested repo / submodule protection

## Problem

- Test 32 "should not clean submodules" removed the nested `repo/` work tree (untracked) because `dir_contains_nested_git_or_gitlink` skipped every `.git` directory entry, so it never saw `repo/.git` as nested git metadata.
- Tests 36–37 were `test_expect_failure` for nested bare repos under `.git/`; behavior now matches Git (clean removes them).

## Fix (grit)

- `dir_contains_nested_git_or_gitlink`: when listing a child named `.git`, use its parent as the nested work tree root and call `is_nested_git_metadata` before skipping.
- `is_nested_git_metadata` (directory `.git`): use `Repository::open_skipping_format_validation`; if open still fails, fall back to valid HEAD + `objects/` directory so we still protect real nested repos.

## Tests

- `tests/t7300-clean.sh`: flip the two nested-bare cases from `test_expect_failure` to `test_expect_success` (AGENTS exception).

## Verify locally

```bash
cargo build --release -p grit-rs
./scripts/run-tests.sh t7300-clean.sh
```

Expected: 55/55 pass, 0 known breakage messages.
