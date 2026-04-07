# Re-Git: no system Git passthrough

Grit must not delegate to the real `git` binary. This document tracks removal of `system_git` / `git_passthrough` and what still needs a native implementation.

## What “no passthrough” means

- **Forbidden:** Spawning the upstream **`git`** executable for normal command work (the old `REAL_GIT` / `/usr/bin/git` pattern).
- **Allowed:** Spawning **`grit`** (same binary via `current_exe` / `grit_executable()`), user-configured programs (editor, mergetool, credential helpers, hooks via `sh`), and incidental OS utilities (`mktemp`, `kill`, `man`, etc.) where Git does the same.
- **Naming:** Helpers like `should_passthrough_from_subdir` in `cwd_pathspec.rs` refer to **Git’s cwd/pathspec edge cases**, not subprocess delegation.

## Completed (this branch)

- [x] Removed `grit/src/commands/system_git.rs` and `grit/src/commands/git_passthrough.rs`.
- [x] Added `grit/src/commands/cwd_pathspec.rs` — pure helpers `should_passthrough_from_subdir` and `has_parent_pathspec_component` (no subprocess).
- [x] Replaced every previous passthrough `run` / `run_current_invocation` path with `bail!("not implemented: …")` and specific messages where the gap is narrow (subdirectory, pathspec shape, etc.).
- [x] **`grit rerere`:** Removed the early passthrough that sent default and `forget` to system Git; those paths now use the existing native `cmd_default` / `cmd_forget` (they were unreachable before).
- [x] **`grit fsck --unreachable`:** Enabled — reporting reused existing `Issue::Unreachable` / `show_unreachable` logic.
- [x] **`grit describe --dirty`:** Dirty detection uses `resolve_head`, `diff_index_to_tree`, and `diff_index_to_worktree` (no `git diff-index`).
- [x] **`grit check-mailmap` (`mailmap.blob`):** Reads blob via `resolve_revision` + ODB (no subprocess).
- [x] **`grit bisect` (next step):** Checkouts midpoint with `checkout::detach_head` (no `git checkout`).
- [x] **`test-httpd`:** Finds `git-http-backend` via `GIT_EXEC_PATH` and well-known paths only (no `git --exec-path` subprocess).
- [x] **Subdirectory:** Removed early `bail!` for **`stash`**, **`revert`**, **`cherry-pick`**, and **`reset`** when cwd is under a worktree subdir; **`rm`** / **`clean`** subdir guards removed — native paths run.
- [x] **`grit_exe`:** `grit/src/grit_exe.rs` — `grit_executable()` (`current_exe` or `"grit"` on `PATH`).
- [x] **`maintenance` / `scalar` / `clone` (submodules) / `submodule`:** Subprocesses use **`grit_executable()`** instead of `REAL_GIT` / `/usr/bin/git`. Scalar clone no longer uses a separate “real git” binary.
- [x] **`grit submodule status` (recorded OID):** `read_submodule_commit` walks `HEAD` tree in-process (no `ls-tree` subprocess).
- [x] **`grit switch`:** Delegates to **`checkout`** after the same pre-checks; `checkout::Args` has **`Default`** for `rest`-only delegation.
- [x] **`send-email`:** Command removed from the binary; harness skips `t9001-send-email`; completion tests for send-email are prereq-gated (no Perl script delegation).

## Where `grit` is still spawned (self-invocation, not upstream git)

These are intentional: child processes run the **same** `grit` binary for sub-operations that are not yet wired as library calls.

| Area | Role |
|------|------|
| `clone` | Submodule clone / internal `grit` steps |
| `submodule` | fetch, update, sync |
| `maintenance` | scheduled tasks (`gc`, `commit-graph`, etc.) |
| `scalar` | All `git_binary()` calls — function returns **`grit_executable()`** (name is historical) |
| `bisect`, `rebase`, `remote` | Helper subprocesses using `current_exe` / self |
| `for-each-repo` | Runs `current_exe` with `-C` + user command (Git-compatible “git in each repo”) |
| `shell` | `-c` wrapper |

## To implement (native), by area

### Entire command had no native body (now errors)

- [ ] **`gc`** — Garbage collection (`grit gc` currently `not implemented`).
- [ ] **`repack`** — Repack objects (`grit repack` stub).
- [ ] **`multi-pack-index`** — MIDX write/verify/repack/compact.
- [ ] **`fast-import`** / **`fast-export`** — Stream formats.

### Gaps (still explicit errors or partial)

- [ ] **`rm`** — Further pathspec parity (magic length limits, etc.). **Done in this pass:** **`..`** resolved lexically from cwd then under the worktree; **`:^` / `:!`** exclusions; **unmerged / conflicted** paths (all stages removed via `Index::remove`, matches deduped, gitlink detected across stages).
- [ ] **`restore`** — **`--patch`** (interactive).
- [ ] **`switch`** — Full flag parity with upstream `git switch` (currently `checkout` subset via `rest`).

### Follow-up (no external `git`, but not “done” product-wise)

- [ ] **`submodule` / `clone`:** Clone / fetch / checkout still spawn **`grit`**; replace with in-process library calls where possible.

### Suggested order (highest leverage first)

1. **`rm` pathspec gaps** — unblocks many porcelain tests without new storage features.
2. **`gc` / `repack`** — compose existing pack/prune pieces in `grit-lib` where they exist; add missing primitives incrementally.
3. **`switch` parity** — builds on `checkout` and config.
4. **`fast-import` / `fast-export`** — large surface area; schedule after core object workflows are solid.

## References

- `grit/src/commands/cwd_pathspec.rs` — cwd/pathspec helpers only.
- `grit/src/grit_exe.rs` — path to this `grit` binary for child processes.
- `grit/src/commands/scalar.rs` — `git_binary()` → `grit_executable()` (historical name).
