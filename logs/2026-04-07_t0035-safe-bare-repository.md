## t0035-safe-bare-repository — completion log (2026-04-07)

### Goal
Make `tests/t0035-safe-bare-repository.sh` fully pass.

### Baseline
- `./scripts/run-tests.sh t0035-safe-bare-repository.sh` → **1/12**.
- Initial hard blocker in setup:
  - `error: HEAD does not point to a valid commit; specify a branch`
  - from `git -C outer-repo worktree add ../outer-secondary` in an unborn repository.
- Additional mismatches after setup:
  - `safe.bareRepository=all` still rejected implicit bare repo access,
  - submodule admin path check expected `.git/modules/subn` but implementation created `.git/modules/subd`.

### Root causes
1. **Worktree add on unborn HEAD**
   - `worktree add <path>` required a valid `HEAD` commit and errored on unborn repos.
   - Upstream Git infers an implicit orphan branch in this case.

2. **Implicit bare-repo trace + bare classification mismatch**
   - Accepted/rejected implicit bare repository probes in `t0035` expect the perf trace marker `implicit-bare-repository:<path>`; this marker was not emitted.
   - Policy checks were applied before opening the candidate repository, so `.git` directories and worktree admin dirs could be misclassified as bare repositories under `safe.bareRepository=explicit`.

3. **Submodule `--name` path mapping**
   - `submodule add --name subn ... subd` used path (`subd`) for `.git/modules/<...>`.
   - Upstream uses module **name** (`subn`) for storage path.

### Implementation
1. **Implicit orphan for unborn worktree add**
   - `grit/src/commands/worktree.rs`
   - Resolve HEAD state before branch-target selection.
   - If no explicit branch mode flags are given and HEAD is unborn, treat `worktree add <path>` as implicit orphan creation.
   - Reused orphan setup flow; branch name defaults to worktree basename.

2. **Trace + policy alignment for implicit bare discovery**
   - `grit-lib/src/repo.rs`
   - Emit `implicit-bare-repository:<path>` lines to `GIT_TRACE2_PERF` destinations for implicit bare discovery paths.
   - Open the repository first, then enforce `safe.bareRepository=explicit` only when `repo.is_bare()` is true.
   - This keeps explicit-policy rejection for actual bare repositories while allowing `.git` and worktree/submodule admin directories.

3. **Named submodule admin directory**
   - `grit/src/commands/submodule.rs`
   - In `run_add`, changed modules-dir path from `.git/modules/<path>` to `.git/modules/<name>`, where `name` honors `--name`.

### Validation
- `cargo fmt` ✅
- `cargo build --release -p grit-rs` ✅
- `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t0035-safe-bare-repository.sh` ✅ 12/12
- `./scripts/run-tests.sh t0035-safe-bare-repository.sh` ✅ 12/12
- regressions:
  - `./scripts/run-tests.sh t1407-worktree-ref-store.sh` ✅ 4/4
  - `./scripts/run-tests.sh t0095-bloom.sh` ✅ 11/11
- quality gates:
  - `cargo clippy --fix --allow-dirty` ✅
  - `cargo test -p grit-lib --lib` ✅ 98/98
