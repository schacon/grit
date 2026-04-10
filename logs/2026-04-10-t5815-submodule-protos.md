# t5815-submodule-protos

## Failures (before)

- `submodule add` failed: `clone --separate-git-dir` rejected for SSH/ext URLs.
- `submodule update ext-module` did not fail without `GIT_ALLOW_PROTOCOL=ext` (ext should default to denied like Git `transport.c`).

## Fixes

1. **`grit/src/protocol.rs`** — Built-in defaults aligned with Git: `http`/`https`/`git`/`ssh` always allowed; `ext` never allowed without config override; other protocols (including `file`) user-only via `GIT_PROTOCOL_FROM_USER`.

2. **`grit/src/commands/clone.rs`** — Removed early bail that blocked `--separate-git-dir` for non-local URLs; SSH and ext clone paths already support separate git dir.

## Verification

- `./scripts/run-tests.sh t5815-submodule-protos.sh` → 8/8
- `cargo test -p grit-lib --lib` → pass
