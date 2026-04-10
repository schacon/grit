# 2026-04-10 — push atomic/reporting parity (`t5543-atomic-push`)

## Scope

- Continue from completed push-default/CAS/push-options work.
- Targeted Phase 4 parity issues in `push --atomic` output and `--receive-pack` wrapper behavior.

## Implemented changes

### `grit/src/commands/push.rs`

- **Atomic pre-reject wording parity**
  - Changed atomic pre-reject parenthetical for fast-forward conflicts from `fetch first` to `non-fast-forward` in atomic mode.
- **Atomic rollback/report ordering parity**
  - Introduced `report_push_rejection` helper and centralized rejection rendering.
  - Refined mirror+atomic rollback reporting to match expected order in `t5543.10`:
    - `main`, `one`, failed ref (`bar` hook declined), then remaining collateral (`two`).
  - Kept rollback ref restoration order deterministic using existing mirror ordering helpers.
- **`--receive-pack` failure-path parity**
  - `push_to_url` now delegates directly to real Git whenever `--receive-pack` is present (matching expected wrapper semantics in tests 12/13), avoiding grit’s local-path transport path that previously masked wrapper exit behavior.
- Preserved existing Phase 3/2 behavior:
  - config-backed `push.pushOption` and CLI precedence
  - recursive submodule push option propagation
  - detached-submodule refspec rewrite path for nested on-demand push

## Validation

- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅ (166 passed)
- `./scripts/run-tests.sh t5543-atomic-push.sh` ✅ **13/13**
- Regression suites:
  - `./scripts/run-tests.sh t5528-push-default.sh` ✅ 31/32 (upstream expected failure remains)
  - `./scripts/run-tests.sh t5533-push-cas.sh` ✅ 23/23
  - `./scripts/run-tests.sh t5545-push-options.sh` ✅ 13/13

## Notes

- During debugging, transient mirror-order diagnostics were added and removed before final validation/commit.
- Dashboard artifacts (`data/test-files.csv`, `docs/*.html`, `docs/test-progress.svg`) refreshed via harness runs.
