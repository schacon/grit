# t3406-rebase-message

## Summary

Made `tests/t3406-rebase-message.sh` pass (32/32).

## Changes

- **`grit rebase`**: Git-compatible stdout messages (up to date / forced / fast-forward), `--stat` / `rebase.stat` / `-n`, early validation for `-C` and `--whitespace`, interactive `-i` listing + diffstat for unrelated-history case; fixed commit collection (stop at merge-base only, not upstream tip); fast-forward path with correct reflog; `GIT_REFLOG_ACTION` persisted in `rebase-apply/reflog-action`; reflog messages for start/finish/pick/continue/abort aligned with Git; start checkout updates HEAD reflog only (not branch ref); index/worktree reset when detaching at onto.
- **`checkout`**: `write_checkout_reflog` now appends to `logs/HEAD` only when switching branches — avoids bogus entries on the branch being left (fixes `conflicts@{1}` after `checkout topic` during tests).
- **`pull`**: Extended `rebase::Args` struct init for new fields.

## Verification

- `./scripts/run-tests.sh t3406-rebase-message.sh`
- `cargo fmt`, `cargo clippy -p grit-rs -p grit-lib --fix --allow-dirty`
- `cargo test -p grit-lib --lib`
