## 2026-04-05 — t6136-pathspec-in-bare

### Claim
- Marked `t6136-pathspec-in-bare` as in progress in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t6136-pathspec-in-bare.sh` initially reported failures.
- Direct traced run showed:
  - `git log -- ..` in bare repo returned success instead of failing.
  - `git ls-files -- ..` failed, but message did not include expected phrase
    "outside repository".

### Root causes
1. `grit/src/commands/log.rs`
   - `log` accepted pathspecs without validating they are inside the repository
     work tree context.
   - In bare repos (or from `.git`), invalid pathspecs like `..` should fail
     with an outside-repository diagnostic, but `log` proceeded and exited 0.

2. `grit/src/commands/ls_files.rs`
   - In bare repos, `ls-files` failed early with:
     `error: cannot ls-files in bare repository`
   - This message did not satisfy `t6136`, which expects diagnostics to mention
     "outside repository" for pathspec scope errors.

### Fixes applied
1. `grit/src/commands/log.rs`
   - Added `validate_log_pathspecs(repo, &args.pathspecs)` in `run()`.
   - Validation enforces:
     - bare repos reject any pathspec with
       `error: pathspec '<spec>' is outside repository`
     - non-bare repos reject pathspecs that normalize outside the work tree.
   - Returns failure before traversal when pathspec scope is invalid.

2. `grit/src/commands/ls_files.rs`
   - Changed bare-repo guard to emit:
     - `error: pathspec '<spec>' is outside repository` when pathspecs provided.
     - fallback `error: cannot ls-files in bare repository` when no pathspec.
   - Keeps previous behavior where appropriate while satisfying pathspec tests.

### Validation
- `./scripts/run-tests.sh t6136-pathspec-in-bare.sh` → **3/3 pass**.
- Direct run:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit timeout 120 bash tests/t6136-pathspec-in-bare.sh`
  - Result: **3/3 pass**.
- Regression checks:
  - `./scripts/run-tests.sh t6134-pathspec-in-submodule.sh` → **3/3 pass**.
  - `./scripts/run-tests.sh t6114-keep-packs.sh` → **3/3 pass**.

### Tracking updates
- Marked `t6136-pathspec-in-bare` complete in `PLAN.md` (3/3).
- Updated `progress.md` counts and recently completed list.
- Updated `test-results.md` with `t6136` pass evidence.
