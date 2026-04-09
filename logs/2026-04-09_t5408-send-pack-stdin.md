# t5408-send-pack-stdin

## Goal

Make `tests/t5408-send-pack-stdin.sh` pass under the harness (grit as `git`).

## Changes

- `grit/src/commands/send_pack.rs`: added `--stdin` (append refspec lines from stdin after argv), `--mirror` (reject when any refspecs are present with exit 129 and Git’s `send-pack` usage text), and pre-flight duplicate detection on normalized destination refs (`multiple updates for ref '…' not allowed`).

## Verification

- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t5408-send-pack-stdin.sh` → 10/10
