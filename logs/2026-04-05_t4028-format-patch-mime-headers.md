# t4028-format-patch-mime-headers

- Claimed `t4028-format-patch-mime-headers` from `PLAN.md` and inspected `git/t/t4028-format-patch-mime-headers.sh`.
- Rebuilt `target/release/grit` with `cargo build --release -p grit-rs` because `scripts/run-upstream-tests.sh` executes the repo-local release binary.
- Re-ran `CARGO_TARGET_DIR=/tmp/grit-build-t4028 bash scripts/run-upstream-tests.sh t4028-format-patch-mime-headers 2>&1 | tail -40` and confirmed the upstream file is already green at 3/3.
- Verified the emitted patch headers directly in a scratch repository: UTF-8 commit bodies produce `MIME-Version`, `Content-Type: text/plain; charset=UTF-8`, and `Content-Transfer-Encoding: 8bit`, and adding `format.headers=x-foo: bar` preserves those MIME headers.
- No Rust source changes were required; the remaining `PLAN.md` entry was stale bookkeeping.
- Updated `PLAN.md`, `progress.md`, and `test-results.md` to reflect the confirmed 3/3 status.
