# t5300-pack-object progress

## Changes

- `grit-lib/pack.rs`: validate `.idx` trailing checksum; verify embedded pack checksum and pack trailer in `verify_pack_and_collect`; resolve each index OID through the pack and compare to `Odb::hash_object_data` (mismatched idx/pack pairs fail).
- `grit-lib/unpack_objects.rs`: `UnpackOptions.strict`; `strict_verify_packed_references` for post-unpack / index-pack connectivity (refs resolve to pack or ODB).
- `grit/index_pack.rs`: `-o`/`--output`, `--keep=`, `--threads` warning, `--strict` / `--fsck-objects` with argv preprocessor for `index-pack --strict RULE`; collision check vs existing ODB for blobs; commit fsck (missing email unless ignored); `sha256` accepted for `--object-format` on non-repo paths.
- `grit/main.rs`: `index-pack` argv preprocessing; dispatch hook.
- `scripts/run-tests.sh`: `GIT_TEST_BUILTIN_HASH=sha1` so t5300 hash comparison tests behave.
- `bundle`/`clone`: `strict: false` on `UnpackOptions`.

## Harness

After this commit: **37/63** passing for `t5300-pack-object` (was 29/63 before branch work in this session).

## Remaining (high level)

- `pack-objects`: Git-fatal messages, `--stdin` rejection, stdin-packs error text, `window<=0` no deltas, OFS_DELTA, `pack.packSizeLimit` splitting, `--threads` warnings, `--name-hash-version`, `--path-walk`, extra positional rejection.
- `index-pack`: reliable non-repo cwd for `nongit` tests; SHA-256 show-index / verify paths need broader object-format support.
- `config -C` for prefetch test; SHA1 collision detection crate or full compare stream.
