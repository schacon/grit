# t5519-push-alternates

## Failure

Test 3 expected Alice’s second commit to stay out of `bob-pub` when `bob-pub` uses `objects/info/alternates` to `alice-pub/objects`. Grit’s local `git push` copies loose objects from the sender without checking the destination ODB (including alternates), so the commit object was copied into `bob-pub` anyway.

## Fixes

1. **`grit-lib`**: `Odb::write_local` / `write_raw_local` — write into the primary store even when the OID exists only in alternates. `unpack_objects` uses `write_local` so receive-pack materializes packed objects locally (matches Git’s unpack-objects).

2. **`grit push`**: `copy_objects_tracked` — skip copying a loose object when `Odb::exists` on the remote `objects/` is true (local loose/pack + alternates). Skip copying a `.pack`/`.idx` pair when every OID in the index already exists on the destination (clone pack deduped against alternate).

## Verification

- `./scripts/run-tests.sh t5519-push-alternates.sh` → 8/8
- `cargo test -p grit-lib --lib`
