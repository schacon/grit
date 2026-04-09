# t5731-protocol-v2-bundle-uri-git

- **Issue:** `ls-remote` and `test-tool bundle-uri ls-remote` only handled `file://` and HTTP; `git://` fell through to local path open and failed. Packet traces for v2 caps used wrong identity in some paths.
- **Change:** Added `git_daemon_url` module (parse `git://`, connect + daemon request pkt). `file_upload_pack_v2`: `ls_remote_git_v2`, `fetch_bundle_uri_lines_git` reusing same v2 bundle-uri + ls-refs flow as `file://`. Wired `ls_remote` and test-tool dispatch. `fetch_transport` uses shared connect helper.
- **Verify:** `./scripts/run-tests.sh t5731-protocol-v2-bundle-uri-git.sh` → 8/8; `cargo test -p grit-lib --lib`.
