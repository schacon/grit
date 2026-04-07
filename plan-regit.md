# Re-Git: no system Git passthrough

Grit must not delegate to the real `git` binary. This document tracks removal of `system_git` / `git_passthrough` and what still needs a native implementation.

## Completed (this branch)

- [x] Removed `grit/src/commands/system_git.rs` and `grit/src/commands/git_passthrough.rs`.
- [x] Added `grit/src/commands/cwd_pathspec.rs` — pure helpers `should_passthrough_from_subdir` and `has_parent_pathspec_component` (no subprocess).
- [x] Replaced every previous passthrough `run` / `run_current_invocation` path with `bail!("not implemented: …")` and specific messages where the gap is narrow (subdirectory, pathspec shape, etc.).
- [x] **`grit rerere`:** Removed the early passthrough that sent default and `forget` to system Git; those paths now use the existing native `cmd_default` / `cmd_forget` (they were unreachable before).
- [x] **`grit fsck --unreachable`:** Enabled — reporting reused existing `Issue::Unreachable` / `show_unreachable` logic.
- [x] **`grit describe --dirty`:** Dirty detection uses `resolve_head`, `diff_index_to_tree`, and `diff_index_to_worktree` (no `git diff-index`).
- [x] **`grit check-mailmap` (`mailmap.blob`):** Reads blob via `resolve_revision` + ODB (no subprocess).
- [x] **`grit bisect` (next step):** Checkouts midpoint with `checkout::detach_head` (no `git checkout`).

## To implement (native), by area

### Entire command had no native body (now errors)

- [ ] **`switch`** — Full branch checkout/switch semantics (ambiguous/worktree pre-checks remain).
- [ ] **`gc`** — Garbage collection.
- [ ] **`repack`** — Repack objects.
- [ ] **`multi-pack-index`** — MIDX write/verify/repack/compact.
- [ ] **`fast-import`** / **`fast-export`** — Stream formats.

### Gaps (previously shimmed; now explicit errors)

- [ ] **`reset`** — `--hard` / `--merge` / `--keep` from a **subdirectory** of the worktree.
- [ ] **`stash`** — From a **subdirectory**.
- [ ] **`revert`** — From a **subdirectory**.
- [ ] **`cherry-pick`** — From a **subdirectory**.
- [ ] **`rm`** — **Subdirectory**; **`..`** in pathspec; **`:^` / `:!`** exclusions; **conflicted** index edge case.
- [ ] **`restore`** — **`--patch`** (interactive).
- [ ] **`clean`** — From **subdirectory**, or pathspec **`.`** / **`..`**.

### Other external-`git` call sites (follow-up)

- [ ] **`clone`** — `REAL_GIT` subprocess (and related transport helpers).
- [ ] **`submodule`** — multiple `REAL_GIT` invocations.
- [ ] **`maintenance`** — `REAL_GIT` for scheduled tasks.
- [ ] **`scalar`** — heavy `REAL_GIT` usage.
- [ ] **`send_email`** — invokes `git` for sendmail-style helpers where applicable.
- [ ] **`test_httpd`** — test binary; optional `git` probe.

## References

- `grit/src/commands/cwd_pathspec.rs` — cwd/pathspec helpers only.
