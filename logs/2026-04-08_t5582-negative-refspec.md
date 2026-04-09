# t5582-fetch-negative-refspec

## Summary

Implemented negative refspec handling for fetch/push, `fetch --prefetch`, `push --prune` with exclusions, default `receive.denyCurrentBranch` when unset, and fixed CLI glob `FETCH_HEAD` when refs are already up to date (duplicate entry bug removed; entries recorded before up-to-date early continue).

## Validation

- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t5582-fetch-negative-refspec.sh` → 16/16
- `cargo test -p grit-lib --lib`

## Notes

- `preprocess_fetch_argv` in `main.rs` restores `refs/heads/*:refs/remotes/<name>/*` when bash expands the glob to one branch and a `^` refspec is present.
- Upload-pack skipping path is skipped when user passes CLI refspecs so negative refspec + FETCH_HEAD logic runs on the local read path.
