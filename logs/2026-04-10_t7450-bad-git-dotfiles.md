# t7450-bad-git-dotfiles

## Fixes

- **`tests/test-tool`**: Implement `test-tool sha1 -b` / `sha256 -b` (raw digest) so `lib-pack.sh` `pack_trailer` appends a valid 20-byte pack hash. The old stub wrote ASCII hex + newline, corrupting packs and breaking strict `index-pack` / `unpack-objects`.
- **`grit-lib` `validate_gitmodules_blob_line`**: Always scan raw `[submodule "..."]` names with `check_submodule_name`. Parsed config entries can omit malicious names when canonical keys reject `..` in subsections.
- **`grit-lib` `verify_packed_dot_special`**: Shared helper for strict pack fsck; `unpack-objects --strict` now runs the same dotfile / `.gitmodules` checks as `index-pack --strict` (odd object order in packs).
- **`tests/test-lib-functions.sh`**: Removed `} 7>&2 2>&4` from `test_must_fail` / `test_might_fail` / `test_expect_code` so `2>file` in tests captures command stderr.

## Verify

```bash
./scripts/run-tests.sh t7450-bad-git-dotfiles.sh
# 50/50 pass
```
