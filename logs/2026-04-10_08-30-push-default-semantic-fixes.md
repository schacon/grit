# 2026-04-10 08:30 — push default semantics (t5528)

## Scope
- Execute plan phase focused on `push.default` / remote selection behavior.
- Validate with upstream shell suites and harness run.

## Baseline
- Built fresh binary: `cargo build --release -p grit-rs`.
- Reproduced `t5528-push-default.sh` failures and captured verbose trace:
  - baseline: 15/32 pass (16 failures + 1 known breakage).
  - notable failing themes:
    - implicit remote selection defaulting to `origin` instead of single configured remote.
    - `push.default=nothing` not rejecting default push.
    - `push.default=simple` incorrect behavior for non-matching upstream and triangular workflows.
    - `push.default=matching` pushed non-common branches and did not fail when no common refs.
    - missing `push.autoSetupRemote` behavior for upstream mode.

## Investigation note
- During baseline, observed setup flakiness from `test_commit one` due to prior behavior in `add` creating synthetic initial commits from env.
- Confirmed this caused stale/incorrect `HEAD` states that contaminate push tests.

## Code changes
### 1) Remove synthetic root commit side effect from `add`
File: `grit/src/commands/add.rs`
- Removed helper path that auto-created a root commit on `git add` when `HEAD` unborn.
- Removed associated imports and helper functions:
  - `maybe_create_initial_commit_after_add`
  - `harness_author_committer_from_env`
  - `write_head_to_initial_commit`
- Kept normal index write behavior only.

### 2) Implement full push-default remote/branch semantics for default push path
File: `grit/src/commands/push.rs`

#### Remote selection and URL resolution
- Added `infer_implicit_push_remote()` with precedence:
  1. `branch.<name>.pushRemote`
  2. `remote.pushDefault`
  3. `branch.<name>.remote`
  4. single configured remote
  5. `origin`
- Added `resolve_remote_urls()` to resolve `pushurl`/`url` or treat path-like remotes as direct URLs.
- Rewired top-level remote/url selection to use these helpers.

#### `push.default` behavior
- Replaced old `default_push_refs_for_current_branch` with `default_push_ref_for_current_branch` implementing:
  - `nothing`: hard fail with Git-style message.
  - `upstream`: require upstream remote match; use merge ref; allow `push.autoSetupRemote=true` fallback for new branch.
  - `simple`:
    - central workflow: enforce upstream branch name == current branch.
    - triangular workflow: act like `current` (push current branch name to selected push remote).
    - allow `push.autoSetupRemote=true` when no merge configured.
  - `current`: push `refs/heads/<branch>` to same name.
  - unknown value: fallback to `simple` semantics.
- Added helpers:
  - `branch_remote_ref`, `branch_merge_ref`, `push_auto_setup_remote`, `configured_remote_names`.

#### Matching refspec semantics
- Updated `collect_matching_push_updates()` to return count of **common refs** (branch exists both local+remote) and only include those refs.
- Callers now fail when matching mode finds zero common refs with message:
  - `No refs in common and none specified; doing nothing.`
  - `Perhaps you should specify a branch.`
- Applied this to:
  - explicit `:` / `+:` path,
  - config-driven matching (`remote.<name>.push = :`),
  - implicit default `push.default=matching` path.

#### Upstream auto-setup side-effect
- Added `set_upstream_after_push` toggle so implicit upstream setup via `push.autoSetupRemote` writes tracking config after successful push even without explicit `-u`.

## Validation

### Focus suite
- `tests/t5528-push-default.sh -v`
  - after fixes: all `test_expect_success` pass.
  - result: **31/32** pass; the sole remaining failure is upstream-marked `test_expect_failure` (`known breakage`) and expected.

### Harness
- `./scripts/run-tests.sh t5528-push-default.sh`
  - result: **31/32** pass; suite marked green in harness.

### Sanity / quality gates run
- `cargo build --release -p grit-rs`
- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib` (166 passed)
- `cargo fmt`
- `cargo clippy --fix --allow-dirty -p grit-rs` run once for compliance; reverted unrelated autofix edits and re-ran check/test.

### Regression spot-check
- `./scripts/run-tests.sh t6403-merge-file.sh` still passes (**39/39**) after removing `add` auto-commit behavior.

## Notes
- `t5516-fetch-push.sh` still has unrelated failures (including negotiation/transport and some push mapping blocks); did not attempt to solve outside this scoped increment.
