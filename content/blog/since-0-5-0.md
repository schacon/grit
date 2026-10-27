---
title: What grit shipped since 0.5.0
slug: since-0-5-0
date: 2026-10-27
author: schacon
summary: Three weeks and 22 releases after v0.5.0, grit is a library first, has the everyday commands people actually use, matches git's speed on a typical command, and has new ideas like change ids and hunk selection for agents. Here is the short version, with a link to the full report.
---

grit v0.5.0 came out on June 18. It was the last release made by hand. On October 6 the [grit factory](https://grit-scm.com/blog/the-grit-maintenance-factory) took over: cloud agents plan, write, review, land and release grit, and a human maintainer sets the direction. Three weeks later we're at v0.5.22.

We put together a report of everything that happened in between, with charts for the daily work, the roadmap, speed, the library and every release: **[maint.grit-scm.com/since-0.5.0](https://maint.grit-scm.com/since-0.5.0)**. Here are the highlights.

## In numbers

- **22 releases**, one nearly every day, from v0.5.1 to v0.5.22.
- **3,970 commits** on `main`, from 1,199 changes, each one reviewed by a second agent before it landed.
- **36 roadmap items**, across two roadmaps, all done.
- **8,021 Rust tests**, up from 549.

## grit is a library first

At v0.5.0, most of grit was a 281,000-line legacy binary, and `grit-lib` held about a third of the code. The legacy crate was deleted on the first day. Every command was then rebuilt on top of `grit-lib`, and `grit-cli` became a thin layer that renders what the library returns. Today the library is 89% of the code.

Along the way the library lost its global state, its printing and its `exit()` calls. It gained a typed `Repository` API with a checked public-API snapshot, and pluggable object and ref storage, so you can keep a repository somewhere other than the filesystem. A standing quality agent keeps going: it moves logic out of the CLI, removes duplicate code and dead code, and types the errors.

## The everyday commands

The second roadmap wired the library through to the commands people use every day. That covers `log` with real revision ranges and filters, shared `diff`/`show`/`log -p` output, and precise `status`, `add` and `commit`. It also covers branches and tags with upstream tracking, `fetch`/`pull`/`push` with `--force-with-lease`, `clone --depth`, a top-level `rebase`, and `stash`, `worktree`, `blame` and `reflog`. Every command has a published, versioned `--json` schema, and every mutating command has a `--dry-run`.

## New ideas, not just a git clone

- **Change ids on every commit.** They follow a change through amends, rebases and history edits, in the format shared with Jujutsu, Gerrit and GitButler.
- **Hunk selection for agents.** `grit diff --json` gives every hunk a stable id, and `add`, `commit`, `restore` and `stash` take exactly that selection. It replaces `-p`, which an agent can't drive.
- **`grit history`** rewrites commits directly (reword, split, drop, move) without starting an interactive rebase.
- **Conflicts you can work through.** `merge`, `pick`, `revert` and `rebase` share one conflict state that the CLI and the library can both inspect, resolve and continue.

## Speed

A daily benchmark times every supported command against stock `git`, on several repository shapes. When it started on October 20, the typical grit command took 2.0× git's time. On October 26 it was 1.06×, and grit was faster than git on 62 of 141 scenarios. Compared with v0.5.0 itself, a clean `status` is 5× faster, and `commit`, `diff` and `merge` are 4–5× faster. An incremental `fetch` went from 76 seconds to 14 milliseconds.

## Since then: dogfooding

With both roadmaps done, the factory now spends its time [using grit the way different people would](https://grit-scm.com/blog/roadmap-two-the-everyday-commands), on Linux, macOS and Windows. Those sessions have filed 418 issues so far, and the agents have closed 383 of them with fixes. The full numbers, and how the factory gets a change from an idea to `main`, are in [the report](https://maint.grit-scm.com/since-0.5.0).
