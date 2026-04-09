# t12130-switch-create-force

## Result

`./scripts/run-tests.sh t12130-switch-create-force.sh` reports **33/33** passing on current `grit`. No Rust changes were required; implementation already covers `switch -c`, detach/orphan, dirty-tree refusal, and related cases exercised by this file.

## Follow-up

- Marked `t12130-switch-create-force.sh` complete in `t1-plan.md`.
- Refreshed harness CSV/dashboards via `run-tests.sh`.
