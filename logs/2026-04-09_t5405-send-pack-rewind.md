# t5405-send-pack-rewind

## Symptom

`git fetch .. main:main` failed with `could not find remote ref 'refs/heads/main'` (protocol v2 advertisement misread as v0) or, after forcing v0, `pack trailing checksum mismatch` on unpack.

## Fixes

1. **`fetch_transport::spawn_upload_pack`**: Always spawn `upload-pack` with protocol v0 (`GIT_PROTOCOL` unset). The negotiation path uses v0 ref ads + want/have/done; default `protocol.version=2` left the advertised ref list empty.

2. **`read_pkt_payload_raw`**: Return `None` for flush/delim/special pkt-lines (`len` 0–2), not `Some([])`, so side-band readers stop at the post-pack flush instead of reading the next pkt-line into the pack buffer.

3. **`read_sideband_pack_until_done`**: Skip channel-1 progress until `PACK` magic; handle magic split across side-band chunks via a small pending buffer.

4. **`pack_objects_upload::spawn_pack_objects_upload`**: Pass `thin: bool`; omit `--thin` when there are no client `have` commits (empty clone / first fetch), matching Git so the pack is self-contained.

5. **`StreamingPackReader::decompress`** (`unpack_objects.rs`): Use `flate2::Decompress` with explicit consumed input length so the pack SHA-1 matches Git when zlib streams do not end on read chunk boundaries (e.g. empty file blob in t5405).

## Validation

- `sh tests/t5405-send-pack-rewind.sh` — 3/3
- `./scripts/run-tests.sh t5405-send-pack-rewind.sh` — 3/3
- `cargo test -p grit-lib --lib` — pass
- `cargo clippy -p grit-lib -p grit-rs` — pass
