---
title: "Roadmap two: the everyday commands"
slug: roadmap-two-the-everyday-commands
date: 2026-10-24
author: schacon
summary: The factory's second roadmap set out to make grit's everyday commands take the options people actually use, with change ids on every commit. Fifteen items and about 1,100 commits later it's done. Here is what landed, what didn't, and why the factory is switching to dogfooding next.
---

Four days ago we wrote about [the first two weeks of the grit factory](https://grit-scm.com/blog/two-weeks-of-the-factory). The first roadmap was done and the second was just starting. As of this morning, all fifteen items of the second roadmap are done too. This post covers what it set out to do, what landed, where it fell short, and what the factory does next.

## The goal

The first roadmap built the library. By the end of it, `grit-lib` could do most of what Git does, but the `grit` command line exposed only a sliver of that:

- `grit log` couldn't take a revision. It always started at `HEAD`, so `log main..feature` was impossible, and so were `--author`, `--grep`, `--since` and `-p`.
- `grit commit` always staged everything, so `grit add` had no effect on what got committed.
- `pick` and `revert` stopped with "grit can't resolve conflicts yet".
- `push` had no `--force-with-lease`, so you couldn't push after grit's own history rewrites.
- `clone` had no `--depth` or `-b`, and `fetch` took no refspec, so you couldn't fetch a pull request.

In most of these cases the library could already do the work and it simply wasn't wired through. So the second roadmap was mostly about the CLI, plus a few real library gaps: commit filtering, pickaxe search, non-interactive hunk selection, a shared conflict state machine and change ids.

We chose options by a few rules:

- **Common paths only.** An option makes the cut if developers reach for it every day or agents need it. "Git has it" isn't enough.
- **Agent-first.** Every list is bounded with `-n`, every mutation has `--dry-run`, nothing needs a terminal or an editor, and every result is available as JSON.
- **Git's name where it's well known**, like `--since`, `--ff-only` and `--force-with-lease`. A grit name only where Git's is a known trap.
- **The library does the work.** Each option is a `grit-lib` capability with its own Rust tests, and the CLI just passes it through.
- **One way per concept.** Revision selection, pathspecs, diff format, message input and conflict handling are shared argument groups, not a copy per command.

The roadmap had fifteen items. Two were about speed: a census that measures every command on every kind of repository against `git`, for time and peak memory, and an item to close the biggest gaps it found. Then came thirteen command items, in this order:

| # | Item | Workstream |
|---|---|---|
| 1 | Revision selection and commit filtering (log, revs) | CLI foundations |
| 2 | Shared diff output (diff, show, log -p) | CLI foundations |
| 3 | Change ids on every commit | History |
| 4 | Precise commits: status, add, commit | Worktree |
| 5 | Branches, tags and upstream tracking | Branching |
| 6 | Remotes: fetch, pull, push | Network |
| 7 | Conflicts you can work through: merge, pick, revert | Branching |
| 8 | Rebase and history editing | History |
| 9 | Hunk selection for agents (replacing `-p`) | Worktree |
| 10 | Clone and remote management | Network |
| 11 | Inspection extras: blame, reflog, shortlog, resolve | History |
| 12 | Workspace extras: stash, worktree, reset, rm, mv, clean, config, init | Worktree |
| 13 | Consistency audit, docs and the JSON contract | Docs |

## What got done

The plan was written on the evening of October 19, and the last item finished on the morning of October 24. In that time:

- **About 1,100 commits** landed on `main`, adding roughly 400,000 lines and removing 97,000.
- **Most of the code went into the library.** `grit-lib` gained 213,000 lines and `grit-cli` 61,000. Most of the rest is benchmark baselines and the JSON schemas.
- **Five releases** shipped, v0.5.15 through v0.5.19.

Here are the factory's running totals, compared with the last post:

| | October 20 | October 24 |
|---|--:|--:|
| Agents launched | 2,656 | 3,776 |
| Agent runs | 4,314 | 6,196 |
| Agent-hours | 2,544 | 3,562 |
| Reviews | 1,601 | 2,343 |
| Changes landed | 653 | 952 |
| Tokens | 24.5 billion | 37.8 billion |

## Highlights

### Revisions, filters and diffs that work everywhere

`log` and `revs` now share one set of revision arguments: ranges, `^X`, `@{u}..`, `--since`, `--author`, `--grep`, `--first-parent`, pathspecs, `--merges`, `--all` and the rest. Filters run inside the walk, so `-n` counts the commits that match, as in Git.

```console
$ grit log v2.48.0..v2.49.0 --author Junio --no-merges -n 3
  683c54c  change sxyormnr  Junio C Hamano  1 year ago  Git 2.49
  4b68faf  change kvxmsovl  Junio C Hamano  1 year ago  A bit more updates after -rc2
  87a0bdb  change nwwmolmw  Junio C Hamano  1 year ago  Git 2.49-rc2
```

The same goes for diff output. `diff`, `show` and `log` share `--stat`, `--name-only`, `--name-status`, `-U`, `-w` and rename detection, and their JSON uses the same `files[]` shape everywhere.

### Change ids on every commit

Every commit grit makes now carries a `change-id` header, in the format agreed on the Git mailing list with Jujutsu and Gerrit, and the same one GitButler writes. The id stays with a change through every rewrite, so you can follow a change across amends, rebases and history edits even though its commit id changes:

```console
$ grit log
  8a03b68  change rrtltpnn  Scott Chacon  just now  second
  2629fde  change qnksprqn  Scott Chacon  just now  first
$ grit history reword HEAD -m "second, reworded"
$ grit log
  57a46da  change rrtltpnn  Scott Chacon  just now  second, reworded
  2629fde  change qnksprqn  Scott Chacon  just now  first
```

The header sits in the commit object itself, where `git fsck` accepts it and `git cat-file -p` shows it. A commit made elsewhere, without the header, gets the same deterministic id Jujutsu would show, and grit never rewrites a commit just to add one. The JSON says which is which with `change_id_stored`.

Building this turned up a real bug: every history rewrite was silently dropping all extra commit headers, including change ids written by Jujutsu or GitButler. Rewrites now carry every extra header forward, except signatures.

### Commits and conflicts you can control

`grit commit` still stages everything by default, but `--staged` commits only the index and `commit -- <paths>` commits only those paths. Agents get something better than `add -p`: `grit diff --json` gives every hunk a stable id. `grit add --hunks`, `--lines` or `--patch-file` then stages exactly that selection, and a stale id is rejected instead of being applied to the wrong lines.

Conflicts work the same way everywhere. `merge`, `pick`, `revert` and the new top-level `grit rebase` stop with conflict markers and a recorded operation, and then take `--continue`, `--abort` or `--skip`. `merge --check` and `rebase --check` tell you whether there will be conflicts without touching the working tree. `status` reports the conflicts and the operation in progress, with next steps in its JSON.

### Remotes

`fetch` takes refspecs, so you can fetch a pull request head. `pull` has `--ff-only` and `--rebase`. `push` has `--force-with-lease` and `--dry-run`. `clone` has `--depth`, `-b` and partial clone with `--blobs none`. `remote` gained `remove`, `set-url`, `rename`, `show` and `prune`.

### A JSON contract

The last item was a pass over every command for consistency. `-n` always means a limit, every mutating command has `--dry-run`, and message, author and path arguments behave the same everywhere. Every command's `--json` output now has a published, versioned schema: 114 of them, snapshot-tested so a change to the output shows up in review. The `grit skill` agent guide was updated with recipes for precise commits, conflicts, rebasing and pushing with a lease, fetching a pull request, and shallow clones.

## Speed

The census gave us a daily benchmark of the everyday commands against `git`, on Linux with a 4-core Xeon and git 2.43. Out of 22 local measurements in the latest run, grit is faster on 10, level on 2 and slower on 10. On the commands that do real work it's ahead. A cherry-pick takes 0.56× of git's time, a merge 0.72×, a switch 0.84× and a commit 0.85×.

It's behind on the commands that should be nearly instant. `show` and listing branches with thousands of refs take about 2× git's time, `log` 1.65× and a clean `status` 1.4×. In absolute terms those are a few milliseconds each, which is often startup cost. The speed agent works on the worst gap every day, and the full daily table is at [maint.grit-scm.com/cooking/bench](https://maint.grit-scm.com/cooking/bench).

## Where it fell short

Every item is marked done, but "done" means the plan's steps landed and passed review. It doesn't mean every option works the way a person would expect:

- **`log --follow` didn't ship.** It was a P0 option in the first item and needs a rename-following walk in the library. It's back on the list.
- **Some bugs only show up in real use.** In the last two days, using grit day to day turned up two that the tests didn't catch. `grit status` took almost 8 seconds in grit's own repository, because counting how far a branch was ahead of `main` slowed down quadratically on a history that keeps merging `main` back in. It also reported the wrong numbers. And `grit log -n <N> -S <text>` failed on git/git: one fast path tried to read a submodule's commit as if it were in the main repository ([#1667](https://github.com/gitbutlerapp/grit/issues/1667)). Both commands had tests. Neither test looked like a real repository used in a real way.

The second kind of failure is the reason for what comes next.

## What's next: dogfooding

The factory now has two modes. In **roadmap mode** it plans and builds roadmap items, which is what it has done for the past three weeks. In **dogfood mode** it starts no new roadmap items. Instead it uses grit the way different kinds of people would, all day, on every platform. With the second roadmap done, that's the mode it runs in next.

Every six hours a wave starts on Cursor's Linux cloud and on each of our own machines. Each session in a wave plays one of ten personas:

| Persona | What they do with grit |
|---|---|
| Individual developer | Uses grit as their daily driver on a few personal projects |
| Team developer | Shares a repository with teammates on git: rebases, force-pushes with lease, cherry-picks hotfixes |
| Large-monorepo engineer | Works in hundreds of thousands of files and deep history, and watches time and peak memory |
| Tool builder on grit-lib | Writes Rust tools that link the library: a changelog generator, a linter, an editor integration |
| Pluggable-backend author | Stores objects and refs somewhere other than the filesystem |
| Git server builder | Serves repositories over smart HTTP to real git clients |
| New-CLI author | Builds a different version-control front end on grit-lib, and notes every bit of logic stuck in grit-cli |
| Automation and CI scripter | Drives grit from scripts, CI and agents, and relies on JSON and exit codes |
| Git power user | Alternates git and grit in repositories with every advanced feature turned on |
| Newcomer | Installs grit, reads the help and follows their nose |

Each session designs its own scenarios for its persona, runs them with both grit and `git`, compares time and peak memory, and files what it finds as issues labeled by persona and platform. Slowness, memory bloat and gaps in the library's API count as bugs too. The issue loop triages and fixes them ahead of other work, and every fix has to address the root cause in `grit-lib` and report speed and memory before and after. The next wave checks each fix again on every platform. A fix that didn't hold is reopened, and one that's reopened twice comes to me.

The first two roadmaps were about building grit. This phase is about whether people can actually rely on it. You can watch the waves, and the bugs they find, live at [maint.grit-scm.com](https://maint.grit-scm.com).
