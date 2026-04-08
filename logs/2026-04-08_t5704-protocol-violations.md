# t5704-protocol-violations

## Goal

Make `tests/t5704-protocol-violations.sh` pass under the grit harness (3 tests).

## Changes

1. **`grit upload-pack` + `GIT_PROTOCOL`**
   - When `GIT_PROTOCOL` requests `version=2` (highest wins, colon-separated), run protocol v2: advertise capabilities, then serve loop like upstream `upload-pack`.
   - Reused `serve_v2` capability/command handling.

2. **Strict flush after command arguments**
   - Added `pkt_line::read_data_lines_until_flush` so a second `0001` delim after args fails with Git’s messages (`expected flush after ls-refs arguments`, etc.).

3. **`grit serve-v2`**
   - Default mode now loops requests after advertising (was incorrectly calling `stateless_rpc` twice).
   - Stateless RPC still handles one request.

4. **`grit ls-remote --upload-pack`**
   - Spawns `sh -c <upload-pack> <repo>` without forcing v2.
   - First pkt-line `version 2` → v2 ls-refs client; otherwise parse v0 ref advertisement (tab or space between oid and ref; NUL before capabilities; `symref=HEAD:` for `--symref`).

5. **`grit-lib`**
   - Exported `ref_matches_ls_remote_patterns` for pattern filtering shared with protocol client.

## Validation

- `./scripts/run-tests.sh t5704-protocol-violations.sh` — 3/3
- `cargo test -p grit-lib --lib`
- `cargo clippy -p grit-rs -p grit-lib`
