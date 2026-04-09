# t1100-commit-tree-options — 2026-04-09

Verified on branch `cursor/t1100-commit-tree-options-a3d5`:

- `./scripts/run-tests.sh t1100-commit-tree-options.sh` → **5/5** tests pass.
- No Rust changes required; `grit commit-tree` already matches upstream expectations (GIT_* identity dates, `-p`/`-m` vs tree argument ordering via clap).

Note: Running the harness can rewrite unrelated CSV/dashboard rows; restored those after the run to avoid noise.
