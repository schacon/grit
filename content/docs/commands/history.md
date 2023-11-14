---
title: grit history
summary: Stateless history rewrite helpers (reword, and more over time).
group: History
order: 6
---

## Synopsis

```text
grit history drop <commit> [--dry-run] [--update-refs=(branches|head)]
grit history reword <commit> -m <message>
grit history reword <commit> --message-file <path|->
grit history reword <commit> [--dry-run] [--update-refs=(branches|head)]

grit history rebase [<branch>] --onto <rev> [--upstream <rev>]
grit history rebase [<branch>] --onto <rev> [--keep-applied] [--empty=(drop|keep|abort)]
grit history rebase [<branch>] --onto <rev> [--dry-run] [--update-refs=(branches|head)]
```

## Description

`grit history` groups **stateless** rewrite operations: grit plans how commits and refs should change, writes new commit objects into the object database, then applies ref updates in one atomic batch. There is no sequencer state file and **no Git hooks** run during planning or apply.

Shared behavior for every subcommand:

- **Conflicts abort** — if replaying a descendant would conflict, nothing is applied (planned objects may already exist in the ODB from planning).
- **`--dry-run`** — print which refs would move; refs, the index, and the worktree stay unchanged. New commit objects are still written during planning, matching Git’s `git history --dry-run` model.
- **`--update-refs`** — `branches` (default) updates every local branch whose tip descends from the edited commit; `head` updates only the commit `HEAD` points at.

### `drop`

Remove one commit from history and replay its descendants onto that commit’s first parent. The dropped commit’s tree changes are omitted; descendant commits are replayed with three-way merges like other history rewrites.

You cannot drop a **root** commit (no parent) or a **merge** commit. If a descendant is a merge, planning refuses (merge replay is not supported yet).

### `reword`

Replace one commit’s message and replay descendants onto the rewritten history. Author, tree, and parents stay the same on the edited commit; committer identity and time come from your configured committer and `GIT_COMMITTER_DATE`, like [`grit commit`](commit.md).

Provide exactly one of `-m` / `--message` or `--message-file` (use `-` to read from stdin). Grit does not open an editor.

If any descendant commit is a **merge**, planning refuses with an error (merge replay is not supported yet). Use a linear range or wait for later `grit history` subcommands that handle merges.

### `rebase`

Replay commits reachable from `<branch>` (or the current branch) but not from `--upstream`, onto `--onto`. This is a **stateless** rebase: there is no `.git/rebase-merge` directory and no step-by-step resume. Grit plans the full replay, writes new commit objects, then moves refs in one batch (including stacked local branches when `--update-refs=branches`, similar to Git’s `--update-refs`).

Commits whose patch already appears on `--onto` are **skipped** by default (Git-style cherry detection using stable patch-ids). Pass `--keep-applied` to force replay anyway.

`--upstream` defaults to the branch’s configured upstream tracking ref (`branch.*.remote` / `branch.*.merge`), or to `--onto` when no upstream is configured.

Unlike [`grit pull`](pull.md), which fetches and integrates a remote tracking branch into your current branch (fast-forward or merge), `grit history rebase` only rewrites **local** history already in your repository onto a new base you name. It does not contact remotes.

Merge commits in the replay range are refused. Conflicts abort before any ref moves.

## Options

| Option | Description |
| --- | --- |
| `<commit>` | Commit to drop or reword (revision spec). |
| `--dry-run` | List ref updates without moving refs. |
| `--update-refs` | `branches` (default) or `head`. |
| `-m`, `--message` | (`reword` only) New commit message (conflicts with `--message-file`). |
| `--message-file` | (`reword` only) Read the new message from a file, or `-` for stdin. |

### `reword` options

| Option | Description |
| --- | --- |
| `<commit>` | Commit to reword (revision spec). |
| `-m`, `--message` | New commit message (conflicts with `--message-file`). |
| `--message-file` | Read the new message from a file, or `-` for stdin. |

### `rebase` options

| Option | Description |
| --- | --- |
| `[<branch>]` | Branch to rebase (defaults to the current branch). |
| `--onto` | New base for replayed commits (revision spec). |
| `--upstream` | Exclude commits reachable from this revision (defaults to upstream tracking, else `--onto`). |
| `--keep-applied` | Replay commits even when the patch is already on `--onto`. |
| `--empty` | `drop` (default), `keep`, or `abort` when a replay would become empty. |
| `--dry-run` | List ref updates without moving refs. |
| `--update-refs` | `branches` (default) or `head`. |
## Examples

Drop the parent of `HEAD` on a linear branch:

```console
$ grit history drop HEAD~1
Dropped 1 commit, replayed 1 descendant commit(s)
  commit d4e5f60 → c0ffee0
  refs/heads/main f6a7b89 → b1b2b3b
```

Reword the parent of `HEAD` on a linear branch:

```console
$ grit history reword HEAD~1 -m " clearer subject line "
Rewrote 2 commit(s)
  commit a1b2c3d → d4e5f60
  refs/heads/main d4e5f60 → f6a7b89
```

Preview ref movements without updating branches:

```console
$ grit history reword HEAD~1 -m "new message" --dry-run
Dry run — refs would change:
  commit a1b2c3d → d4e5f60
  refs/heads/main f6a7b89 → c0ffee0
```

Rebase `feature` onto `main` while skipping patches already on `main`:

```console
$ grit history rebase feature --onto main
Rewrote 2 commit(s)
  commit a1b2c3d → d4e5f60
  refs/heads/feature f6a7b89 → c0ffee0
```

## JSON output

Drop schema: [v1](/schemas/history.drop/v1.json) — pass `--json` on `grit history drop`.

Reword schema: [v1](/schemas/history.reword/v1.json) — pass `--json` on `grit history reword`.

Rebase schema: [v1](/schemas/history.rebase/v1.json) — pass `--json` on `grit history rebase`.

All subcommands emit the same object shape. Pass `--json` on `grit history drop`, `grit history reword`, or `grit history rebase`:

| Field | Type | Meaning |
| ----- | ---- | ------- |
| `operation` | string | `"drop"`, `"reword"`, or `"rebase"`. |
| `rewritten` | array | `{old,new}` full hex OIDs for each replayed commit (`drop` omits the removed commit). |
| `refs` | array | `{name,old,new}` for each ref that would move or moved. |
| `head` | object or null | `{old,new}` when `HEAD`'s commit OID changes; `null` when unchanged. |
| `dry_run` | boolean | `true` when `--dry-run` was set. |

Example:

```json
{
  "operation": "reword",
  "rewritten": [
    {
      "old": "6e513d5cd6ab8d618238e22d7ca55d942b55964a",
      "new": "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d"
    }
  ],
  "refs": [
    {
      "name": "refs/heads/main",
      "old": "9f8e7d6c5b4a392817061524334352637281910",
      "new": "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567"
    }
  ],
  "head": {
    "old": "9f8e7d6c5b4a392817061524334352637281910",
    "new": "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567"
  },
  "dry_run": false
}
```

On replay conflicts, stdout is a single JSON object (exit code 1) with `error`, `kind` (`conflict`), and `conflicts` (sorted paths), same shape as [`grit pick`](pick.md).

## Markdown output

Schemas: [drop v1](/schemas/history.drop/v1.json), [reword v1](/schemas/history.reword/v1.json)

Pass [`--markdown`](../global-options/) for a `History drop` or `History reword` heading, replayed commits, ref updates, and optional `HEAD` movement.

## See also

[Library guide: History rewrite](../library/history.md), [`grit pull`](pull.md), [`grit log`](log.md), [`grit pick`](pick.md)
