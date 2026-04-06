## t2027-checkout-track — 2026-04-05

- Claimed next near-complete target after `t2023`: `t2027-checkout-track` (4/5).
- Reproduced failures with:
  - `./scripts/run-tests.sh t2027-checkout-track.sh` → 4/5
  - `GUST_BIN=/workspace/tests/grit TEST_VERBOSE=1 bash tests/t2027-checkout-track.sh`
- Root cause:
  - Ambiguous remote-tracking branch checkout diagnostics/hints diverged for
    `checkout` vs `switch` when branch name existed on multiple remotes.
  - `grit checkout trunk` emitted `checkout`-only hint, but test expected
    `switch` invocation to emit `switch --track` guidance too.
  - `grit switch trunk` delegated to system git, whose hint text differed from
    expected fixture text in this test suite.

- Implementation:
  - `grit/src/commands/checkout.rs`
    - Added detection for ambiguous remote-tracking branch names when no local
      branch or revision matches.
    - Added helper `find_ambiguous_remote_tracking()` using `refs::list_refs`
      over `refs/remotes/`.
    - Emitted stable hint block including both:
      - `git checkout --track <remote>/<name>`
      - `git switch --track <remote>/<name>`
    - Kept fatal error wording aligned with existing behavior.
  - `grit/src/commands/switch.rs`
    - Added matching ambiguous remote-tracking detection path before passthrough.
    - Emits deterministic hint/fatal text consistent with expected test output.
    - Added argument parsing helper reuse for target extraction and detection.

- Validation:
  - `cargo fmt` ✅
  - `cargo build --release -p grit-rs` ✅
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t2027-checkout-track.sh` ✅ (5/5)
  - `./scripts/run-tests.sh t2027-checkout-track.sh` ✅ (5/5)
  - `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated churn files afterward)
  - `cargo test -p grit-lib --lib` ✅ (96/96)
