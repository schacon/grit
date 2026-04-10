# t3405-rebase-malformed

## Failure

Test 5: `rebase -i` with `reword` and fake editor writing `FAKE_COMMIT_MESSAGE=" "` must fail (`test_must_fail`). Grit accepted whitespace-only as a non-empty message.

## Fix

- Added `message_from_reword_editor` in `grit/src/commands/rebase.rs`: run `cleanup_edited_commit_message` (same as `git commit` after editor), reject if `trim()` is empty with Git-style stderr, then `apply_commit_msg_cleanup` with existing rebase cleanup mode.
- Wired all three `run_commit_editor_for_reword` call sites through this helper.

## Verification

- `./scripts/run-tests.sh t3405-rebase-malformed.sh` → 5/5
- `cargo test -p grit-lib --lib`
