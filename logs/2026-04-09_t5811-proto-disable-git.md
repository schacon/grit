# t5811-proto-disable-git

## Summary

- Resolved broken `tests/lib-git-daemon.sh` (merge conflict markers); aligned with harness (no `test_atexit`/`say`, EXIT trap, robust `stop_git_daemon`).
- `git://` clone/fetch/push: after `check_protocol_allowed("git", …)`, delegate to `REAL_GIT` (default `/usr/bin/git`) with **subcommand + args only** so `-C` is not applied twice.
- `protocol.*.allow=user` now honors `GIT_PROTOCOL_FROM_USER` (default allow).
- Build fixes: removed stale `verbose` field from `rebase::Args` in `pull.rs`; fixed `add.rs` unused `cfg_ver`/`cfg_many`.

## Verification

- `cargo fmt`, `cargo clippy --fix`, `cargo test -p grit-lib --lib`
- `GUST_BIN=.../tests/grit GIT_TEST_GIT_DAEMON=true bash tests/t5811-proto-disable-git.sh` → 26/26 pass
