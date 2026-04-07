# t11490-commit-fixup-squash

## Result

- Ran `./scripts/run-tests.sh t11490-commit-fixup-squash.sh` on branch `cursor/t11490-commit-fixup-squash-020d`.
- **33/33 tests pass** (no Rust changes required; `data/test-files.csv` had stale 32/1 counts).

## Notes

- Covers `commit -m`, `-F` (file/stdin), `--amend`, `--allow-empty`, `--allow-empty-message`, `-a`, `--author`, `--date`, `-q`, plus log/rev-list sanity checks.
