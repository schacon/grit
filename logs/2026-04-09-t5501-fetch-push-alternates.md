# t5501-fetch-push-alternates

## Failures

1. **Local `git fetch`**: `spawn_upload_pack` passed `effective_client_protocol_version()` (default 2) to the child, so `upload-pack` advertised protocol v2 capability lines. The fetch client used `read_advertisement` (v0 ref list parser), saw no `refs/heads/*` lines, and failed with `could not find remote ref 'refs/heads/main'`.

2. **`count_objects` mismatch after fetch**: `clone --reference` copied all loose objects from the source into the clone, so `one.count` was huge while `fetcher` only had objects fetched from `one` (Git keeps reference clones thin via alternates only).

## Fixes

- `fetch_transport.rs`: `spawn_upload_pack` always spawns upload-pack with protocol v0 (`client_proto == 0`, clears `GIT_PROTOCOL`) for pipe negotiation used by `fetch_via_upload_pack_skipping`.

- `clone.rs` (both local-clone branches): when `--reference` or `--reference-if-able` is set, skip `copy_objects`; always add the source `objects` directory to `info/alternates` (deduped) so objects resolve like Git.

## Validation

- `./scripts/run-tests.sh t5501-fetch-push-alternates.sh`
- `cargo test -p grit-lib --lib`
- `cargo check -p grit-rs`
