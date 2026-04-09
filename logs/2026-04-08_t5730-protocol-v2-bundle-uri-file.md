# t5730-protocol-v2-bundle-uri-file

## Summary

Implemented protocol v2 over local `grit upload-pack` for `file://` URLs so bundle-uri advertisement, optional `command=bundle-uri`, `ls-remote`, and clone preflight match upstream harness expectations (`GIT_TRACE_PACKET` greps).

## Key changes

- New `grit/src/file_upload_pack_v2.rs`: spawn upload-pack with `GIT_PROTOCOL=version=2`, trace `packet: git<` / `git>` lines, read ls-refs only until flush (avoid stdin/stdout deadlock with serve loop), optional bundle-uri + v2 fetch (sideband pack discard).
- `serve_v2` fetch: stream pack through side-band-64k like Git upload-pack v2.
- `ls-remote`: `file://` + `protocol.version=2` uses upload-pack path; exported `parse_v2_ls_refs_output`.
- `clone`: `file://` + v2 runs preflight before local object copy; respects `transfer.bundleURI` and `--bundle-uri`.
- `test-tool bundle-uri ls-remote`: `file://` uses local upload-pack instead of HTTP only.
- `trace_packet::trace_packet_git` for Git-compatible trace lines.

## Validation

- `./scripts/run-tests.sh t5730-protocol-v2-bundle-uri-file.sh` — 8/8 pass
- `cargo test -p grit-lib --lib` — pass
