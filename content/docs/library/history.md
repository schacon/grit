---
title: History rewrite
summary: Plan in-memory history edits, then apply ref and worktree updates atomically.
---

The [`history`](rustdoc:grit_lib::revwalk::history) module supports stateless history editing: build a [`RewritePlan`](rustdoc:grit_lib::revwalk::history::RewritePlan) in memory, inspect or dry-run it, then [`apply_rewrite`](rustdoc:grit_lib::revwalk::history::apply_rewrite) to move refs and align the worktree.

## Model

1. **Plan** — [`plan_rewrite`](rustdoc:grit_lib::revwalk::history::plan_rewrite) walks affected commits, writes replacement objects into the ODB, and records ref movements in a [`RewritePlan`](rustdoc:grit_lib::revwalk::history::RewritePlan). Refs, the index, and the worktree are untouched during planning.
2. **Apply** — [`apply_rewrite`](rustdoc:grit_lib::revwalk::history::apply_rewrite) runs a compare-and-swap ref batch, then (in non-bare repos) checks out from the old `HEAD` tree to the new one while preserving unrelated staged and unstaged changes.

[`CommitEdit`](rustdoc:grit_lib::revwalk::history::CommitEdit) describes per-commit rewrites, drops, insert-before commits, reparenting, and patch-id reuse. [`RewriteOptions`](rustdoc:grit_lib::revwalk::history::RewriteOptions) controls scope, empty-commit policy, and the committer ident used for new objects.

[`rebase_onto`](rustdoc:grit_lib::revwalk::history::rebase_onto) replays `upstream..tip` onto a new base in topological order, skipping commits whose stable patch-id already appears on `onto` (unless [`RebaseOntoOptions::skip_applied`](rustdoc:grit_lib::revwalk::history::RebaseOntoOptions::skip_applied) is `false`). Merge commits in the range are refused.

For a single message change, [`reword`](rustdoc:grit_lib::revwalk::history::reword) builds the [`CommitEdit`](rustdoc:grit_lib::revwalk::history::CommitEdit) and runs [`plan_rewrite`](rustdoc:grit_lib::revwalk::history::plan_rewrite) after [`normalize_rewrite_message`](rustdoc:grit_lib::revwalk::history::normalize_rewrite_message) (stripspace, trailing newline, reject empty).

To remove one commit, [`drop`](rustdoc:grit_lib::revwalk::history::drop) inserts [`CommitEdit::Drop`](rustdoc:grit_lib::revwalk::history::CommitEdit) for that OID (rejecting root and merge commits) and plans replay of descendants onto the mapped parent.

To fold a revision range, [`squash`](rustdoc:grit_lib::revwalk::history::squash) resolves the range (connected, single tip), validates fixup/amend/squash markers, builds [`HistoryEdits`](rustdoc:grit_lib::revwalk::history::HistoryEdits) (rewrite oldest with tip tree, drop inner commits), then calls [`plan_rewrite`](rustdoc:grit_lib::revwalk::history::plan_rewrite). Use [`SquashMessage`](rustdoc:grit_lib::revwalk::history::SquashMessage) for oldest, combined, or custom messages.

To apply staged index changes onto an earlier commit, [`fixup`](rustdoc:grit_lib::history::fixup) three-way merges `HEAD`'s tree, the target tree, and the index tree, then plans descendant replay. Call [`apply_fixup_rewrite`](rustdoc:grit_lib::history::apply_fixup_rewrite) (not [`apply_rewrite`](rustdoc:grit_lib::history::apply_rewrite)) so staged paths are consumed and the worktree is updated without treating the fixup staging as conflicting local edits.

[`split`](rustdoc:grit_lib::revwalk::history::split) peels pathspec-selected changes from one commit into a new parent commit via [`CommitEdit::SplitBefore`](rustdoc:grit_lib::revwalk::history::CommitEdit::SplitBefore). The original commit keeps its tree; descendants replay with parent-only updates when their trees are unchanged. Interactive hunk selection is not supported — use pathspecs only. [`SplitOptions`](rustdoc:grit_lib::revwalk::history::SplitOptions) supplies an optional [`rest_message`](rustdoc:grit_lib::revwalk::history::SplitOptions::rest_message) for the remaining commit.

## Apply options and result

[`ApplyOptions`](rustdoc:grit_lib::revwalk::history::ApplyOptions) supplies the reflog identity and message written for every ref update in the batch.

[`AppliedRewrite`](rustdoc:grit_lib::revwalk::history::AppliedRewrite) returns the ref updates that were committed and the commit OID `HEAD` resolves to afterward.

## Dry-run ref lines

[`RewritePlan::ref_update_lines`](rustdoc:grit_lib::revwalk::history::RewritePlan::ref_update_lines) returns `update <ref> <new> <old>` lines suitable for `git update-ref --stdin`, so callers can preview ref motion without applying.

## Errors

[`HistoryError`](rustdoc:grit_lib::revwalk::history::HistoryError) covers planning failures (conflicts, out-of-scope commits, branch checked out elsewhere) and apply failures:

- [`LocalChangesWouldBeOverwritten`](rustdoc:grit_lib::revwalk::history::HistoryError::LocalChangesWouldBeOverwritten) — staged, unstaged, or untracked paths would be clobbered by the planned tree checkout.
- [`RefCasMismatch`](rustdoc:grit_lib::revwalk::history::HistoryError::RefCasMismatch) — a ref tip changed after planning; nothing is written.
- [`HeadMovedSincePlan`](rustdoc:grit_lib::revwalk::history::HistoryError::HeadMovedSincePlan) — `HEAD` no longer points at the ref or commit recorded when the plan was built; nothing is written.
- [`RefApply`](rustdoc:grit_lib::revwalk::history::HistoryError::RefApply) / [`WorktreeApply`](rustdoc:grit_lib::revwalk::history::HistoryError::WorktreeApply) — ref batch or post-ref checkout failed (apply rolls back ref updates when checkout fails).
- [`ApplyRollbackIncomplete`](rustdoc:grit_lib::revwalk::history::HistoryError::ApplyRollbackIncomplete) — checkout failed after refs moved and ref rollback or worktree restore also failed.
- [`EmptyMessage`](rustdoc:grit_lib::revwalk::history::HistoryError::EmptyMessage) — reword message cleanup left an empty body.
- [`SplitSelectsNothing`](rustdoc:grit_lib::revwalk::history::HistoryError::SplitSelectsNothing) / [`SplitSelectsEverything`](rustdoc:grit_lib::revwalk::history::HistoryError::SplitSelectsEverything) — pathspec split covered no changes or every change in the commit.

## Example

Reword one commit on the current branch, plan, and apply:

<!-- include: grit-examples/src/bin/history_reword.rs -->

```bash
cargo run --bin history_reword /path/to/repo <commit-ish> "New message"
```
