# t6406-merge-attr — FETCH_HEAD for-merge parsing

## Failure

Test 10 (`up-to-date merge without common ancestor`) failed: after `git fetch ../repo2 main`, `git merge --allow-unrelated-histories FETCH_HEAD` did not see a merge candidate.

## Root cause

`FETCH_HEAD` lines marked for-merge use Git’s double-tab form: `<oid>\t\tbranch 'main' of …`. `read_fetch_head_merge_oids` split on `\t` and treated the empty second field as the “second column”, then skipped every line as if it were `not-for-merge`.

## Fix

- Added `grit_lib::fetch_head::merge_object_ids_hex` (same rules as `fmt_merge_msg::parse_fetch_head`).
- `merge::read_fetch_head_merge_oids` and `pull::merge_heads_from_fetch_head` now use it.

## Validation

Run locally before push:

```bash
cargo fmt && cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib
cargo test -p grit-lib --lib
./scripts/run-tests.sh t6406-merge-attr.sh
```
