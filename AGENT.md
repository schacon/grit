---
description: "Functionally complete Git reimplmentation in idiomatic, library focused Rust"
alwaysApply: true
---

# AGENT.md

A complete rewrite of Git in idiomatic, library focused Rust code. This file is the durable build contract for autonomous runs.

## Product intent

Gust is a complete reimplementation of the Git version control project in Rust, written via an autonomous coding loop in Codex.

Success is reimplementing all of the Git command line tools such that they can pass the entire unit test suite in the Git project.

## Source of truth

The canonical Git source code we're targeting to replicate the functionality of is in the `git/` subdirectory.

The tests we're trying to make pass with our new implementation is in the `git/t/` directory.

Manpage documentation is located in `git/Documentation` directory as `*.doc` files.

## Looping rules

There may be several agents working in this directory to coordinate implementation. The prioritized execution plan is in `plan.md` and should be a prioritized list of functionality that should work, grouped by command (such as `git commit`). Each command will have a detailed list of subfunctionality to implement and pass testing.

As you run, find something in the list that starts with a blank checkbox `[ ]` and claim it by marking it as `[~]`. When it appears to pass the associated tests, mark it as done with `[x]`. For each task you take, keep a log of your work in `logs/` as a timestamped log file (such as `2026-03-31_05:30-git-add-simple.md`).

## Loop contract

On each iteration:

1. Read the `plan.md` and this file.
2. Pick exactly one highest-value remaining item.
3. Search the codebase before assuming it is missing.
4. Read the tests in `git/t/` and determine which are related.
5. Read the documentation for the command in `git/Documentation`.
6. Implement the functionality you are focusing on for that subcommand.
7. Update `plan.md` to reflect reality. Whenever you change any task checkbox there, update **`progress.md`**: total tasks, completed count, and a short list of what remains (derive counts from `plan.md` lines matching `- [x]` / `- [ ]` / `- [~]` under task headings).
8. After meaningful test runs, refresh **`test-results.md`** with a concise summary of `cargo test --workspace` and `./tests/harness/run.sh` (or note what was skipped).
9. Update this file only if you discover durable run/build/test knowledge.
10. Update the log for this task as you go.
11. Commit if the increment is coherent and validated.
12. Immediately continue to the next item unless the repo is truly complete, blocked, unsafe, or user-stopped.

Do not stop just because you reached a nice milestone.

## Completion rule

The loop is only complete when the v1 subcommands are fully implmented, pass all associated tests, and fulfill the documentation.

If stopping, state one exact reason:

- `complete`
- `blocked`
- `unsafe`
- `user-stopped`

## Rust Style and Idioms

- Use traits for behaviour boundaries.
- Derive `Default` when all fields have sensible defaults.
- Use concrete types (`struct`/`enum`) over `serde_json::Value` wherever shape is known.
- **Match on types, never strings.** Only convert to strings at serialization/display boundaries.
- Prefer `From`/`Into`/`TryFrom`/`TryInto` over manual conversions. Ask before adding manual conversion paths.
- **Forbidden:** `Mutex<()>` / `Arc<Mutex<()>>` — mutex must guard actual state.
- Use `anyhow::Result` for app errors, `thiserror` for library errors. Propagate with `?`.
- **Never `.unwrap()`/`.expect()` in production.** Workspace lints deny these. Use `?`, `ok_or_else`, `unwrap_or_default`, `unwrap_or_else(|e| e.into_inner())` for locks.
- Prefer `Option<T>` over sentinel values.
- Use `time` crate (workspace dep) for date/time — no manual epoch math or magic constants like `86400`.
- Prefer guard clauses (early returns) over nested `if` blocks.
- Prefer iterators/combinators over manual loops. Use `Cow<'_, str>` when allocation is conditional.
- **No banner/separator comments.** Do not use decorative divider comments like `// ── Section ───`. Use normal `//` comments or doc comments to explain _why_, not to visually partition files.

## Dependencies

- **Do not use `gix` (gitoxide) or `git2` (libgit2).** This should be a clean reimplementation of Git and not rely on any other existing libraries.
- You may introduce any other stable Rust libraries that improve the process (such as for SHA1 hashing or command line parsing).

## Architecture and Design

- For code that you create, **always** include doc comments for all public functions, structs, enums, and methods and also document function parameters, return values, and errors.
- Documentation and comments **must** be kept up-to-date with code changes.
- Avoid implicitly using the current time like `std::time::SystemTime::now()`, instead pass the current time as argument.
- Keep public API surfaces small. Use `#[must_use]` where return values matter.

## Library crate layout and public API

The Git-compatible engine should live in a **library crate** (`grit-lib`); the **`grit` binary** should stay a thin layer: parse CLI, open a `Repository` (or equivalent), call library APIs, map `grit_lib::Error` to exit codes and stderr. Agents should implement features in the library first and only wire them through the binary.

### When to use one crate vs several

