# t5802-connect-helper

- Implemented `ext::` (git-remote-ext) URL support: `grit/src/ext_transport.rs`.
- Wired `clone` (`run_ext_clone`) and `fetch` (`ext::` branch in `fetch_remote`).
- Extended `fetch_transport`: `spawn_upload_pack_with_proto`, pub helpers for ext negotiation; v0 upload-pack child clears inherited `GIT_PROTOCOL` when `client_proto == 0`.
- Harness: `./scripts/run-tests.sh t5802-connect-helper.sh` → 8/8 pass.
- Commit: `feat: support ext:: remote URLs (connect helper)`.
- PR: https://github.com/schacon/grit/pull/347
- Push: used `/usr/bin/git push` because grit-as-git mis-parsed HTTPS `origin` URL.
