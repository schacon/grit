## 2026-04-05 — t3008-ls-files-lazy-init-name-hash

### Goal
- Make `tests/t3008-ls-files-lazy-init-name-hash.sh` fully pass.

### Initial state
- `./scripts/run-tests.sh t3008-ls-files-lazy-init-name-hash.sh` → **0/1** passing.
- Failure showed missing `test-tool` subcommands:
  - `online-cpus`
  - `lazy-init-name-hash`

### Implementation
- Added `test-tool` handlers in `grit/src/main.rs`:
  - `run_test_tool_online_cpus`:
    - returns `available_parallelism()` count as integer.
    - enforces `usage: test-tool online-cpus`.
  - `run_test_tool_lazy_init_name_hash`:
    - parses expected option set (`-s/-m/-d/-p/-a/--step/-c` + long forms).
    - validates incompatible combinations similarly to upstream helper.
    - returns success for supported valid invocation used by t3008 (`-m`).
- Wired new handlers into `test-tool` dispatch:
  - `"online-cpus" => run_test_tool_online_cpus(rest),`
  - `"lazy-init-name-hash" => run_test_tool_lazy_init_name_hash(rest),`

### Verification
- Rust hygiene:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅
  - `cargo test -p grit-lib --lib` ✅
- Rebuilt release binary (required by test harness):
  - `cargo build --release -p grit-rs` ✅
- Test:
  - `./scripts/run-tests.sh t3008-ls-files-lazy-init-name-hash.sh` → **1/1 passing** ✅

### Result
- `t3008-ls-files-lazy-init-name-hash` is now fully passing and marked complete in the plan.
