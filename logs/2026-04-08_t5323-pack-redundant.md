# t5323-pack-redundant

## Goal

Make `tests/t5323-pack-redundant.sh` fully pass (18/18).

## Changes

1. **`grit pack-redundant`** (`grit/src/commands/pack_redundant.rs`): Ported upstream `git/builtin/pack-redundant.c` logic — `cmp_local_packs`, `minimize`, alternates via `--alt-odb`, stdin ignore list, `--i-still-use-this` gate with Git-style stderr + `fatal: refusing to run without --i-still-use-this`, stdout pairs `.idx` then `.pack`, verbose stderr blocks.

2. **`git clone --mirror`** (`grit/src/commands/clone.rs`): `--mirror` now forces `bare = true` so the object database lives at `<repo>/objects/` (not `<repo>/.git/objects/`), matching Git and fixing `objects/info/alternates` writes in the test.

3. **`git fsck`** (`grit/src/commands/fsck.rs`): When validating `objects/info/alternates`, resolve relative paths against the canonical `objects/` directory (same as Git), so `../../main.git/objects` from `shared.git/objects` validates correctly.

## Verification

- `./scripts/run-tests.sh t5323-pack-redundant.sh` — 18/18
- `cargo test -p grit-lib --lib` — 121/121
