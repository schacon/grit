# t5507-remote-environment

## Goal

Make `tests/t5507-remote-environment.sh` fully pass (receive-side config isolation + scp-style SSH URL resolution under test harness).

## Changes

- **`grit/src/commands/push.rs`**
  - Treat scp-style URLs (`host:path`) as direct push targets so `git push host:remote` does not look up a remote named `host:remote`.
  - Load receive-pack policy (`receive.denyCurrentBranch`, etc.) with `ConfigSet::load_repo_local_only` so `git -c receive.denyCurrentBranch=false` on the pushing side does not affect local/SSH simulated pushes.
- **`grit/src/ssh_transport.rs`**
  - Resolve relative scp paths under `TRASH_DIRECTORY` first (`$TRASH/remote`), then nested `$TRASH/host/remote`, matching fake-SSH tests that `cd $TRASH_DIRECTORY` before `eval` of the remote command.

## Validation

- `./scripts/run-tests.sh t5507-remote-environment.sh` → 5/5
- `cargo test -p grit-lib --lib` → pass
