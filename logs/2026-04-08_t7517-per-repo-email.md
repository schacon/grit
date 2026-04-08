# t7517-per-repo-email

## Goal

Make `tests/t7517-per-repo-email.sh` fully pass (Git `user.useConfigOnly`, `author.*` / `committer.*`, rebase ident behavior).

## Changes

- Added `grit/src/ident.rs`: Git-style identity resolution (env → role-specific config → `user.*`, `user.useConfigOnly` blocks `EMAIL` / synthetic fallback unless any of `user.email` / `author.email` / `committer.email` is set; loose committer helper for reflog).
- `commit.rs`: `resolve_author` / `resolve_committer` use the shared resolver.
- `rebase.rs`: same for cherry-pick committer; interactive rebase prints picks then continues replay (no sequence editor yet); `cherry_pick_for_rebase` fast-path when `HEAD == parent` (noop pick, no new commit / no committer needed).
- `var.rs`: strict `GIT_*_IDENT` uses the same email rules; non-strict listing uses lenient email.

## Validation

- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7517-per-repo-email.sh` → 16/16
