# t3418-rebase-continue (partial)

## Done

- **rev_list**: Symmetric-diff left/right map no longer marks commits reachable from *both* tips as right-only. Shared ancestry entries are omitted from the map so `--right-only --cherry-pick` matches Git (fixes empty rebase todo when all commits were misclassified).
- **merge**: Unknown `-s <name>` resolves to `git-merge-<name>` on `PATH`; invokes it with merge bases, `--`, `HEAD`, remote OID, and passes `-X` as `--<option>` (Git `try_merge_command`). Wired into single-strategy merge and `try_merge_strategies`.
- **rebase**: Write `.git/ORIG_HEAD` when starting a rebase and when the todo is empty after cherry filtering (no-op skip path).

## Still failing in t3418 (23 tests)

Rerere autoupdate flags during rebase, `break` / patch file, reschedule-failed-exec, commentChar=auto, fixup/skip message cleanup, and full strategy persistence for `-r` still need sequencer/rebase state parity with Git.

## Verify

- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t3419-rebase-patch-id.sh` (still 8/8)
- `./scripts/run-tests.sh t3418-rebase-continue.sh` (7/30 at time of log)
