# t7515-status-symlinks

## Problem

`t7515-status-symlinks.sh` compared `git status --porcelain` to expected output without a `##` branch line. Grit forced `args.branch = true` for explicit `--porcelain=v1` whenever untracked files were shown, so porcelain always started with `## master`, diverging from upstream Git.

## Fix

Removed the block in `grit/src/commands/status.rs` that set `args.branch = true` for that case. Plain `status --porcelain` now omits the `##` line unless `-b` / `--branch` (or `status.branch` for non-porcelain) is used, matching `/usr/bin/git` 2.43.

## Verification

- `GUST_BIN=target/release/grit sh tests/t7515-status-symlinks.sh -v -i` — 3/3
- `./scripts/run-tests.sh t7515-status-symlinks.sh` — updates CSV/dashboards
