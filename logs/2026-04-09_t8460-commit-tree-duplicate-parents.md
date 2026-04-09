# t8460-commit-tree-multi

## Issue

Harness reported 26/27: test `duplicate parents are preserved` failed.

## Cause

`commit-tree` deduplicated `-p` parents via `HashSet`. Upstream Git (and the test) keep every `-p` occurrence, including duplicates.

## Fix

In `grit/src/commands/commit_tree.rs`, collect parents in order without deduplication.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t8460-commit-tree-multi.sh` → 27/27
