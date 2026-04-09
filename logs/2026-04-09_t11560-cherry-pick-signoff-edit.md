# t11560-cherry-pick-signoff-edit

**Date:** 2026-04-09

## Outcome

`./scripts/run-tests.sh t11560-cherry-pick-signoff-edit.sh` reports **32/32** passing on branch `cursor/t11560-cherry-pick-signoff-cf63` (no Rust changes required in this session; behavior already matches the suite).

## Coverage

The file exercises `grit cherry-pick` with `--signoff`/`-s`, `-n`/`--no-commit`, `-x`, message preservation, author/committer identity for signoff, abbreviated OIDs, sequential picks, and tree parity vs `git cherry-pick`.

## Follow-up

None for this file.
