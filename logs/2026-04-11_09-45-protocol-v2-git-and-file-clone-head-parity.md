# 2026-04-11 09:45 — protocol-v2 git:// transport stability and file:// unborn HEAD parity

## Scope

Continue `t5702-protocol-v2.sh` execution with focus on the highest-impact remaining early failures:

- `git://` v2 clone/fetch/pull failures (`4-7`) with `EAGAIN` / missing repo side-effects.
- file:// v2 clone HEAD propagation parity (`16-21`), especially unborn/default-branch semantics.

## Changes made

### 1) `git://` v2 fetch/clone transport stabilization (`grit/src/fetch_transport.rs`)

- Keep mutable ref advertisement/head-symref state and attempt v2 `ls-refs` when:
  - server advertises v2, **or**
  - client requested v2 and initial ad is empty.
- Added mixed-mode fallback behavior:
  - if v2 `ls-refs` fails and server did not explicitly advertise v2, continue with parsed v0/v1 refs.
- Introduced explicit `use_v2_fetch` path selection:
  - send protocol-v2 `fetch` command and parse v2 pack response when v2 path is active.
- Fixed v2 pack response handling:
  - do **not** block waiting for an extra pkt-line after sideband pack termination in git:// mode.
  - this removed `Resource temporarily unavailable (os error 11)` failures on clone/fetch.
- Restored conservative ls-refs prefix fallback behavior for empty refspec flows:
  - request `refs/heads/` and `refs/tags/` when no usable derived prefixes exist.

### 2) file:// clone preflight HEAD metadata (`grit/src/file_upload_pack_v2.rs`)

- Clone ls-refs preflight now requests `unborn`.
- Added metadata parser that extracts in one pass:
  - `wants`,
  - advertised `HEAD` symref target,
  - advertised `HEAD` oid.
- Added source-HEAD fallback helpers:
  - read source `HEAD` symref/oid from repo files when server ls-refs omits data.
  - fallback is gated by `lsrefs.unborn` (`ignore` disables symref fallback).
- For non-empty unborn-head sources, avoid forcing sole advertised branch as checkout target:
  - if source `HEAD` is symref to missing branch and advertised refs collapse to one real head,
    clear preflight `head_oid` so clone keeps unborn branch semantics.

### 3) clone branch/warning parity (`grit/src/commands/clone.rs`)

- Preserve source unborn HEAD branch for non-bare local/file clones even when a single remote-tracking branch exists.
- Expand warning condition to treat “source unborn HEAD preserved” as warning-worthy:
  - emit `warning: remote HEAD refers to nonexistent ref, unable to checkout` in non-bare case.

## Validation

### Build / quality

- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` ✅
  - reverted unrelated `grit-lib/src/repo.rs` edits.
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅

### Targeted tests

- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-7 -v` ✅
  - `4`, `5`, `6`, `7` now pass (git:// v2 clone/fetch/pull restored).
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=9-21 -v` ✅ for targeted file:// head cases
  - `16`, `17`, `18`, `19`, `21` behavior now matches expected semantics.
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=19 -v` ✅
  - verifies warning + `HEAD` symbolic ref expectation for non-bare unborn-head clone.

### Harness checkpoint

- `./scripts/run-tests.sh t5702-protocol-v2.sh` → **58/85** (from 52/85).

## Notes

- This increment intentionally focused on protocol-v2 transport stability and clone-head parity.
- Remaining `t5702` failures are in later/other clusters (partial clone/filter, custom path env behavior, and HTTP packet/packfile-uri checks).
