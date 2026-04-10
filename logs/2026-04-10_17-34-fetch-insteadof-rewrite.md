## 2026-04-10 17:34 — t5516 fetch url.insteadOf parity

### Goal
- Improve `t5516-fetch-push.sh` by fixing fetch URL rewrite parity.

### Investigation
- Reproduced the failing `t5516` cases with verbose harness output.
- Confirmed `5516.8 fetch with insteadOf` failed because `fetch` used the raw configured URL and did not apply `url.<base>.insteadOf`.
- Confirmed `t5509` remains 13/15 and failures 6/10 still reproduce under real Git in this harness context.

### Code changes
- Added `grit/src/url_rewrite.rs`:
  - `rewrite_fetch_url(config, url)` applies longest-prefix `url.<base>.insteadOf`.
  - `rewrite_push_url(config, url)` applies longest-prefix `url.<base>.pushInsteadOf` then falls back to `insteadOf` (helper added for follow-up push parity work).
- Wired module in `grit/src/main.rs` with `mod url_rewrite;`.
- Updated `grit/src/commands/fetch.rs`:
  - Preserved raw remote URL as `raw_url`.
  - Applied `crate::url_rewrite::rewrite_fetch_url(config, &raw_url)` before transport/path resolution.
  - Restored explicit early rejection for `--no-ipv4`/`--no-ipv6` with Git-style `unknown option` diagnostics.

### Validation
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs` ✅ (reverted unrelated edits)
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5516-fetch-push.sh` → **59/124** (was 58/124)
  - `fetch with insteadOf` now passes.

### Notes
- This increment focuses on fetch-side URL rewrite parity.
- Push-side alias/rewrite parity remains a follow-up area for `t5516` (e.g. “push into aliased refs (inconsistent)” and related cases).
