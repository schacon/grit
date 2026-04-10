## Task
- Continue phase-6 `t5516-fetch-push` parity work.
- Focus this slice on push-side URL rewrite behavior (`insteadOf` / `pushInsteadOf`) and reassess `t5509` namespace failures.

## Investigation
- Confirmed `t5509-fetch-push-namespaces.sh` remains **13/15** and that the two failing cases (6, 10) also fail under system `git` in this harness environment.
- Focus shifted to `t5516` push URL rewrite failures:
  - `not ok 17 - push with insteadOf`
  - `not ok 18 - push with pushInsteadOf`
- Root cause in `push`:
  - path-style remote arguments (e.g. `trash/testrepo`) were passed directly to transport resolution without applying `url.<base>.insteadOf` / `pushInsteadOf`.
  - configured `remote.<name>.url` path also bypassed push-side URL rewrite.

## Code changes
- Updated `grit/src/commands/push.rs`:
  1. In `run()`, when CLI remote is path/URL-style, rewrite remote argument through:
     - `crate::url_rewrite::rewrite_push_url(&config, r)`
  2. Use rewritten value for:
     - URL list passed to `push_to_url`
     - `path_style_remote` classification (based on rewritten URL).
  3. In `resolve_remote_urls()`, apply push rewrite for configured `remote.<name>.url` before returning URL + path-style classification.

## Validation
- Quality gates:
  - `cargo fmt` ✅
  - `cargo check -p grit-rs` ✅
  - `cargo clippy --fix --allow-dirty -p grit-rs` ✅ (reverted unrelated clippy-only edits)
  - `cargo test -p grit-lib --lib` ✅
  - `cargo build --release -p grit-rs` ✅
- Targeted parity tests:
  - `bash tests/t5516-fetch-push.sh --run=1,17,18,19` (harness env) ✅
    - `push with insteadOf` now passes
    - `push with pushInsteadOf` now passes
    - explicit `remote.<name>.pushurl` precedence case remains passing
- Suite snapshots:
  - `./scripts/run-tests.sh t5516-fetch-push.sh` → **62/124**
  - `./scripts/run-tests.sh t5509-fetch-push-namespaces.sh` → **13/15** (unchanged; known harness parity artifact with system `git`)

## Result
- Push-side URL rewrite parity for `insteadOf` and `pushInsteadOf` is now implemented in native `grit push`.
- `t5516` failure set no longer includes cases 17 and 18.
