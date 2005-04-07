# t5314-pack-cycle-detection

## Problem

`t5314-pack-cycle-detection.sh` failed on `git repack -ad 2>stderr` + `test_must_be_empty stderr`.

Two causes:

1. **Inter-pack delta cycles** — `optimize_blob_deltas` merged `packed_delta_base_oid` hints from multiple packs, producing `A→B` and `B→A`. The new pack then contained an unresolvable cycle. Git fixes this in `break_delta_chains()` (first DFS pass: drop the edge when the base is already ACTIVE).

2. **Stderr noise** — Grit always printed `Total N (delta …)` to stderr. Git only prints that summary when `progress` is on (effectively `isatty(2)`), so repack with stderr redirected stays silent.

## Fix

- `grit/src/commands/pack_objects.rs`: `break_reused_delta_cycles()` on the blob delta map before depth limiting; gate `eprintln!` on `!args.quiet && std::io::stderr().is_terminal()` (and `IsTerminal` import).

## Verification

- `cargo build --release -p grit-rs`
- `cd tests && GUST_BIN=... sh t5314-pack-cycle-detection.sh` — 2/2 pass
- `./scripts/run-tests.sh t5314-pack-cycle-detection.sh` — updates CSV/dashboards
