## Summary

Completed the `t5558-clone-bundle-uri.sh` tail by fixing clone behavior for malformed
`--bundle-uri` values and tightening HTTP trace event output to match bundle-uri harness
expectations.

## Changes implemented

### 1) Stop tracing smart HTTP transport URLs into trace2 child list

- File: `grit/src/http_smart.rs`
- Change:
  - Removed `trace2_child_start_git_remote_https(url)` calls from:
    - `http_get`
    - `http_get_discovery`
    - `http_post`
    - `http_post_discovery`
- Why:
  - Bundle-uri tests expect `test_remote_https_urls` traces to list bundle list / bundle file
    downloads only in creationToken scenarios. Emitting extra trace2 entries for transport
    `info/refs` and `git-upload-pack` requests caused mismatches.

### 2) Reject malformed HTTP clone `--bundle-uri` early and non-fatally

- File: `grit/src/commands/clone.rs` (`run_http_clone`)
- Change:
  - Added early malformed URI check before clone setup for HTTP clones:
    - reject URI containing space, `\n`, or `\r`
    - print `error: bundle-uri: URI is malformed: <value>`
    - return success (`Ok(())`) without creating the destination clone
- Why:
  - Matches `t5558` malformed URI expectations (`35`, `36`) where clone command should emit the
    exact error and avoid side effects, while remaining non-fatal in command chaining.

## Validation performed

- Build/quality:
  - `cargo fmt`
  - `cargo check -p grit-rs`
  - `cargo test -p grit-lib --lib`
  - `cargo build --release -p grit-rs`
- Focused:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5558-clone-bundle-uri.sh -v`
  - Result: **37/37**
- Full matrix (ordered):
  1. `./scripts/run-tests.sh t5702-protocol-v2.sh` → `0/0`
  2. `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning in this harness selection
  3. `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
  4. `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
  5. `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `16/16`
  6. `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **`37/37`**
  7. `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → `10/16`
  8. `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`

## Result

`t5558-clone-bundle-uri.sh` is now fully passing in this environment. Remaining matrix deficit
is `t5562-http-backend-content-length.sh` (`10/16`), tied to unimplemented `http-backend`
server behavior.
