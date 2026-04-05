# t2010-checkout-ambiguous

- Started: 2026-04-05 20:34 CEST
- Scope: verify and fix the remaining `t2010-checkout-ambiguous` failures, then update tracking files and commit.

## What I checked

- Read `AGENTS.md` and the `PLAN.md` entry for `t2010-checkout-ambiguous`.
- Inspected `git/t/t2010-checkout-ambiguous.sh` and `git/Documentation/git-checkout.adoc`.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t2010 bash scripts/run-upstream-tests.sh t2010 2>&1 | tail -40`.

## Result

- The upstream run already passed: 10 tests, 10 passing, 0 failing.
- No Rust source changes were required for this task.
- `plan.md` was already marked `[x]`; updated `progress.md` and `test-results.md` to reflect the fresh verification result.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t2010 cargo fmt --all 2>/dev/null; true` as requested.
- Blocker: `git add` and `git commit` cannot run in this sandbox because Git cannot create `.git/index.lock` (`Operation not permitted`).
- Blocker: `git push origin main` failed because the sandbox cannot resolve `github.com`.
- Blocker: `openclaw system event --text "Done: t2010-checkout-ambiguous" --mode now` failed locally with `ERROR: SecItemCopyMatching failed -50`.
