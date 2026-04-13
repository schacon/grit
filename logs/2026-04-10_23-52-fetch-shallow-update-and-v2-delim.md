## 2026-04-10 23:52 — fetch shallow/update-shallow + v2 delim handling

### Scope
- Continue PLAN execution for fetch HTTP/shallow parity.
- Focused on `t5537-fetch-shallow.sh` regressions and v2 fetch response parsing robustness.

### Changes made

1. **`fetch` CLI parity for shallow updates**
   - Added `--update-shallow` to `grit fetch` args.
   - Wired parse+plumbing through `pull`'s internal fetch argument construction.

2. **Local fetch shallow/ref filtering behavior**
   - Added logic to detect remote refs requiring shallow-boundary updates.
   - When `--update-shallow`/depth/deepen/unshallow options are **not** active:
     - skip updating refs that would require shallow boundary mutation.
   - When `--update-shallow` is active:
     - merge remote shallow boundary OIDs for fetched tips into local `.git/shallow`.

3. **Unshallow correctness for shallow remotes**
   - `--unshallow` now removes local `.git/shallow` only when remote is not shallow (for local/ext remote repos).

4. **Protocol v2 response robustness**
   - Accept `Packet::Delim` in v2 fetch response loops:
     - `grit/src/http_smart.rs`
     - `grit/src/file_upload_pack_v2.rs`
   - Avoids spurious `unexpected fetch response: Delim` failures in one-time-script/v2 paths.

5. **Ref-filtering carveout for direct CLI refspec fetches**
   - For explicit CLI refspec fetches, do not suppress target refs due to shallow-update filtering.
   - Keeps direct `git fetch <remote> <src>:<dst>` updates intact while preserving filtered behavior for configured-refspec bulk updates.

### Validation commands run
- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`
- `cargo test -p grit-lib --lib`
- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t5537-fetch-shallow.sh`
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh`
- `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`

### Observed results
- `t5537-fetch-shallow.sh`: improved from **6/16** baseline to **10/16**.
  - `--update-shallow` cluster now passes (10/11/12/13).
  - Remaining failures: 4, 6, 8, 14, 15, 16 (mix of existing depth/unshallow, clone option parity, and one-time-script network/error-path behavior).
- `t5558-clone-bundle-uri.sh`: remains **27/37**.
- `t5562-http-backend-content-length.sh`: remains **10/16** (suite includes `git http-backend` command behavior not fully implemented in grit).

### Notes
- `clippy --fix` touched an unrelated file (`grit-lib/src/repo.rs`); reverted before proceeding.
- Harness dashboards and `data/test-files.csv` updated by `run-tests.sh`.
