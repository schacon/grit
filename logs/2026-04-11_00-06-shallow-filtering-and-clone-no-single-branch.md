## 2026-04-11 00:06 — shallow filtering + clone no-single-branch follow-up

### Goal
Continue Phase C shallow parity work from the plan, targeting remaining `t5537` failures and the shallow clone option gap (`--no-single-branch`).

### Changes implemented

1) `grit clone`: add `--no-single-branch` compatibility flag
- File: `grit/src/commands/clone.rs`
- Added:
  - `Args::no_single_branch: bool` with clap long option `--no-single-branch`.
  - mutual exclusion check with `--single-branch`.
  - normalization: when `--no-single-branch` is set, force `single_branch = false`.
- Purpose:
  - unblock shallow/repack workflow paths that invoke clone with `--no-single-branch` (`t5537.15` stopped failing on unknown option).

2) Fetch shallow boundary filtering refinement
- File: `grit/src/commands/fetch.rs`
- In `refs_requiring_update_shallow(...)`:
  - compute `required_new_boundaries` strictly as `remote_boundary - local_boundary`
    (do not special-case empty local boundary by blocking everything),
  - skip `refs/tags/*` from shallow-boundary blocking logic.
- Purpose:
  - avoid over-filtering refs/tags and reduce false positives where boundary updates are not actually required.

### Validation run evidence

- Build gates:
  - `cargo fmt` ✅
  - `cargo check -p grit-rs` ✅
  - `cargo build --release -p grit-rs` ✅

- Harness matrix checkpoints:
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **10/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **10/16**

- Focused verbose:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`
  - Remaining failures: `6, 8, 9, 14, 15, 16`
  - Improvement confirmed for option parsing path in `5537.15`: no longer errors on unknown `--no-single-branch`.

### Notes
- `t5537` remains at 10/16; failures are now concentrated in:
  - unshallow/update boundary semantics (`6, 8, 9`),
  - shallow read-only/repack behavior (`14, 15`),
  - HTTP one-time-script connectivity ordering (`16`).
- Continue next iteration with targeted fixes for these remaining six tests.
