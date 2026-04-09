# 2026-04-09 — t0411-clone-from-partial (7/7)

## Summary

Made `tests/t0411-clone-from-partial.sh` pass by aligning promisor / upload-pack / clone behavior with Git.

## Key changes

- **fetch_transport**: Parse leading `VAR=value` before rewriting `… git-upload-pack` → `grit upload-pack` (t0411 file:// fetch with `GIT_TEST_ASSUME_DIFFERENT_OWNER`). Always spawn local upload-pack with client proto **0** so v0 `want`/`have` negotiation is not confused with protocol v2.
- **fetch**: `file://` URL remotes call `enforce_safe_directory_git_dir` on the opened remote repo (dubious ownership).
- **upload-pack**: Default `GIT_NO_LAZY_FETCH=1` when unset (match Git); enforce safe directory on server repo.
- **promisor_hydrate**: Git-compatible `GIT_NO_LAZY_FETCH` parsing; keep promisor remote when path missing; `resolve_remote_repo_path` fallback when canonicalize fails.
- **main**: `--no-lazy-fetch` sets `GIT_NO_LAZY_FETCH=1`.
- **clone**: Shallow source uses upload-pack path; inherit `blob:none` promisor state from promisor source when cloning without `--filter`; abort inherited promisor clone when lazy fetch disallowed by env (test 6); checkout attempts `try_lazy_fetch_promisor_object` for missing blobs (test 7).

## Validation

- `./scripts/run-tests.sh t0411-clone-from-partial.sh` → 7/7
- `cargo clippy -p grit-rs -p grit-lib -- -D warnings`
- `cargo test -p grit-lib --lib`