- **Start with one library crate** plus the binary crate in a workspace unless a split is clearly needed. Prefer **modules** (`objects`, `index`, `refs`, `odb`, `tree`, `worktree`, …) for boundaries before adding more crates.
- **Split into additional library crates** when there is a stable boundary that yields real benefit: faster incremental builds for huge code, optional `#[cfg(feature = …)]` surfaces, or a subsystem that tests/tools want to depend on without pulling the whole repo stack. Avoid many tiny crates without a strong reason.
- **Integration tests** and future callers (benchmarks, fuzz targets) should depend on the **library**, not on private modules of the binary.

### What the library API should look like

- **Entry type:** Expose a single primary handle (e.g. `Repository`) obtained by opening a path or an explicit `GitDir` + work tree. Most operations are methods on that type or on focused borrows (`repo.index()`, `repo.odb()`) so callers do not thread global state.
- **Typed operations, not argv:** Public APIs take enums and newtypes (`ObjectId`, `RefName`, modes, tree entry kinds), not unparsed CLI strings. Parsing human-facing strings belongs at the CLI boundary.
- **Explicit context:** Time, randomness, and environment (e.g. `HOME`, config discovery) are **arguments or injectable providers**, not hidden `std::env` reads inside deep library calls—so the library stays testable and matches the rule against implicit “now” in core logic.
- **Errors:** Library uses **`thiserror`** enums with specific variants per failure mode; binary may wrap with `anyhow` for top-level reporting. Do not leak stringly “Git stderr” shapes from the library as the only error type.
- **IO boundaries:** Prefer passing `&mut dyn Read` / `Write` / `AsRef<Path>` where streams matter; for whole-repo operations, centralize filesystem access enough that tests can use temp dirs or in-memory backends without reimplementing commands.
- **Visibility:** Default to `pub(crate)` and lift to `pub` only when part of the supported API. Use `#[doc(hidden)]` sparingly for compatibility shims, not to hide a messy surface.
- **Stability mindset:** Treat the library as a long-lived API: avoid `pub` reexports of entire dependency modules; prefer small, documented extension points (traits) only where Git’s own abstraction demands it.

### Traits and boundaries

- Use **traits** for behaviors that must vary (e.g. object storage backend, ref storage, optional fsmonitor-style hooks) or for non-consuming extension points—not for every struct.
- Keep “plumbing” operations as **coherent methods** on the appropriate type (`Index::write_tree`, `Odb::hash_object`) rather than a flat bag of free functions, unless a function group is truly stateless.

## Testing

- As tests in `git/t/` are being implemented, copy them to `./tests` and run them from there with `grit` aliased to `git` for the purposes of the tests.
- Do not write or run tests that are not from this directory.
- **Never run tests inside the main repo** — always use `/tmp/` scratch directories to avoid corrupting the working tree, index, or refs.
- Run upstream test suite: `bash scripts/run-upstream-tests.sh` (runs all `git/t/` tests against grit in an isolated `/tmp/` directory)
- Aggregate results: `bash scripts/aggregate-upstream.sh`
- Run our own tests: `bash tests/harness/run-all-count.sh`
- Run benchmarks: `bash bench/run.sh`
- Update dashboard after test runs: `bash scripts/update-dashboard.sh`

## Current State (as of 2026-04-02)

- **143 commands** implemented (100% of git's command-list.txt, excluding GUI/docs/foreign-SCM)
- **21.6% upstream test pass rate** (6,117 / 28,266 tests from git/t/)
- **720 custom test files** with 24,914 test cases (99.9% pass rate)
- **40 benchmarks** comparing grit vs C git
- Biggest gaps: config parsing (308 failures), for-each-ref format atoms (398), cat-file batch mode (218), diff output formatting (1315)

## Committing and Version Control

- **Do not use GitButler** (`but`) or GitButler MCP for this project. Use plain **`git`** on branch **`main`** only.
- Commit after every subagent completes its scoped task: stage the files that subagent changed, write a clear message describing that work, then `git commit`.
- **After every successful commit** that concludes a subagent handoff, run **`git push origin main`** so the remote stays backed up (skip only if `origin` is missing or push is impossible—then say so in the log).
- Before committing, always run `cargo fmt` and `cargo clippy --fix --allow-dirty` and ensure no warnings remain.
- After running passing tests, run scripts/update-dashboard.sh

## Parallel Agent Workflow

When running multiple subagents in parallel:

1. **Always use git worktrees** — each agent gets its own worktree to avoid file conflicts and cargo lock contention:
   ```bash
   git worktree add -b <branch> /home/hasi/grit-worktrees/<agent-name> main
   ```
2. **Set CARGO_TARGET_DIR** per worktree so builds don't fight over the same target directory:
   ```bash
   CARGO_TARGET_DIR=/tmp/grit-build-<agent-name> cargo build --release -p grit
   ```
3. **Merge back to main** when each agent finishes:
   ```bash
   cd /home/hasi/grit && git merge --no-edit <branch>
   ```
4. **Clean up** worktrees after merging:
   ```bash
   git worktree remove /home/hasi/grit-worktrees/<agent-name>
   git branch -D <branch>
   ```
5. **Never run tests inside the main repo** — always use `/tmp/` for test scratch directories.
6. **Max parallel agents**: Keep to a reasonable number (3-5 for deepening work, up to 10 for independent new features) to avoid resource contention.
