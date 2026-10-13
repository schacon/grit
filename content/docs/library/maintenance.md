---
title: Maintenance
summary: Object fsck validation and repository hooks.
---

Repository maintenance helpers live in [`maintenance`](rustdoc:grit_lib::maintenance):

- [`Repository::write_commit_graph`](rustdoc:grit_lib::repo::Repository::write_commit_graph) — atomic monolithic `objects/info/commit-graph` with optional changed-path Bloom filters ([`CommitGraphOptions`](rustdoc:grit_lib::maintenance::CommitGraphOptions), [`CommitGraphWriteReport`](rustdoc:grit_lib::maintenance::CommitGraphWriteReport)).
- [`Repository::write_midx`](rustdoc:grit_lib::repo::Repository::write_midx) — write `multi-pack-index` ([`MidxOptions`](rustdoc:grit_lib::maintenance::MidxOptions), [`MidxWriteReport`](rustdoc:grit_lib::maintenance::MidxWriteReport)).
- [`References::pack`](rustdoc:grit_lib::refs::References::pack) — pack loose refs into `packed-refs` or compact reftable stacks ([`PackRefsOptions`](rustdoc:grit_lib::maintenance::PackRefsOptions)).
- [`repack_all`](rustdoc:grit_lib::maintenance::repack_all), [`gc`](rustdoc:grit_lib::maintenance::gc), and [`fsck`](rustdoc:grit_lib::maintenance::fsck) orchestrate repack, prune, and validation.

Maintenance APIs split into **repository fsck**, **object validation**, and **hook execution**:

- [`fsck`](rustdoc:grit_lib::fsck) — repository-wide integrity checks (loose/pack objects, connectivity from [`reachability_roots`](rustdoc:grit_lib::fsck::reachability_roots), dangling objects, refs, MIDX, commit-graph). Progress via [`ProgressSink`](rustdoc:grit_lib::progress::ProgressSink); severity follows `fsck.<msg-id>` config.
- [`maintenance::fsck`](rustdoc:grit_lib::maintenance::fsck) — CLI-oriented wrapper with [`FsckOptions`](rustdoc:grit_lib::maintenance::FsckOptions) toggles for objects, refs, and MIDX.
- [`maintenance::repack`](rustdoc:grit_lib::maintenance::repack) — all-into-one repack using [`reachability_roots`](rustdoc:grit_lib::fsck::reachability_roots), optional `.rev`/`.bitmap` sidecars, pack pruning, and unreachable loose-object pruning via [`RepackOptions`](rustdoc:grit_lib::maintenance::RepackOptions) / [`RepackReport`](rustdoc:grit_lib::maintenance::RepackReport). [`Repository::repack`](rustdoc:grit_lib::repo::Repository::repack) is a thin alias.
- [`fsck_standalone`](rustdoc:grit_lib::fsck_standalone) — validate loose object bytes before hashing ([`fsck_object`](rustdoc:grit_lib::fsck_standalone::fsck_object), [`FsckError`](rustdoc:grit_lib::fsck_standalone::FsckError) with Git-compatible message ids). Connectivity walks for receive-pack and bundles live in [`connectivity`](rustdoc:grit_lib::connectivity).
- [`hooks`](rustdoc:grit_lib::hooks) — resolve and run hook scripts through the repository [`CommandRunner`](rustdoc:grit_lib::command_runner::CommandRunner) ([`run_hook`](rustdoc:grit_lib::hooks::run_hook), [`HookResult`](rustdoc:grit_lib::hooks::HookResult)). [`Repository::hooks`](rustdoc:grit_lib::repo::Repository::hooks) lists scripts via [`RepositoryHooks::list`](rustdoc:grit_lib::repo_handles::RepositoryHooks::list) ([`HookInfo`](rustdoc:grit_lib::repo_handles::HookInfo), [`HookSource`](rustdoc:grit_lib::repo_handles::HookSource)). Tests often install [`RecordingRunner`](rustdoc:grit_lib::command_runner::RecordingRunner) via [`RepositoryOptions`](rustdoc:grit_lib::environment::RepositoryOptions).
- [`patch_ids`](rustdoc:grit_lib::diff::patch_ids) — [`Repository::patch_id`](rustdoc:grit_lib::repo::Repository::patch_id) and [`compute_patch_id`](rustdoc:grit_lib::diff::patch_ids::compute_patch_id) for stable/unstable commit equivalence (merge commits return `None`).

The `grit` CLI maps fsck and hook failures to its own exit codes and messages; the library returns typed results only.

## Example

<!-- include: grit-examples/src/bin/guide_maintenance.rs -->

This example fsck-validates `HEAD`’s commit object, installs a `pre-commit` hook that exits 0, and runs it through `run_hook`. The test expects `git fsck --strict` to stay clean afterward.
