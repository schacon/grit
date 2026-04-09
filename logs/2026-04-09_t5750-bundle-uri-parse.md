# t5750-bundle-uri-parse

## Goal

Make `tests/t5750-bundle-uri-parse.sh` pass (13 cases) by implementing Git-compatible
`test-tool bundle-uri parse-key-values` and `parse-config`.

## Changes

- Added `grit/src/bundle_uri_test_tool.rs`: bundle list state, `bundle_list_update` matching
  `git/bundle-uri.c` key rules, `relative_url` via `git_path::relative_url`, config-file scan
  with Git-style bad-line validation for empty key/value, fatal on “strip one component” like Git.
- Wired `parse-key-values` and `parse-config` in `grit/src/main.rs` `test-tool bundle-uri` dispatch
  (exit code from parse errors).

## Verification

- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5750-bundle-uri-parse.sh` → 13/13

## Notes

`parse-config` uses base URI `<uri>` unchanged when passed that literal (matches
`test-bundle-uri.c`), so relative URI resolution matches Git’s test expectations.
