# t13040-restore-quiet-progress

## Summary

Verified `tests/t13040-restore-quiet-progress.sh` passes **30/30** against current `grit` after fast-forwarding this branch to `origin/main` (`d21876f3` at time of run).

## Command

```bash
./scripts/run-tests.sh t13040-restore-quiet-progress.sh
```

## Notes

- No Rust changes were required: `grit restore` already implements the behaviors exercised by the file (`--staged`, `--worktree`, `--source`, pathspecs, symlinks, `--quiet` with empty stdout/stderr capture).
- Harness refreshed `data/test-files.csv` and dashboard HTML as part of the run.
