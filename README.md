# Gust

Gust is a **from-scratch reimplementation of Git** in idiomatic Rust. The goal is to match Git’s behavior closely enough that the upstream test suite (under `git/t/`) can be ported and run against this tool.

**v1** focuses on **plumbing** only—commands like `init`, `hash-object`, `cat-file`, the index and tree tools, `commit-tree`, and `update-ref`—not the full porcelain CLI. See `AGENT.md` for the full contract and `plan.md` for the task breakdown.

The reference Git source and tests live in the `git/` directory.

## Running tests

From the repository root:

1. **Rust unit and integration tests (workspace)**

   ```bash
   cargo test --workspace
   ```

   Uses the `gust` and `gust-lib` crates (`--release` optional).

2. **Ported Git shell tests (harness)**

   Build the debug binary first (the harness will build `-p gust` if the binary is missing):

   ```bash
   cargo build -p gust
   ./tests/harness/run.sh
   ```

   To use a specific binary:

   ```bash
   GUST_BIN=/path/to/gust ./tests/harness/run.sh
   ```

   Which scripts run is controlled by `tests/harness/selected-tests.txt` (one script name per non-comment line). Individual scripts can be run from `tests/`:

   ```bash
   cd tests
   GUST_BIN=../target/debug/gust sh ./t0001-init.sh
   ```

3. **Where results are recorded**
   - Latest summarized output: [`test-results.md`](test-results.md) (refreshed when plan work updates tests).
   - Plan checklist: [`plan.md`](plan.md); counts and remaining work: [`progress.md`](progress.md).
