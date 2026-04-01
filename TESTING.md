## Running tests

From the repository root:

1. **Rust unit and integration tests (workspace)**

   ```bash
   cargo test --workspace
   ```

   Uses the `grit` and `grit-lib` crates (`--release` optional).

2. **Ported Git shell tests (harness)**

   Build the debug binary first (the harness will build `-p grit` if the binary is missing):

   ```bash
   cargo build -p grit
   ./tests/harness/run-all.sh
   ```

   To use a specific binary:

   ```bash
   GUST_BIN=/path/to/grit ./tests/harness/run-all.sh
   ```

   Individual scripts can be run from `tests/`:

   ```bash
   cd tests
   GUST_BIN=../target/debug/grit sh ./t0001-init.sh
   ```

3. **Where results are recorded**
   - Latest summarized output: [`test-results.md`](test-results.md) (refreshed when plan work updates tests).
   - Plan checklist: [`plan.md`](plan.md); counts and remaining work: [`progress.md`](progress.md).
