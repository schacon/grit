# t7603-merge-reduce-heads

## Changes

- **Octopus head reduction**: After deduping and skipping ancestors of `HEAD`, drop any merge head that is an ancestor of another listed head (c4 dropped when c5 present).
- **Sequential merge bases**: Each octopus step uses merge base vs the *running* tip (ephemeral step commits in ODB), matching Git’s fast-forward between heads before the next merge.
- **Octopus conflicts**: On conflict, write full `MERGE_HEAD` (all requested merge OIDs), `MERGE_MSG`, stage unmerged entries, refresh worktree like two-way merge; exit 1.
- **`git commit`**: When `MERGE_HEAD` has more than one OID, parents are **only** those OIDs (not `HEAD` + merges), matching resolution commits after octopus conflicts.
- **`git pull`**: Multiple `FETCH_HEAD` merge lines now invoke merge with explicit refspecs or OID hexes instead of single `FETCH_HEAD` resolution.

## Validation

- `./scripts/run-tests.sh t7603-merge-reduce-heads.sh` → 13/13
- `cargo test -p grit-lib --lib`
