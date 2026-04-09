# t5515-fetch-merge-logic (in progress)

## Goal

Make `tests/t5515-fetch-merge-logic.sh` pass under the harness (grit as `git`).

## Changes implemented

- **`grit fetch`**: default remote from `branch.<current>.remote` when no remote arg; path-based URL display (`../` vs `../.git/`); co-fetch all configured remotes that share the same repository URL (clone `origin` + extra remotes); union refspecs for object copy roots; legacy `.git/remotes/*` and `.git/branches/*`; normalize shorthand fetch refspec sources (`two:` → `refs/heads/two:`); normalize `remotes/...` dst to `refs/remotes/...`; FETCH_HEAD ordering/sorting closer to Git; `tag <name>` CLI handling; remote `HEAD` updates; prune per coalesced remote; `--output` uses tracking remote prefix.
- **`grit init`**: write `[init] defaultBranch` in new repo config (for default-branch vs detached `HEAD` on fetch source).
- **`grit tag`**: parse `GIT_COMMITTER_DATE` through the same path as commits so tagger lines use epoch+offset (matches Git tag object bytes when identities match).

## Verification

- `cargo fmt`, `cargo test -p grit-lib --lib` — pass.
- `./scripts/run-tests.sh t5515-fetch-merge-logic.sh` — **still 1/65 pass** (setup only).

## Remaining failure analysis

With a full harness-style tree (OID cache + `A U Thor` / `C O Mitter` + fixed dates), manual replay of `br-config-explicit` shows **FETCH_HEAD and `show-ref` match fixtures** after recent fetch/tag fixes.

The harness still reports 64 failures without printing `diff` output; the trash dir’s `expect_f` often reflects the **last** failing case (not test 2), which suggests the comparison phase is still failing for a systematic reason (e.g. subshell/`return` vs `exit`, or a command in the test body not matching Git’s behavior under `/bin/sh`).

## Next steps (for a follow-up)

1. Run a single test body under `bash -x` from the trash `cloned/` dir to capture the first non-zero exit or `test_cmp` stderr.
2. Confirm `git for-each-ref … | while … git update-ref -d` propagates failures identically to Git when grit is `git` (pipefail / `return` in POSIX sh).
3. Re-run full t5515 once the harness body is confirmed clean.

## Reason not complete

`blocked` — fixture-aligned behavior is largely in place, but the shell harness still fails all cases after setup; needs targeted tracing of the test body exit path.
